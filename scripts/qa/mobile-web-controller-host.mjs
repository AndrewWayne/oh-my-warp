#!/usr/bin/env node
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { WebSocketServer } from "ws";
import * as ed from "@noble/ed25519";
import { sha512 } from "@noble/hashes/sha512";

ed.etc.sha512Async = (...m) => Promise.resolve(sha512(ed.etc.concatBytes(...m)));
ed.etc.sha512Sync = (...m) => sha512(ed.etc.concatBytes(...m));

const scriptDir = fileURLToPath(new URL(".", import.meta.url));
const repoRoot = resolve(scriptDir, "../..");
const port = Number(process.env.OMW_QA_MOCK_PORT || "8787");
const bindHost = process.env.OMW_QA_MOCK_BIND || "0.0.0.0";
const publicBaseUrl =
  process.env.OMW_QA_PUBLIC_BASE_URL || `http://127.0.0.1:${port}`;
const distDir =
  process.env.OMW_QA_WEB_DIST ||
  join(repoRoot, "apps/web-controller/dist");

const pairToken = "ABCD1234";
const hostId = "qa-host";
const hostName = "QA Host";
const sessionId = "11111111-1111-4111-8111-111111111111";
const hostPriv = ed.utils.randomPrivateKey();
const hostPub = await ed.getPublicKeyAsync(hostPriv);
const startedAt = new Date().toISOString();
const logs = [];
const clients = new Set();

function b64u(bytes) {
  return Buffer.from(bytes).toString("base64url");
}

function unb64u(s) {
  return new Uint8Array(Buffer.from(s, "base64url"));
}

function text(bytes) {
  return new TextDecoder().decode(bytes);
}

function canonicalFrame({ v, seq, ts, kind, payload }) {
  const s =
    "{" +
    `"kind":${JSON.stringify(kind)}` +
    `,"payload":${JSON.stringify(b64u(payload))}` +
    `,"seq":${seq}` +
    `,"ts":${JSON.stringify(ts)}` +
    `,"v":${v}` +
    "}";
  return new TextEncoder().encode(s);
}

let inboundSeq = 0;
async function signedFrame(kind, payload) {
  const frame = {
    v: 1,
    seq: inboundSeq++,
    ts: new Date().toISOString(),
    kind,
    payload,
  };
  const sig = await ed.signAsync(canonicalFrame(frame), hostPriv);
  return JSON.stringify({ ...frame, payload: b64u(payload), sig: b64u(sig) });
}

async function readBody(req) {
  const chunks = [];
  for await (const chunk of req) chunks.push(chunk);
  const raw = Buffer.concat(chunks).toString("utf8");
  return raw ? JSON.parse(raw) : {};
}

function cors(res) {
  res.setHeader("Access-Control-Allow-Origin", "*");
  res.setHeader("Access-Control-Allow-Methods", "GET,POST,DELETE,OPTIONS");
  res.setHeader(
    "Access-Control-Allow-Headers",
    "Authorization,Content-Type,X-Omw-Signature,X-Omw-Nonce,X-Omw-Ts,X-Omw-Protocol-Version",
  );
}

function json(res, status, body) {
  cors(res);
  res.writeHead(status, { "Content-Type": "application/json" });
  res.end(JSON.stringify(body));
}

async function broadcastOutput(output) {
  const payload = new TextEncoder().encode(output);
  const frame = await signedFrame("output", payload);
  for (const ws of clients) {
    if (ws.readyState === ws.OPEN) {
      ws.send(frame);
    }
  }
}

const contentTypes = new Map([
  [".html", "text/html; charset=utf-8"],
  [".js", "text/javascript; charset=utf-8"],
  [".css", "text/css; charset=utf-8"],
  [".json", "application/json; charset=utf-8"],
  [".svg", "image/svg+xml"],
  [".png", "image/png"],
  [".ico", "image/x-icon"],
  [".webmanifest", "application/manifest+json"],
]);

async function serveStatic(reqPath, res) {
  const normalized = normalize(reqPath === "/" ? "/index.html" : reqPath)
    .replace(/^(\.\.(\/|\\|$))+/, "");
  const abs = join(distDir, normalized);
  try {
    const body = await readFile(abs);
    const type = contentTypes.get(extname(abs)) || "application/octet-stream";
    res.writeHead(200, { "Content-Type": type });
    res.end(body);
    return true;
  } catch {
    try {
      const body = await readFile(join(distDir, "index.html"));
      res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
      res.end(body);
      return true;
    } catch {
      return false;
    }
  }
}

const server = createServer(async (req, res) => {
  cors(res);
  if (req.method === "OPTIONS") {
    res.writeHead(204);
    res.end();
    return;
  }

  const url = new URL(req.url || "/", publicBaseUrl);
  logs.push({
    at: new Date().toISOString(),
    type: "http",
    method: req.method,
    path: url.pathname,
  });

  if (req.method === "GET" && url.pathname === "/api/v1/host-info") {
    json(res, 200, { v: 1, host_id: hostId, host_name: hostName });
    return;
  }

  if (req.method === "POST" && url.pathname === "/api/v1/pair/redeem") {
    const body = await readBody(req);
    logs.push({ at: new Date().toISOString(), type: "pair-redeem", body });
    json(res, 200, {
      v: 1,
      device_id: "device-qa",
      capabilities: ["pty:read", "pty:write"],
      capability_token: "QA_CAP_TOKEN",
      host_pubkey: b64u(hostPub),
      host_name: hostName,
      host_id: hostId,
      issued_at: startedAt,
      expires_at: new Date(Date.now() + 10 * 60 * 1000).toISOString(),
    });
    return;
  }

  if (req.method === "GET" && url.pathname === "/api/v1/sessions") {
    json(res, 200, {
      sessions: [
        {
          id: sessionId,
          name: "qa-shell",
          created_at: startedAt,
          alive: true,
        },
      ],
    });
    return;
  }

  if (req.method === "POST" && url.pathname === "/api/v1/sessions") {
    json(res, 200, { id: sessionId });
    return;
  }

  if (req.method === "DELETE" && url.pathname.startsWith("/api/v1/sessions/")) {
    res.writeHead(204);
    res.end();
    return;
  }

  if (req.method === "GET" && url.pathname === "/qa/logs") {
    json(res, 200, { logs });
    return;
  }

  if (req.method === "POST" && url.pathname === "/qa/reset") {
    logs.length = 0;
    json(res, 200, { ok: true });
    return;
  }

  if (req.method === "POST" && url.pathname === "/qa/output") {
    const body = await readBody(req);
    const output = typeof body.text === "string" ? body.text : "";
    logs.push({
      at: new Date().toISOString(),
      type: "qa-output",
      bytes: Buffer.byteLength(output, "utf8"),
    });
    await broadcastOutput(output);
    json(res, 200, { ok: true, clients: clients.size });
    return;
  }

  if (
    req.method === "GET" &&
    (url.pathname === "/" || url.pathname.startsWith("/terminal"))
  ) {
    res.writeHead(302, { Location: `/pair?t=${pairToken}` });
    res.end();
    return;
  }

  if (req.method === "GET" && (await serveStatic(url.pathname, res))) {
    return;
  }

  json(res, 404, { error: "not_found", path: url.pathname });
});

const wss = new WebSocketServer({ server, path: `/ws/v1/pty/${sessionId}` });
wss.on("connection", async (ws, req) => {
  clients.add(ws);
  logs.push({ at: new Date().toISOString(), type: "ws-open", url: req.url });

  ws.send(
    await signedFrame(
      "control",
      new TextEncoder().encode(JSON.stringify({ type: "size", rows: 40, cols: 120 })),
    ),
  );
  ws.send(await signedFrame("output", new TextEncoder().encode("QA mock shell ready\r\n$ ")));

  ws.on("message", async (raw) => {
    const wire = raw.toString();
    try {
      const frame = JSON.parse(wire);
      const payload = unb64u(frame.payload || "");
      logs.push({
        at: new Date().toISOString(),
        type: "ws-frame",
        kind: frame.kind,
        seq: frame.seq,
        bytes: Array.from(payload),
        text: text(payload),
      });
      if (frame.kind === "input") {
        ws.send(await signedFrame("output", payload));
      }
      if (frame.kind === "control") {
        ws.send(await signedFrame("output", new TextEncoder().encode("\r\n[resized]\r\n$ ")));
      }
    } catch (err) {
      logs.push({ at: new Date().toISOString(), type: "ws-bad-frame", error: String(err) });
    }
  });

  ws.on("close", (code, reason) => {
    clients.delete(ws);
    logs.push({ at: new Date().toISOString(), type: "ws-close", code, reason: reason.toString() });
  });
});

server.listen(port, bindHost, () => {
  console.log(`omw mobile QA host listening on ${bindHost}:${port}`);
  console.log(`serving Web Controller dist: ${distDir}`);
  console.log(`phone URL: ${publicBaseUrl}/pair?t=${pairToken}`);
  console.log(`logs: ${publicBaseUrl}/qa/logs`);
  console.log("");
  console.log("Before running this host, build the current branch:");
  console.log("  npm run build --workspace @oh-my-warp/web-controller");
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => {
    server.close(() => process.exit(0));
  });
}
