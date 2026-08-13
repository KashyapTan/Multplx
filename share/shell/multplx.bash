# Bash adapter for an activated Multplx child shell.
# This file is loaded with bash --rcfile, sources the user's ordinary .bashrc
# exactly once, then applies static presentation and the shim PATH.

case $- in
  *i*) ;;
  *) return ;;
esac

readonly MX_ROOT_OVERRIDE MX_HOME MX_SHIM_DIR MX_REAL_CLAUDE MX_REAL_CODEX MX_REAL_CURSOR_AGENT MX_REAL_PI MULTPLX_ACTIVE MX_LAUNCH_VALIDATED
export MX_ROOT_OVERRIDE MX_HOME MX_SHIM_DIR MX_REAL_CLAUDE MX_REAL_CODEX MX_REAL_CURSOR_AGENT MX_REAL_PI MULTPLX_ACTIVE MX_LAUNCH_VALIDATED
if [ "${MX_LAUNCH_BACKEND_EXPLICIT:-0}" = 1 ]; then
  if [ "${MX_LAUNCH_BACKEND_VALUE:-}" = auto ]; then
    unset MX_BACKEND
    readonly MX_BACKEND
  else
    MX_BACKEND=$MX_LAUNCH_BACKEND_VALUE
    readonly MX_BACKEND
    export MX_BACKEND
  fi
fi
unset MX_LAUNCH_BACKEND_EXPLICIT MX_LAUNCH_BACKEND_VALUE

__mx_user_bashrc=${MX_BASH_USER_RC:-${HOME:-}/.bashrc}
if [ -n "$__mx_user_bashrc" ] && [ -r "$__mx_user_bashrc" ]; then
  # shellcheck disable=SC1090
  . "$__mx_user_bashrc"
fi
unset __mx_user_bashrc

__mx_path_result=
__mx_path_remaining=${PATH-}
__mx_path_first=1
while :; do
  case "$__mx_path_remaining" in
    *:*)
      __mx_path_entry=${__mx_path_remaining%%:*}
      __mx_path_remaining=${__mx_path_remaining#*:}
      __mx_path_more=1
      ;;
    *)
      __mx_path_entry=$__mx_path_remaining
      __mx_path_more=0
      ;;
  esac
  if [ "$__mx_path_entry" != "$MX_SHIM_DIR" ]; then
    if [ "$__mx_path_first" -eq 1 ]; then
      __mx_path_result=$__mx_path_entry
      __mx_path_first=0
    else
      __mx_path_result=$__mx_path_result:$__mx_path_entry
    fi
  fi
  [ "$__mx_path_more" -eq 1 ] || break
done
if [ "$__mx_path_first" -eq 0 ]; then
  PATH=$MX_SHIM_DIR:$__mx_path_result
else
  PATH=$MX_SHIM_DIR
fi
export PATH
unset __mx_path_result __mx_path_remaining __mx_path_first __mx_path_entry __mx_path_more

if [ -z "${MULTPLX_PROMPT_APPLIED:-}" ]; then
  if [ -t 1 ] && [ -z "${NO_COLOR:-}" ] && [ "${TERM:-}" != dumb ]; then
    PS1="${PS1-}\[\033[36m\] multplx\[\033[0m\] "
  else
    PS1="${PS1-} multplx "
  fi
  MULTPLX_PROMPT_APPLIED=1
  readonly MULTPLX_PROMPT_APPLIED
  export MULTPLX_PROMPT_APPLIED
fi

case ${TERM:-} in
  xterm*|screen*|tmux*|rxvt*|alacritty*|kitty*)
    [ -t 1 ] && printf '\033]0;multplx\007'
    ;;
esac
