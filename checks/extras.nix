# Every requirement of every extra the package selects is installed in Hermes's own
# venv at a matching version (checks/extras.py). Catches extras upstream dropped or
# renamed (honcho, hindsight, vertex) at check time instead of in a failed release.
{ lib, runCommand, hermesPkg, hermesSrc }:
runCommand "check-extras" { } ''
  # Follow the exec chain (desktop shim -> makeWrapper -> upstream wrapper) to the venv.
  file=${hermesPkg}/bin/hermes python=""
  for _ in 1 2 3 4 5; do
    python=$(grep -oE '/nix/store/[a-z0-9]+-hermes-agent-env/bin/python3' "$file" | head -1 || true)
    [ -n "$python" ] && break
    file=$(grep -oE 'exec (-a "\$0" )?"/nix/store/[^"]+"' "$file" | grep -oE '/nix/store/[^"]+' | tail -1 || true)
    [ -n "$file" ] || break
  done
  [ -x "$python" ] || { echo "[-] could not locate Hermes's venv interpreter"; exit 1; }
  "$python" -I ${./extras.py} ${hermesSrc}/pyproject.toml \
    ${lib.escapeShellArgs hermesPkg.passthru.extraDependencyGroups}
  touch $out
''
