#!/usr/bin/env bash
# Shared validation and atomic artifact helpers for merge polling on GitHub,
# the only supported forge. Callers must validate task IDs and raw PR URLs
# before constructing task paths or performing any side effect.
#
# The stored identity is provider-tagged: provider, url, host, path, number.
# "path" is the full project path, which is owner/repository on GitHub. Only
# provider=github records are valid; any URL whose host is not github.com is a
# hard validation error, and a stored record carrying another provider tag
# (for example one written by an older install) is refused everywhere. Every
# consumer re-derives the identity from the stored URL and refuses any record
# whose parts do not reconstruct that exact URL.
#
# A validated exact merged result is retired through a private receipt only
# after its durable wake is appended.
# The receipt binds the terminal observation to the canonical registration and
# lets a restart finish fixed-path removal without executing state-file bytes.

MX_PR_PROVIDER=
MX_PR_URL=
MX_PR_HOST=
MX_PR_PATH=
MX_PR_OWNER=
MX_PR_REPO=
MX_PR_NUMBER=
MX_PR_DATA_PROVIDER=
MX_PR_DATA_URL=
MX_PR_DATA_HOST=
MX_PR_DATA_PATH=
MX_PR_DATA_NUMBER=
MX_PR_META_PROVIDER=
MX_PR_META_URL=
MX_PR_META_HOST=
MX_PR_META_PATH=
MX_PR_META_NUMBER=
MX_PR_REG_ID=
MX_PR_REG_PROVIDER=
MX_PR_REG_URL=
MX_PR_REG_HOST=
MX_PR_REG_PATH=
MX_PR_REG_NUMBER=
MX_PR_REG_DATA_HASH=
MX_PR_REG_TEMPLATE_HASH=
MX_PR_REG_DATA_IDENTITY=
MX_PR_REG_CHECK_IDENTITY=
MX_PR_POLL_DATA_TMP=
MX_PR_POLL_CHECK_TMP=
MX_PR_POLL_REG_TMP=
MX_PR_POLL_DATA_DEST=
MX_PR_POLL_CHECK_DEST=
MX_PR_POLL_REG_DEST=
MX_PR_POLL_EXPECT_ID=
MX_PR_POLL_EXPECT_PROVIDER=
MX_PR_POLL_EXPECT_URL=
MX_PR_POLL_EXPECT_HOST=
MX_PR_POLL_EXPECT_PATH=
MX_PR_POLL_EXPECT_NUMBER=
MX_PR_POLL_EXPECT_DATA_HASH=
MX_PR_POLL_EXPECT_TEMPLATE_HASH=
MX_PR_POLL_EXPECT_DATA_IDENTITY=
MX_PR_POLL_EXPECT_CHECK_IDENTITY=
MX_PR_POLL_TEMPLATE=
MX_PR_POLL_STATE_DEVICE=
MX_PR_POLL_SNAPSHOT_ID=
MX_PR_POLL_SNAPSHOT_PROVIDER=
MX_PR_POLL_SNAPSHOT_URL=
MX_PR_POLL_SNAPSHOT_HOST=
MX_PR_POLL_SNAPSHOT_PATH=
MX_PR_POLL_SNAPSHOT_NUMBER=
MX_PR_POLL_SNAPSHOT_DATA_HASH=
MX_PR_POLL_SNAPSHOT_TEMPLATE_HASH=
MX_PR_POLL_SNAPSHOT_DATA_IDENTITY=
MX_PR_POLL_SNAPSHOT_CHECK_IDENTITY=
MX_PR_POLL_SNAPSHOT_REG_HASH=
MX_PR_POLL_SNAPSHOT_REG_IDENTITY=
MX_PR_RETIRE_ID=
MX_PR_RETIRE_PROVIDER=
MX_PR_RETIRE_URL=
MX_PR_RETIRE_HOST=
MX_PR_RETIRE_PATH=
MX_PR_RETIRE_NUMBER=
MX_PR_RETIRE_DATA_HASH=
MX_PR_RETIRE_TEMPLATE_HASH=
MX_PR_RETIRE_DATA_IDENTITY=
MX_PR_RETIRE_CHECK_IDENTITY=
MX_PR_RETIRE_REG_HASH=
MX_PR_RETIRE_REG_IDENTITY=
MX_PR_RETIRE_RECEIPT_HASH=
MX_PR_RETIRE_RECEIPT_IDENTITY=
MX_PR_POLL_RETIREMENT_REJECTED=

mx_task_id_path_safe() {
  local id=${1-}
  local LC_ALL=C
  case "$id" in
    ''|.*|*[!A-Za-z0-9._-]*) return 1 ;;
  esac
}

mx_pr_task_id_valid() {
  local id=${1-}
  mx_task_id_path_safe "$id"
}

mx_task_id_creation_valid() {
  local id=${1-}
  mx_pr_task_id_valid "$id" || return 1
  [ "${#id}" -le 64 ]
}

# Parse a canonical GitHub pull request URL into the provider-tagged identity.
# Validation is strict: the GitHub username and repository rules are applied
# exactly, and any URL on another host is a hard validation error rather than
# a record for some other forge.
#
# MX_PR_OWNER and MX_PR_REPO are additionally set because bin/mx-pr-merge.sh
# addresses GitHub by owner/repository.
mx_pr_url_parse() {
  local raw=${1-} pattern
  local LC_ALL=C
  MX_PR_PROVIDER=
  MX_PR_URL=
  MX_PR_HOST=
  MX_PR_PATH=
  MX_PR_OWNER=
  MX_PR_REPO=
  MX_PR_NUMBER=
  pattern='^https://github\.com/([A-Za-z0-9]|[A-Za-z0-9][A-Za-z0-9-]{0,37}[A-Za-z0-9])/([A-Za-z0-9._-]{1,100})/pull/([1-9][0-9]*)$'
  if [[ "$raw" =~ $pattern ]]; then
    [[ "${BASH_REMATCH[1]}" != *--* ]] || return 1
    [ "${BASH_REMATCH[2]}" != . ] && [ "${BASH_REMATCH[2]}" != .. ] || return 1
    MX_PR_PROVIDER=github
    MX_PR_URL=$raw
    MX_PR_HOST=github.com
    MX_PR_PATH="${BASH_REMATCH[1]}/${BASH_REMATCH[2]}"
    # Consumed by bin/mx-pr-merge.sh, which addresses GitHub by owner/repository.
    # shellcheck disable=SC2034
    MX_PR_OWNER=${BASH_REMATCH[1]}
    # shellcheck disable=SC2034
    MX_PR_REPO=${BASH_REMATCH[2]}
    MX_PR_NUMBER=${BASH_REMATCH[3]}
    return 0
  fi
  return 1
}

mx_pr_head_valid() {
  local head=${1-}
  local LC_ALL=C
  [[ "$head" =~ ^[0-9a-f]{40}$|^[0-9a-f]{64}$ ]]
}

mx_pr_file_mode() {
  if [ "$(uname)" = Darwin ]; then
    stat -f %Lp "$1" 2>/dev/null
  else
    stat -c %a "$1" 2>/dev/null
  fi
}

mx_pr_file_device() {
  if [ "$(uname)" = Darwin ]; then
    stat -f %d "$1" 2>/dev/null
  else
    stat -c %d "$1" 2>/dev/null
  fi
}

mx_pr_file_link_count() {
  if [ "$(uname)" = Darwin ]; then
    stat -f %l "$1" 2>/dev/null
  else
    stat -c %h "$1" 2>/dev/null
  fi
}

mx_pr_file_inode() {
  if [ "$(uname)" = Darwin ]; then
    stat -f %i "$1" 2>/dev/null
  else
    stat -c %i "$1" 2>/dev/null
  fi
}

mx_pr_file_identity() {
  local device inode
  device=$(mx_pr_file_device "$1") || return 1
  inode=$(mx_pr_file_inode "$1") || return 1
  [ -n "$device" ] && [ -n "$inode" ] || return 1
  printf '%s:%s\n' "$device" "$inode"
}

mx_pr_sha256() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" 2>/dev/null | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" 2>/dev/null | awk '{print $1}'
  else
    return 1
  fi
}

mx_pr_private_file_valid() {
  local path=$1 mode=$2 device=$3
  [ -f "$path" ] && [ ! -L "$path" ] || return 1
  [ "$(mx_pr_file_mode "$path")" = "$mode" ] || return 1
  [ "$(mx_pr_file_device "$path")" = "$device" ] || return 1
  [ "$(mx_pr_file_link_count "$path")" = 1 ]
}

mx_pr_regular_destination_or_absent() {
  local path=$1
  [ ! -L "$path" ] || return 1
  if [ -e "$path" ]; then
    [ -f "$path" ] && [ "$(mx_pr_file_link_count "$path")" = 1 ]
  fi
}

mx_pr_regular_destination_on_device_or_absent() {
  local path=$1 device=$2
  mx_pr_regular_destination_or_absent "$path" || return 1
  [ ! -e "$path" ] || [ "$(mx_pr_file_device "$path")" = "$device" ]
}

mx_pr_metadata_identity_parse() {
  local file=$1 line value pr_count=0 seen_pr=0 post_pr_invalid=0
  MX_PR_META_PROVIDER=
  MX_PR_META_URL=
  MX_PR_META_HOST=
  MX_PR_META_PATH=
  MX_PR_META_NUMBER=
  [ -f "$file" ] && [ ! -L "$file" ] || return 1
  [ "$(mx_pr_file_link_count "$file")" = 1 ] || return 1
  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
      pr=*)
        pr_count=$((pr_count + 1))
        [ "$pr_count" -eq 1 ] || continue
        value=${line#pr=}
        if mx_pr_url_parse "$value"; then
          MX_PR_META_PROVIDER=$MX_PR_PROVIDER
          MX_PR_META_URL=$MX_PR_URL
          MX_PR_META_HOST=$MX_PR_HOST
          MX_PR_META_PATH=$MX_PR_PATH
          MX_PR_META_NUMBER=$MX_PR_NUMBER
        fi
        seen_pr=1
        ;;
      pr_head=*)
        if [ "$seen_pr" -eq 1 ]; then
          value=${line#pr_head=}
          mx_pr_head_valid "$value" || post_pr_invalid=1
        fi
        ;;
      x_request=*|x_request_ts=*|x_followups=*|x_platform=*|x_reply_max_chars=*)
        ;;
      *)
        [ "$seen_pr" -eq 0 ] || post_pr_invalid=1
        ;;
    esac
  done < "$file"
  [ "$pr_count" -eq 1 ] || return 1
  [ "$post_pr_invalid" -eq 0 ] || return 1
  [ -n "$MX_PR_META_URL" ]
}

# Sidecar layout: provider, url, host, path, number, one per line. A sidecar
# written before the provider tag existed has a URL on its first line and one
# line fewer, so it fails both the field count and the provider comparison and
# is refused rather than misread as a provider-tagged record.
mx_pr_poll_data_parse() {
  local file=$1 provider url host path number
  MX_PR_DATA_PROVIDER=
  MX_PR_DATA_URL=
  MX_PR_DATA_HOST=
  MX_PR_DATA_PATH=
  MX_PR_DATA_NUMBER=
  [ -f "$file" ] && [ ! -L "$file" ] || return 1
  exec 8< "$file" || return 1
  IFS= read -r provider <&8 || { exec 8<&-; return 1; }
  IFS= read -r url <&8 || { exec 8<&-; return 1; }
  IFS= read -r host <&8 || { exec 8<&-; return 1; }
  IFS= read -r path <&8 || { exec 8<&-; return 1; }
  IFS= read -r number <&8 || { exec 8<&-; return 1; }
  if IFS= read -r _extra <&8; then
    exec 8<&-
    return 1
  fi
  exec 8<&-
  mx_pr_url_parse "$url" || return 1
  [ "$provider" = "$MX_PR_PROVIDER" ] || return 1
  [ "$host" = "$MX_PR_HOST" ] || return 1
  [ "$path" = "$MX_PR_PATH" ] || return 1
  [ "$number" = "$MX_PR_NUMBER" ] || return 1
  MX_PR_DATA_PROVIDER=$MX_PR_PROVIDER
  MX_PR_DATA_URL=$MX_PR_URL
  MX_PR_DATA_HOST=$MX_PR_HOST
  MX_PR_DATA_PATH=$MX_PR_PATH
  MX_PR_DATA_NUMBER=$MX_PR_NUMBER
}

# Registration layout: version tag, task id, then the same provider-tagged
# identity as the sidecar, then the two hashes and the two file identities.
# The version tag moved to v2 with the provider tag, so a registration written
# by the previous release is recognised as old and refused. The non-executing
# migration in bin/mx-pr-check-migrate.sh then rebuilds that poll from the
# task's recorded pull request URL.
mx_pr_poll_registration_parse() {
  local file=$1 version id provider url host path number data_hash template_hash data_identity check_identity
  MX_PR_REG_ID=
  MX_PR_REG_PROVIDER=
  MX_PR_REG_URL=
  MX_PR_REG_HOST=
  MX_PR_REG_PATH=
  MX_PR_REG_NUMBER=
  MX_PR_REG_DATA_HASH=
  MX_PR_REG_TEMPLATE_HASH=
  MX_PR_REG_DATA_IDENTITY=
  MX_PR_REG_CHECK_IDENTITY=
  [ -f "$file" ] && [ ! -L "$file" ] || return 1
  exec 7< "$file" || return 1
  IFS= read -r version <&7 || { exec 7<&-; return 1; }
  IFS= read -r id <&7 || { exec 7<&-; return 1; }
  IFS= read -r provider <&7 || { exec 7<&-; return 1; }
  IFS= read -r url <&7 || { exec 7<&-; return 1; }
  IFS= read -r host <&7 || { exec 7<&-; return 1; }
  IFS= read -r path <&7 || { exec 7<&-; return 1; }
  IFS= read -r number <&7 || { exec 7<&-; return 1; }
  IFS= read -r data_hash <&7 || { exec 7<&-; return 1; }
  IFS= read -r template_hash <&7 || { exec 7<&-; return 1; }
  IFS= read -r data_identity <&7 || { exec 7<&-; return 1; }
  IFS= read -r check_identity <&7 || { exec 7<&-; return 1; }
  if IFS= read -r _extra <&7; then
    exec 7<&-
    return 1
  fi
  exec 7<&-
  [ "$version" = mx-pr-poll-registration-v2 ] || return 1
  mx_pr_task_id_valid "$id" || return 1
  mx_pr_url_parse "$url" || return 1
  [ "$provider" = "$MX_PR_PROVIDER" ] || return 1
  [ "$host" = "$MX_PR_HOST" ] || return 1
  [ "$path" = "$MX_PR_PATH" ] || return 1
  [ "$number" = "$MX_PR_NUMBER" ] || return 1
  [[ "$data_hash" =~ ^[0-9a-f]{64}$ ]] || return 1
  [[ "$template_hash" =~ ^[0-9a-f]{64}$ ]] || return 1
  [[ "$data_identity" =~ ^[0-9]+:[0-9]+$ ]] || return 1
  [[ "$check_identity" =~ ^[0-9]+:[0-9]+$ ]] || return 1
  MX_PR_REG_ID=$id
  MX_PR_REG_PROVIDER=$MX_PR_PROVIDER
  MX_PR_REG_URL=$MX_PR_URL
  MX_PR_REG_HOST=$MX_PR_HOST
  MX_PR_REG_PATH=$MX_PR_PATH
  MX_PR_REG_NUMBER=$MX_PR_NUMBER
  MX_PR_REG_DATA_HASH=$data_hash
  MX_PR_REG_TEMPLATE_HASH=$template_hash
  MX_PR_REG_DATA_IDENTITY=$data_identity
  MX_PR_REG_CHECK_IDENTITY=$check_identity
}

mx_pr_poll_cleanup() {
  [ -z "$MX_PR_POLL_DATA_TMP" ] || rm -f -- "$MX_PR_POLL_DATA_TMP"
  [ -z "$MX_PR_POLL_CHECK_TMP" ] || rm -f -- "$MX_PR_POLL_CHECK_TMP"
  [ -z "$MX_PR_POLL_REG_TMP" ] || rm -f -- "$MX_PR_POLL_REG_TMP"
  MX_PR_POLL_DATA_TMP=
  MX_PR_POLL_CHECK_TMP=
  MX_PR_POLL_REG_TMP=
}

mx_pr_poll_revoke_final() {
  local failed=0
  # Neutralize the runnable name first so a failed rearm cannot consume state
  # whose transactional registration did not commit successfully.
  if [ -e "$MX_PR_POLL_CHECK_DEST" ] || [ -L "$MX_PR_POLL_CHECK_DEST" ]; then
    rm -f -- "$MX_PR_POLL_CHECK_DEST" || failed=1
  fi
  if [ -e "$MX_PR_POLL_REG_DEST" ] || [ -L "$MX_PR_POLL_REG_DEST" ]; then
    rm -f -- "$MX_PR_POLL_REG_DEST" || failed=1
  fi
  if [ -e "$MX_PR_POLL_DATA_DEST" ] || [ -L "$MX_PR_POLL_DATA_DEST" ]; then
    rm -f -- "$MX_PR_POLL_DATA_DEST" || failed=1
  fi
  [ ! -e "$MX_PR_POLL_CHECK_DEST" ] && [ ! -L "$MX_PR_POLL_CHECK_DEST" ] || failed=1
  [ ! -e "$MX_PR_POLL_REG_DEST" ] && [ ! -L "$MX_PR_POLL_REG_DEST" ] || failed=1
  [ ! -e "$MX_PR_POLL_DATA_DEST" ] && [ ! -L "$MX_PR_POLL_DATA_DEST" ] || failed=1
  return "$failed"
}

mx_pr_poll_prepare() {
  local state=$1 id=$2 provider=$3 url=$4 host=$5 path=$6 number=$7 template=$8
  mx_pr_task_id_valid "$id" || return 1
  mx_pr_url_parse "$url" || return 1
  [ "$provider" = "$MX_PR_PROVIDER" ] || return 1
  [ "$host" = "$MX_PR_HOST" ] || return 1
  [ "$path" = "$MX_PR_PATH" ] || return 1
  [ "$number" = "$MX_PR_NUMBER" ] || return 1
  [ -f "$template" ] || return 1

  [ ! -L "$state" ] || return 1
  mkdir -p "$state" || return 1
  [ -d "$state" ] && [ ! -L "$state" ] || return 1
  umask 077
  MX_PR_POLL_DATA_DEST="$state/$id.pr-poll"
  MX_PR_POLL_CHECK_DEST="$state/$id.check.sh"
  MX_PR_POLL_REG_DEST="$state/$id.pr-poll-registration"
  MX_PR_POLL_EXPECT_ID=$id
  MX_PR_POLL_EXPECT_PROVIDER=$provider
  MX_PR_POLL_EXPECT_URL=$url
  MX_PR_POLL_EXPECT_HOST=$host
  MX_PR_POLL_EXPECT_PATH=$path
  MX_PR_POLL_EXPECT_NUMBER=$number
  MX_PR_POLL_TEMPLATE=$template
  MX_PR_POLL_STATE_DEVICE=$(mx_pr_file_device "$state") || return 1
  [ -n "$MX_PR_POLL_STATE_DEVICE" ] || return 1
  MX_PR_POLL_DATA_TMP=$(mktemp "$state/.mx-pr-poll-data.XXXXXX") || return 1
  MX_PR_POLL_CHECK_TMP=$(mktemp "$state/.mx-pr-poll-check.XXXXXX") || {
    mx_pr_poll_cleanup
    return 1
  }
  MX_PR_POLL_REG_TMP=$(mktemp "$state/.mx-pr-poll-registration.XXXXXX") || {
    mx_pr_poll_cleanup
    return 1
  }

  if ! printf '%s\n%s\n%s\n%s\n%s\n' "$provider" "$url" "$host" "$path" "$number" > "$MX_PR_POLL_DATA_TMP" \
    || ! chmod 0600 "$MX_PR_POLL_DATA_TMP" \
    || ! mx_pr_private_file_valid "$MX_PR_POLL_DATA_TMP" 600 "$MX_PR_POLL_STATE_DEVICE" \
    || ! mx_pr_poll_data_parse "$MX_PR_POLL_DATA_TMP" \
    || [ "$MX_PR_DATA_PROVIDER" != "$provider" ] \
    || [ "$MX_PR_DATA_URL" != "$url" ] \
    || [ "$MX_PR_DATA_HOST" != "$host" ] \
    || [ "$MX_PR_DATA_PATH" != "$path" ] \
    || [ "$MX_PR_DATA_NUMBER" != "$number" ] \
    || ! cp "$template" "$MX_PR_POLL_CHECK_TMP" \
    || ! chmod 0600 "$MX_PR_POLL_CHECK_TMP" \
    || ! mx_pr_private_file_valid "$MX_PR_POLL_CHECK_TMP" 600 "$MX_PR_POLL_STATE_DEVICE" \
    || ! cmp -s "$template" "$MX_PR_POLL_CHECK_TMP"; then
    mx_pr_poll_cleanup
    return 1
  fi
  MX_PR_POLL_EXPECT_DATA_HASH=$(mx_pr_sha256 "$MX_PR_POLL_DATA_TMP") || { mx_pr_poll_cleanup; return 1; }
  MX_PR_POLL_EXPECT_TEMPLATE_HASH=$(mx_pr_sha256 "$MX_PR_POLL_CHECK_TMP") || { mx_pr_poll_cleanup; return 1; }
  MX_PR_POLL_EXPECT_DATA_IDENTITY=$(mx_pr_file_identity "$MX_PR_POLL_DATA_TMP") || { mx_pr_poll_cleanup; return 1; }
  MX_PR_POLL_EXPECT_CHECK_IDENTITY=$(mx_pr_file_identity "$MX_PR_POLL_CHECK_TMP") || { mx_pr_poll_cleanup; return 1; }
  if ! printf '%s\n%s\n%s\n%s\n%s\n%s\n%s\n%s\n%s\n%s\n%s\n' \
      mx-pr-poll-registration-v2 "$id" "$provider" "$url" "$host" "$path" "$number" \
      "$MX_PR_POLL_EXPECT_DATA_HASH" "$MX_PR_POLL_EXPECT_TEMPLATE_HASH" \
      "$MX_PR_POLL_EXPECT_DATA_IDENTITY" "$MX_PR_POLL_EXPECT_CHECK_IDENTITY" \
      > "$MX_PR_POLL_REG_TMP" \
    || ! chmod 0600 "$MX_PR_POLL_REG_TMP" \
    || ! mx_pr_private_file_valid "$MX_PR_POLL_REG_TMP" 600 "$MX_PR_POLL_STATE_DEVICE" \
    || ! mx_pr_poll_registration_parse "$MX_PR_POLL_REG_TMP" \
    || [ "$MX_PR_REG_ID" != "$id" ] \
    || [ "$MX_PR_REG_DATA_HASH" != "$MX_PR_POLL_EXPECT_DATA_HASH" ] \
    || [ "$MX_PR_REG_TEMPLATE_HASH" != "$MX_PR_POLL_EXPECT_TEMPLATE_HASH" ]; then
    mx_pr_poll_cleanup
    return 1
  fi
}

mx_pr_poll_publish_prepared() {
  [ -n "$MX_PR_POLL_DATA_TMP" ] && [ -n "$MX_PR_POLL_CHECK_TMP" ] \
    && [ -n "$MX_PR_POLL_REG_TMP" ] || return 1
  mx_pr_regular_destination_on_device_or_absent "$MX_PR_POLL_DATA_DEST" "$MX_PR_POLL_STATE_DEVICE" || return 1
  mx_pr_regular_destination_on_device_or_absent "$MX_PR_POLL_REG_DEST" "$MX_PR_POLL_STATE_DEVICE" || return 1
  mx_pr_regular_destination_on_device_or_absent "$MX_PR_POLL_CHECK_DEST" "$MX_PR_POLL_STATE_DEVICE" || return 1

  if ! mv -f -- "$MX_PR_POLL_DATA_TMP" "$MX_PR_POLL_DATA_DEST"; then
    mx_pr_poll_revoke_final || true
    return 1
  fi
  MX_PR_POLL_DATA_TMP=
  if ! mx_pr_private_file_valid "$MX_PR_POLL_DATA_DEST" 600 "$MX_PR_POLL_STATE_DEVICE" \
    || [ "$(mx_pr_file_identity "$MX_PR_POLL_DATA_DEST")" != "$MX_PR_POLL_EXPECT_DATA_IDENTITY" ] \
    || [ "$(mx_pr_sha256 "$MX_PR_POLL_DATA_DEST")" != "$MX_PR_POLL_EXPECT_DATA_HASH" ] \
    || ! mx_pr_poll_data_parse "$MX_PR_POLL_DATA_DEST" \
    || [ "$MX_PR_DATA_PROVIDER" != "$MX_PR_POLL_EXPECT_PROVIDER" ] \
    || [ "$MX_PR_DATA_URL" != "$MX_PR_POLL_EXPECT_URL" ] \
    || [ "$MX_PR_DATA_HOST" != "$MX_PR_POLL_EXPECT_HOST" ] \
    || [ "$MX_PR_DATA_PATH" != "$MX_PR_POLL_EXPECT_PATH" ] \
    || [ "$MX_PR_DATA_NUMBER" != "$MX_PR_POLL_EXPECT_NUMBER" ]; then
    mx_pr_poll_revoke_final || true
    return 1
  fi

  if ! mv -f -- "$MX_PR_POLL_REG_TMP" "$MX_PR_POLL_REG_DEST"; then
    mx_pr_poll_revoke_final || true
    return 1
  fi
  MX_PR_POLL_REG_TMP=
  if ! mx_pr_private_file_valid "$MX_PR_POLL_REG_DEST" 600 "$MX_PR_POLL_STATE_DEVICE" \
    || ! mx_pr_poll_registration_parse "$MX_PR_POLL_REG_DEST" \
    || [ "$MX_PR_REG_ID" != "$MX_PR_POLL_EXPECT_ID" ] \
    || [ "$MX_PR_REG_PROVIDER" != "$MX_PR_POLL_EXPECT_PROVIDER" ] \
    || [ "$MX_PR_REG_URL" != "$MX_PR_POLL_EXPECT_URL" ] \
    || [ "$MX_PR_REG_HOST" != "$MX_PR_POLL_EXPECT_HOST" ] \
    || [ "$MX_PR_REG_PATH" != "$MX_PR_POLL_EXPECT_PATH" ] \
    || [ "$MX_PR_REG_NUMBER" != "$MX_PR_POLL_EXPECT_NUMBER" ] \
    || [ "$MX_PR_REG_DATA_HASH" != "$MX_PR_POLL_EXPECT_DATA_HASH" ] \
    || [ "$MX_PR_REG_TEMPLATE_HASH" != "$MX_PR_POLL_EXPECT_TEMPLATE_HASH" ] \
    || [ "$MX_PR_REG_DATA_IDENTITY" != "$MX_PR_POLL_EXPECT_DATA_IDENTITY" ] \
    || [ "$MX_PR_REG_CHECK_IDENTITY" != "$MX_PR_POLL_EXPECT_CHECK_IDENTITY" ]; then
    mx_pr_poll_revoke_final || true
    return 1
  fi

  if ! mx_pr_regular_destination_on_device_or_absent "$MX_PR_POLL_CHECK_DEST" "$MX_PR_POLL_STATE_DEVICE" \
    || ! mv -f -- "$MX_PR_POLL_CHECK_TMP" "$MX_PR_POLL_CHECK_DEST"; then
    mx_pr_poll_revoke_final || true
    return 1
  fi
  MX_PR_POLL_CHECK_TMP=
  if ! mx_pr_poll_artifacts_valid "${MX_PR_POLL_CHECK_DEST%/*}" "$MX_PR_POLL_EXPECT_ID" "$MX_PR_POLL_TEMPLATE"; then
    mx_pr_poll_revoke_final || true
    return 1
  fi
}

mx_pr_poll_artifacts_valid() {
  local state=$1 id=$2 template=$3 state_device check data registration meta data_hash template_hash data_identity check_identity
  mx_pr_task_id_valid "$id" || return 1
  [ -d "$state" ] && [ ! -L "$state" ] || return 1
  state_device=$(mx_pr_file_device "$state") || return 1
  check="$state/$id.check.sh"
  data="$state/$id.pr-poll"
  registration="$state/$id.pr-poll-registration"
  meta="$state/$id.meta"
  mx_pr_private_file_valid "$check" 600 "$state_device" || return 1
  mx_pr_private_file_valid "$data" 600 "$state_device" || return 1
  mx_pr_private_file_valid "$registration" 600 "$state_device" || return 1
  [ -f "$meta" ] && [ ! -L "$meta" ] || return 1
  [ "$(mx_pr_file_link_count "$meta")" = 1 ] || return 1
  cmp -s "$template" "$check" || return 1
  mx_pr_poll_data_parse "$data" || return 1
  data_hash=$(mx_pr_sha256 "$data") || return 1
  template_hash=$(mx_pr_sha256 "$check") || return 1
  data_identity=$(mx_pr_file_identity "$data") || return 1
  check_identity=$(mx_pr_file_identity "$check") || return 1
  mx_pr_poll_registration_parse "$registration" || return 1
  [ "$MX_PR_REG_ID" = "$id" ] || return 1
  [ "$MX_PR_REG_PROVIDER" = "$MX_PR_DATA_PROVIDER" ] || return 1
  [ "$MX_PR_REG_URL" = "$MX_PR_DATA_URL" ] || return 1
  [ "$MX_PR_REG_HOST" = "$MX_PR_DATA_HOST" ] || return 1
  [ "$MX_PR_REG_PATH" = "$MX_PR_DATA_PATH" ] || return 1
  [ "$MX_PR_REG_NUMBER" = "$MX_PR_DATA_NUMBER" ] || return 1
  [ "$MX_PR_REG_DATA_HASH" = "$data_hash" ] || return 1
  [ "$MX_PR_REG_TEMPLATE_HASH" = "$template_hash" ] || return 1
  [ "$MX_PR_REG_DATA_IDENTITY" = "$data_identity" ] || return 1
  [ "$MX_PR_REG_CHECK_IDENTITY" = "$check_identity" ] || return 1
  mx_pr_metadata_identity_parse "$meta" || return 1
  [ "$MX_PR_META_PROVIDER" = "$MX_PR_DATA_PROVIDER" ] || return 1
  [ "$MX_PR_META_URL" = "$MX_PR_DATA_URL" ] || return 1
  [ "$MX_PR_META_HOST" = "$MX_PR_DATA_HOST" ] || return 1
  [ "$MX_PR_META_PATH" = "$MX_PR_DATA_PATH" ] || return 1
  [ "$MX_PR_META_NUMBER" = "$MX_PR_DATA_NUMBER" ]
}

mx_pr_poll_snapshot_capture() {
  local state=$1 id=$2 template=$3 registration
  mx_pr_poll_artifacts_valid "$state" "$id" "$template" || return 1
  registration="$state/$id.pr-poll-registration"
  MX_PR_POLL_SNAPSHOT_REG_HASH=$(mx_pr_sha256 "$registration") || return 1
  MX_PR_POLL_SNAPSHOT_REG_IDENTITY=$(mx_pr_file_identity "$registration") || return 1
  MX_PR_POLL_SNAPSHOT_ID=$id
  MX_PR_POLL_SNAPSHOT_PROVIDER=$MX_PR_DATA_PROVIDER
  MX_PR_POLL_SNAPSHOT_URL=$MX_PR_DATA_URL
  MX_PR_POLL_SNAPSHOT_HOST=$MX_PR_DATA_HOST
  MX_PR_POLL_SNAPSHOT_PATH=$MX_PR_DATA_PATH
  MX_PR_POLL_SNAPSHOT_NUMBER=$MX_PR_DATA_NUMBER
  MX_PR_POLL_SNAPSHOT_DATA_HASH=$MX_PR_REG_DATA_HASH
  MX_PR_POLL_SNAPSHOT_TEMPLATE_HASH=$MX_PR_REG_TEMPLATE_HASH
  MX_PR_POLL_SNAPSHOT_DATA_IDENTITY=$MX_PR_REG_DATA_IDENTITY
  MX_PR_POLL_SNAPSHOT_CHECK_IDENTITY=$MX_PR_REG_CHECK_IDENTITY
}

mx_pr_poll_snapshot_matches() {
  local state=$1 id=$2 template=$3 registration reg_hash reg_identity
  [ -n "$MX_PR_POLL_SNAPSHOT_ID" ] && [ "$id" = "$MX_PR_POLL_SNAPSHOT_ID" ] || return 1
  mx_pr_poll_artifacts_valid "$state" "$id" "$template" || return 1
  registration="$state/$id.pr-poll-registration"
  reg_hash=$(mx_pr_sha256 "$registration") || return 1
  reg_identity=$(mx_pr_file_identity "$registration") || return 1
  [ "$MX_PR_DATA_PROVIDER" = "$MX_PR_POLL_SNAPSHOT_PROVIDER" ] || return 1
  [ "$MX_PR_DATA_URL" = "$MX_PR_POLL_SNAPSHOT_URL" ] || return 1
  [ "$MX_PR_DATA_HOST" = "$MX_PR_POLL_SNAPSHOT_HOST" ] || return 1
  [ "$MX_PR_DATA_PATH" = "$MX_PR_POLL_SNAPSHOT_PATH" ] || return 1
  [ "$MX_PR_DATA_NUMBER" = "$MX_PR_POLL_SNAPSHOT_NUMBER" ] || return 1
  [ "$MX_PR_REG_DATA_HASH" = "$MX_PR_POLL_SNAPSHOT_DATA_HASH" ] || return 1
  [ "$MX_PR_REG_TEMPLATE_HASH" = "$MX_PR_POLL_SNAPSHOT_TEMPLATE_HASH" ] || return 1
  [ "$MX_PR_REG_DATA_IDENTITY" = "$MX_PR_POLL_SNAPSHOT_DATA_IDENTITY" ] || return 1
  [ "$MX_PR_REG_CHECK_IDENTITY" = "$MX_PR_POLL_SNAPSHOT_CHECK_IDENTITY" ] || return 1
  [ "$reg_hash" = "$MX_PR_POLL_SNAPSHOT_REG_HASH" ] || return 1
  [ "$reg_identity" = "$MX_PR_POLL_SNAPSHOT_REG_IDENTITY" ]
}

mx_pr_poll_retirement_parse() {
  local file=$1 version id provider url host path number data_hash template_hash
  local data_identity check_identity reg_hash reg_identity result _extra
  MX_PR_RETIRE_ID=
  MX_PR_RETIRE_PROVIDER=
  MX_PR_RETIRE_URL=
  MX_PR_RETIRE_HOST=
  MX_PR_RETIRE_PATH=
  MX_PR_RETIRE_NUMBER=
  MX_PR_RETIRE_DATA_HASH=
  MX_PR_RETIRE_TEMPLATE_HASH=
  MX_PR_RETIRE_DATA_IDENTITY=
  MX_PR_RETIRE_CHECK_IDENTITY=
  MX_PR_RETIRE_REG_HASH=
  MX_PR_RETIRE_REG_IDENTITY=
  [ -f "$file" ] && [ ! -L "$file" ] || return 1
  exec 9< "$file" || return 1
  IFS= read -r version <&9 || { exec 9<&-; return 1; }
  IFS= read -r id <&9 || { exec 9<&-; return 1; }
  IFS= read -r provider <&9 || { exec 9<&-; return 1; }
  IFS= read -r url <&9 || { exec 9<&-; return 1; }
  IFS= read -r host <&9 || { exec 9<&-; return 1; }
  IFS= read -r path <&9 || { exec 9<&-; return 1; }
  IFS= read -r number <&9 || { exec 9<&-; return 1; }
  IFS= read -r data_hash <&9 || { exec 9<&-; return 1; }
  IFS= read -r template_hash <&9 || { exec 9<&-; return 1; }
  IFS= read -r data_identity <&9 || { exec 9<&-; return 1; }
  IFS= read -r check_identity <&9 || { exec 9<&-; return 1; }
  IFS= read -r reg_hash <&9 || { exec 9<&-; return 1; }
  IFS= read -r reg_identity <&9 || { exec 9<&-; return 1; }
  IFS= read -r result <&9 || { exec 9<&-; return 1; }
  if IFS= read -r _extra <&9; then
    exec 9<&-
    return 1
  fi
  exec 9<&-
  [ "$version" = mx-pr-poll-retirement-v1 ] || return 1
  mx_pr_task_id_valid "$id" || return 1
  mx_pr_url_parse "$url" || return 1
  [ "$provider" = "$MX_PR_PROVIDER" ] || return 1
  [ "$host" = "$MX_PR_HOST" ] || return 1
  [ "$path" = "$MX_PR_PATH" ] || return 1
  [ "$number" = "$MX_PR_NUMBER" ] || return 1
  [[ "$data_hash" =~ ^[0-9a-f]{64}$ ]] || return 1
  [[ "$template_hash" =~ ^[0-9a-f]{64}$ ]] || return 1
  [[ "$data_identity" =~ ^[0-9]+:[0-9]+$ ]] || return 1
  [[ "$check_identity" =~ ^[0-9]+:[0-9]+$ ]] || return 1
  [[ "$reg_hash" =~ ^[0-9a-f]{64}$ ]] || return 1
  [[ "$reg_identity" =~ ^[0-9]+:[0-9]+$ ]] || return 1
  [ "$result" = merged ] || return 1
  MX_PR_RETIRE_ID=$id
  MX_PR_RETIRE_PROVIDER=$provider
  MX_PR_RETIRE_URL=$url
  MX_PR_RETIRE_HOST=$host
  MX_PR_RETIRE_PATH=$path
  MX_PR_RETIRE_NUMBER=$number
  MX_PR_RETIRE_DATA_HASH=$data_hash
  MX_PR_RETIRE_TEMPLATE_HASH=$template_hash
  MX_PR_RETIRE_DATA_IDENTITY=$data_identity
  MX_PR_RETIRE_CHECK_IDENTITY=$check_identity
  MX_PR_RETIRE_REG_HASH=$reg_hash
  MX_PR_RETIRE_REG_IDENTITY=$reg_identity
}

mx_pr_poll_retirement_receipt_valid() {
  local state=$1 id=$2 receipt state_device meta
  mx_pr_task_id_valid "$id" || return 1
  [ -d "$state" ] && [ ! -L "$state" ] || return 1
  state_device=$(mx_pr_file_device "$state") || return 1
  receipt="$state/$id.pr-poll-retirement"
  mx_pr_private_file_valid "$receipt" 600 "$state_device" || return 1
  mx_pr_poll_retirement_parse "$receipt" || return 1
  [ "$MX_PR_RETIRE_ID" = "$id" ] || return 1
  meta="$state/$id.meta"
  mx_pr_metadata_identity_parse "$meta" || return 1
  [ "$MX_PR_META_PROVIDER" = "$MX_PR_RETIRE_PROVIDER" ] || return 1
  [ "$MX_PR_META_URL" = "$MX_PR_RETIRE_URL" ] || return 1
  [ "$MX_PR_META_HOST" = "$MX_PR_RETIRE_HOST" ] || return 1
  [ "$MX_PR_META_PATH" = "$MX_PR_RETIRE_PATH" ] || return 1
  [ "$MX_PR_META_NUMBER" = "$MX_PR_RETIRE_NUMBER" ] || return 1
  MX_PR_RETIRE_RECEIPT_HASH=$(mx_pr_sha256 "$receipt") || return 1
  MX_PR_RETIRE_RECEIPT_IDENTITY=$(mx_pr_file_identity "$receipt") || return 1
}

mx_pr_poll_retirement_data_valid() {
  local state=$1 id=$2 state_device data data_hash data_identity
  state_device=$(mx_pr_file_device "$state") || return 1
  data="$state/$id.pr-poll"
  mx_pr_private_file_valid "$data" 600 "$state_device" || return 1
  mx_pr_poll_data_parse "$data" || return 1
  data_hash=$(mx_pr_sha256 "$data") || return 1
  data_identity=$(mx_pr_file_identity "$data") || return 1
  [ "$MX_PR_DATA_PROVIDER" = "$MX_PR_RETIRE_PROVIDER" ] || return 1
  [ "$MX_PR_DATA_URL" = "$MX_PR_RETIRE_URL" ] || return 1
  [ "$MX_PR_DATA_HOST" = "$MX_PR_RETIRE_HOST" ] || return 1
  [ "$MX_PR_DATA_PATH" = "$MX_PR_RETIRE_PATH" ] || return 1
  [ "$MX_PR_DATA_NUMBER" = "$MX_PR_RETIRE_NUMBER" ] || return 1
  [ "$data_hash" = "$MX_PR_RETIRE_DATA_HASH" ] || return 1
  [ "$data_identity" = "$MX_PR_RETIRE_DATA_IDENTITY" ]
}

mx_pr_poll_retirement_registration_valid() {
  local state=$1 id=$2 state_device registration reg_hash reg_identity
  state_device=$(mx_pr_file_device "$state") || return 1
  registration="$state/$id.pr-poll-registration"
  mx_pr_private_file_valid "$registration" 600 "$state_device" || return 1
  mx_pr_poll_registration_parse "$registration" || return 1
  reg_hash=$(mx_pr_sha256 "$registration") || return 1
  reg_identity=$(mx_pr_file_identity "$registration") || return 1
  [ "$MX_PR_REG_ID" = "$id" ] || return 1
  [ "$MX_PR_REG_PROVIDER" = "$MX_PR_RETIRE_PROVIDER" ] || return 1
  [ "$MX_PR_REG_URL" = "$MX_PR_RETIRE_URL" ] || return 1
  [ "$MX_PR_REG_HOST" = "$MX_PR_RETIRE_HOST" ] || return 1
  [ "$MX_PR_REG_PATH" = "$MX_PR_RETIRE_PATH" ] || return 1
  [ "$MX_PR_REG_NUMBER" = "$MX_PR_RETIRE_NUMBER" ] || return 1
  [ "$MX_PR_REG_DATA_HASH" = "$MX_PR_RETIRE_DATA_HASH" ] || return 1
  [ "$MX_PR_REG_TEMPLATE_HASH" = "$MX_PR_RETIRE_TEMPLATE_HASH" ] || return 1
  [ "$MX_PR_REG_DATA_IDENTITY" = "$MX_PR_RETIRE_DATA_IDENTITY" ] || return 1
  [ "$MX_PR_REG_CHECK_IDENTITY" = "$MX_PR_RETIRE_CHECK_IDENTITY" ] || return 1
  [ "$reg_hash" = "$MX_PR_RETIRE_REG_HASH" ] || return 1
  [ "$reg_identity" = "$MX_PR_RETIRE_REG_IDENTITY" ]
}

mx_pr_poll_retirement_check_valid() {
  local state=$1 id=$2 state_device check check_hash check_identity
  state_device=$(mx_pr_file_device "$state") || return 1
  check="$state/$id.check.sh"
  mx_pr_private_file_valid "$check" 600 "$state_device" || return 1
  check_hash=$(mx_pr_sha256 "$check") || return 1
  check_identity=$(mx_pr_file_identity "$check") || return 1
  [ "$check_hash" = "$MX_PR_RETIRE_TEMPLATE_HASH" ] || return 1
  [ "$check_identity" = "$MX_PR_RETIRE_CHECK_IDENTITY" ]
}

mx_pr_poll_retirement_state_valid() {
  local state=$1 id=$2 check data registration has_check=0 has_data=0 has_registration=0
  mx_pr_poll_retirement_receipt_valid "$state" "$id" || return 1
  check="$state/$id.check.sh"
  data="$state/$id.pr-poll"
  registration="$state/$id.pr-poll-registration"
  [ ! -e "$check" ] && [ ! -L "$check" ] || has_check=1
  [ ! -e "$data" ] && [ ! -L "$data" ] || has_data=1
  [ ! -e "$registration" ] && [ ! -L "$registration" ] || has_registration=1
  if [ "$has_check" -eq 1 ]; then
    [ "$has_data" -eq 1 ] && [ "$has_registration" -eq 1 ] || return 1
    mx_pr_poll_retirement_check_valid "$state" "$id" || return 1
    mx_pr_poll_retirement_data_valid "$state" "$id" || return 1
    mx_pr_poll_retirement_registration_valid "$state" "$id" || return 1
    return 0
  fi
  if [ "$has_registration" -eq 1 ]; then
    [ "$has_data" -eq 1 ] || return 1
    mx_pr_poll_retirement_data_valid "$state" "$id" || return 1
    mx_pr_poll_retirement_registration_valid "$state" "$id" || return 1
    return 0
  fi
  [ "$has_data" -eq 0 ] || mx_pr_poll_retirement_data_valid "$state" "$id"
}

mx_pr_poll_retirement_remove_exact() {
  local path=$1 state_device=$2 expected_identity=$3 expected_hash=$4
  mx_pr_private_file_valid "$path" 600 "$state_device" || return 1
  [ "$(mx_pr_file_identity "$path")" = "$expected_identity" ] || return 1
  [ "$(mx_pr_sha256 "$path")" = "$expected_hash" ] || return 1
  rm -f -- "$path" || return 1
  [ ! -e "$path" ] && [ ! -L "$path" ]
}

mx_pr_poll_retirement_discard_obsolete() {
  local state=$1 id=$2 template=$3 receipt registration state_device
  local receipt_hash receipt_identity current_reg_hash current_reg_identity
  mx_pr_task_id_valid "$id" || return 1
  [ -d "$state" ] && [ ! -L "$state" ] || return 1
  state_device=$(mx_pr_file_device "$state") || return 1
  receipt="$state/$id.pr-poll-retirement"
  mx_pr_private_file_valid "$receipt" 600 "$state_device" || return 1
  mx_pr_poll_retirement_parse "$receipt" || return 1
  [ "$MX_PR_RETIRE_ID" = "$id" ] || return 1
  receipt_hash=$(mx_pr_sha256 "$receipt") || return 1
  receipt_identity=$(mx_pr_file_identity "$receipt") || return 1
  mx_pr_poll_artifacts_valid "$state" "$id" "$template" || return 1
  registration="$state/$id.pr-poll-registration"
  current_reg_hash=$(mx_pr_sha256 "$registration") || return 1
  current_reg_identity=$(mx_pr_file_identity "$registration") || return 1
  if [ "$current_reg_hash" = "$MX_PR_RETIRE_REG_HASH" ] \
    && [ "$current_reg_identity" = "$MX_PR_RETIRE_REG_IDENTITY" ] \
    && [ "$MX_PR_REG_DATA_IDENTITY" = "$MX_PR_RETIRE_DATA_IDENTITY" ] \
    && [ "$MX_PR_REG_CHECK_IDENTITY" = "$MX_PR_RETIRE_CHECK_IDENTITY" ]; then
    return 1
  fi
  mx_pr_poll_retirement_remove_exact "$receipt" "$state_device" \
    "$receipt_identity" "$receipt_hash"
}

mx_pr_poll_retirement_publish() {
  local state=$1 id=$2 template=$3 result=$4 receipt state_device tmp
  [ "$result" = merged ] || return 1
  mx_pr_poll_snapshot_matches "$state" "$id" "$template" || return 1
  state_device=$(mx_pr_file_device "$state") || return 1
  receipt="$state/$id.pr-poll-retirement"
  mx_pr_regular_destination_on_device_or_absent "$receipt" "$state_device" || return 1
  [ ! -e "$receipt" ] && [ ! -L "$receipt" ] || return 1
  umask 077
  tmp=$(mktemp "$state/.mx-pr-poll-retirement.XXXXXX") || return 1
  if ! printf '%s\n%s\n%s\n%s\n%s\n%s\n%s\n%s\n%s\n%s\n%s\n%s\n%s\n%s\n' \
      mx-pr-poll-retirement-v1 \
      "$MX_PR_POLL_SNAPSHOT_ID" \
      "$MX_PR_POLL_SNAPSHOT_PROVIDER" \
      "$MX_PR_POLL_SNAPSHOT_URL" \
      "$MX_PR_POLL_SNAPSHOT_HOST" \
      "$MX_PR_POLL_SNAPSHOT_PATH" \
      "$MX_PR_POLL_SNAPSHOT_NUMBER" \
      "$MX_PR_POLL_SNAPSHOT_DATA_HASH" \
      "$MX_PR_POLL_SNAPSHOT_TEMPLATE_HASH" \
      "$MX_PR_POLL_SNAPSHOT_DATA_IDENTITY" \
      "$MX_PR_POLL_SNAPSHOT_CHECK_IDENTITY" \
      "$MX_PR_POLL_SNAPSHOT_REG_HASH" \
      "$MX_PR_POLL_SNAPSHOT_REG_IDENTITY" \
      merged > "$tmp" \
    || ! chmod 0600 "$tmp" \
    || ! mx_pr_private_file_valid "$tmp" 600 "$state_device" \
    || ! mx_pr_poll_retirement_parse "$tmp" \
    || [ "$MX_PR_RETIRE_ID" != "$id" ] \
    || ! mx_pr_poll_snapshot_matches "$state" "$id" "$template" \
    || ! mx_pr_regular_destination_on_device_or_absent "$receipt" "$state_device" \
    || [ -e "$receipt" ] || [ -L "$receipt" ] \
    || ! mv -f -- "$tmp" "$receipt"; then
    rm -f -- "$tmp"
    return 1
  fi
  mx_pr_poll_retirement_receipt_valid "$state" "$id" || return 1
}

mx_pr_poll_retirement_recover_one() {
  local state=$1 id=$2 template=$3 receipt state_device check data registration
  local receipt_hash receipt_identity
  mx_pr_task_id_valid "$id" || return 1
  receipt="$state/$id.pr-poll-retirement"
  if [ ! -e "$receipt" ] && [ ! -L "$receipt" ]; then
    return 0
  fi
  if ! mx_pr_poll_retirement_state_valid "$state" "$id"; then
    mx_pr_poll_retirement_discard_obsolete "$state" "$id" "$template" && return 0
    return 1
  fi
  state_device=$(mx_pr_file_device "$state") || return 1
  check="$state/$id.check.sh"
  data="$state/$id.pr-poll"
  registration="$state/$id.pr-poll-registration"
  receipt_hash=$MX_PR_RETIRE_RECEIPT_HASH
  receipt_identity=$MX_PR_RETIRE_RECEIPT_IDENTITY
  if [ -e "$check" ] || [ -L "$check" ]; then
    mx_pr_poll_retirement_remove_exact "$check" "$state_device" \
      "$MX_PR_RETIRE_CHECK_IDENTITY" "$MX_PR_RETIRE_TEMPLATE_HASH" || return 1
  fi
  if [ -e "$registration" ] || [ -L "$registration" ]; then
    mx_pr_poll_retirement_remove_exact "$registration" "$state_device" \
      "$MX_PR_RETIRE_REG_IDENTITY" "$MX_PR_RETIRE_REG_HASH" || return 1
  fi
  if [ -e "$data" ] || [ -L "$data" ]; then
    mx_pr_poll_retirement_remove_exact "$data" "$state_device" \
      "$MX_PR_RETIRE_DATA_IDENTITY" "$MX_PR_RETIRE_DATA_HASH" || return 1
  fi
  mx_pr_poll_retirement_remove_exact "$receipt" "$state_device" \
    "$receipt_identity" "$receipt_hash" || return 1
  [ ! -e "$check" ] && [ ! -L "$check" ] \
    && [ ! -e "$registration" ] && [ ! -L "$registration" ] \
    && [ ! -e "$data" ] && [ ! -L "$data" ] \
    && [ ! -e "$receipt" ] && [ ! -L "$receipt" ]
}

mx_pr_poll_retirement_recover_all() {
  local state=$1 template=$2 receipt id
  MX_PR_POLL_RETIREMENT_REJECTED=
  for receipt in "$state"/*.pr-poll-retirement; do
    [ -e "$receipt" ] || [ -L "$receipt" ] || continue
    id=$(basename "$receipt" .pr-poll-retirement)
    if ! mx_pr_task_id_valid "$id" \
      || ! mx_pr_poll_retirement_recover_one "$state" "$id" "$template"; then
      MX_PR_POLL_RETIREMENT_REJECTED="$MX_PR_POLL_RETIREMENT_REJECTED $receipt"
    fi
  done
  [ -z "$MX_PR_POLL_RETIREMENT_REJECTED" ]
}
