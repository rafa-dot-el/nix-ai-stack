# Reusable hardening check for one package (ADR-0002):
#  1. each makeWrapper script named in `wrappers` defaults every variable in `env`
#     (--set-default) and sets none of `forbidden`;
#  2. `probe` (a command, typically `<bin> --version`) makes no off-host connect()
#     while it runs in the network-less sandbox (strace). Startup only.
#  3. optionally, every variable name occurs in `binary`, so a switch the vendor
#     renamed fails the check instead of silently doing nothing (as
#     ANTIGRAVITY_DISABLE_AUTOUPDATE did).
# `wrappers` are paths to scripts or to shims that exec one (followed up to 3 hops).
# First-party binaries have no wrapper and no switches: `wrappers = [ ]`, `env = { }`, probe only.
{ lib, runCommand, strace, gnugrep }:
{ name, wrappers, env, forbidden ? [ ], probe ? null, binary ? null, extraCheck ? "" }:
let
  expectLines = lib.concatStringsSep "\n" (lib.mapAttrsToList (k: v: "${k}=${v}") env);
  firstKey = if env == { } then "" else lib.head (lib.attrNames env);
in
runCommand "check-hardening-${name}" { nativeBuildInputs = [ strace gnugrep ]; } ''
  set -euo pipefail
  wrapper_for() {
    f=$1
    for _ in 1 2 3 4; do
      grep -q 'export ${firstKey}=' "$f" && { echo "$f"; return; }
      f=$(grep -oE '^exec (-a "\$0" )?"/nix/store/[^"]+"' "$f" | grep -oE '/nix/store/[^"]+' | head -1 || true)
      [ -n "$f" ] || break
    done
    echo "[-] no hardened wrapper behind $1" >&2; return 1
  }
  for bin in ${lib.escapeShellArgs wrappers}; do
    w=$(wrapper_for "$bin")
    while IFS='=' read -r key value; do
      [ -n "$key" ] || continue
      grep -qF "export $key=\''${$key-'$value'}" "$w" \
        || { echo "[-] $bin: $key is not defaulted to $value"; exit 1; }
    done <<'VARS'
  ${expectLines}
  VARS
    for key in ${lib.escapeShellArgs forbidden}; do
      if grep -q "export $key=" "$w"; then echo "[-] $bin sets forbidden $key"; exit 1; fi
    done
    echo "[+] $(basename "$bin"): ${toString (lib.length (lib.attrNames env))} variables defaulted, none forbidden"
  done
  ${lib.optionalString (binary != null) ''
    for key in ${lib.escapeShellArgs (lib.attrNames env)}; do
      grep -aqF "$key" ${binary} || { echo "[-] ${binary} never mentions $key: renamed or removed upstream"; exit 1; }
    done
    echo "[+] every variable name occurs in the binary"
  ''}
  ${extraCheck}
  ${lib.optionalString (probe != null) ''
    export HOME=$TMPDIR XDG_CONFIG_HOME=$TMPDIR/config XDG_CACHE_HOME=$TMPDIR/cache
    strace -f -qq -e trace=connect -o $TMPDIR/connect.log ${probe} >$TMPDIR/probe.out 2>&1 || true
    outside=$(grep -E 'connect\(' $TMPDIR/connect.log | grep -E 'AF_INET6?' \
              | grep -vE 'inet_addr\("127\.|inet_pton\(AF_INET6, "::1"' || true)
    if [ -n "$outside" ]; then echo "[-] probe tried to reach the network:"; echo "$outside"; exit 1; fi
    echo "[+] probe: $(grep -c 'connect(' $TMPDIR/connect.log || true) connect() calls, none off-host"
  ''}
  touch $out
''
