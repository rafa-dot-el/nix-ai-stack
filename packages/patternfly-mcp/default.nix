{ lib, buildNpmPackage, fetchFromGitHub, nodejs_22, coreutils, runCommand }:

# PatternFly's MCP server (official patternfly org, MIT), built from the tagged
# source with its own package-lock.json. Replaces the runtime `npx -y
# @patternfly/patternfly-mcp@<version>` the Cursor plugin used. No telemetry or
# update check (its "stats" go to node:diagnostics_channel in-process); it does
# fetch PatternFly docs from raw.githubusercontent.com when a tool asks for them.
buildNpmPackage (finalAttrs: {
  pname = "patternfly-mcp";
  version = "2.5.0";

  src = fetchFromGitHub {
    owner = "patternfly";
    repo = "patternfly-mcp";
    tag = "v${finalAttrs.version}";
    hash = "sha256-iJDn1n4W6jW3WTEd9cXfwP+WVbWOcIOjXDnsI0yGPe8=";
  };

  nodejs = nodejs_22;
  npmDepsHash = "sha256-NcP+Pl0GT86UMsJ95kMbwxrT9aL84X/Xj0OGO6kboSs=";

  # The lock does not pin an optional eslint peer, which npm otherwise tries to
  # resolve from the registry (ENOTCACHED offline).
  npmFlags = [ "--legacy-peer-deps" ];

  # `npm run build` = clean + tsc --noEmit + pkgroll into dist/.
  npmBuildScript = "build";

  passthru.tests.smoke = runCommand "patternfly-mcp-smoke" { nativeBuildInputs = [ coreutils ]; } ''
    export HOME=$TMPDIR
    reply=$(printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
      | timeout 60 ${finalAttrs.finalPackage}/bin/patternfly-mcp 2>$TMPDIR/stderr | head -1 || true)
    case "$reply" in
      *'"serverInfo"'*) echo "[+] initialize: $(echo "$reply" | cut -c1-160)" ;;
      *) echo "[-] no initialize reply: '$reply'"; cat $TMPDIR/stderr; exit 1 ;;
    esac
    touch $out
  '';

  meta = {
    description = "Model Context Protocol server for PatternFly documentation and components";
    homepage = "https://github.com/patternfly/patternfly-mcp";
    license = lib.licenses.mit;
    sourceProvenance = with lib.sourceTypes; [ fromSource ];
    platforms = lib.platforms.linux;
    mainProgram = "patternfly-mcp";
  };
})
