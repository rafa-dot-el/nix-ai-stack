#!@shell@
# likec4-validate-hook — Claude Code PostToolUse hook: after an Edit/Write of a
# *.c4 file, validate the LikeC4 workspace it belongs to and feed errors back.
#
# Register in settings.json:
#   "PostToolUse": [{ "matcher": "Edit|MultiEdit|Write",
#                     "hooks": [{ "type": "command", "command": "likec4-validate-hook", "timeout": 60 }] }]
#
# Workspace: walking up from the file, the first directory that is a project
# (has likec4.config.*) -- or its parent when a sibling directory is another
# project (cross-project imports) -- or the first directory with a project as
# a direct child (config-less dirs such as shared/). Stops at the git root or
# $HOME. No workspace found: silent pass.
#
# Runs `likec4 validate --no-layout` (syntax + semantics, every project, ~1 s
# on the estate); layout drift stays with `task check`. Exit 2 with the error
# lines on stderr is shown to Claude; exit 0 otherwise.
set -u

file="$("@jq@" -r '.tool_input.file_path // empty' 2>/dev/null)" || exit 0
case "$file" in *.c4|*.likec4) ;; *) exit 0 ;; esac

has_config() {
  for c in "$1"/likec4.config.json "$1"/likec4.config.ts "$1"/likec4.config.js \
           "$1"/likec4.config.mjs "$1"/likec4.config.mts "$1"/.likec4rc; do
    [ -e "$c" ] && return 0
  done
  return 1
}

child_has_config() {  # $1 dir, $2 child to skip (optional)
  for d in "$1"/*/; do
    d="${d%/}"
    [ -d "$d" ] && [ "$d" != "${2:-}" ] && has_config "$d" && return 0
  done
  return 1
}

dir="$(cd "$(dirname "$file")" 2>/dev/null && pwd -P)" || exit 0
stop="$("@git@" -C "$dir" rev-parse --show-toplevel 2>/dev/null || true)"
home="$(cd "$HOME" 2>/dev/null && pwd -P || echo /)"
workspace=
while :; do
  if has_config "$dir"; then
    parent="$(dirname "$dir")"
    if child_has_config "$parent" "$dir"; then workspace="$parent"; else workspace="$dir"; fi
    break
  fi
  if child_has_config "$dir"; then workspace="$dir"; break; fi
  [ "$dir" = / ] || [ "$dir" = "$stop" ] || [ "$dir" = "$home" ] && break
  dir="$(dirname "$dir")"
done
[ -n "$workspace" ] || exit 0

out="$("@out@/bin/likec4" validate --no-layout "$workspace" 2>&1)" && exit 0
{
  echo "likec4 validate failed for workspace $workspace (after editing $file):"
  printf '%s\n' "$out" | sed 's/\x1b\[[0-9;]*m//g' | grep -v -E '^[0-9:.]+ +INFO ' | head -40
} >&2
exit 2
