#!/usr/bin/env node
/**
 * One-shot loopback review server for mx-vplan.sh.
 *
 * Internal modes:
 *   mx-vplan-server.mjs --serve <artifact> <root> <run-record> <token> <first-port>
 *   mx-vplan-server.mjs --comments <artifact>
 *
 * The server binds only 127.0.0.1, tries 20 consecutive ports, injects the
 * review SDK into the served copy, and never changes the artifact until a
 * token-authenticated POST /confirm succeeds. Confirm merges the inert
 * #vplan-comments JSON block with atomic temp-file + rename semantics, sends
 * the response, removes its identity-bound run record, and exits.
 */

import crypto from "node:crypto";
import fs from "node:fs";
import fsp from "node:fs/promises";
import http from "node:http";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const HOST = "127.0.0.1";
const PORT_COUNT = 20;
const MAX_BODY_BYTES = 1024 * 1024;
const MAX_COMMENTS = 500;
const COMMENT_PATTERN =
  /<script type="application\/json" id="vplan-comments">\s*([\s\S]*?)\s*<\/script>/g;
const COMMENT_ID_PATTERN = /\bid=(["'])vplan-comments\1/g;
const REQUIRED_FIELDS = [
  "id",
  "selector",
  "anchor_text",
  "nearest_heading",
  "comment",
  "ts",
  "resolved",
];
const STRING_LIMITS = {
  id: 200,
  selector: 2048,
  anchor_text: 4096,
  nearest_heading: 512,
  comment: 20000,
  ts: 64,
};

function usage() {
  process.stderr.write(
    "usage: mx-vplan-server.mjs --serve <artifact> <root> <run-record> <token> <first-port>\n" +
      "       mx-vplan-server.mjs --comments <artifact>\n",
  );
}

function fail(message) {
  throw new Error(message);
}

function isPlainObject(value) {
  return (
    value !== null &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    Object.getPrototypeOf(value) === Object.prototype
  );
}

function normalizeComment(value, label) {
  if (!isPlainObject(value)) {
    fail(`${label} must be an object`);
  }
  for (const key of REQUIRED_FIELDS) {
    if (!(key in value)) {
      fail(`${label} is missing required field '${key}'`);
    }
  }
  for (const key of Object.keys(value)) {
    if (!REQUIRED_FIELDS.includes(key)) {
      fail(`${label} has unknown field '${key}'`);
    }
  }
  for (const [key, limit] of Object.entries(STRING_LIMITS)) {
    if (typeof value[key] !== "string") {
      fail(`${label}.${key} must be a string`);
    }
    if (value[key].length > limit) {
      fail(`${label}.${key} exceeds ${limit} characters`);
    }
  }
  if (value.id.length === 0 || value.selector.length === 0 || value.comment.trim().length === 0) {
    fail(`${label} requires non-empty id, selector, and comment fields`);
  }
  if (
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/.test(value.ts) ||
    Number.isNaN(Date.parse(value.ts))
  ) {
    fail(`${label}.ts must be an ISO-8601 timestamp`);
  }
  if (typeof value.resolved !== "boolean") {
    fail(`${label}.resolved must be a boolean`);
  }
  return {
    id: value.id,
    selector: value.selector,
    anchor_text: value.anchor_text,
    nearest_heading: value.nearest_heading,
    comment: value.comment,
    ts: value.ts,
    resolved: value.resolved,
  };
}

function normalizeCommentArray(value, label) {
  if (!Array.isArray(value)) {
    fail(`${label} must be an array`);
  }
  if (value.length > MAX_COMMENTS) {
    fail(`${label} exceeds the ${MAX_COMMENTS}-comment limit`);
  }
  const ids = new Set();
  const comments = value.map((entry, index) => {
    const comment = normalizeComment(entry, `${label}[${index}]`);
    if (ids.has(comment.id)) {
      fail(`${label} contains duplicate id '${comment.id}'`);
    }
    ids.add(comment.id);
    return comment;
  });
  return comments;
}

function parseCommentBlock(html) {
  const idMatches = [...html.matchAll(COMMENT_ID_PATTERN)];
  COMMENT_PATTERN.lastIndex = 0;
  const blockMatches = [...html.matchAll(COMMENT_PATTERN)];
  COMMENT_PATTERN.lastIndex = 0;
  if (idMatches.length === 0 && blockMatches.length === 0) {
    return { comments: [], match: null };
  }
  if (idMatches.length !== 1 || blockMatches.length !== 1) {
    fail("artifact has a malformed or duplicate #vplan-comments block");
  }
  let parsed;
  try {
    parsed = JSON.parse(blockMatches[0][1]);
  } catch (error) {
    fail(`artifact has malformed #vplan-comments JSON: ${error.message}`);
  }
  return {
    comments: normalizeCommentArray(parsed, "existing comments"),
    match: blockMatches[0],
  };
}

function parseConfirmPayload(value) {
  if (!isPlainObject(value)) {
    fail("confirm payload must be an object");
  }
  const keys = Object.keys(value);
  if (keys.length !== 1 || keys[0] !== "comments") {
    fail("confirm payload must contain only the required 'comments' field");
  }
  return normalizeCommentArray(value.comments, "comments");
}

function mergeComments(existing, incoming) {
  const merged = existing.map((comment) => ({ ...comment }));
  const indexById = new Map(merged.map((comment, index) => [comment.id, index]));
  for (const comment of incoming) {
    if (!indexById.has(comment.id)) {
      indexById.set(comment.id, merged.length);
      merged.push(comment);
      continue;
    }
    const index = indexById.get(comment.id);
    const prior = merged[index];
    for (const key of REQUIRED_FIELDS.filter((field) => field !== "resolved")) {
      if (prior[key] !== comment[key]) {
        fail(`comment id '${comment.id}' collides with different persisted content`);
      }
    }
    merged[index] = { ...prior, resolved: prior.resolved || comment.resolved };
  }
  return merged;
}

function serializeComments(comments) {
  const json = JSON.stringify(comments, null, 2).replaceAll("<", "\\u003c");
  return `<script type="application/json" id="vplan-comments">\n${json}\n</script>`;
}

function mergeCommentBlock(html, incoming) {
  const parsed = parseCommentBlock(html);
  const comments = mergeComments(parsed.comments, incoming);
  const block = serializeComments(comments);
  if (parsed.match) {
    const start = parsed.match.index;
    const end = start + parsed.match[0].length;
    return { html: `${html.slice(0, start)}${block}${html.slice(end)}`, comments };
  }
  const closers = [...html.matchAll(/<\/body\s*>/gi)];
  if (closers.length !== 1) {
    fail("artifact must contain exactly one closing </body> tag");
  }
  const index = closers[0].index;
  const before = html.slice(0, index);
  const separator = before.endsWith("\n") ? "" : "\n";
  return {
    html: `${before}${separator}${block}\n${html.slice(index)}`,
    comments,
  };
}

async function atomicWrite(file, contents) {
  const stat = await fsp.stat(file);
  const directory = path.dirname(file);
  const temporary = path.join(
    directory,
    `.${path.basename(file)}.vplan-${process.pid}-${crypto.randomBytes(6).toString("hex")}.tmp`,
  );
  let handle;
  try {
    handle = await fsp.open(temporary, "wx", stat.mode & 0o777);
    await handle.writeFile(contents, "utf8");
    await handle.sync();
    await handle.close();
    handle = null;
    await fsp.rename(temporary, file);
    try {
      const directoryHandle = await fsp.open(directory, "r");
      await directoryHandle.sync();
      await directoryHandle.close();
    } catch {
      // Directory fsync is unavailable on some supported filesystems.
    }
  } catch (error) {
    if (handle) {
      await handle.close().catch(() => {});
    }
    await fsp.unlink(temporary).catch(() => {});
    throw error;
  }
}

function parseRecord(contents) {
  const record = {};
  for (const line of contents.split("\n")) {
    const separator = line.indexOf("=");
    if (separator <= 0) {
      continue;
    }
    record[line.slice(0, separator)] = line.slice(separator + 1);
  }
  return record;
}

function cleanupRunRecord(runRecord, token) {
  try {
    const record = parseRecord(fs.readFileSync(runRecord, "utf8"));
    if (record.token === token && record.pid === String(process.pid)) {
      fs.unlinkSync(runRecord);
    }
  } catch (error) {
    if (error.code !== "ENOENT") {
      process.stderr.write(`mx-vplan-server: could not clean run record: ${error.message}\n`);
    }
  }
}

function contentType(file) {
  switch (path.extname(file).toLowerCase()) {
    case ".css":
      return "text/css; charset=utf-8";
    case ".gif":
      return "image/gif";
    case ".html":
      return "text/html; charset=utf-8";
    case ".jpeg":
    case ".jpg":
      return "image/jpeg";
    case ".js":
    case ".mjs":
      return "text/javascript; charset=utf-8";
    case ".json":
      return "application/json; charset=utf-8";
    case ".png":
      return "image/png";
    case ".svg":
      return "image/svg+xml";
    case ".webp":
      return "image/webp";
    default:
      return "application/octet-stream";
  }
}

function isWithin(parent, candidate) {
  const relative = path.relative(parent, candidate);
  return relative === "" || (!relative.startsWith(`..${path.sep}`) && relative !== ".." && !path.isAbsolute(relative));
}

function encodePathSegments(value) {
  return value
    .split(path.sep)
    .filter(Boolean)
    .map((segment) => encodeURIComponent(segment))
    .join("/");
}

function injectReviewSurface(html, artifact, root, token) {
  const artifactDirectory = path.dirname(path.relative(root, artifact));
  const baseSuffix =
    artifactDirectory === "."
      ? ""
      : `${encodePathSegments(artifactDirectory)}${artifactDirectory ? "/" : ""}`;
  const headInjection =
    `<base data-vplan-injected href="/__vplan/root/${baseSuffix}">\n` +
    `<meta data-vplan-injected name="vplan-token" content="${token}">\n` +
    '<link data-vplan-injected rel="stylesheet" href="/__vplan/sdk.css">';
  const bodyInjection = '<script data-vplan-injected src="/__vplan/sdk.js"></script>';
  const headMatch = html.match(/<head(?:\s[^>]*)?>/i);
  const bodyMatches = [...html.matchAll(/<\/body\s*>/gi)];
  if (!headMatch || bodyMatches.length !== 1) {
    fail("artifact must contain one <head> and exactly one closing </body> tag");
  }
  const headEnd = headMatch.index + headMatch[0].length;
  let injected = `${html.slice(0, headEnd)}\n${headInjection}${html.slice(headEnd)}`;
  const closer = [...injected.matchAll(/<\/body\s*>/gi)][0];
  injected = `${injected.slice(0, closer.index)}${bodyInjection}\n${injected.slice(closer.index)}`;
  return injected;
}

function setCommonHeaders(response) {
  response.setHeader("Cache-Control", "no-store");
  response.setHeader("Content-Security-Policy", [
    "default-src 'self' data: blob:",
    "script-src 'self' 'unsafe-inline' blob:",
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data: blob:",
    "font-src 'self' data:",
    "connect-src 'self'",
    "object-src 'none'",
    "base-uri 'self'",
    "frame-ancestors 'none'",
  ].join("; "));
  response.setHeader("Referrer-Policy", "no-referrer");
  response.setHeader("X-Content-Type-Options", "nosniff");
  response.setHeader("X-Frame-Options", "DENY");
}

function sendJson(response, status, value) {
  const body = `${JSON.stringify(value)}\n`;
  response.writeHead(status, {
    "Content-Type": "application/json; charset=utf-8",
    "Content-Length": Buffer.byteLength(body),
    Connection: "close",
  });
  response.end(body);
}

async function readJsonBody(request) {
  const chunks = [];
  let size = 0;
  for await (const chunk of request) {
    size += chunk.length;
    if (size > MAX_BODY_BYTES) {
      fail(`confirm payload exceeds ${MAX_BODY_BYTES} bytes`);
    }
    chunks.push(chunk);
  }
  if (chunks.length === 0) {
    fail("confirm payload is empty");
  }
  try {
    return JSON.parse(Buffer.concat(chunks).toString("utf8"));
  } catch (error) {
    fail(`confirm payload is not valid JSON: ${error.message}`);
  }
}

async function printComments(artifactArgument) {
  const artifact = await fsp.realpath(artifactArgument);
  const html = await fsp.readFile(artifact, "utf8");
  const { comments } = parseCommentBlock(html);
  process.stdout.write(`${JSON.stringify(comments, null, 2)}\n`);
}

async function serve(artifactArgument, rootArgument, runRecord, token, portArgument) {
  const artifact = await fsp.realpath(artifactArgument);
  const root = await fsp.realpath(rootArgument);
  const assetDirectory = path.join(root, "share", "vplan");
  const artifactDirectory = path.dirname(artifact);
  if (!isWithin(root, artifact)) {
    fail("artifact must be inside the Multplx root");
  }
  if (!/^[a-f0-9]{32,128}$/.test(token)) {
    fail("review token is invalid");
  }
  const firstPort = Number(portArgument);
  if (!Number.isInteger(firstPort) || firstPort < 1 || firstPort > 65516) {
    fail("first port must be an integer from 1 through 65516");
  }
  const idleSeconds = Number(process.env.MX_VPLAN_IDLE_SECS || "1800");
  if (!Number.isInteger(idleSeconds) || idleSeconds < 1 || idleSeconds > 86400) {
    fail("MX_VPLAN_IDLE_SECS must be an integer from 1 through 86400");
  }

  let server;
  let boundPort;
  let idleTimer;
  let confirming = false;
  let shuttingDown = false;
  const sockets = new Set();

  const finish = (exitCode = 0) => {
    if (shuttingDown) {
      return;
    }
    shuttingDown = true;
    clearTimeout(idleTimer);
    cleanupRunRecord(runRecord, token);
    if (!server) {
      process.exit(exitCode);
      return;
    }
    server.close(() => process.exit(exitCode));
    setTimeout(() => {
      for (const socket of sockets) {
        socket.destroy();
      }
      process.exit(exitCode);
    }, 250).unref();
  };

  const resetIdleTimer = () => {
    clearTimeout(idleTimer);
    idleTimer = setTimeout(() => finish(0), idleSeconds * 1000);
    idleTimer.unref();
  };

  const requestHandler = async (request, response) => {
    resetIdleTimer();
    setCommonHeaders(response);
    const requestUrl = new URL(request.url || "/", `http://${HOST}`);
    try {
      if (request.method === "GET" && requestUrl.pathname === "/") {
        const html = await fsp.readFile(artifact, "utf8");
        const injected = injectReviewSurface(html, artifact, root, token);
        response.writeHead(200, {
          "Content-Type": "text/html; charset=utf-8",
          "Content-Length": Buffer.byteLength(injected),
        });
        response.end(injected);
        return;
      }

      if (request.method === "GET" && requestUrl.pathname.startsWith("/__vplan/")) {
        let file;
        if (requestUrl.pathname === "/__vplan/sdk.js") {
          file = path.join(assetDirectory, "sdk.js");
        } else if (requestUrl.pathname === "/__vplan/sdk.css") {
          file = path.join(assetDirectory, "sdk.css");
        } else if (requestUrl.pathname === "/__vplan/mermaid.min.js") {
          file = path.join(assetDirectory, "mermaid.min.js");
        } else if (requestUrl.pathname.startsWith("/__vplan/root/")) {
          const relativeUrl = requestUrl.pathname.slice("/__vplan/root/".length);
          let relative;
          try {
            relative = relativeUrl
              .split("/")
              .map((segment) => decodeURIComponent(segment))
              .join(path.sep);
          } catch {
            fail("asset path is not valid URL encoding");
          }
          file = path.resolve(root, relative);
          if (!isWithin(root, file)) {
            fail("asset path escapes the Multplx root");
          }
          if (!isWithin(artifactDirectory, file) && !isWithin(assetDirectory, file)) {
            fail("asset path is outside the artifact and vplan asset directories");
          }
        } else {
          response.writeHead(404);
          response.end("not found\n");
          return;
        }
        const stat = await fsp.stat(file);
        if (!stat.isFile()) {
          fail("asset path is not a file");
        }
        const contents = await fsp.readFile(file);
        response.writeHead(200, {
          "Content-Type": contentType(file),
          "Content-Length": contents.length,
        });
        response.end(contents);
        return;
      }

      if (request.method === "POST" && requestUrl.pathname === "/confirm") {
        if (confirming) {
          sendJson(response, 409, { error: "confirm already in progress" });
          return;
        }
        if (request.headers["x-vplan-token"] !== token) {
          sendJson(response, 403, { error: "invalid review token" });
          return;
        }
        if (!(request.headers["content-type"] || "").toLowerCase().startsWith("application/json")) {
          sendJson(response, 415, { error: "content-type must be application/json" });
          return;
        }
        confirming = true;
        try {
          const payload = parseConfirmPayload(await readJsonBody(request));
          const html = await fsp.readFile(artifact, "utf8");
          const merged = mergeCommentBlock(html, payload);
          if (merged.html !== html) {
            await atomicWrite(artifact, merged.html);
          }
          response.once("finish", () => setTimeout(() => finish(0), 25));
          sendJson(response, 200, { saved: payload.length, total: merged.comments.length });
          setTimeout(() => finish(0), 250);
        } catch (error) {
          confirming = false;
          sendJson(response, 400, { error: error.message });
        }
        return;
      }

      response.writeHead(404);
      response.end("not found\n");
    } catch (error) {
      const status = error.code === "ENOENT" ? 404 : 400;
      if (!response.headersSent) {
        sendJson(response, status, { error: error.message });
      } else {
        response.destroy(error);
      }
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
      if (error.code !== "EADDRINUSE") {
        throw error;
      }
    }
  }
  if (!server) {
    fail(`no loopback port available in range ${firstPort}-${firstPort + PORT_COUNT - 1}`);
  }

  process.on("SIGTERM", () => finish(0));
  process.on("SIGINT", () => finish(0));
  process.on("uncaughtException", (error) => {
    process.stderr.write(`mx-vplan-server: ${error.stack || error.message}\n`);
    finish(1);
  });
  process.on("unhandledRejection", (error) => {
    process.stderr.write(`mx-vplan-server: ${error.stack || error}\n`);
    finish(1);
  });
  process.stdout.write(`READY ${boundPort}\n`);
}

async function main() {
  const [mode, ...args] = process.argv.slice(2);
  if (mode === "--comments" && args.length === 1) {
    await printComments(args[0]);
    return;
  }
  if (mode === "--serve" && args.length === 5) {
    await serve(...args);
    return;
  }
  usage();
  process.exitCode = 2;
}

main().catch((error) => {
  process.stderr.write(`mx-vplan-server: ${error.message}\n`);
  process.exitCode = 1;
});
