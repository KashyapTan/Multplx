#!/usr/bin/env bash
# Usage: mx-codex-idle.sh [--status|--register|--retry|--end|--run]
# Stop-owned exact-thread Codex queue bridge; manual recovery needs CODEX_THREAD_ID.
# --retry permits duplicate input after uncertain acceptance.
# --register captures native SessionStart readiness; --register and --run are internal.
# Models supervise through emitted protocol, not these internal lifecycle flags.
set -euo pipefail
SOURCE_ROOT=${MX_RUST_SOURCE_ROOT:-$(cd -- "$(dirname -- "$0")/.." && pwd)}
BINARY=${MX_RUST_BIN:-$SOURCE_ROOT/target/release/mx}
[ -x "$BINARY" ] || { printf 'mx-codex-idle: Rust release binary unavailable at %s\n' "$BINARY" >&2; exit 1; }
export MX_RUST_SOURCE_ROOT="$SOURCE_ROOT"
exec "$BINARY" supervision mx-codex-idle.sh "$@"
