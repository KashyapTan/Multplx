#!/usr/bin/env bash

MX_CUSTOM_CHECK_HASH=
MX_CUSTOM_CHECK_SNAPSHOT=

mx_custom_check_sha256() {
  local file=$1
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" 2>/dev/null | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" 2>/dev/null | awk '{print $1}'
  else
    return 1
  fi
}

mx_custom_check_trust_read() {
  local state=$1 id=$2 trust state_device version hash
  MX_CUSTOM_CHECK_HASH=
  mx_pr_task_id_valid "$id" || return 1
  [ -d "$state" ] && [ ! -L "$state" ] || return 1
  state_device=$(mx_pr_file_device "$state") || return 1
  trust="$state/$id.check-trust"
  mx_pr_private_file_valid "$trust" 600 "$state_device" || return 1
  exec 9< "$trust" || return 1
  IFS= read -r version <&9 || { exec 9<&-; return 1; }
  IFS= read -r hash <&9 || { exec 9<&-; return 1; }
  if IFS= read -r _extra <&9; then
    exec 9<&-
    return 1
  fi
  exec 9<&-
  [ "$version" = mx-custom-check-v1 ] || return 1
  [[ "$hash" =~ ^[0-9a-f]{64}$ ]] || return 1
  MX_CUSTOM_CHECK_HASH=$hash
}

mx_custom_check_registered() {
  local state=$1 id=$2 check hash state_device
  check="$state/$id.check.sh"
  mx_custom_check_trust_read "$state" "$id" || return 1
  state_device=$(mx_pr_file_device "$state") || return 1
  mx_pr_private_file_valid "$check" 700 "$state_device" || return 1
  hash=$(mx_custom_check_sha256 "$check") || return 1
  [ "$hash" = "$MX_CUSTOM_CHECK_HASH" ]
}

mx_custom_check_snapshot_prepare() {
  local state=$1 id=$2 check hash state_device
  mx_custom_check_snapshot_cleanup
  check="$state/$id.check.sh"
  mx_custom_check_trust_read "$state" "$id" || return 1
  state_device=$(mx_pr_file_device "$state") || return 1
  mx_pr_private_file_valid "$check" 700 "$state_device" || return 1
  MX_CUSTOM_CHECK_SNAPSHOT=$(mktemp "$state/.mx-custom-check.XXXXXX") || return 1
  cp "$check" "$MX_CUSTOM_CHECK_SNAPSHOT" || { mx_custom_check_snapshot_cleanup; return 1; }
  chmod 0600 "$MX_CUSTOM_CHECK_SNAPSHOT" || { mx_custom_check_snapshot_cleanup; return 1; }
  [ -f "$MX_CUSTOM_CHECK_SNAPSHOT" ] && [ ! -L "$MX_CUSTOM_CHECK_SNAPSHOT" ] \
    || { mx_custom_check_snapshot_cleanup; return 1; }
  [ "$(mx_pr_file_mode "$MX_CUSTOM_CHECK_SNAPSHOT")" = 600 ] \
    || { mx_custom_check_snapshot_cleanup; return 1; }
  [ "$(mx_pr_file_device "$MX_CUSTOM_CHECK_SNAPSHOT")" = "$state_device" ] \
    || { mx_custom_check_snapshot_cleanup; return 1; }
  [ "$(mx_pr_file_link_count "$MX_CUSTOM_CHECK_SNAPSHOT")" = 1 ] \
    || { mx_custom_check_snapshot_cleanup; return 1; }
  hash=$(mx_custom_check_sha256 "$MX_CUSTOM_CHECK_SNAPSHOT") \
    || { mx_custom_check_snapshot_cleanup; return 1; }
  [ "$hash" = "$MX_CUSTOM_CHECK_HASH" ] || { mx_custom_check_snapshot_cleanup; return 1; }
}

mx_custom_check_snapshot_cleanup() {
  [ -z "$MX_CUSTOM_CHECK_SNAPSHOT" ] || rm -f -- "$MX_CUSTOM_CHECK_SNAPSHOT"
  MX_CUSTOM_CHECK_SNAPSHOT=
}
