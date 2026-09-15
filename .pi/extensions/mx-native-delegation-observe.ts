import { spawn } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

const extensionRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const sourceRoot = process.env.MX_RUST_SOURCE_ROOT || extensionRoot;
const observer = `${sourceRoot}/bin/mx-native-observe.sh`;
const delegationTools = new Set(
  (process.env.MX_PI_DELEGATION_TOOLS || "agent,subagent,spawn_agent,delegate_task")
    .split(",")
    .map((name) => name.trim())
    .filter(Boolean),
);

function observe(
  event: "start" | "result" | "interrupted" | "reconcile",
  payload: object,
  awaitCompletion = false,
): Promise<void> | void {
  const run = new Promise<void>((resolveRun) => {
    const child = spawn(observer, ["--provider", "pi", "--event", event], {
      env: process.env,
      stdio: ["pipe", "ignore", "ignore"],
    });
    const timeout = setTimeout(() => {
      child.kill("SIGKILL");
      resolveRun();
    }, 5000);
    child.on("error", () => {
      clearTimeout(timeout);
      resolveRun();
    });
    child.on("close", () => {
      clearTimeout(timeout);
      resolveRun();
    });
    child.stdin.on("error", () => {});
    child.stdin.end(JSON.stringify(payload));
  });
  if (awaitCompletion) return run;
  void run;
}

export default function (pi: ExtensionAPI) {
  pi.on?.("session_start", async (event) => {
    await observe("reconcile", event as object, true);
  });

  pi.on("tool_call", (event) => {
    if (event.type !== "tool_call" || !delegationTools.has(event.toolName)) return {};
    observe("start", {
      turn_id: event.toolCallId,
      tool_name: event.toolName,
    });
    return {};
  });

  pi.on("tool_result", (event) => {
    if (event.type !== "tool_result" || !delegationTools.has(event.toolName)) return;
    observe(event.isError ? "interrupted" : "result", {
      turn_id: event.toolCallId,
      tool_name: event.toolName,
    });
  });
}
