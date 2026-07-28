#!/usr/bin/env node
/**
 * Minimal stdio MCP adapter for the task-bound `report_status` tool.
 *
 * The shell wrapper remains the sole owner of the state enum, binding checks,
 * line grammar, and append. This process reads the enum once through
 * `mx-report --list-states`, advertises it in the tool schema, validates raw
 * JSON-RPC callers as defense in depth, and delegates successful calls back to
 * the wrapper.
 *
 * Transport is newline-delimited JSON-RPC over stdin/stdout. Nothing except
 * protocol messages is written to stdout.
 */

import { execFileSync, spawnSync } from "node:child_process";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const reportPath = join(scriptDir, "mx-report");

let states;
try {
  states = execFileSync(reportPath, ["--list-states"], {
    encoding: "utf8",
    env: process.env,
  })
    .trim()
    .split("\n")
    .filter(Boolean);
} catch (error) {
  process.stderr.write(`mx-report-mcp: cannot load state vocabulary: ${error.message}\n`);
  process.exit(1);
}

const stateSet = new Set(states);
const taskId = process.env.MX_TASK_ID || "";
const tool = {
  name: "report_status",
  description:
    "Append one validated status event for this task. Use this instead of writing a status file directly.",
  inputSchema: {
    type: "object",
    properties: {
      state: { type: "string", enum: states },
      message: { type: "string", maxLength: 300 },
      key: { type: "string", pattern: "^[A-Za-z0-9._-]+$" },
    },
    required: ["state", "message"],
    additionalProperties: false,
  },
};

function send(payload) {
  process.stdout.write(`${JSON.stringify(payload)}\n`);
}

function result(id, value) {
  send({ jsonrpc: "2.0", id, result: value });
}

function rpcError(id, code, message) {
  send({ jsonrpc: "2.0", id: id ?? null, error: { code, message } });
}

function validateArguments(args) {
  if (!args || typeof args !== "object" || Array.isArray(args)) {
    return "arguments must be an object";
  }
  const keys = Object.keys(args);
  if (keys.some((key) => !["state", "message", "key"].includes(key))) {
    return "arguments contain an unsupported property";
  }
  if (!Object.hasOwn(args, "state") || !Object.hasOwn(args, "message")) {
    return "state and message are required";
  }
  if (typeof args.state !== "string" || !stateSet.has(args.state)) {
    return `state must be one of: ${states.join(", ")}`;
  }
  if (typeof args.message !== "string" || args.message.length > 300) {
    return "message must be a string of at most 300 characters";
  }
  if (/[\r\n]/u.test(args.message)) {
    return "message must be exactly one line";
  }
  if (
    Object.hasOwn(args, "key") &&
    (typeof args.key !== "string" || !/^[A-Za-z0-9._-]+$/u.test(args.key))
  ) {
    return "key may contain only A-Z, a-z, 0-9, dot, underscore, and dash";
  }
  return null;
}

function handleCall(id, params) {
  if (!params || params.name !== "report_status") {
    rpcError(id, -32602, "unknown tool");
    return;
  }
  const validationError = validateArguments(params.arguments);
  if (validationError) {
    rpcError(id, -32602, validationError);
    return;
  }
  if (!taskId) {
    result(id, {
      content: [
        {
          type: "text",
          text: "mx-report-mcp: no task binding found; MX_TASK_ID is unset",
        },
      ],
      isError: true,
    });
    return;
  }

  const args = [
    "--id",
    taskId,
    "--state",
    params.arguments.state,
    "--message",
    params.arguments.message,
  ];
  if (Object.hasOwn(params.arguments, "key")) {
    args.push("--key", params.arguments.key);
  }

  const run = spawnSync(reportPath, args, {
    encoding: "utf8",
    env: process.env,
  });
  if (run.error || run.status !== 0) {
    const detail =
      (run.stderr || run.error?.message || `mx-report exited ${run.status}`).trim();
    result(id, {
      content: [{ type: "text", text: detail }],
      isError: true,
    });
    return;
  }
  result(id, {
    content: [
      {
        type: "text",
        text: `${params.arguments.state} status reported for task ${taskId}`,
      },
    ],
  });
}

function handle(message) {
  if (!message || message.jsonrpc !== "2.0" || typeof message.method !== "string") {
    rpcError(message?.id, -32600, "invalid JSON-RPC request");
    return;
  }
  switch (message.method) {
    case "initialize":
      result(message.id, {
        protocolVersion: message.params?.protocolVersion || "2025-06-18",
        capabilities: { tools: {} },
        serverInfo: { name: "multplx-status", version: "1.0.0" },
      });
      break;
    case "notifications/initialized":
    case "notifications/cancelled":
      break;
    case "ping":
      result(message.id, {});
      break;
    case "tools/list":
      result(message.id, { tools: [tool] });
      break;
    case "tools/call":
      handleCall(message.id, message.params);
      break;
    default:
      if (message.id !== undefined) {
        rpcError(message.id, -32601, `method not found: ${message.method}`);
      }
  }
}

const input = createInterface({ input: process.stdin, crlfDelay: Infinity });
input.on("line", (line) => {
  if (!line.trim()) return;
  let message;
  try {
    message = JSON.parse(line);
  } catch {
    rpcError(null, -32700, "parse error");
    return;
  }
  try {
    handle(message);
  } catch (error) {
    rpcError(message?.id, -32603, `internal error: ${error.message}`);
  }
});
