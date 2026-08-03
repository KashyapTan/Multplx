#!/usr/bin/env bash
# Shared, side-effect-free validation helpers for the Multplx launcher and
# installer.  bin/mx-launcher.sh owns the command grammar and exit statuses;
# this library owns literal path-file decoding, checkout validation, and
# recursion-safe executable discovery.

mx_launcher_error() {
  printf 'multplx: %s\n' "$*" >&2
}

# mx_launcher_read_path_file <file> <variable>
# Accept exactly one absolute path followed by exactly one newline.
# Comparing the byte count with the decoded Bash string rejects extra lines,
# missing final newlines, and embedded NUL bytes without evaluating any byte as
# shell syntax.  Spaces, Unicode, and shell metacharacters remain ordinary path
# data.
mx_launcher_read_path_file() {
  local LC_ALL=C file=$1 variable=$2 value bytes
  if [ -L "$file" ] || [ ! -f "$file" ]; then
    mx_launcher_error "path file is missing, linked, or not regular: $file"
    return 1
  fi
  bytes=$(LC_ALL=C wc -c <"$file" 2>/dev/null) || {
    mx_launcher_error "cannot read path file: $file"
    return 1
  }
  bytes=${bytes//[[:space:]]/}
  LC_ALL=C IFS= read -r value <"$file" || {
    mx_launcher_error "path file must contain one newline-terminated absolute path: $file"
    return 1
  }
  if [ "$bytes" -ne "$(( ${#value} + 1 ))" ]; then
    mx_launcher_error "path file must contain exactly one newline-terminated path: $file"
    return 1
  fi
  case "$value" in
    /*) ;;
    *)
      mx_launcher_error "path file does not contain an absolute path: $file"
      return 1
      ;;
  esac
  printf -v "$variable" '%s' "$value"
}

mx_launcher_canonical_dir() {
  local path=$1 label=$2
  [ -d "$path" ] || {
    mx_launcher_error "$label directory does not exist: $path"
    return 1
  }
  (cd "$path" 2>/dev/null && pwd -P) || {
    mx_launcher_error "cannot resolve $label directory: $path"
    return 1
  }
}

mx_launcher_validate_root() {
  local root=$1 top
  [ -f "$root/AGENTS.md" ] || {
    mx_launcher_error "code root is missing AGENTS.md: $root"
    return 1
  }
  [ -d "$root/bin" ] && [ -d "$root/.agents/skills" ] || {
    mx_launcher_error "code root is missing Multplx scripts or skills: $root"
    return 1
  }
  [ -x "$root/bin/mx-launcher.sh" ] || {
    mx_launcher_error "code root is missing an executable launcher: $root/bin/mx-launcher.sh"
    return 1
  }
  if [ -L "$root/.git" ] || [ ! -d "$root/.git" ]; then
    mx_launcher_error "code root must be a plain checkout, not a linked worktree: $root"
    return 1
  fi
  top=$(git -C "$root" rev-parse --show-toplevel 2>/dev/null) || {
    mx_launcher_error "code root is not a git checkout: $root"
    return 1
  }
  top=$(mx_launcher_canonical_dir "$top" "git top level") || return 1
  [ "$top" = "$root" ] || {
    mx_launcher_error "code root must be the checkout top level: $root"
    return 1
  }
}

mx_launcher_validate_home() {
  local home=$1 part
  [ "$home" != / ] || {
    mx_launcher_error "operational home may not be the filesystem root"
    return 1
  }
  for part in config data projects state; do
    if [ -L "$home/$part" ] || [ ! -d "$home/$part" ]; then
      mx_launcher_error "operational home is missing a real $part directory: $home/$part"
      return 1
    fi
  done
}

mx_launcher_validate_managed_clean() {
  local root=$1 home=$2 dirty
  [ "$root" != "$home" ] || return 0
  dirty=$(git -C "$root" status --porcelain --untracked-files=normal 2>/dev/null) || {
    mx_launcher_error "cannot inspect managed runtime checkout: $root"
    return 1
  }
  [ -z "$dirty" ] || {
    mx_launcher_error "managed runtime checkout is dirty; inspect or repair it before launch: $root"
    return 1
  }
}

mx_launcher_runtime_is_managed() {
  [ "$(git -C "$1" config --local --get multplx.managed 2>/dev/null || true)" = true ]
}

# Search PATH as data rather than using command -v, which may resolve an
# exported shell function.  The optional skipped directory is the harness-shim
# directory and prevents a nested activation from capturing itself.
mx_launcher_find_executable() {
  local name=$1 skip_dir=${2:-} candidate directory
  candidate=$(type -P "$name" 2>/dev/null) || return 1
  case "$candidate" in
    /*) ;;
    *)
      directory=${candidate%/*}
      [ "$directory" != "$candidate" ] || directory=.
      directory=$(cd "$directory" 2>/dev/null && pwd -P) || return 1
      candidate=$directory/${candidate##*/}
      ;;
  esac
  [ -f "$candidate" ] && [ -x "$candidate" ] || return 1
  if [ -n "$skip_dir" ] && [ -e "$skip_dir/$name" ] && [ "$candidate" -ef "$skip_dir/$name" ]; then
    return 1
  fi
  printf '%s\n' "$candidate"
}

mx_launcher_prepend_path_once() {
  local wanted=$1 entry result= remaining=${PATH-} first=1 more
  while :; do
    case "$remaining" in
      *:*) entry=${remaining%%:*}; remaining=${remaining#*:}; more=1 ;;
      *) entry=$remaining; more=0 ;;
    esac
    if [ "$entry" != "$wanted" ]; then
      if [ "$first" -eq 1 ]; then
        result=$entry
        first=0
      else
        result=$result:$entry
      fi
    fi
    [ "$more" -eq 1 ] || break
  done
  if [ "$first" -eq 0 ]; then
    PATH=$wanted:$result
  else
    PATH=$wanted
  fi
  export PATH
}
