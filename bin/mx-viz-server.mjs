#!/usr/bin/env node
/**
 * Disposable loopback dashboard server for mx-viz.sh.
 *
 * Internal usage:
 *   mx-viz-server.mjs --serve <root> <home> <state> <run-record> <token> <first-port>
 *
 * The server is GET-only, binds 127.0.0.1, walks 20 ports, refreshes the
 * canonical system snapshot only on demand, and serves artifacts only from
 * canonicalized data/ and docs/ roots.
 */

import crypto from "node:crypto";
import fs from "node:fs";
import fsp from "node:fs/promises";
import http from "node:http";
import path from "node:path";
import process from "node:process";
import { execFile } from "node:child_process";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);
const HOST = "127.0.0.1";
const PORT_COUNT = 20;
const VERSION = 1;
const MAX_OUTPUT_BYTES = 32 * 1024 * 1024;
const ALLOWED_ARTIFACT_ROOTS = new Set(["data", "docs"]);

function usage() {
  process.stderr.write(
    "usage: mx-viz-server.mjs --serve <root> <home> <state> <run-record> <token> <first-port>\n",
  );
}

function fail(message) {
  throw new Error(message);
}

function isWithin(parent, candidate) {
  const relative = path.relative(parent, candidate);
  return (
    relative === "" ||
    (!relative.startsWith(`..${path.sep}`) && relative !== ".." && !path.isAbsolute(relative))
  );
}

function parsePositiveInteger(name, fallback, maximum) {
  const value = Number(process.env[name] || fallback);
  if (!Number.isInteger(value) || value < 1 || value > maximum) {
    fail(`${name} must be an integer from 1 through ${maximum}`);
  }
  return value;
}

function parseRefreshSeconds() {
  const value = Number(process.env.MX_VIZ_REFRESH_SECS || "2");
  if (!Number.isFinite(value) || value < 0.1 || value > 300) {
    fail("MX_VIZ_REFRESH_SECS must be a number from 0.1 through 300");
  }
  return value;
}

function parseRecord(contents) {
  const record = {};
  for (const line of contents.split("\n")) {
    const separator = line.indexOf("=");
    if (separator > 0) record[line.slice(0, separator)] = line.slice(separator + 1);
  }
  return record;
}

function cleanupRunRecord(runRecord, token) {
  try {
    const record = parseRecord(fs.readFileSync(runRecord, "utf8"));
    if (record.token === token && record.pid === String(process.pid)) fs.unlinkSync(runRecord);
  } catch (error) {
    if (error.code !== "ENOENT") {
      process.stderr.write(`mx-viz-server: could not clean run record: ${error.message}\n`);
    }
  }
}

function contentType(file) {
  switch (path.extname(file).toLowerCase()) {
    case ".css": return "text/css; charset=utf-8";
    case ".gif": return "image/gif";
    case ".html": return "text/html; charset=utf-8";
    case ".jpeg":
    case ".jpg": return "image/jpeg";
    case ".js":
    case ".mjs": return "text/javascript; charset=utf-8";
    case ".json": return "application/json; charset=utf-8";
    case ".md":
    case ".txt": return "text/plain; charset=utf-8";
    case ".pdf": return "application/pdf";
    case ".png": return "image/png";
    case ".svg": return "image/svg+xml";
    case ".webp": return "image/webp";
    default: return "application/octet-stream";
  }
}

function setCommonHeaders(response) {
  response.setHeader("Cache-Control", "no-store");
  response.setHeader("Content-Security-Policy", [
    "default-src 'self'",
    "script-src 'self'",
    "style-src 'self'",
    "img-src 'self' data:",
    "font-src 'self' data:",
    "connect-src 'self'",
    "object-src 'none'",
    "base-uri 'none'",
    "frame-ancestors 'none'",
  ].join("; "));
  response.setHeader("Referrer-Policy", "no-referrer");
  response.setHeader("X-Content-Type-Options", "nosniff");
  response.setHeader("X-Frame-Options", "DENY");
}

function setArtifactHeaders(response) {
  response.setHeader("Content-Security-Policy", [
    "default-src 'none'",
    "script-src 'none'",
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data:",
    "font-src 'self' data:",
    "object-src 'none'",
    "base-uri 'none'",
    "form-action 'none'",
    "frame-ancestors 'self'",
  ].join("; "));
  response.setHeader("X-Frame-Options", "SAMEORIGIN");
}

function sendJson(response, status, value, extraHeaders = {}) {
  const body = `${JSON.stringify(value)}\n`;
  response.writeHead(status, {
    "Content-Type": "application/json; charset=utf-8",
    "Content-Length": Buffer.byteLength(body),
    ...extraHeaders,
  });
  response.end(body);
}

function encodeSegments(value) {
  return value.split(path.sep).filter(Boolean).map(encodeURIComponent).join("/");
}

async function artifactEntry(root, rootName, file, label, kind) {
  try {
    const realRoot = await fsp.realpath(path.join(root, rootName));
    const realFile = await fsp.realpath(file);
    const stat = await fsp.stat(realFile);
    if (!isWithin(root, realRoot) || !stat.isFile() || !isWithin(realRoot, realFile)) return null;
    const relative = path.relative(realRoot, realFile);
    return {
      root: rootName,
      path: relative.split(path.sep).join("/"),
      label,
      kind,
      url: `/artifact/${rootName}/${encodeSegments(relative)}`,
    };
  } catch {
    return null;
  }
}

async function collectArtifacts(root, snapshot) {
  const entries = [];
  const dataRoot = snapshot?.roots?.data;
  if (typeof dataRoot === "string") {
    const seen = new Set();
    for (const task of snapshot.tasks || []) {
      if (typeof task?.id !== "string") continue;
      for (const [name, kind] of [["plan.html", "task-plan"], ["brief.md", "brief"], ["report.md", "report"]]) {
        const file = path.join(dataRoot, task.id, name);
        const entry = await artifactEntry(root, "data", file, `${task.id}/${name}`, kind);
        if (entry && !seen.has(entry.url)) {
          seen.add(entry.url);
          entries.push(entry);
        }
      }
    }
    for (const report of snapshot.scout_reports || []) {
      if (typeof report?.path !== "string") continue;
      const entry = await artifactEntry(root, "data", report.path, `${report.id}/report.md`, "report");
      if (entry && !seen.has(entry.url)) {
        seen.add(entry.url);
        entries.push(entry);
      }
    }
  }
  return entries;
}

async function executeJson(command, args, environment, acceptedExitCodes = [0]) {
  let stdout;
  try {
    ({ stdout } = await execFileAsync(command, args, {
      env: environment,
      encoding: "utf8",
      maxBuffer: MAX_OUTPUT_BYTES,
      timeout: 60000,
    }));
  } catch (error) {
    if (!acceptedExitCodes.includes(Number(error.code)) || typeof error.stdout !== "string") throw error;
    stdout = error.stdout;
  }
  const trimmed = stdout.trim();
  if (!trimmed) fail(`${path.basename(command)} returned no JSON`);
  return { raw: trimmed, value: JSON.parse(trimmed) };
}

async function serve(rootArgument, homeArgument, stateArgument, runRecord, token, portArgument) {
  const root = await fsp.realpath(rootArgument);
  const home = await fsp.realpath(homeArgument);
  const state = await fsp.realpath(stateArgument);
  const assetDirectory = path.join(root, "share", "viz");
  if (!isWithin(home, state) && state !== home) fail("state path must stay inside MX_HOME");
  if (!/^[a-f0-9]{32,128}$/.test(token)) fail("server token is invalid");
  const firstPort = Number(portArgument);
  if (!Number.isInteger(firstPort) || firstPort < 1 || firstPort > 65516) {
    fail("first port must be an integer from 1 through 65516");
  }
  const idleSeconds = parsePositiveInteger("MX_VIZ_IDLE_SECS", "1800", 86400);
  const pollMs = parsePositiveInteger("MX_VIZ_POLL_MS", "2500", 60000);
  const refreshMs = parseRefreshSeconds() * 1000;
  const snapshotCommand = process.env.MX_VIZ_SNAPSHOT_BIN || path.join(root, "bin", "mx-system-snapshot.sh");
  const doctorCommand = process.env.MX_VIZ_DOCTOR_BIN || path.join(root, "bin", "mx-doctor.sh");
  const timelineCommand = process.env.MX_VIZ_TIMELINE_BIN || path.join(root, "bin", "mx-timeline.sh");
  const started = new Date().toISOString();
  const childEnvironment = {
    ...process.env,
    MX_ROOT_OVERRIDE: root,
    MX_HOME: home,
    MX_STATE_OVERRIDE: state,
  };

  let server;
  let boundPort;
  let idleTimer;
  let shuttingDown = false;
  let lastRequestAt = Date.now();
  let lastPollAt = null;
  let cache = null;
  let refreshPromise = null;
  const sockets = new Set();

  const finish = (exitCode = 0) => {
    if (shuttingDown) return;
    shuttingDown = true;
    clearTimeout(idleTimer);
    cleanupRunRecord(runRecord, token);
    if (!server) {
      process.exit(exitCode);
      return;
    }
    server.close(() => process.exit(exitCode));
    setTimeout(() => {
      for (const socket of sockets) socket.destroy();
      process.exit(exitCode);
    }, 250).unref();
  };

  const resetIdleTimer = () => {
    lastRequestAt = Date.now();
    clearTimeout(idleTimer);
    idleTimer = setTimeout(() => finish(0), idleSeconds * 1000);
    idleTimer.unref();
  };

  const refreshSnapshot = async () => {
    const now = Date.now();
    if (cache && now - cache.refreshedAt < refreshMs) return cache;
    if (refreshPromise) return refreshPromise;
    refreshPromise = (async () => {
      const snapshot = await executeJson(snapshotCommand, ["--json"], childEnvironment);
      const artifacts = await collectArtifacts(root, snapshot.value);
      const serverData = { version: VERSION, started, pid: process.pid };
      const body = `{"server":${JSON.stringify(serverData)},"artifacts":${JSON.stringify(artifacts)},"snapshot":${snapshot.raw}}\n`;
      const hash = crypto.createHash("sha256").update(body).digest("hex");
      const snapshotHash = crypto.createHash("sha256").update(snapshot.raw).digest("hex");
      cache = { body, hash, snapshotHash, refreshedAt: Date.now(), generated: snapshot.value.generated || null };
      return cache;
    })();
    try {
      return await refreshPromise;
    } finally {
      refreshPromise = null;
    }
  };

  const serveFile = async (response, file, transform = null) => {
    const stat = await fsp.stat(file);
    if (!stat.isFile()) fail("requested path is not a file");
    let contents = await fsp.readFile(file);
    if (transform) contents = Buffer.from(transform(contents.toString("utf8")), "utf8");
    response.writeHead(200, {
      "Content-Type": contentType(file),
      "Content-Length": contents.length,
    });
    response.end(contents);
  };

  const serveArtifact = async (rawPath, response) => {
    setArtifactHeaders(response);
    let decoded;
    try {
      decoded = decodeURIComponent(rawPath);
    } catch {
      response.writeHead(403);
      response.end("forbidden\n");
      return;
    }
    const suffix = decoded.slice("/artifact/".length);
    const segments = suffix.split("/");
    if (segments.length < 2 || segments.some((segment) => segment === "" || segment === "." || segment === ".." || segment.includes("\0"))) {
      response.writeHead(403);
      response.end("forbidden\n");
      return;
    }
    const [rootName, ...relativeSegments] = segments;
    if (!ALLOWED_ARTIFACT_ROOTS.has(rootName)) {
      response.writeHead(403);
      response.end("forbidden\n");
      return;
    }
    const allowedRoot = await fsp.realpath(path.join(root, rootName));
    if (!isWithin(root, allowedRoot)) {
      response.writeHead(403);
      response.end("forbidden\n");
      return;
    }
    const candidate = path.resolve(allowedRoot, ...relativeSegments);
    if (!isWithin(allowedRoot, candidate)) {
      response.writeHead(403);
      response.end("forbidden\n");
      return;
    }
    let realFile;
    try {
      realFile = await fsp.realpath(candidate);
    } catch (error) {
      if (error.code === "ENOENT") {
        response.writeHead(404);
        response.end("not found\n");
        return;
      }
      throw error;
    }
    if (!isWithin(allowedRoot, realFile)) {
      response.writeHead(403);
      response.end("forbidden\n");
      return;
    }
    await serveFile(response, realFile);
  };

  const requestHandler = async (request, response) => {
    resetIdleTimer();
    setCommonHeaders(response);
    const rawTarget = request.url || "/";
    const rawPath = rawTarget.split("?", 1)[0];
    if (request.method !== "GET") {
      response.writeHead(405, { Allow: "GET" });
      response.end("method not allowed\n");
      return;
    }
    try {
      if (rawPath.startsWith("/artifact/")) {
        await serveArtifact(rawPath, response);
        return;
      }
      const requestUrl = new URL(rawTarget, `http://${HOST}`);
      if (requestUrl.pathname === "/") {
        await serveFile(response, path.join(assetDirectory, "index.html"), (source) =>
          source.replaceAll("__MX_VIZ_POLL_MS__", String(pollMs)));
        return;
      }
      if (requestUrl.pathname === "/assets/app.js" || requestUrl.pathname === "/assets/app.css") {
        await serveFile(response, path.join(assetDirectory, path.basename(requestUrl.pathname)));
        return;
      }
      if (requestUrl.pathname === "/api/meta") {
        sendJson(response, 200, {
          version: VERSION,
          started,
          pid: process.pid,
          port: boundPort,
          last_request_at: new Date(lastRequestAt).toISOString(),
          last_poll_at: lastPollAt ? new Date(lastPollAt).toISOString() : null,
          snapshot_generated: cache?.generated || null,
          hash: cache?.hash || null,
        });
        return;
      }
      if (requestUrl.pathname === "/api/state") {
        lastPollAt = Date.now();
        const current = await refreshSnapshot();
        const etag = `"${current.hash}"`;
        if (request.headers["if-none-match"] === etag || request.headers["if-none-match"] === current.hash) {
          response.writeHead(304, { ETag: etag, "X-Multplx-Content-Hash": current.hash });
          response.end();
          return;
        }
        response.writeHead(200, {
          "Content-Type": "application/json; charset=utf-8",
          "Content-Length": Buffer.byteLength(current.body),
          ETag: etag,
          "X-Multplx-Content-Hash": current.hash,
          "X-Multplx-Snapshot-Hash": current.snapshotHash,
        });
        response.end(current.body);
        return;
      }
      if (requestUrl.pathname === "/api/doctor") {
        const doctor = await executeJson(doctorCommand, ["--json"], childEnvironment, [0, 1, 2]);
        sendJson(response, 200, doctor.value);
        return;
      }
      if (requestUrl.pathname.startsWith("/api/timeline/")) {
        const id = decodeURIComponent(requestUrl.pathname.slice("/api/timeline/".length));
        if (!/^[A-Za-z0-9._-]{1,128}$/.test(id)) {
          response.writeHead(400);
          response.end("invalid task id\n");
          return;
        }
        const { stdout } = await execFileAsync(timelineCommand, [id, "--json"], {
          env: childEnvironment,
          encoding: "utf8",
          maxBuffer: MAX_OUTPUT_BYTES,
          timeout: 30000,
        });
        const records = stdout.split("\n").filter(Boolean).map((line) => JSON.parse(line));
        sendJson(response, 200, { task: id, records });
        return;
      }
      response.writeHead(404);
      response.end("not found\n");
    } catch (error) {
      const status = error.code === "ENOENT" ? 404 : 503;
      if (!response.headersSent) sendJson(response, status, { error: error.message });
      else response.destroy(error);
    }
  };

  for (let port = firstPort; port < firstPort + PORT_COUNT; port += 1) {
    const candidate = http.createServer(requestHandler);
    candidate.on("connection", (socket) => {
      sockets.add(socket);
      socket.on("close", () => sockets.delete(socket));
    });
    try {
      await new Promise((resolve, reject) => {
        const onError = (error) => {
          candidate.off("listening", onListening);
          reject(error);
        };
        const onListening = () => {
          candidate.off("error", onError);
          resolve();
        };
        candidate.once("error", onError);
        candidate.once("listening", onListening);
        candidate.listen(port, HOST);
      });
      server = candidate;
      boundPort = port;
      resetIdleTimer();
      break;
    } catch (error) {
      candidate.close();
      if (error.code !== "EADDRINUSE") throw error;
    }
  }
  if (!server) fail(`no loopback port available in range ${firstPort}-${firstPort + PORT_COUNT - 1}`);

  process.on("SIGTERM", () => finish(0));
  process.on("SIGINT", () => finish(0));
  process.on("uncaughtException", (error) => {
    process.stderr.write(`mx-viz-server: ${error.stack || error.message}\n`);
    finish(1);
  });
  process.on("unhandledRejection", (error) => {
    process.stderr.write(`mx-viz-server: ${error.stack || error}\n`);
    finish(1);
  });
  process.stdout.write(`READY ${boundPort}\n`);
}

async function main() {
  const [mode, ...args] = process.argv.slice(2);
  if (mode === "--serve" && args.length === 6) {
    await serve(...args);
    return;
  }
  usage();
  process.exitCode = 2;
}

main().catch((error) => {
  process.stderr.write(`mx-viz-server: ${error.message}\n`);
  process.exitCode = 1;
});
