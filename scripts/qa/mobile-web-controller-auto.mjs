#!/usr/bin/env node
import { spawn } from "node:child_process";
import { createServer } from "node:net";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = fileURLToPath(new URL(".", import.meta.url));
const repoRoot = resolve(scriptDir, "../..");
const hostScript = join(repoRoot, "scripts/qa/mobile-web-controller-host.mjs");
const distDir = join(repoRoot, "apps/web-controller/dist");
const pairToken = "ABCD1234";
const expectedSessionId = "11111111-1111-4111-8111-111111111111";
const mobileViewportWidth = 375;
const mobileViewportHeight = 844;

const now = new Date();
const stamp = now.toISOString().replace(/[:.]/g, "-");
const defaultReportDir = join(
  repoRoot,
  ".gstack/qa-reports",
  `mobile-web-mock-${stamp}`,
);
const reportDir = resolve(process.env.OMW_QA_REPORT_DIR || defaultReportDir);
const skipBuild = process.env.OMW_QA_SKIP_BUILD === "1";
const keepOpen = process.argv.includes("--keep-open");
const headed = process.argv.includes("--headed") || process.env.OMW_QA_HEADED === "1";
const chromePath = process.env.CHROME_PATH || findChrome();

const children = [];
const screenshots = [];
const assertions = [];

function pass(name, details = undefined) {
  assertions.push({ name, status: "pass", details });
  console.log(`PASS ${name}${details ? ` - ${details}` : ""}`);
}

function fail(name, details = undefined) {
  assertions.push({ name, status: "fail", details });
  throw new Error(`${name}${details ? `: ${details}` : ""}`);
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function main() {
  if (!chromePath) {
    throw new Error(
      "Chrome was not found. Set CHROME_PATH to a Chrome or Chromium binary.",
    );
  }

  await mkdir(reportDir, { recursive: true });

  if (!skipBuild) {
    await run("npm", ["run", "build", "--workspace", "@oh-my-warp/web-controller"]);
  }

  if (!existsSync(join(distDir, "index.html"))) {
    throw new Error(`Web Controller dist is missing: ${distDir}`);
  }

  const hostPort = await freePort();
  const debugPort = await freePort();
  const baseUrl = `http://127.0.0.1:${hostPort}`;
  const host = startChild(process.execPath, [hostScript], {
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

  const userDataDir = await mkdtemp(join(tmpdir(), "omw-mobile-web-qa-chrome-"));
  const chromeArgs = [
    `--remote-debugging-port=${debugPort}`,
    `--user-data-dir=${userDataDir}`,
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-background-networking",
    "--disable-extensions",
    "--disable-features=Translate,MediaRouter",
    `--window-size=${mobileViewportWidth},${mobileViewportHeight}`,
    "about:blank",
  ];
  if (!headed) chromeArgs.unshift("--headless=new");

  const chrome = startChild(chromePath, chromeArgs, {
    env: process.env,
    label: "chrome",
  });

  try {
    const target = await waitForChromeTarget(debugPort, 10_000);
    const cdp = await connectCdp(target.webSocketDebuggerUrl);
    try {
      await setupMobilePage(cdp);

      const pairUrl = `${baseUrl}/pair?t=${pairToken}`;
      await cdp.send("Page.navigate", { url: pairUrl });
      await cdp.send("Page.bringToFront");

      await waitFor(
        async () => {
          const state = await evaluate(cdp, pageStateExpression());
          return state.path.startsWith("/terminal/") && state.status === "connected";
        },
        20_000,
        "terminal connected after pair flow",
      );
      pass("pair flow auto-opens the live terminal");
      await screenshot(cdp, "01-terminal-connected.png");

      const connectedState = await evaluate(cdp, pageStateExpression());
      if (connectedState.path !== `/terminal/qa-host/${expectedSessionId}`) {
        fail("terminal route is the expected session", connectedState.path);
      }
      pass("terminal route is the expected session");

      await assertNoHorizontalOverflow(cdp);
      await assertShortcutStripVisible(cdp);
      await assertPrimaryShortcutRowFits(cdp);

      await tapSelector(cdp, '[data-testid="xterm-container"]');
      await delay(100);
      await cdp.send("Input.insertText", { text: "echo qa-auto\r" });
      await delay(250);
      pass("normal terminal text and Return were sent");

      for (const label of [
        "shift tab",
        "esc",
        "tab",
        "^C",
        "arrow up",
        "arrow down",
        "arrow left",
        "arrow right",
      ]) {
        await tapButton(cdp, label);
      }
      await tapButton(cdp, "show extra shortcuts");
      await waitForVisible(cdp, '[data-testid="terminal-shortcut-overflow"]');
      await screenshot(cdp, "02-more-drawer.png");
      for (const label of ["^D", "^L", "/", "|", "?"]) {
        await tapButton(cdp, label);
      }
      pass("primary and overflow terminal shortcuts were tapped");

      await emulateKeyboard(cdp, {
        height: 520,
        width: mobileViewportWidth,
        offsetTop: 0,
        offsetLeft: 0,
      });
      await waitFor(
        async () => {
          const dock = await evaluate(cdp, dockStateExpression());
          return dock.position === "fixed" && dock.bottom <= 528 && dock.bottom > 470;
        },
        5_000,
        "shortcut strip docks to simulated keyboard edge",
      );
      pass("shortcut strip docks to the visual viewport when keyboard is open");
      await screenshot(cdp, "03-keyboard-docked.png");

      await emulateKeyboard(cdp, {
        height: 520,
        width: mobileViewportWidth,
        offsetTop: -180,
        offsetLeft: 0,
      });
      await waitFor(
        async () => {
          const dock = await evaluate(cdp, dockStateExpression());
          return dock.position === "fixed" && dock.bottom <= 528 && dock.bottom > 470;
        },
        5_000,
        "shortcut strip ignores negative iOS rubber-band visual viewport offset",
      );
      pass("shortcut strip stays docked through iOS rubber-band viewport offsets");

      await evaluate(cdp, `(() => {
        if (document.activeElement instanceof HTMLElement) {
          document.activeElement.blur();
        }
        return true;
      })()`);
      await emulateKeyboard(cdp, {
        height: mobileViewportHeight,
        width: mobileViewportWidth,
        offsetTop: 0,
        offsetLeft: 0,
      });
      await waitFor(
        async () => {
          const dock = await evaluate(cdp, dockStateExpression());
          return dock.position !== "fixed";
        },
        5_000,
        "shortcut strip returns to normal dock when keyboard is closed",
      );
      pass("shortcut strip returns when keyboard is closed");

      await postJson(`${baseUrl}/qa/output`, {
        text: longTerminalOutput(),
      });
      await waitFor(
        async () => {
          const metrics = await evaluate(cdp, xtermScrollExpression());
          return metrics.scrollHeight > metrics.clientHeight + 20;
        },
        5_000,
        "terminal has scrollback after injected output",
      );
      const beforeScroll = await evaluate(cdp, xtermScrollExpression());
      await swipeSelector(cdp, '[data-testid="xterm-container"]', {
        startYRatio: 0.45,
        endYRatio: 0.82,
      });
      await delay(300);
      const afterScroll = await evaluate(cdp, xtermScrollExpression());
      if (afterScroll.scrollTop >= beforeScroll.scrollTop) {
        fail(
          "touch scroll moves terminal scrollback",
          `before=${beforeScroll.scrollTop} after=${afterScroll.scrollTop}`,
        );
      }
      pass(
        "touch scroll moves terminal scrollback",
        `before=${beforeScroll.scrollTop} after=${afterScroll.scrollTop}`,
      );
      await screenshot(cdp, "04-scrolled-terminal.png");

      await tapSelector(cdp, '[data-testid="terminal-back-button"]');
      await waitFor(
        async () => {
          const state = await evaluate(cdp, pageStateExpression());
          return state.path === "/host/qa-host";
        },
        10_000,
        "back button reaches Sessions",
      );
      pass("terminal back button reaches Sessions without bounce");
      await tapButtonByText(cdp, "Open");
      await waitFor(
        async () => {
          const state = await evaluate(cdp, pageStateExpression());
          return state.path.startsWith("/terminal/") && state.status === "connected";
        },
        10_000,
        "Open returns to terminal",
      );
      pass("Sessions Open returns to terminal");

      await validateHostLogs(baseUrl);
      await writeSummary(baseUrl, "pass");
      console.log("");
      console.log(`Mobile web auto QA passed. Report: ${reportDir}`);
    } finally {
      cdp.close();
    }
  } finally {
    if (!keepOpen) {
      await stopChild(chrome);
      await stopChild(host);
      await rm(userDataDir, {
        recursive: true,
        force: true,
        maxRetries: 3,
        retryDelay: 100,
      });
    } else {
      console.log(`Keeping host and Chrome open. Report: ${reportDir}`);
    }
  }
}

async function setupMobilePage(cdp) {
  await cdp.send("Page.enable");
  await cdp.send("Runtime.enable");
  await cdp.send("Network.enable");
  await cdp.send("Page.addScriptToEvaluateOnNewDocument", {
    source: visualViewportQaShim(),
  });
  await cdp.send("Network.setUserAgentOverride", {
    userAgent:
      "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1",
    platform: "iPhone",
  });
  await cdp.send("Emulation.setDeviceMetricsOverride", {
    width: mobileViewportWidth,
    height: mobileViewportHeight,
    screenWidth: mobileViewportWidth,
    screenHeight: mobileViewportHeight,
    deviceScaleFactor: 3,
    mobile: true,
  });
  await cdp.send("Emulation.setTouchEmulationEnabled", {
    enabled: true,
    maxTouchPoints: 5,
  });
}

function visualViewportQaShim() {
  return `
(() => {
  const eventTarget = new EventTarget();
  let state = {
    width: ${mobileViewportWidth},
    height: ${mobileViewportHeight},
    offsetLeft: 0,
    offsetTop: 0,
    pageLeft: 0,
    pageTop: 0,
    scale: 1,
  };
  const visualViewport = {
    get width() { return state.width; },
    get height() { return state.height; },
    get offsetLeft() { return state.offsetLeft; },
    get offsetTop() { return state.offsetTop; },
    get pageLeft() { return state.pageLeft; },
    get pageTop() { return state.pageTop; },
    get scale() { return state.scale; },
    addEventListener: eventTarget.addEventListener.bind(eventTarget),
    removeEventListener: eventTarget.removeEventListener.bind(eventTarget),
    dispatchEvent: eventTarget.dispatchEvent.bind(eventTarget),
  };
  try {
    Object.defineProperty(window, "visualViewport", {
      configurable: true,
      value: visualViewport,
    });
  } catch {
    return;
  }
  window.__omwQaSetVisualViewport = (next) => {
    state = { ...state, ...next };
    eventTarget.dispatchEvent(new Event("resize"));
    eventTarget.dispatchEvent(new Event("scroll"));
    window.dispatchEvent(new Event("resize"));
  };
})();
`;
}

async function assertNoHorizontalOverflow(cdp) {
  const overflow = await evaluate(cdp, `(() => {
    const root = document.scrollingElement || document.documentElement;
    return {
      innerWidth: window.innerWidth,
      scrollWidth: root.scrollWidth,
      bodyScrollWidth: document.body.scrollWidth,
    };
  })()`);
  if (overflow.scrollWidth > overflow.innerWidth + 1) {
    fail("page has no horizontal overflow", JSON.stringify(overflow));
  }
  pass("page has no horizontal overflow", `${overflow.scrollWidth}/${overflow.innerWidth}`);
}

async function assertShortcutStripVisible(cdp) {
  const rect = await rectForSelector(cdp, '[data-testid="terminal-shortcut-strip"]');
  if (!rect || rect.height < 40 || rect.bottom <= 0) {
    fail("shortcut strip is visible", JSON.stringify(rect));
  }
  pass("shortcut strip is visible", `${Math.round(rect.width)}x${Math.round(rect.height)}`);
}

async function assertPrimaryShortcutRowFits(cdp) {
  const metrics = await evaluate(cdp, `(() => {
    const row = document.querySelector('[data-testid="terminal-shortcut-primary-row"]');
    if (!row) return null;
    return {
      clientWidth: row.clientWidth,
      scrollWidth: row.scrollWidth,
      buttonCount: row.querySelectorAll("button").length,
    };
  })()`);
  if (!metrics) fail("primary shortcut row exists");
  if (metrics.scrollWidth > metrics.clientWidth + 1) {
    fail("primary shortcut row fits 375px phones", JSON.stringify(metrics));
  }
  pass(
    "primary shortcut row fits 375px phones",
    `${metrics.scrollWidth}/${metrics.clientWidth} with ${metrics.buttonCount} buttons`,
  );
}

async function validateHostLogs(baseUrl) {
  const { logs } = await getJson(`${baseUrl}/qa/logs`);
  const wsOpenCount = logs.filter((entry) => entry.type === "ws-open").length;
  if (wsOpenCount < 2) {
    fail("host saw terminal WebSocket connects", `count=${wsOpenCount}`);
  }
  pass("host saw terminal WebSocket connects", `count=${wsOpenCount}`);

  const pairRedeem = logs.find((entry) => entry.type === "pair-redeem");
  if (!pairRedeem) fail("host saw pair redeem");
  pass(
    "host saw pair redeem",
    `${pairRedeem.body?.device_name || "unknown device"} / ${pairRedeem.body?.platform || "unknown platform"}`,
  );

  const inputFrames = logs.filter((entry) => entry.type === "ws-frame" && entry.kind === "input");
  const inputText = inputFrames.map((entry) => entry.text || "").join("");
  if (!inputText.includes("echo qa-auto")) {
    fail("host saw normal terminal typing", JSON.stringify(inputFrames.map((entry) => entry.text)));
  }
  pass("host saw normal terminal typing", "echo qa-auto");

  const expected = new Map([
    ["Shift-Tab", [27, 91, 90]],
    ["Esc", [27]],
    ["Tab", [9]],
    ["Ctrl-C", [3]],
    ["Up", [27, 91, 65]],
    ["Down", [27, 91, 66]],
    ["Left", [27, 91, 68]],
    ["Right", [27, 91, 67]],
    ["Ctrl-D", [4]],
    ["Ctrl-L", [12]],
    ["Slash", [47]],
    ["Pipe", [124]],
    ["Question", [63]],
  ]);
  for (const [name, bytes] of expected) {
    const seen = inputFrames.some((entry) => sameBytes(entry.bytes, bytes));
    if (!seen) fail(`host saw ${name} shortcut bytes`, JSON.stringify(bytes));
  }
  pass("host saw every expected shortcut byte sequence");

  const resizeFrames = logs
    .filter((entry) => entry.type === "ws-frame" && entry.kind === "control")
    .map((entry) => {
      try {
        return JSON.parse(entry.text);
      } catch {
        return null;
      }
    })
    .filter((entry) => entry?.type === "resize");
  const tiny = resizeFrames.filter((entry) => entry.rows < 8 || entry.cols < 20);
  if (tiny.length > 0) {
    fail("host saw no tiny resize frames", JSON.stringify(tiny));
  }
  pass(
    "host saw no tiny resize frames",
    resizeFrames.length === 0
      ? "no resize needed"
      : `min=${Math.min(...resizeFrames.map((entry) => entry.rows))} rows`,
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
  for (let i = 1; i <= 90; i += 1) {
    lines.push(`qa scroll line ${String(i).padStart(2, "0")}`);
  }
  return `\r\n${lines.join("\r\n")}\r\n$ `;
}

async function writeSummary(baseUrl, status) {
  const logs = await getJson(`${baseUrl}/qa/logs`).catch((err) => ({
    error: String(err),
  }));
  const summary = {
    status,
    generatedAt: new Date().toISOString(),
    baseUrl,
    reportDir,
    screenshots,
    assertions,
    logs,
    limitations: [
      "This lane drives mobile browser touch, viewport, visualViewport, xterm, and WebSocket behavior in local Chrome.",
      "It does not exercise the native iOS software keyboard or browser-owned accessory row; use the Appium/real-device lane for that.",
    ],
  };
  await writeFile(
    join(reportDir, "summary.json"),
    `${JSON.stringify(summary, null, 2)}\n`,
  );
}

async function screenshot(cdp, name) {
  const result = await cdp.send("Page.captureScreenshot", {
    format: "png",
    captureBeyondViewport: false,
  });
  const path = join(reportDir, name);
  await writeFile(path, Buffer.from(result.data, "base64"));
  screenshots.push(path);
  pass(`screenshot ${basename(path)}`);
}

async function emulateKeyboard(cdp, state) {
  await evaluate(
    cdp,
    `window.__omwQaSetVisualViewport(${JSON.stringify(state)})`,
  );
  await delay(260);
}

async function waitForVisible(cdp, selector) {
  await waitFor(
    async () => {
      const rect = await rectForSelector(cdp, selector);
      return !!rect && rect.width > 0 && rect.height > 0;
    },
    5_000,
    `${selector} visible`,
  );
}

async function tapSelector(cdp, selector) {
  const rect = await waitForRect(cdp, selector);
  await tapPoint(cdp, rect.left + rect.width / 2, rect.top + rect.height / 2);
}

async function tapButton(cdp, ariaLabel) {
  const rect = await waitFor(
    () => rectForButton(cdp, { ariaLabel }),
    5_000,
    `button ${ariaLabel} exists`,
  );
  await tapPoint(cdp, rect.left + rect.width / 2, rect.top + rect.height / 2);
  await delay(140);
}

async function tapButtonByText(cdp, text) {
  const rect = await waitFor(
    () => rectForButton(cdp, { text }),
    5_000,
    `button ${text} exists`,
  );
  await tapPoint(cdp, rect.left + rect.width / 2, rect.top + rect.height / 2);
  await delay(140);
}

async function swipeSelector(cdp, selector, options) {
  const rect = await waitForRect(cdp, selector);
  const x = rect.left + rect.width / 2;
  const startY = rect.top + rect.height * options.startYRatio;
  const endY = rect.top + rect.height * options.endYRatio;
  await cdp.send("Input.dispatchTouchEvent", {
    type: "touchStart",
    touchPoints: [{ x, y: startY, radiusX: 2, radiusY: 2, force: 1 }],
  });
  const steps = 8;
  for (let i = 1; i <= steps; i += 1) {
    const y = startY + ((endY - startY) * i) / steps;
    await cdp.send("Input.dispatchTouchEvent", {
      type: "touchMove",
      touchPoints: [{ x, y, radiusX: 2, radiusY: 2, force: 1 }],
    });
    await delay(16);
  }
  await cdp.send("Input.dispatchTouchEvent", {
    type: "touchEnd",
    touchPoints: [],
  });
}

async function tapPoint(cdp, x, y) {
  await cdp.send("Input.dispatchTouchEvent", {
    type: "touchStart",
    touchPoints: [{ x, y, radiusX: 2, radiusY: 2, force: 1 }],
  });
  await delay(40);
  await cdp.send("Input.dispatchTouchEvent", {
    type: "touchEnd",
    touchPoints: [],
  });
}

async function waitForRect(cdp, selector) {
  return waitFor(
    () => rectForSelector(cdp, selector),
    5_000,
    `${selector} exists`,
  );
}

async function rectForSelector(cdp, selector) {
  return evaluate(
    cdp,
    `(() => {
      const el = document.querySelector(${JSON.stringify(selector)});
      if (!el) return null;
      const rect = el.getBoundingClientRect();
      return {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
        width: rect.width,
        height: rect.height,
      };
    })()`,
  );
}

async function rectForButton(cdp, { ariaLabel, text }) {
  return evaluate(
    cdp,
    `(() => {
      const ariaLabel = ${JSON.stringify(ariaLabel || null)};
      const text = ${JSON.stringify(text || null)};
      const el = Array.from(document.querySelectorAll("button, a")).find((candidate) => {
        if (ariaLabel && candidate.getAttribute("aria-label") === ariaLabel) return true;
        if (text && candidate.textContent.trim() === text) return true;
        return false;
      });
      if (!el) return null;
      const rect = el.getBoundingClientRect();
      return {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
        width: rect.width,
        height: rect.height,
      };
    })()`,
  );
}

function pageStateExpression() {
  return `(() => ({
    path: window.location.pathname,
    search: window.location.search,
    status: document.querySelector('[data-testid="conn-status"]')?.textContent?.trim() || null,
    title: document.title,
  }))()`;
}

function dockStateExpression() {
  return `(() => {
    const el = document.querySelector('[data-testid="terminal-shortcut-surface"]');
    if (!el) return null;
    const rect = el.getBoundingClientRect();
    const style = getComputedStyle(el);
    return {
      position: style.position,
      transform: style.transform,
      top: rect.top,
      bottom: rect.bottom,
      height: rect.height,
      width: rect.width,
      viewportHeight: window.innerHeight,
      visualHeight: window.visualViewport?.height || null,
    };
  })()`;
}

function xtermScrollExpression() {
  return `(() => {
    const viewport = document.querySelector('.xterm-viewport');
    if (!viewport) return { scrollTop: 0, scrollHeight: 0, clientHeight: 0 };
    return {
      scrollTop: viewport.scrollTop,
      scrollHeight: viewport.scrollHeight,
      clientHeight: viewport.clientHeight,
    };
  })()`;
}

async function evaluate(cdp, expression) {
  const result = await cdp.send("Runtime.evaluate", {
    expression,
    awaitPromise: true,
    returnByValue: true,
  });
  if (result.exceptionDetails) {
    throw new Error(
      result.exceptionDetails.text ||
        result.exceptionDetails.exception?.description ||
        "Runtime.evaluate failed",
    );
  }
  return result.result?.value;
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
    await delay(100);
  }
  throw new Error(
    `${label} timed out${lastError ? `; last error: ${lastError.message}` : ""}`,
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

async function waitForChromeTarget(port, timeoutMs) {
  return waitFor(
    async () => {
      const targets = await getJson(`http://127.0.0.1:${port}/json/list`).catch(
        () => [],
      );
      return targets.find((target) => target.type === "page");
    },
    timeoutMs,
    "Chrome DevTools target",
  );
}

async function getJson(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url} returned ${res.status}`);
  return res.json();
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

function connectCdp(wsUrl) {
  const ws = new WebSocket(wsUrl);
  let nextId = 1;
  const pending = new Map();
  let closed = false;

  const openPromise = new Promise((resolve, reject) => {
    ws.addEventListener("open", resolve, { once: true });
    ws.addEventListener("error", reject, { once: true });
  });

  ws.addEventListener("message", (event) => {
    const data =
      typeof event.data === "string" ? event.data : Buffer.from(event.data).toString("utf8");
    const message = JSON.parse(data);
    if (!message.id) return;
    const callbacks = pending.get(message.id);
    if (!callbacks) return;
    pending.delete(message.id);
    if (message.error) callbacks.reject(new Error(message.error.message));
    else callbacks.resolve(message.result || {});
  });

  ws.addEventListener("close", () => {
    closed = true;
    for (const callbacks of pending.values()) {
      callbacks.reject(new Error("CDP WebSocket closed"));
    }
    pending.clear();
  });

  return openPromise.then(() => ({
    send(method, params = {}) {
      if (closed) return Promise.reject(new Error("CDP WebSocket is closed"));
      const id = nextId;
      nextId += 1;
      const promise = new Promise((resolve, reject) => {
        pending.set(id, { resolve, reject });
      });
      ws.send(JSON.stringify({ id, method, params }));
      return promise;
    },
    close() {
      ws.close();
    },
  }));
}

async function run(command, args) {
  await new Promise((resolvePromise, reject) => {
    const child = spawn(command, args, {
      cwd: repoRoot,
      stdio: "inherit",
      env: process.env,
    });
    child.on("error", reject);
    child.on("exit", (code, signal) => {
      if (code === 0) resolvePromise();
      else reject(new Error(`${command} ${args.join(" ")} failed (${code ?? signal})`));
    });
  });
}

function startChild(command, args, { env, label }) {
  const child = spawn(command, args, {
    cwd: repoRoot,
    env,
    stdio: ["ignore", "pipe", "pipe"],
  });
  children.push(child);
  child.stdout.on("data", (chunk) => {
    process.stdout.write(`[${label}] ${chunk}`);
  });
  child.stderr.on("data", (chunk) => {
    process.stderr.write(`[${label}] ${chunk}`);
  });
  child.on("exit", (code, signal) => {
    if (code !== 0 && code !== null) {
      process.stderr.write(`[${label}] exited with ${code}\n`);
    } else if (signal && signal !== "SIGTERM") {
      process.stderr.write(`[${label}] exited on ${signal}\n`);
    }
  });
  return child;
}

async function stopChild(child) {
  if (!child || child.killed || child.exitCode !== null) return;
  const exited = new Promise((resolvePromise) => {
    child.once("exit", resolvePromise);
  });
  child.kill("SIGTERM");
  await Promise.race([exited, delay(1500)]);
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

function findChrome() {
  const candidates = [
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
  ];
  return candidates.find((candidate) => existsSync(candidate)) || "";
}

process.on("SIGINT", () => {
  for (const child of children) stopChild(child);
  process.exit(130);
});

try {
  await main();
} catch (err) {
  console.error("");
  console.error(`Mobile web auto QA failed: ${err instanceof Error ? err.message : String(err)}`);
  try {
    await writeFile(
      join(reportDir, "summary.json"),
      `${JSON.stringify(
        {
          status: "fail",
          generatedAt: new Date().toISOString(),
          reportDir,
          screenshots,
          assertions,
          error: err instanceof Error ? err.stack || err.message : String(err),
        },
        null,
        2,
      )}\n`,
    );
  } catch {
    /* best effort */
  }
  for (const child of children) stopChild(child);
  process.exitCode = 1;
}
