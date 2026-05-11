#!/usr/bin/env node
import { spawn, spawnSync } from "node:child_process";
import { createServer } from "node:net";
import { mkdir, readFile, stat, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { remote } from "webdriverio";

const scriptDir = fileURLToPath(new URL(".", import.meta.url));
const repoRoot = resolve(scriptDir, "../..");
const hostScript = join(repoRoot, "scripts/qa/mobile-web-controller-host.mjs");
const distDir = join(repoRoot, "apps/web-controller/dist");
const simulatorApp =
  "/Applications/Xcode.app/Contents/Developer/Applications/Simulator.app";

const pairToken = "ABCD1234";
const deviceName = process.env.OMW_QA_IOS_DEVICE_NAME || "omw QA iPhone";
const deviceType =
  process.env.OMW_QA_IOS_DEVICE_TYPE ||
  "com.apple.CoreSimulator.SimDeviceType.iPhone-16";
const requestedRuntime = process.env.OMW_QA_IOS_RUNTIME || "";
const appiumHome = resolve(process.env.APPIUM_HOME || ".tmp/appium");
const npmCache = resolve(process.env.NPM_CONFIG_CACHE || ".tmp/npm-cache");

const now = new Date();
const stamp = now.toISOString().replace(/[:.]/g, "-");
const remoteControlMode =
  process.argv.includes("--remote-control") ||
  process.env.OMW_QA_IOS_REMOTE_CONTROL === "1";
const shellFirstMode =
  process.argv.includes("--shell-first") ||
  process.env.OMW_QA_IOS_SHELL_FIRST === "1";
const reportName = remoteControlMode
  ? `mobile-ios-remote-control-${stamp}`
  : `mobile-ios-safari-mock-${stamp}`;
const defaultReportDir = join(
  repoRoot,
  ".gstack/qa-reports",
  reportName,
);
const reportDir = resolve(process.env.OMW_QA_REPORT_DIR || defaultReportDir);
const skipBuild = process.env.OMW_QA_SKIP_BUILD === "1";
const keepOpen = process.argv.includes("--keep-open");
const skipWdaWarmup = process.env.OMW_QA_IOS_SKIP_WDA_WARMUP === "1";

const children = [];
const screenshots = [];
const assertions = [];
let lastQaLogs = null;
let summaryWritten = false;
let baseUrl = "";
let driver = null;
let remoteHostReady = null;
let remoteHostOutput = "";

function pass(name, details = undefined) {
  assertions.push({ name, status: "pass", details });
  console.log(`PASS ${name}${details ? ` - ${details}` : ""}`);
}

function note(name, details = undefined) {
  assertions.push({ name, status: "note", details });
  console.log(`NOTE ${name}${details ? ` - ${details}` : ""}`);
}

function fail(name, details = undefined) {
  assertions.push({ name, status: "fail", details });
  throw new Error(`${name}${details ? `: ${details}` : ""}`);
}

function delay(ms) {
  return new Promise((resolvePromise) => setTimeout(resolvePromise, ms));
}

async function main() {
  await mkdir(reportDir, { recursive: true });

  if (!existsSync(join(repoRoot, "node_modules/appium/package.json"))) {
    fail("Appium is installed", "run npm install");
  }
  if (!existsSync(join(appiumHome, "node_modules/appium-xcuitest-driver"))) {
    fail("XCUITest driver is installed", "run npm run qa:mobile:setup");
  }
  pass("Appium and XCUITest dependencies are present");

  const runtime = await resolveRuntime();
  const device = await ensureSimulatorDevice(runtime.identifier);
  pass("iOS simulator is available", `${device.name} ${device.udid}`);

  if (!skipBuild) {
    await run("npm", ["run", "build", "--workspace", "@oh-my-warp/web-controller"]);
  }
  if (!existsSync(join(distDir, "index.html"))) {
    fail("Web Controller dist exists", distDir);
  }

  const appiumPort = await freePort();
  const wdaPort = await freePort();
  let pairUrl = "";
  let host = null;

  if (remoteControlMode) {
    const started = await startRemoteControlHost();
    host = started.host;
    remoteHostReady = started.ready;
    baseUrl = remoteHostReady.baseUrl;
    pairUrl = remoteHostReady.pairUrl;
    await waitForHttp(`${baseUrl}/api/v1/host-info`, 20_000);
    pass("real omw-remote host is reachable", baseUrl);
    if (!shellFirstMode) {
      await waitForByteDumpText(
        (text) => /claude/i.test(text),
        120_000,
        "remote-control workload startup bytes",
      );
      pass("remote-control workload rendered initial PTY output");
    }
  } else {
    const hostPort = await freePort();
    baseUrl = `http://127.0.0.1:${hostPort}`;
    pairUrl = `${baseUrl}/pair?t=${pairToken}`;
    host = startChild(process.execPath, [hostScript], {
      env: {
        ...process.env,
        OMW_QA_MOCK_BIND: "127.0.0.1",
        OMW_QA_MOCK_PORT: String(hostPort),
        OMW_QA_PUBLIC_BASE_URL: baseUrl,
        OMW_QA_WEB_DIST: distDir,
      },
      label: "host",
    });
    await waitForHttp(`${baseUrl}/api/v1/host-info`, 10_000);
    await postJson(`${baseUrl}/qa/reset`, {});
    pass("mock host is reachable", baseUrl);
  }

  await bootSimulator(device.udid);
  await configureSimulatorForTerminalQa(device.udid);
  pass("simulator booted with software keyboard enabled");

  const appium = startChild(
    process.execPath,
    [join(repoRoot, "node_modules/appium/index.js"), "--port", String(appiumPort), "--log-level", "info"],
    {
      env: {
        ...process.env,
        APPIUM_HOME: appiumHome,
        NPM_CONFIG_CACHE: npmCache,
      },
      label: "appium",
    },
  );
  await waitForHttp(`http://127.0.0.1:${appiumPort}/status`, 20_000);
  pass("Appium server is reachable", `:${appiumPort}`);

  try {
    if (!skipWdaWarmup) {
      await warmWebDriverAgent(appiumPort, wdaPort, runtime, device);
    }

    driver = await openNativeSafariSession(appiumPort, wdaPort, runtime, device);
    pass("native Safari XCUITest session started");

    await clearSafariFirstRun();
    await openSafariUrl(pairUrl);
    if (remoteControlMode) {
      await delay(3000);
      if (shellFirstMode) {
        await openShellSessionFromSessions();
        pass("pair URL opened real omw-remote sessions screen and started a shell");
      } else {
        pass("pair URL opened against real pre-registered remote-control session");
      }
      await assertKeyboardState(false, "terminal initially opens without iOS keyboard");
    } else {
      await waitForHostLog(
        (entry) => entry.type === "pair-redeem",
        40_000,
        "pair redeem from iOS Safari",
      );
      await waitForHostLog(
        (entry) => entry.type === "ws-open",
        50_000,
        "terminal WebSocket from iOS Safari",
      );
      pass("pair flow opens terminal in native Safari");
    }
    await screenshot("01-ios-terminal-connected.png");

    if (remoteControlMode) {
      if (shellFirstMode) {
        await exerciseShellFirstRemoteControlJourney();
      } else {
        await exercisePrestartedRemoteControlWorkloadJourney();
      }
    } else {
      await focusTerminalAndType();
      await waitForInputText("echo ios-auto", 20_000);
      pass("iOS keyboard text reached terminal");

      await tapShortcutControls();
      await validateHostLogs();

      await postJson(`${baseUrl}/qa/output`, { text: longTerminalOutput() });
      await delay(1000);
      await nativeScrollTerminal();
      await screenshot("04-ios-native-scroll.png");
      pass("native drag gesture exercised terminal scrollback");
    }

    await writeSummary("pass");
    console.log("");
    console.log(`Mobile iOS auto QA passed. Report: ${reportDir}`);
  } catch (err) {
    await writeSummary("fail", err).catch(() => {});
    throw err;
  } finally {
    await cleanup({ appium, host, device });
  }
}

async function warmWebDriverAgent(appiumPort, wdaPort, runtime, device) {
  note("warming WebDriverAgent", "first run can take several minutes");
  let warmupDriver = null;
  try {
    warmupDriver = await remote({
      hostname: "127.0.0.1",
      port: appiumPort,
      path: "/",
      logLevel: "error",
      connectionRetryTimeout: 420_000,
      connectionRetryCount: 0,
      capabilities: baseCapabilities(runtime, device, {
        "appium:bundleId": "com.apple.Preferences",
        "appium:wdaLocalPort": wdaPort,
        "appium:appLaunchStateTimeoutSec": 90,
      }),
    });
    pass("WebDriverAgent warmup session started");
  } finally {
    if (warmupDriver) {
      await warmupDriver.deleteSession().catch(() => {});
    }
  }
}

async function openNativeSafariSession(appiumPort, wdaPort, runtime, device) {
  return remote({
    hostname: "127.0.0.1",
    port: appiumPort,
    path: "/",
    logLevel: "error",
    connectionRetryTimeout: 420_000,
    connectionRetryCount: 0,
    capabilities: baseCapabilities(runtime, device, {
      "appium:bundleId": "com.apple.mobilesafari",
      "appium:wdaLocalPort": wdaPort,
      "appium:appLaunchStateTimeoutSec": 90,
      "appium:includeSafariInWebviews": true,
      "appium:webviewConnectTimeout": 120_000,
    }),
  });
}

async function startRemoteControlHost() {
  const realRoot = resolve(
    process.env.OMW_QA_REAL_ROOT || join(reportDir, "remote-control-host"),
  );
  const realWorkDir = resolve(
    process.env.OMW_QA_REAL_WORKDIR || join(realRoot, "remote-workdir"),
  );
  const byteDump = resolve(
    process.env.OMW_BYTE_DUMP || join(reportDir, "remote-control-byte-dump.bin"),
  );
  const inputDump = resolve(
    process.env.OMW_INPUT_DUMP || join(reportDir, "remote-control-input-dump.bin"),
  );
  await mkdir(realWorkDir, { recursive: true });

  const cargo = cargoInvocation();
  const host = startChild(
    cargo.command,
    [...cargo.args, "run", "-p", "omw-remote", "--bin", "qa-mobile-remote-control"],
    {
      env: {
        ...process.env,
        OMW_QA_REAL_ROOT: realRoot,
        OMW_QA_REAL_WORKDIR: realWorkDir,
        OMW_QA_REAL_START_MODE: shellFirstMode ? "shell" : "claude",
        OMW_QA_REAL_CLEAN_SHELL: process.env.OMW_QA_REAL_CLEAN_SHELL || "0",
        OMW_BYTE_DUMP: byteDump,
        OMW_INPUT_DUMP: inputDump,
      },
      label: "real-host",
      onOutput: (chunk) => {
        remoteHostOutput = `${remoteHostOutput}${chunk}`.slice(-80_000);
      },
    },
  );
  const ready = await waitForRemoteControlHostReady(host, 180_000);
  pass("real omw remote-control QA harness started", ready.workDir);
  return { host, ready };
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
      note("using explicit rustup toolchain", toolchain);
      return { command: "rustup", args: ["run", toolchain, "cargo"] };
    }
  }

  return { command: "cargo", args: [] };
}

async function waitForRemoteControlHostReady(child, timeoutMs) {
  let buffered = "";
  return new Promise((resolvePromise, reject) => {
    const timeout = setTimeout(() => {
      reject(new Error("real omw remote-control QA harness did not become ready"));
    }, timeoutMs);
    const onData = (chunk) => {
      buffered += chunk.toString("utf8");
      const lines = buffered.split(/\r?\n/);
      buffered = lines.pop() || "";
      for (const line of lines) {
        const jsonText = line.match(/^OMW_QA_REAL_READY\s+(.+)$/)?.[1];
        if (!jsonText) continue;
        try {
          clearTimeout(timeout);
          child.stdout.off("data", onData);
          child.off("exit", onExit);
          resolvePromise(JSON.parse(jsonText));
          return;
        } catch (err) {
          clearTimeout(timeout);
          child.stdout.off("data", onData);
          child.off("exit", onExit);
          reject(err);
          return;
        }
      }
    };
    const onExit = (code, signal) => {
      clearTimeout(timeout);
      child.stdout.off("data", onData);
      reject(
        new Error(
          `real omw remote-control QA harness exited before ready (${code ?? signal})`,
        ),
      );
    };
    child.stdout.on("data", onData);
    child.once("exit", onExit);
  });
}

function baseCapabilities(runtime, device, extra) {
  return {
    platformName: "iOS",
    "appium:automationName": "XCUITest",
    "appium:platformVersion": runtime.version,
    "appium:deviceName": device.name,
    "appium:udid": device.udid,
    "appium:noReset": false,
    "appium:autoAcceptAlerts": true,
    "appium:connectHardwareKeyboard": false,
    "appium:forceSimulatorSoftwareKeyboardPresence": true,
    "appium:forceTurnOnSoftwareKeyboardSimulator": true,
    "appium:simulatorStartupTimeout": 300_000,
    "appium:wdaLaunchTimeout": 300_000,
    "appium:wdaConnectionTimeout": 300_000,
    "appium:wdaStartupRetries": 2,
    "appium:wdaStartupRetryInterval": 10_000,
    "appium:reduceMotion": true,
    "appium:shouldTerminateApp": true,
    ...extra,
  };
}

async function clearSafariFirstRun() {
  for (let attempt = 0; attempt < 4; attempt += 1) {
    let tapped = false;
    for (const label of ["Continue", "Not Now", "Done", "OK", "Allow"]) {
      tapped = (await tapIfVisible(label, 1200)) || tapped;
    }
    if (!tapped) return;
    await delay(700);
  }
}

async function openSafariUrl(url) {
  try {
    await driver.execute("mobile: deepLink", {
      url,
      bundleId: "com.apple.mobilesafari",
    });
  } catch (err) {
    note("mobile: deepLink failed, falling back to simctl openurl", errStr(err));
    await run("xcrun", ["simctl", "openurl", driver.capabilities.udid, url], {
      timeoutMs: 45_000,
    });
  }
}

async function focusTerminalAndType() {
  await focusTerminalAndShowKeyboard();
  await typeWithIosKeyboard("echo ios-auto");
  await delay(500);
  await pressIosKeyboardReturn();
}

async function focusTerminalAndShowKeyboard(
  screenshotName = "02-ios-keyboard-visible.png",
) {
  const rect = await driver.getWindowRect();
  const points = [
    { x: rect.width * 0.5, y: rect.height * 0.48 },
    { x: rect.width * 0.5, y: rect.height * 0.58 },
    { x: rect.width * 0.5, y: rect.height * 0.68 },
  ];

  let keyboardShown = false;
  for (const point of points) {
    await nativeTap(point.x, point.y);
    await delay(900);
    keyboardShown = await driver.isKeyboardShown().catch(() => false);
    if (keyboardShown) break;
  }
  if (!keyboardShown) {
    await screenshot("02-ios-keyboard-not-shown.png");
    fail("iOS software keyboard is shown after tapping terminal");
  }
  pass("iOS software keyboard is shown");
  await screenshot(screenshotName);
}

async function tapShortcutControls() {
  const rect = await driver.getWindowRect();
  const rowY = shortcutPrimaryY(rect);

  for (const control of [
    ["shift tab", 0.06],
    ["esc", 0.17],
    ["tab", 0.28],
    ["^C", 0.39],
    ["arrow up", 0.5],
    ["arrow down", 0.61],
    ["arrow left", 0.72],
    ["arrow right", 0.83],
  ]) {
    await tapShortcutAt(control[0], rect.width * control[1], rowY);
  }

  await tapShortcutAt("show extra shortcuts", rect.width * 0.94, rowY);
  await delay(500);
  await screenshot("03-ios-more-drawer.png");

  const overflowY = shortcutOverflowY(rect);
  for (const control of [
    ["^D", 0.28],
    ["^L", 0.44],
    ["/", 0.61],
    ["|", 0.77],
    ["?", 0.93],
  ]) {
    await tapShortcutAt(control[0], rect.width * control[1], overflowY);
  }
  pass("native taps exercised primary and overflow shortcut controls");
}

async function tapOverflowShortcutControls() {
  const rect = await driver.getWindowRect();
  const rowY = shortcutPrimaryY(rect);
  await tapShortcutAt("show extra shortcuts", rect.width * 0.94, rowY);
  await delay(500);
  await screenshot("08-ios-remote-control-more-drawer.png");

  const overflowY = shortcutOverflowY(rect);
  for (const control of [
    ["^L", 0.44],
    ["/", 0.61],
    ["|", 0.77],
    ["?", 0.93],
  ]) {
    await tapShortcutAt(control[0], rect.width * control[1], overflowY);
  }
  pass("native taps exercised safe overflow shortcut controls");
}

async function openShellSessionFromSessions() {
  if (await tapIfVisible("Start a new shell", 15_000)) {
    await delay(3000);
    return;
  }

  const rect = await driver.getWindowRect();
  note("Start a new shell accessibility tap missed", "falling back to coordinate tap");
  await nativeTap(rect.width * 0.78, rect.height * 0.16);
  await delay(3000);
}

async function returnToSessionsFromTerminal(screenshotName) {
  const keyboardShown = await driver.isKeyboardShown().catch(() => false);
  if (keyboardShown) {
    await hideKeyboardFromShortcutStrip();
  } else {
    await driver.hideKeyboard().catch(() => {});
    await delay(700);
  }
  if (!(await tapIfVisible("Back to sessions", 8_000))) {
    const rect = await driver.getWindowRect();
    note("Back to sessions accessibility tap missed", "falling back to coordinate tap");
    await nativeTap(rect.width * 0.08, rect.height * 0.12);
  }
  await waitForVisible(
    "Start a new shell",
    20_000,
    "sessions screen is reachable from terminal",
  );
  await screenshot(screenshotName);
}

async function openExistingShellFromSessions() {
  if (!(await tapIfVisible("Open", 20_000))) {
    fail("existing shell can be reopened from Sessions");
  }
  await delay(2500);
  await assertKeyboardState(false, "existing shell reconnect opens without keyboard");
  await screenshot("05-ios-existing-shell-reconnected.png");
}

async function stopCurrentShellFromSessions() {
  if (!(await tapIfVisible("Stop", 20_000))) {
    fail("active shell exposes Stop on Sessions screen");
  }
  await waitFor(
    async () => (!(await isVisible("Open")) ? true : null),
    20_000,
    "stopped shell disappears from Sessions list",
  );
  pass("stopped shell disappears from Sessions list");
  await screenshot("13-ios-session-stopped.png");
}

async function tapPrimaryShortcutControls() {
  const rect = await driver.getWindowRect();
  const rowY = shortcutPrimaryY(rect);

  for (const control of [
    ["shift tab", 0.06],
    ["esc", 0.17],
    ["tab", 0.28],
    ["^C", 0.39],
    ["arrow up", 0.5],
    ["arrow down", 0.61],
    ["arrow left", 0.72],
    ["arrow right", 0.83],
  ]) {
    await tapShortcutAt(control[0], rect.width * control[1], rowY);
  }
  pass("native taps exercised primary shortcut controls");
}

async function tapSinglePrimaryShortcut(label, xRatio) {
  const rect = await driver.getWindowRect();
  await tapShortcutAt(label, rect.width * xRatio, shortcutPrimaryY(rect));
}

async function hideKeyboardFromShortcutStrip() {
  const rect = await driver.getWindowRect();
  if (!(await isVisible("hide keyboard"))) {
    if (!(await tapIfVisible("show extra shortcuts", 3_000))) {
      await tapShortcutAt("show extra shortcuts", rect.width * 0.94, shortcutPrimaryY(rect));
    }
    await waitForVisible(
      "hide keyboard",
      5_000,
      "shortcut overflow exposes hide keyboard",
    );
  }

  if (!(await tapIfVisible("hide keyboard", 3_000))) {
    await tapShortcutAt("hide keyboard", rect.width * 0.12, shortcutOverflowY(rect));
  }
  await delay(900);
  const stillShown = await driver.isKeyboardShown().catch(() => false);
  if (stillShown) {
    note("shortcut hide did not dismiss keyboard", "falling back to Appium hideKeyboard");
    await driver.hideKeyboard().catch((err) => note("hideKeyboard fallback failed", errStr(err)));
    await delay(900);
  }
  await assertKeyboardState(false, "shortcut strip can hide the iOS keyboard");
}

async function exercisePrestartedRemoteControlWorkloadJourney() {
  await screenshot("02-ios-remote-workload-cold-no-keyboard.png");
  await assertKeyboardState(false, "remote-control workload starts in non-keyboard mode");

  await focusTerminalAndShowKeyboard("03-ios-remote-workload-keyboard-expanded.png");
  await assertKeyboardState(true, "tap expands remote-control workload into iOS keyboard mode");
  await clearClaudeWorkspaceTrustIfPrompted();

  await typeWithIosKeyboard("hello from ios keyboard");
  await waitForInputDumpText(
    "hello from ios keyboard",
    20_000,
    "real iOS keyboard text reaches remote-control PTY",
  );
  pass("real iOS keyboard text reached remote-control PTY");
  await screenshot("04-ios-remote-workload-typed-text.png");

  const beforeClear = await inputDumpSize();
  await tapSinglePrimaryShortcut("^C", 0.35);
  await waitForInputDumpBytesAfter(
    beforeClear,
    [3],
    20_000,
    "remote-control workload receives Ctrl-C from shortcut strip",
  );
  pass("remote-control workload input prompt cleared via shortcut Ctrl-C");

  await hideKeyboardFromShortcutStrip();
  await screenshot("05-ios-remote-workload-keyboard-hidden.png");

  await focusTerminalAndShowKeyboard("06-ios-remote-workload-keyboard-reopened.png");
  await assertKeyboardState(true, "tap reopens iOS keyboard after hiding it");

  const startupBytes = await byteDumpSize();
  const beforeHelpInput = await inputDumpSize();
  await typeWithIosKeyboard("/help");
  await delay(500);
  await pressIosKeyboardReturn();
  await waitForInputDumpTextAfter(
    beforeHelpInput,
    "/help",
    20_000,
    "remote-control workload receives /help text from iOS keyboard",
  );
  await waitForInputDumpBytesAfter(
    beforeHelpInput,
    [13],
    20_000,
    "remote-control workload receives iOS keyboard Return",
  );
  await waitForByteDumpGrowth(
    startupBytes,
    64,
    45_000,
    "remote-control workload reacts to /help from iOS keyboard",
  );
  pass("remote-control workload reacted to iOS keyboard slash command");
  await screenshot("07-ios-remote-workload-help.png");

  const beforeShortcuts = await inputDumpSize();
  await tapPrimaryShortcutControls();
  await validateInputDumpSequencesSince(beforeShortcuts, [
    ["Shift-Tab", [27, 91, 90]],
    ["Esc", [27]],
    ["Tab", [9]],
    ["Ctrl-C", [3]],
    ["Up", [27, 91, 65]],
    ["Down", [27, 91, 66]],
    ["Left", [27, 91, 68]],
    ["Right", [27, 91, 67]],
  ]);
  pass("remote-control workload received every primary shortcut byte sequence");

  const beforeOverflow = await inputDumpSize();
  await tapOverflowShortcutControls();
  await validateInputDumpSequencesSince(beforeOverflow, [
    ["Ctrl-L", [12]],
    ["Slash", [47]],
    ["Pipe", [124]],
    ["Question", [63]],
  ]);
  pass("remote-control workload received safe overflow shortcut byte sequences");

  pass("remote-control workload handled shortcut navigation controls");

  const beforeScrollInput = await inputDumpSize();
  await nativeScrollTerminal("down");
  await screenshot("09-ios-remote-workload-scroll-down.png");
  await nativeScrollTerminal("up");
  await screenshot("10-ios-remote-workload-scroll-up.png");
  await assertInputDumpOnlyNavigationBytesSince(
    beforeScrollInput,
    "native terminal scroll does not type literal keyboard text into remote-control workload",
  );
  pass("native drag gestures exercised remote-control workload terminal scroll both directions");
}

async function exerciseShellFirstRemoteControlJourney() {
  await screenshot("02-ios-remote-shell-cold-no-keyboard.png");
  await assertKeyboardState(false, "remote-control shell starts in non-keyboard mode");

  await focusTerminalAndShowKeyboard("03-ios-remote-shell-keyboard-expanded.png");
  await assertKeyboardState(true, "tap expands shell into iOS keyboard mode");

  await runShellCommand(
    "pwd",
    (text) => text.includes(remoteHostReady.workDir),
    "remote-control shell starts in disposable workdir",
  );
  await screenshot("04-ios-remote-shell-pwd.png");

  await runShellCommand(
    "echo omw-terminal-normal",
    /omw-terminal-normal/,
    "normal terminal command round-trip succeeds",
  );

  await returnToSessionsFromTerminal("05-ios-sessions-existing-shell.png");
  await openExistingShellFromSessions();
  await focusTerminalAndShowKeyboard("06-ios-existing-shell-keyboard-expanded.png");
  await runShellCommand(
    "echo omw-existing-shell",
    /omw-existing-shell/,
    "existing shell reconnect remains writable",
  );

  const beforeClaudeInput = await inputDumpSize();
  const beforeClaudeOutput = await byteDumpSize();
  await typeWithIosKeyboard("claude");
  await pressIosKeyboardReturn();
  await waitForInputDumpTextAfter(
    beforeClaudeInput,
    "claude",
    20_000,
    "remote-control shell receives Claude launch command",
  );
  await waitForByteDumpText(
    (text) =>
      text.length > beforeClaudeOutput &&
      /Claude\s+Code|auto mode on|Opus/i.test(text.slice(beforeClaudeOutput)),
    90_000,
    "Claude Code launches from a phone-started shell",
  );
  pass("Claude Code launched from shell-first remote-control journey");
  await delay(2500);
  await screenshot("07-ios-remote-workload-launched-from-shell.png");

  await clearClaudeWorkspaceTrustIfPrompted();
  await assertClaudeInteractiveAfterShellLaunch();

  const beforeShortcuts = await inputDumpSize();
  await tapPrimaryShortcutControls();
  await validateInputDumpSequencesSince(beforeShortcuts, [
    ["Shift-Tab", [27, 91, 90]],
    ["Esc", [27]],
    ["Tab", [9]],
    ["Ctrl-C", [3]],
    ["Up", [27, 91, 65]],
    ["Down", [27, 91, 66]],
    ["Left", [27, 91, 68]],
    ["Right", [27, 91, 67]],
  ]);
  pass("remote-control journey received every primary shortcut byte sequence");

  const beforeOverflow = await inputDumpSize();
  await tapOverflowShortcutControls();
  await validateInputDumpSequencesSince(beforeOverflow, [
    ["Ctrl-L", [12]],
    ["Slash", [47]],
    ["Pipe", [124]],
    ["Question", [63]],
  ]);
  pass("remote-control journey received safe overflow shortcut byte sequences");

  const beforeScrollInput = await inputDumpSize();
  await nativeScrollTerminal("down");
  await screenshot("11-ios-remote-scroll-down.png");
  await nativeScrollTerminal("up");
  await screenshot("12-ios-remote-scroll-up.png");
  await assertInputDumpOnlyNavigationBytesSince(
    beforeScrollInput,
    "native terminal scroll does not type literal keyboard text into remote-control PTY",
  );
  pass("native drag gestures exercised remote-control terminal scroll both directions");

  await exitClaudeToShell();
  await exerciseCodexCliSmoke();
  await returnToSessionsFromTerminal("12b-ios-sessions-before-stop.png");
  await stopCurrentShellFromSessions();
  await openShellSessionFromSessions();
  pass("new shell starts after prior shell is stopped");
  await assertKeyboardState(false, "fresh shell after stop opens without keyboard");
  await focusTerminalAndShowKeyboard("14-ios-fresh-shell-after-stop-keyboard-expanded.png");
  await runShellCommand(
    "echo omw-after-stop-new-shell",
    /omw-after-stop-new-shell/,
    "fresh shell after Stop is usable",
  );
  await screenshot("15-ios-fresh-shell-after-stop-command.png");
}

async function assertClaudeInteractiveAfterShellLaunch() {
  const beforeHelpInput = await inputDumpSize();
  const beforeHelpOutput = await byteDumpSize();

  await typeWithIosKeyboard("/help");
  await pressIosKeyboardReturn();
  await waitForInputDumpTextAfter(
    beforeHelpInput,
    "/help",
    20_000,
    "remote-control shell-first journey sends a Claude slash command",
  );
  await waitForInputDumpBytesAfter(
    beforeHelpInput,
    [13],
    20_000,
    "remote-control shell-first journey submits the Claude slash command",
  );

  await delay(3000);
  const immediateOutput = (await readByteDumpText()).slice(beforeHelpOutput);
  const shellHandledHelp =
    /(?:zsh|sh|bash):.*\/help|command not found: \/help|no such file or directory: \/help/i.test(
      immediateOutput,
    );
  if (shellHandledHelp) {
    fail(
      "Claude Code remains interactive after phone launch",
      "the shell handled /help, which means Claude started and exited before the journey became usable",
    );
  }

  await waitForByteDumpText(
    (text) => {
      const afterHelp = text.slice(beforeHelpOutput);
      return (
        afterHelp.length > 64 &&
        /Claude\s+Code|slash|commands|help|resume|compact/i.test(afterHelp) &&
        !/(?:zsh|sh|bash):.*\/help|command not found: \/help|no such file or directory: \/help/i.test(afterHelp)
      );
    },
    45_000,
    "Claude Code responds interactively after phone launch",
  );
  pass("Claude Code stayed interactive after shell-first phone launch");
  await screenshot("08-ios-remote-workload-help-after-launch.png");
}

async function exitClaudeToShell() {
  await tapIfVisible("hide extra shortcuts", 2_000);
  await delay(500);

  const beforeCancelInput = await inputDumpSize();
  await tapSinglePrimaryShortcut("^C", 0.39);
  await waitForInputDumpBytesAfter(
    beforeCancelInput,
    [3],
    20_000,
    "Claude Code receives Ctrl-C before exit",
  );
  await delay(1000);

  const beforeExitInput = await inputDumpSize();
  await typeWithIosKeyboard("/exit");
  await pressIosKeyboardReturn();
  await waitForInputDumpTextAfter(
    beforeExitInput,
    "/exit",
    20_000,
    "Claude Code receives slash-exit before shell continuation",
  );
  await delay(3000);
  await runShellCommand(
    "echo omw-after-claude",
    /omw-after-claude/,
    "shell remains usable after Claude Code journey",
  );
}

async function exerciseCodexCliSmoke() {
  await runShellCommand(
    "codex --version",
    /codex-cli/i,
    "Codex CLI version command runs in real terminal",
    45_000,
  );
  await runShellCommand(
    "codex --help",
    /Codex CLI|Usage:\s+codex/i,
    "Codex CLI help renders in real terminal",
    45_000,
  );
  await screenshot("12a-ios-codex-cli-smoke.png");
}

async function clearClaudeWorkspaceTrustIfPrompted() {
  const text = await readByteDumpText();
  if (!/trust|untrusted/i.test(text)) return;
  note("Claude workspace trust prompt detected", "pressing Return on the default action");
  await pressIosKeyboardReturn();
  await delay(2500);
}

async function assertKeyboardState(expectedShown, label) {
  const shown = await driver.isKeyboardShown().catch(() => false);
  if (shown !== expectedShown) {
    await screenshot(`${safeName(label)}.png`).catch(() => {});
    fail(label, `expected keyboard ${expectedShown ? "shown" : "hidden"}, got ${shown ? "shown" : "hidden"}`);
  }
  pass(label);
}

async function pressIosKeyboardReturn() {
  note("native keyboard return");
  await driver.execute("mobile: keys", {
    keys: ["\n"],
  });
  await delay(250);
}

async function tapShortcutAt(label, x, y) {
  note(`native tap ${label}`, `${Math.round(x)},${Math.round(y)}`);
  await nativeTap(x, y);
  await delay(250);
}

async function tapIfVisible(label, timeoutMs) {
  try {
    const el = await driver.$(`~${label}`);
    await el.waitForDisplayed({ timeout: timeoutMs });
    await el.click();
    return true;
  } catch {
    return false;
  }
}

async function isVisible(label) {
  try {
    const el = await driver.$(`~${label}`);
    return await el.isDisplayed();
  } catch {
    return false;
  }
}

async function waitForVisible(accessibilityLabel, timeoutMs, label) {
  await waitFor(
    async () => ((await isVisible(accessibilityLabel)) ? true : null),
    timeoutMs,
    label,
  );
  pass(label);
}

function shortcutPrimaryY(rect) {
  return rect.height * 0.49;
}

function shortcutOverflowY(rect) {
  return rect.height * 0.49;
}

async function nativeTap(x, y) {
  await driver.execute("mobile: tap", {
    x: Math.round(x),
    y: Math.round(y),
  });
}

async function nativeScrollTerminal(direction = "down") {
  const rect = await driver.getWindowRect();
  const stripY = shortcutPrimaryY(rect);
  const topY = Math.max(rect.height * 0.28, 190);
  const bottomY = Math.min(rect.height * 0.42, stripY - 58);
  if (bottomY <= topY + 32) {
    fail(
      "terminal scroll gesture has enough vertical room",
      `top=${Math.round(topY)} bottom=${Math.round(bottomY)} strip=${Math.round(stripY)}`,
    );
  }
  const fromY = direction === "up" ? bottomY : topY;
  const toY = direction === "up" ? topY : bottomY;
  await driver.execute("mobile: dragFromToForDuration", {
    duration: 0.8,
    fromX: Math.round(rect.width * 0.52),
    fromY: Math.round(fromY),
    toX: Math.round(rect.width * 0.52),
    toY: Math.round(toY),
  });
  await delay(600);
}

async function typeWithIosKeyboard(text) {
  await driver.execute("mobile: keys", {
    keys: Array.from(text),
  });
}

async function runShellCommand(command, expectedOutput, label, timeoutMs = 30_000) {
  const beforeInput = await inputDumpSize();
  const beforeOutput = await byteDumpSize();
  await typeWithIosKeyboard(command);
  await pressIosKeyboardReturn();
  await waitForInputDumpTextAfter(
    beforeInput,
    command,
    20_000,
    `${label}: command text reaches PTY`,
  );
  await waitForInputDumpBytesAfter(
    beforeInput,
    [13],
    20_000,
    `${label}: Return reaches PTY`,
  );
  await waitForByteDumpTextAfter(
    beforeOutput,
    (text) => matchesExpectedOutput(text, expectedOutput),
    timeoutMs,
    label,
  );
  pass(label);
}

function matchesExpectedOutput(text, expectedOutput) {
  if (typeof expectedOutput === "string") {
    return text.includes(expectedOutput);
  }
  if (expectedOutput instanceof RegExp) {
    return expectedOutput.test(text);
  }
  return expectedOutput(text);
}

async function validateHostLogs() {
  const { logs } = await getJson(`${baseUrl}/qa/logs`);
  const pairRedeem = logs.find((entry) => entry.type === "pair-redeem");
  if (!pairRedeem) fail("host saw iOS pair redeem");
  pass(
    "host saw iOS pair redeem",
    `${pairRedeem.body?.device_name || "unknown"} / ${pairRedeem.body?.platform || "unknown"}`,
  );

  const wsOpenCount = logs.filter((entry) => entry.type === "ws-open").length;
  if (wsOpenCount < 1) fail("host saw iOS terminal WebSocket");
  pass("host saw iOS terminal WebSocket", `count=${wsOpenCount}`);

  await waitForInputText("echo ios-auto", 1);

  const inputFrames = logs.filter((entry) => entry.type === "ws-frame" && entry.kind === "input");
  const expected = new Map([
    ["Shift-Tab", [27, 91, 90]],
    ["Esc", [27]],
    ["Tab", [9]],
    ["Ctrl-C", [3]],
    ["Up", [27, 91, 65]],
    ["Down", [27, 91, 66]],
    ["Enter", [13]],
    ["Ctrl-D", [4]],
    ["Ctrl-L", [12]],
    ["Slash", [47]],
    ["Pipe", [124]],
    ["Question", [63]],
    ["Left", [27, 91, 68]],
    ["Right", [27, 91, 67]],
  ]);
  for (const [name, bytes] of expected) {
    const seen = inputFrames.some((entry) => sameBytes(entry.bytes, bytes));
    if (!seen) fail(`host saw ${name} shortcut bytes`, JSON.stringify(bytes));
  }
  pass("host saw every expected shortcut byte sequence");

  const tinyResize = logs
    .filter((entry) => entry.type === "ws-frame" && entry.kind === "control")
    .map((entry) => {
      try {
        return JSON.parse(entry.text);
      } catch {
        return null;
      }
    })
    .filter((entry) => entry?.type === "resize" && (entry.rows < 8 || entry.cols < 20));
  if (tinyResize.length > 0) {
    fail("host saw no tiny iOS resize frames", JSON.stringify(tinyResize));
  }
  pass("host saw no tiny iOS resize frames");
}

async function waitForInputText(text, timeoutMs) {
  await waitFor(
    async () => {
      const { logs } = await getJson(`${baseUrl}/qa/logs`);
      const inputText = logs
        .filter((entry) => entry.type === "ws-frame" && entry.kind === "input")
        .map((entry) => entry.text || "")
        .join("");
      return inputText.includes(text);
    },
    timeoutMs,
    `host input text ${text}`,
  );
}

async function waitForHostLog(predicate, timeoutMs, label) {
  return waitFor(
    async () => {
      const { logs } = await getJson(`${baseUrl}/qa/logs`);
      return logs.find(predicate);
    },
    timeoutMs,
    label,
  );
}

async function waitForByteDumpText(predicate, timeoutMs, label) {
  return waitFor(
    async () => {
      const text = await readByteDumpText();
      return predicate(text) ? text : null;
    },
    timeoutMs,
    label,
  );
}

async function waitForByteDumpTextAfter(offset, predicate, timeoutMs, label) {
  return waitFor(
    async () => {
      const bytes = await readByteDumpBytes();
      const text = bytes.subarray(Math.min(offset, bytes.length)).toString("utf8");
      return predicate(text) ? text : null;
    },
    timeoutMs,
    label,
  );
}

async function waitForInputDumpText(text, timeoutMs, label) {
  return waitForInputDumpTextAfter(0, text, timeoutMs, label);
}

async function waitForInputDumpTextAfter(offset, text, timeoutMs, label) {
  const needle = Buffer.from(text, "utf8");
  return waitForInputDumpBytesAfter(offset, Array.from(needle), timeoutMs, label);
}

async function waitForInputDumpBytesAfter(offset, expectedBytes, timeoutMs, label) {
  const needle = Buffer.from(expectedBytes);
  return waitFor(
    async () => {
      const bytes = await readInputDumpBytes();
      const slice = bytes.subarray(Math.min(offset, bytes.length));
      return slice.indexOf(needle) >= 0 ? bytes.length : null;
    },
    timeoutMs,
    label,
  );
}

async function validateInputDumpSequencesSince(offset, expected) {
  const bytes = await readInputDumpBytes();
  const slice = bytes.subarray(Math.min(offset, bytes.length));
  for (const [name, sequence] of expected) {
    const needle = Buffer.from(sequence);
    if (slice.indexOf(needle) < 0) {
      fail(`remote-control input dump saw ${name} bytes`, JSON.stringify(sequence));
    }
  }
}

async function assertInputDumpOnlyNavigationBytesSince(offset, label) {
  const bytes = await readInputDumpBytes();
  const slice = bytes.subarray(Math.min(offset, bytes.length));
  let index = 0;
  while (index < slice.length) {
    const isCsiArrow =
      slice[index] === 27 &&
      slice[index + 1] === 91 &&
      [65, 66, 67, 68].includes(slice[index + 2]);
    const isSs3Arrow =
      slice[index] === 27 &&
      slice[index + 1] === 79 &&
      [65, 66, 67, 68].includes(slice[index + 2]);
    if (isCsiArrow || isSs3Arrow) {
      index += 3;
      continue;
    }
    fail(
      label,
      `unexpected bytes after scroll=${JSON.stringify(Array.from(slice))}`,
    );
  }
  pass(label, slice.length ? `navigation bytes=${JSON.stringify(Array.from(slice))}` : "no PTY input");
}

async function waitForByteDumpGrowth(beforeBytes, minDelta, timeoutMs, label) {
  return waitFor(
    async () => {
      const size = await byteDumpSize();
      return size >= beforeBytes + minDelta ? size : null;
    },
    timeoutMs,
    label,
  );
}

async function inputDumpSize() {
  if (!remoteHostReady?.inputDump) return 0;
  try {
    return (await stat(remoteHostReady.inputDump)).size;
  } catch {
    return 0;
  }
}

async function readInputDumpBytes() {
  if (!remoteHostReady?.inputDump) return Buffer.alloc(0);
  try {
    return await readFile(remoteHostReady.inputDump);
  } catch {
    return Buffer.alloc(0);
  }
}

async function byteDumpSize() {
  if (!remoteHostReady?.byteDump) return 0;
  try {
    return (await stat(remoteHostReady.byteDump)).size;
  } catch {
    return 0;
  }
}

async function readByteDumpBytes() {
  if (!remoteHostReady?.byteDump) return Buffer.alloc(0);
  try {
    return await readFile(remoteHostReady.byteDump);
  } catch {
    return Buffer.alloc(0);
  }
}

async function readByteDumpText() {
  return (await readByteDumpBytes()).toString("utf8");
}

async function screenshot(name) {
  const data = await driver.takeScreenshot();
  const path = join(reportDir, name);
  await writeFile(path, Buffer.from(data, "base64"));
  screenshots.push(path);
  pass(`screenshot ${basename(path)}`);
}

async function writeSummary(status, error = undefined) {
  summaryWritten = true;
  const logs = await summaryLogs();
  await writeFile(
    join(reportDir, "summary.json"),
    `${JSON.stringify(
      {
        status,
        generatedAt: new Date().toISOString(),
        reportDir,
        baseUrl,
        deviceName,
        mode: remoteControlMode
          ? shellFirstMode
            ? "remote-control-shell-first"
            : "remote-control-prestarted-workload"
          : "mock-host",
        remoteHost: remoteHostReady,
        screenshots,
        assertions,
        logs,
        error: error ? errStr(error) : undefined,
        lessonsBakedIn: [
          "Native lane uses Safari as an app first, avoiding brittle session-start webview attach.",
          "WDA warmup and long timeouts absorb first-run build/boot cost.",
          "Software keyboard is forced on so keyboard-mode regressions are visible.",
          "Host WebSocket logs remain the source of truth for terminal bytes and resize collapse.",
          "Screenshots are captured at connected, keyboard, overflow, and terminal-scroll milestones.",
          "The remote-control lane uses a real omw-remote host and verifies PTY output/input bytes via OMW_BYTE_DUMP and OMW_INPUT_DUMP.",
          "The shell-first lane covers Sessions -> Start a new shell -> run commands -> reconnect an existing shell -> launch Claude from the phone-started shell.",
          "The remote-control lane verifies Claude remains interactive with /help, then returns to shell and smokes Codex CLI without a model call.",
          "The lifecycle lane stops the active shell from Sessions and proves a fresh shell can start afterward.",
        ],
      },
      null,
      2,
    )}\n`,
  );
}

async function summaryLogs() {
  if (!baseUrl) return null;
  if (!remoteControlMode) {
    return getJson(`${baseUrl}/qa/logs`).catch((err) => lastQaLogs || { error: errStr(err) });
  }
  const text = await readByteDumpText();
  const inputBytes = await readInputDumpBytes();
  return {
    harnessOutputTail: remoteHostOutput.slice(-20_000),
    byteDumpBytes: await byteDumpSize(),
    inputDumpBytes: inputBytes.length,
    inputDumpHexTail: inputBytes.subarray(-4000).toString("hex"),
    inputDumpTextTail: inputBytes.subarray(-4000).toString("utf8"),
    byteDumpTextTail: text.slice(-4000),
  };
}

async function cleanup({ appium, host, device }) {
  if (driver) {
    await driver.deleteSession().catch(() => {});
    driver = null;
  }
  if (!keepOpen) {
    stopChild(appium);
    stopChild(host);
    await cleanupWdaBuildProcesses();
    if (device?.udid) {
      await run("xcrun", ["simctl", "shutdown", device.udid], {
        allowFailure: true,
        timeoutMs: 20_000,
      });
    }
  } else {
    console.log(`Keeping native QA stack open. Report: ${reportDir}`);
  }
}

async function cleanupWdaBuildProcesses() {
  await run(
    "/usr/bin/pkill",
    ["-f", `${repoRoot}/.tmp/appium/node_modules/appium-xcuitest-driver/node_modules/appium-webdriveragent/WebDriverAgent.xcodeproj`],
    {
      allowFailure: true,
      timeoutMs: 5000,
    },
  );
}

async function resolveRuntime() {
  const data = JSON.parse(await capture("xcrun", ["simctl", "list", "--json", "runtimes"]));
  const runtimes = (data.runtimes || [])
    .filter((runtime) => runtime.isAvailable && runtime.identifier.includes("SimRuntime.iOS"))
    .sort((a, b) => (a.version < b.version ? 1 : -1));
  if (requestedRuntime) {
    const found = runtimes.find(
      (runtime) =>
        runtime.identifier === requestedRuntime ||
        runtime.name === requestedRuntime ||
        runtime.version === requestedRuntime,
    );
    if (!found) fail("requested iOS runtime exists", requestedRuntime);
    return normalizeRuntime(found);
  }
  if (runtimes.length === 0) {
    fail(
      "iOS Simulator runtime exists",
      "install one with xcodebuild -downloadPlatform iOS",
    );
  }
  return normalizeRuntime(runtimes[0]);
}

function normalizeRuntime(runtime) {
  const version = runtime.version || runtime.name?.match(/\d+(?:\.\d+)*/)?.[0] || "";
  return {
    ...runtime,
    version,
  };
}

async function ensureSimulatorDevice(runtimeId) {
  const existing = await findSimulatorDevice(deviceName);
  if (existing) return existing;
  const udid = (
    await capture("xcrun", ["simctl", "create", deviceName, deviceType, runtimeId])
  ).trim();
  if (!udid) fail("created iOS simulator device", deviceName);
  return { name: deviceName, udid, state: "Shutdown" };
}

async function findSimulatorDevice(name) {
  const data = JSON.parse(
    await capture("xcrun", ["simctl", "list", "--json", "devices", "available"]),
  );
  for (const devices of Object.values(data.devices || {})) {
    const found = devices.find((device) => device.name === name && device.isAvailable !== false);
    if (found) return found;
  }
  return null;
}

async function bootSimulator(udid) {
  await run("xcrun", ["simctl", "boot", udid], {
    allowFailure: true,
    timeoutMs: 30_000,
  });
  await run("/usr/bin/open", ["-Fn", simulatorApp, "--args", "-CurrentDeviceUDID", udid], {
    allowFailure: true,
    timeoutMs: 20_000,
  });
  await run("xcrun", ["simctl", "bootstatus", udid, "-b"], {
    timeoutMs: 300_000,
  });
}

async function configureSimulatorForTerminalQa(udid) {
  await run(
    "xcrun",
    ["simctl", "spawn", udid, "defaults", "write", "NSGlobalDomain", "AppleKeyboardPrediction", "-bool", "false"],
    { allowFailure: true },
  );
  await run(
    "xcrun",
    ["simctl", "spawn", udid, "defaults", "write", "NSGlobalDomain", "KeyboardAutocorrection", "-bool", "false"],
    { allowFailure: true },
  );
}

function sameBytes(actual, expected) {
  return (
    Array.isArray(actual) &&
    actual.length === expected.length &&
    actual.every((value, index) => value === expected[index])
  );
}

function longTerminalOutput() {
  const lines = [];
  for (let i = 1; i <= 80; i += 1) {
    lines.push(`ios qa scroll line ${String(i).padStart(2, "0")}`);
  }
  return `\r\n${lines.join("\r\n")}\r\n$ `;
}

async function waitFor(fn, timeoutMs, label) {
  const start = Date.now();
  let lastError;
  while (Date.now() - start < timeoutMs) {
    try {
      const value = await fn();
      if (value) return value;
    } catch (err) {
      lastError = err;
    }
    await delay(250);
  }
  throw new Error(
    `${label} timed out${lastError ? `; last error: ${errStr(lastError)}` : ""}`,
  );
}

async function waitForHttp(url, timeoutMs) {
  await waitFor(
    async () => {
      const res = await fetch(url).catch(() => null);
      return !!res && res.ok;
    },
    timeoutMs,
    `HTTP ${url}`,
  );
}

async function getJson(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url} returned ${res.status}`);
  const body = await res.json();
  if (url.endsWith("/qa/logs")) {
    lastQaLogs = body;
  }
  return body;
}

async function postJson(url, body) {
  const res = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`${url} returned ${res.status}`);
  return res.json();
}

async function freePort() {
  return new Promise((resolvePromise, reject) => {
    const server = createServer();
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = typeof address === "object" && address ? address.port : null;
      server.close(() => {
        if (port) resolvePromise(port);
        else reject(new Error("Could not allocate a free port"));
      });
    });
    server.on("error", reject);
  });
}

async function capture(command, args, options = {}) {
  const chunks = [];
  const errors = [];
  await run(command, args, {
    ...options,
    onStdout: (chunk) => chunks.push(chunk),
    onStderr: (chunk) => errors.push(chunk),
    quiet: true,
  });
  return Buffer.concat(chunks).toString("utf8") || Buffer.concat(errors).toString("utf8");
}

async function run(command, args, options = {}) {
  const {
    allowFailure = false,
    timeoutMs = 120_000,
    onStdout,
    onStderr,
    quiet = false,
  } = options;
  await new Promise((resolvePromise, reject) => {
    const child = spawn(command, args, {
      cwd: repoRoot,
      env: process.env,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let timedOut = false;
    const timeout = setTimeout(() => {
      timedOut = true;
      child.kill("SIGTERM");
    }, timeoutMs);
    child.stdout.on("data", (chunk) => {
      onStdout?.(chunk);
      if (!quiet) process.stdout.write(chunk);
    });
    child.stderr.on("data", (chunk) => {
      onStderr?.(chunk);
      if (!quiet) process.stderr.write(chunk);
    });
    child.on("error", (err) => {
      clearTimeout(timeout);
      if (allowFailure) resolvePromise();
      else reject(err);
    });
    child.on("exit", (code, signal) => {
      clearTimeout(timeout);
      if (timedOut) {
        const err = new Error(`${command} ${args.join(" ")} timed out after ${timeoutMs}ms`);
        if (allowFailure) resolvePromise();
        else reject(err);
        return;
      }
      if (code === 0 || allowFailure) resolvePromise();
      else reject(new Error(`${command} ${args.join(" ")} failed (${code ?? signal})`));
    });
  });
}

function startChild(command, args, { env, label, onOutput }) {
  const child = spawn(command, args, {
    cwd: repoRoot,
    env,
    stdio: ["ignore", "pipe", "pipe"],
  });
  children.push(child);
  child.stdout.on("data", (chunk) => {
    onOutput?.(chunk.toString("utf8"));
    process.stdout.write(`[${label}] ${chunk}`);
  });
  child.stderr.on("data", (chunk) => {
    onOutput?.(chunk.toString("utf8"));
    process.stderr.write(`[${label}] ${chunk}`);
  });
  return child;
}

function stopChild(child) {
  if (!child || child.killed || child.exitCode !== null) return;
  child.kill("SIGTERM");
}

function safeName(value) {
  return value.replace(/[^a-z0-9]+/gi, "-").replace(/^-|-$/g, "").toLowerCase();
}

function errStr(err) {
  return err instanceof Error ? err.message : String(err);
}

process.on("SIGINT", async () => {
  for (const child of children) stopChild(child);
  if (driver) await driver.deleteSession().catch(() => {});
  process.exit(130);
});

try {
  await main();
} catch (err) {
  console.error("");
  console.error(`Mobile iOS auto QA failed: ${errStr(err)}`);
  if (!summaryWritten) {
    await writeSummary("fail", err).catch(() => {});
  }
  for (const child of children) stopChild(child);
  if (driver) await driver.deleteSession().catch(() => {});
  process.exitCode = 1;
}
