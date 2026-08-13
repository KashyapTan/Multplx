# Zsh adapter for an activated Multplx child shell.
# The launcher copies this file to a private temporary ZDOTDIR.  It restores
# the user's original ZDOTDIR before sourcing the ordinary .zshrc exactly once,
# then removes the one-shot adapter and applies static presentation.

[[ -o interactive ]] || return

typeset -grx MX_ROOT_OVERRIDE MX_HOME MX_SHIM_DIR MX_REAL_CLAUDE MX_REAL_CODEX MX_REAL_CURSOR_AGENT MX_REAL_PI MULTPLX_ACTIVE MX_LAUNCH_VALIDATED
if [[ ${MX_LAUNCH_BACKEND_EXPLICIT:-0} == 1 ]]; then
  if [[ ${MX_LAUNCH_BACKEND_VALUE:-} == auto ]]; then
    unset MX_BACKEND
    typeset -gr MX_BACKEND
  else
    typeset -grx MX_BACKEND=$MX_LAUNCH_BACKEND_VALUE
  fi
fi
unset MX_LAUNCH_BACKEND_EXPLICIT MX_LAUNCH_BACKEND_VALUE

typeset __mx_adapter_dir=${MX_ZSH_ADAPTER_DIR:-}
typeset __mx_original_set=${MX_ORIGINAL_ZDOTDIR_SET:-0}
typeset __mx_original_zdotdir=${MX_ORIGINAL_ZDOTDIR:-}
if [[ $__mx_original_set == 1 ]]; then
  ZDOTDIR=$__mx_original_zdotdir
  export ZDOTDIR
else
  unset ZDOTDIR
fi

if [[ -n $__mx_adapter_dir && -f $__mx_adapter_dir/.zshrc && ! -L $__mx_adapter_dir/.zshrc ]]; then
  command rm -f -- "$__mx_adapter_dir/.zshrc"
  command rmdir -- "$__mx_adapter_dir" 2>/dev/null || true
fi
unset MX_ZSH_ADAPTER_DIR MX_ORIGINAL_ZDOTDIR_SET MX_ORIGINAL_ZDOTDIR

typeset __mx_user_zshrc
if [[ $__mx_original_set == 1 ]]; then
  __mx_user_zshrc=$__mx_original_zdotdir/.zshrc
else
  __mx_user_zshrc=${HOME:-}/.zshrc
fi
if [[ -n $__mx_user_zshrc && -r $__mx_user_zshrc ]]; then
  source "$__mx_user_zshrc"
fi
unset __mx_user_zshrc __mx_adapter_dir __mx_original_set __mx_original_zdotdir

typeset __mx_path_result=
typeset __mx_path_remaining=${PATH-}
typeset -i __mx_path_first=1
typeset -i __mx_path_more
typeset __mx_path_entry
while true; do
  if [[ $__mx_path_remaining == *:* ]]; then
    __mx_path_entry=${__mx_path_remaining%%:*}
    __mx_path_remaining=${__mx_path_remaining#*:}
    __mx_path_more=1
  else
    __mx_path_entry=$__mx_path_remaining
    __mx_path_more=0
  fi
  if [[ $__mx_path_entry != "$MX_SHIM_DIR" ]]; then
    if (( __mx_path_first )); then
      __mx_path_result=$__mx_path_entry
      __mx_path_first=0
    else
      __mx_path_result=$__mx_path_result:$__mx_path_entry
    fi
  fi
  (( __mx_path_more )) || break
done
if (( __mx_path_first )); then
  PATH=$MX_SHIM_DIR
else
  PATH=$MX_SHIM_DIR:$__mx_path_result
fi
export PATH
unset __mx_path_result __mx_path_remaining __mx_path_first __mx_path_more __mx_path_entry

if [[ -z ${MULTPLX_PROMPT_APPLIED:-} ]]; then
  if [[ -t 1 && -z ${NO_COLOR:-} && ${TERM:-} != dumb ]]; then
    RPROMPT="${RPROMPT:+$RPROMPT }%F{cyan}multplx%f"
  else
    RPROMPT="${RPROMPT:+$RPROMPT }multplx"
  fi
  typeset -grx MULTPLX_PROMPT_APPLIED=1
fi

case ${TERM:-} in
  xterm*|screen*|tmux*|rxvt*|alacritty*|kitty*)
    [[ -t 1 ]] && print -n -- $'\e]0;multplx\a'
    ;;
esac
