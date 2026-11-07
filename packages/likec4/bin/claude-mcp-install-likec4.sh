#!@shell@
# claude-mcp-install-likec4 — register the LikeC4 MCP server for the current
# project in Claude Code (project scope: <repo root>/.mcp.json, committed).
#
#   claude-mcp-install-likec4 [--name NAME] [WORKSPACE]
#
# WORKSPACE (default: current directory) must contain a LikeC4 project
# (likec4.config.* or .likec4rc within 3 levels). Inside a git repository the
# entry is written repo-relative (`likec4-mcp --repo <path>`), so it works on
# every machine and from any subdirectory; outside one, the absolute path.
#
# Merges into an existing .mcp.json (other servers untouched) and is
# idempotent. Claude asks to approve new project servers on the next start;
# commit .mcp.json. Exit codes: 64 usage, 66 no LikeC4 project found.
set -eu

name=likec4
workspace=.
while [ $# -gt 0 ]; do
  case "$1" in
    --name) [ $# -ge 2 ] || { echo "[-] --name needs a value" >&2; exit 64; }; name="$2"; shift 2 ;;
    -h|--help) sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) echo "[-] unknown option: $1" >&2; exit 64 ;;
    *) workspace="$1"; shift ;;
  esac
done

workspace="$(cd "$workspace" 2>/dev/null && pwd -P)" || {
  echo "[-] claude-mcp-install-likec4: not a directory: $workspace" >&2; exit 66; }

if [ -z "$(find "$workspace" -maxdepth 3 \( -name 'likec4.config.*' -o -name '.likec4rc' \) \
             -not -path '*/node_modules/*' -print -quit)" ]; then
  echo "[-] claude-mcp-install-likec4: no LikeC4 project (likec4.config.*) under $workspace" >&2
  exit 66
fi

if root="$(cd "$workspace" && "@git@" rev-parse --show-toplevel 2>/dev/null)"; then
  root="$(cd "$root" && pwd -P)"
  rel="${workspace#"$root"}"; rel="${rel#/}"
  entry="$("@jq@" -n --arg p "${rel:-.}" '{type:"stdio",command:"likec4-mcp",args:["--repo",$p]}')"
else
  root="$workspace"
  entry="$("@jq@" -n --arg p "$workspace" '{type:"stdio",command:"likec4-mcp",args:[$p]}')"
fi

file="$root/.mcp.json"
[ -e "$file" ] || printf '{}\n' > "$file"
current="$("@jq@" -c --arg n "$name" '.mcpServers[$n] // empty' "$file")"
if [ "$current" = "$("@jq@" -c . <<<"$entry")" ]; then
  echo "[*] $file: '$name' already registered"
  exit 0
fi

tmp="$(mktemp "$file.XXXXXX")"
"@jq@" --arg n "$name" --argjson e "$entry" '.mcpServers[$n] = $e' "$file" > "$tmp"
mv "$tmp" "$file"
echo "[+] $file: '$name' -> $("@jq@" -c '.args' <<<"$entry")"
echo "[*] next: commit .mcp.json; approve the server when Claude Code next starts in $root"
