#!@shell@
# likec4-mcp — LikeC4 MCP server over stdio for agent sessions.
#
#   likec4-mcp [WORKSPACE]      workspace as given (default $LIKEC4_WORKSPACE, else .)
#   likec4-mcp --repo PATH      PATH relative to the git repository root of the
#                               current directory, so a committed .mcp.json works
#                               whichever subdirectory the agent was started in
#
# Written by claude-mcp-install-likec4 / hermes-mcp-install-likec4.
# --no-watch: static workspace; the model is re-read on the next session.
# Note: the `apply-semantic-layout` tool is not annotated read-only; review
# `git diff` after a session that used it.
set -eu

if [ "${1:-}" = "--repo" ]; then
  [ $# -eq 2 ] || { echo "[-] likec4-mcp: usage: likec4-mcp --repo PATH" >&2; exit 64; }
  root="$("@git@" rev-parse --show-toplevel 2>/dev/null)" || {
    echo "[-] likec4-mcp: --repo needs a git repository; $PWD is not in one" >&2
    exit 66
  }
  workspace="$root/$2"
else
  workspace="${1:-${LIKEC4_WORKSPACE:-.}}"
fi

exec "@out@/bin/likec4" mcp --stdio --no-watch "$workspace"
