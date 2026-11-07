# playwright-mcp from nixpkgs (nixpkgs-unstable, the newer of the two channels: 0.0.76),
# with its browsers from the same nixpkgs, so nothing is downloaded at runtime.
# Replaces the Claude plugin's floating `npx @playwright/mcp@latest`. No telemetry or
# update check (verified in source). Exposed unchanged plus an MCP smoke test.
{ lib, playwright-mcp, runCommand, coreutils }:
playwright-mcp.overrideAttrs (old: {
  passthru = (old.passthru or { }) // {
    tests.smoke = runCommand "playwright-mcp-smoke" { nativeBuildInputs = [ coreutils ]; } ''
      export HOME=$TMPDIR
      reply=$(printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
        | timeout 60 ${lib.getExe playwright-mcp} --headless --isolated 2>$TMPDIR/stderr | head -1 || true)
      case "$reply" in
        *'"serverInfo"'*) echo "[+] initialize: $(echo "$reply" | cut -c1-160)" ;;
        *) echo "[-] no initialize reply: '$reply'"; cat $TMPDIR/stderr; exit 1 ;;
      esac
      touch $out
    '';
  };
})
