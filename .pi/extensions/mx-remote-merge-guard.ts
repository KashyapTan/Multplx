import { spawn } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

const extensionRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const sourceRoot = process.env.MX_RUST_SOURCE_ROOT || extensionRoot;
const guard = `${sourceRoot}/bin/mx-subagent-pretool-check.sh`;

function check(command: string): Promise<{ code: number; reason: string }> {
  return new Promise((resolveCheck) => {
    const child = spawn(guard, ["--command", command], {
      env: process.env,
      stdio: ["ignore", "ignore", "pipe"],
    });
    let reason = "";
    child.stderr.on("data", (chunk) => {
      reason += chunk.toString();
    });
    child.on("error", () => resolveCheck({ code: 1, reason: "" }));
    child.on("close", (code) => resolveCheck({ code: code ?? 1, reason }));
  });
}

export default function (pi: ExtensionAPI) {
  pi.on("tool_call", async (event) => {
    if (event.type !== "tool_call" || event.toolName !== "bash") return {};
    const command = String((event.input as { command?: unknown })?.command ?? "");
    if (!command) return {};
    const result = await check(command);
    if (result.code !== 2) return {};
    return {
      block: true,
      reason: result.reason.trim() || "remote PR merges are human-only",
    };
  });
}
