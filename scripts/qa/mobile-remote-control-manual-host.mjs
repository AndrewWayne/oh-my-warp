#!/usr/bin/env node
import { spawn, spawnSync } from "node:child_process";
import { mkdir } from "node:fs/promises";
import { createServer } from "node:net";
import { networkInterfaces } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = fileURLToPath(new URL(".", import.meta.url));
const repoRoot = resolve(scriptDir, "../..");
const stamp = new Date().toISOString().replace(/[:.]/g, "-");
const reportDir = resolve(
  process.env.OMW_QA_REPORT_DIR ||
    join(repoRoot, ".gstack/qa-reports", `mobile-remote-control-manual-${stamp}`),
);
const requestedPort = Number(process.env.OMW_QA_PHONE_PORT || "8788");
const phoneHost = process.env.OMW_QA_PHONE_HOST || detectLanIp();
const skipBuild = process.env.OMW_QA_SKIP_BUILD === "1";
const startMode = process.env.OMW_QA_REAL_START_MODE || "shell";
const cleanShell = process.env.OMW_QA_REAL_CLEAN_SHELL || "0";

if (!phoneHost) {
  console.error(
    "Could not find a LAN IPv4 address. Set OMW_QA_PHONE_HOST, for example OMW_QA_PHONE_HOST=192.168.1.23.",
  );
  process.exit(1);
}

await mkdir(reportDir, { recursive: true });

if (!skipBuild) {
  runChecked("npm", [
    "run",
    "build",
    "--workspace",
    "@oh-my-warp/web-controller",
  ]);
}

const cargo = cargoInvocation();
const realRoot = join(reportDir, "remote-control-host");
const realWorkDir = join(realRoot, "remote-workdir");
const byteDump = join(reportDir, "remote-control-byte-dump.bin");
const inputDump = join(reportDir, "remote-control-input-dump.bin");
await mkdir(realWorkDir, { recursive: true });

const bind = process.env.OMW_QA_REAL_BIND
  ? await assertBindAvailable(process.env.OMW_QA_REAL_BIND)
  : await findAvailableBind("0.0.0.0", requestedPort);
const publicBaseUrl =
  process.env.OMW_QA_PUBLIC_BASE_URL || `http://${phoneHost}:${parseBind(bind).port}`;

runManualPreflight();

const child = spawn(
  cargo.command,
  [...cargo.args, "run", "-p", "omw-remote", "--bin", "qa-mobile-remote-control"],
  {
    cwd: repoRoot,
    env: {
      ...process.env,
      OMW_QA_REAL_BIND: bind,
      OMW_QA_REAL_START_MODE: startMode,
      OMW_QA_REAL_CLEAN_SHELL: cleanShell,
      OMW_QA_PUBLIC_BASE_URL: publicBaseUrl,
      OMW_QA_REAL_ROOT: realRoot,
      OMW_QA_REAL_WORKDIR: realWorkDir,
      OMW_BYTE_DUMP: byteDump,
      OMW_INPUT_DUMP: inputDump,
    },
    stdio: ["ignore", "pipe", "pipe"],
  },
);

let ready = false;
let stdoutBuffer = "";

child.stdout.on("data", (chunk) => {
  const text = chunk.toString("utf8");
  stdoutBuffer += text;
  const lines = stdoutBuffer.split(/\r?\n/);
  stdoutBuffer = lines.pop() || "";
  for (const line of lines) {
    const jsonText = line.match(/^OMW_QA_REAL_READY\s+(.+)$/)?.[1];
    if (!jsonText) {
      process.stdout.write(`${line}\n`);
      continue;
    }
    ready = true;
    const info = JSON.parse(jsonText);
    console.log("");
    const journey =
      info.mode === "shell"
        ? "Start a new shell, then run `claude` inside it when ready."
        : "Claude Code is pre-started as the active PTY.";
    console.log("Real omw remote-control QA host is running.");
    console.log("");
    console.log(`Open this URL on your phone:`);
    console.log(info.pairUrl);
    console.log("");
    console.log(`Base URL: ${info.baseUrl}`);
    console.log(`Bind: ${info.bind}`);
    console.log(`Mode: ${info.mode}`);
    console.log(`New shell command: ${info.shellProgram} ${(info.shellArgs || []).join(" ")}`.trim());
    console.log(`Workdir: ${info.workDir}`);
    console.log(`Byte dump: ${info.byteDump}`);
    console.log(`Input dump: ${info.inputDump}`);
    console.log("");
    console.log(journey);
    console.log("Keep this process running while you QA. Press Ctrl-C to stop.");
  }
});

child.stderr.on("data", (chunk) => {
  process.stderr.write(chunk);
});

child.on("exit", (code, signal) => {
  if (!ready) {
    console.error(`real omw remote-control manual QA host exited before ready (${code ?? signal})`);
  } else {
    console.log(`real omw remote-control manual QA host stopped (${code ?? signal})`);
  }
  process.exitCode = code ?? 1;
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => {
    child.kill(signal);
  });
}

function detectLanIp() {
  const interfaces = networkInterfaces();
  const preferredNames = ["en0", "en1", "bridge100"];
  for (const name of preferredNames) {
    const match = interfaces[name]?.find(isUsableIpv4);
    if (match) return match.address;
  }
  for (const entries of Object.values(interfaces)) {
    const match = entries?.find(isUsableIpv4);
    if (match) return match.address;
  }
  return "";
}

function isUsableIpv4(entry) {
  return entry.family === "IPv4" && !entry.internal;
}

async function findAvailableBind(host, firstPort) {
  for (let port = firstPort; port < firstPort + 20; port += 1) {
    const bindCandidate = `${host}:${port}`;
    if (await canBind(bindCandidate)) {
      if (port !== firstPort) {
        console.log(`Port ${firstPort} is busy; using ${port} for this phone QA run.`);
      }
      return bindCandidate;
    }
  }
  console.error(
    `No available phone QA port found in ${firstPort}-${firstPort + 19}. Stop an old QA host or set OMW_QA_PHONE_PORT.`,
  );
  process.exit(1);
}

async function assertBindAvailable(bindValue) {
  if (!(await canBind(bindValue))) {
    console.error(
      `Cannot start phone QA host: ${bindValue} is already in use. Stop the old host or choose another OMW_QA_REAL_BIND.`,
    );
    process.exit(1);
  }
  return bindValue;
}

async function canBind(bindValue) {
  const { host, port } = parseBind(bindValue);
  return new Promise((resolvePromise) => {
    const server = createServer();
    server.once("error", () => resolvePromise(false));
    server.listen(port, host, () => {
      server.close(() => resolvePromise(true));
    });
  });
}

function parseBind(bindValue) {
  const idx = bindValue.lastIndexOf(":");
  if (idx < 0) {
    console.error(`Invalid bind address ${bindValue}. Expected host:port.`);
    process.exit(1);
  }
  const host = bindValue.slice(0, idx);
  const port = Number(bindValue.slice(idx + 1));
  if (!host || !Number.isInteger(port) || port <= 0) {
    console.error(`Invalid bind address ${bindValue}. Expected host:port.`);
    process.exit(1);
  }
  return { host, port };
}

function runManualPreflight() {
  if (
    startMode === "shell" &&
    cleanShell === "1" &&
    process.env.OMW_QA_ALLOW_CLEAN_SHELL !== "1"
  ) {
    console.error(
      [
        "Refusing to start manual phone QA with a clean shell.",
        "Manual real-phone QA must match product behavior, so it uses your default shell startup by default.",
        "Set OMW_QA_ALLOW_CLEAN_SHELL=1 only when intentionally debugging stripped-shell behavior.",
      ].join("\n"),
    );
    process.exit(1);
  }

  if (process.env.OMW_QA_SKIP_CLAUDE_PREFLIGHT === "1") return;

  const claude = spawnSync("claude", ["--version"], {
    cwd: realWorkDir,
    env: process.env,
    encoding: "utf8",
  });
  if (claude.status !== 0) {
    console.error("Claude preflight failed; not printing a phone URL that cannot run the target journey.");
    console.error((claude.stderr || claude.stdout || "claude --version failed").trim());
    console.error("Set OMW_QA_SKIP_CLAUDE_PREFLIGHT=1 only when testing non-Claude shell behavior.");
    process.exit(claude.status ?? 1);
  }
  console.log(`Preflight: ${claude.stdout.trim()}`);
  console.log(`Preflight: manual host will use ${cleanShell === "1" ? "clean" : "default"} shell mode.`);
}

function cargoInvocation() {
  if (process.env.OMW_QA_RUSTUP_TOOLCHAIN) {
    return {
      command: "rustup",
      args: ["run", process.env.OMW_QA_RUSTUP_TOOLCHAIN, "cargo"],
    };
  }

  const cargoProbe = spawnSync("cargo", ["--version"], {
    cwd: repoRoot,
    env: process.env,
    encoding: "utf8",
  });
  if (cargoProbe.status === 0) return { command: "cargo", args: [] };

  const rustupProbe = spawnSync("rustup", ["toolchain", "list"], {
    cwd: repoRoot,
    env: process.env,
    encoding: "utf8",
  });
  if (rustupProbe.status === 0) {
    const toolchain = rustupProbe.stdout
      .split(/\r?\n/)
      .map((line) => line.trim().split(/\s+/)[0])
      .find((line) => line && line !== "no");
    if (toolchain) {
      return { command: "rustup", args: ["run", toolchain, "cargo"] };
    }
  }

  return { command: "cargo", args: [] };
}

function runChecked(command, args) {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    env: process.env,
    stdio: "inherit",
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}
