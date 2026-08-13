# shellcheck shell=bash
# Source-compatible transport ABI for the Rust backlog owner.

MX_BACKLOG_SCRIPT_DIR=$(CDPATH='' cd -- "${BASH_SOURCE[0]%/*}" && pwd -P)
# shellcheck source=bin/mx-rust-runtime.sh
. "$MX_BACKLOG_SCRIPT_DIR/mx-rust-runtime.sh"

mx_backlog_rust() {
  local rust_bin
  rust_bin=$(mx_rust_runtime_bin) || return $?
  "$rust_bin" backlog "$@"
}

mx_backlog_backend_value() {
  local rust_bin
  rust_bin=$(mx_rust_runtime_bin) || return $?
  "$rust_bin" backlog-backend "$1"
}

mx_backlog_backend_manual() { [ "$(mx_backlog_backend_value "$1")" = manual ]; }
mx_backlog_backend_available() { ! mx_backlog_backend_manual "$1"; }
mx_backlog_validate() { mx_backlog_rust validate --file "$1"; }
mx_backlog_list() { mx_backlog_rust list --file "$1" --limit "$2"; }
mx_backlog_show() { mx_backlog_rust show "$2" --file "$1"; }

mx_backlog_add() {
  local file=$1 id=$2 title=$3
  shift 3
  mx_backlog_rust add "$id" "$title" --file "$file" "$@"
}

mx_backlog_done() {
  local file=$1 id=$2
  shift 2
  mx_backlog_rust done "$id" --file "$file" "$@"
}

mx_backlog_ready() { mx_backlog_rust ready --file "$1"; }

mx_backlog_hold() {
  local file=$1 id=$2
  shift 2
  mx_backlog_rust hold "$id" --file "$file" "$@"
}

mx_backlog_mv() {
  local source=$1 destination=$2
  shift 2
  mx_backlog_rust mv "$@" --file "$source" --to "$destination"
}

mx_backlog_update() {
  local file=$1 id=$2
  shift 2
  mx_backlog_rust update "$id" --file "$file" "$@"
}

mx_backlog_block() {
  local file=$1 id=$2
  shift 2
  mx_backlog_rust block "$id" --file "$file" "$@"
}

mx_backlog_unblock() {
  local file=$1 id=$2
  shift 2
  mx_backlog_rust unblock "$id" --file "$file" "$@"
}
