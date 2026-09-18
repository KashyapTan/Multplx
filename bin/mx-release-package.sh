#!/usr/bin/env bash
# Build an isolated, version-matched Multplx release package from tracked assets.
set -euo pipefail

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
ROOT=$(cd "$SCRIPT_DIR/.." && pwd -P)
OUTPUT=${1:-}
BINARY=${2:-$ROOT/target/release/mx}

if [ -z "$OUTPUT" ] || [ "${3+x}" = x ]; then
  printf 'usage: %s OUTPUT_DIRECTORY [RELEASE_BINARY]\n' "$0" >&2
  exit 2
fi
case "$OUTPUT" in /*) ;; *) OUTPUT=$PWD/$OUTPUT ;; esac
case "$BINARY" in /*) ;; *) BINARY=$PWD/$BINARY ;; esac
[ -x "$BINARY" ] || { printf 'multplx: release binary is not executable: %s\n' "$BINARY" >&2; exit 2; }
[ ! -e "$OUTPUT" ] || { printf 'multplx: output already exists: %s\n' "$OUTPUT" >&2; exit 2; }

VERSION=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT/Cargo.toml" | head -n 1)
[ -n "$VERSION" ] || { printf 'multplx: could not read workspace version\n' >&2; exit 1; }
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
case "$OS" in darwin) OS=macos ;; linux) ;; *) printf 'multplx: unsupported package OS: %s\n' "$OS" >&2; exit 2 ;; esac
ARCH=$(uname -m)
case "$ARCH" in arm64) ARCH=aarch64 ;; x86_64|aarch64) ;; *) printf 'multplx: unsupported package architecture: %s\n' "$ARCH" >&2; exit 2 ;; esac

mkdir -p "$OUTPUT/bin" "$OUTPUT/runtime"
cp "$BINARY" "$OUTPUT/bin/multplx"
chmod 755 "$OUTPUT/bin/multplx"
cp "$BINARY" "$OUTPUT/bin/mx"
chmod 755 "$OUTPUT/bin/mx"
printf '%s\n' "$VERSION" >"$OUTPUT/VERSION"
printf '%s-%s\n' "$OS" "$ARCH" >"$OUTPUT/PLATFORM"
cp "$ROOT/AGENTS_E.md" "$OUTPUT/runtime/AGENTS.md"

while IFS= read -r -d '' relative; do
  case "$relative" in
    docs/calm-mode-feasibility.md|docs/mx-test-*.json) continue ;;
  esac
  target=$OUTPUT/runtime/$relative
  mkdir -p "$(dirname -- "$target")"
  cp "$ROOT/$relative" "$target"
done < <(git -C "$ROOT" ls-files -z -- \
  bin share .agents/skills .claude/settings.json .codex .cursor .pi docs workflows skills)

# Claude normally sees this as a source-tree symlink. Packages avoid symlinks so
# the installer can validate every byte without path traversal.
mkdir -p "$OUTPUT/runtime/.claude/skills"
while IFS= read -r -d '' relative; do
  suffix=${relative#.agents/skills/}
  target=$OUTPUT/runtime/.claude/skills/$suffix
  mkdir -p "$(dirname -- "$target")"
  cp "$ROOT/$relative" "$target"
done < <(git -C "$ROOT" ls-files -z -- .agents/skills)

(
  cd "$OUTPUT"
  find . -type f ! -name SHA256SUMS -print0 \
    | LC_ALL=C sort -z \
    | while IFS= read -r -d '' file; do
        relative=${file#./}
        if [ -x "$file" ]; then mode=0755; else mode=0644; fi
        if command -v shasum >/dev/null 2>&1; then
          digest=$(shasum -a 256 "$file" | awk '{print $1}')
        else
          digest=$(sha256sum "$file" | awk '{print $1}')
        fi
        printf '%s\t%s\t%s\n' "$digest" "$mode" "$relative"
      done >SHA256SUMS
)

archive=$OUTPUT.tar.gz
tar -C "$(dirname -- "$OUTPUT")" -czf "$archive" "$(basename -- "$OUTPUT")"
if command -v shasum >/dev/null 2>&1; then
  (cd "$(dirname -- "$archive")" && shasum -a 256 "$(basename -- "$archive")" >"$(basename -- "$archive").sha256")
else
  (cd "$(dirname -- "$archive")" && sha256sum "$(basename -- "$archive")" >"$(basename -- "$archive").sha256")
fi
printf '%s\n' "$archive"
