{ lib, buildNpmPackage, fetchFromGitHub, nodejs_22, makeWrapper, runCommand, coreutils, claude-code }:

let
  hardening = import ../../lib/hardening.nix { inherit lib; };
  # The Agent SDK passes its environment to the claude it spawns, so the claude-code
  # hardening applies here too, and CLAUDE_CODE_EXECUTABLE makes that claude the
  # hardened Nix one instead of the SDK's own bundled binary.
  env = hardening.env.claude-code // { CLAUDE_CODE_EXECUTABLE = lib.getExe claude-code; };
in
buildNpmPackage (finalAttrs: {
  pname = "claude-agent-acp";
  version = "0.99.1";

  src = fetchFromGitHub {
    owner = "agentclientprotocol";
    repo = "claude-agent-acp";
    tag = "v${finalAttrs.version}";
    hash = "sha256-Xj9QXZrqWe5njHlnoudfW0areQDfzk0VXx3psGukrxY=";
  };

  nodejs = nodejs_22;
  npmDepsHash = "sha256-g5M1nFzdDdHhv0cTN6YtCMGB05WagnwTv52hD9f6TH4=";

  nativeBuildInputs = [ makeWrapper ];

  npmBuildScript = "build";

  installPhase = ''
    runHook preInstall

    mkdir -p $out/{bin,lib/claude-agent-acp}
    cp -r dist $out/lib/claude-agent-acp/
    cp -r node_modules $out/lib/claude-agent-acp/
    cp package.json $out/lib/claude-agent-acp/

    makeWrapper ${lib.getExe finalAttrs.nodejs} $out/bin/claude-agent-acp \
      --add-flags "$out/lib/claude-agent-acp/dist/index.js" \
      ${hardening.toWrapperArgsString env}

    runHook postInstall
  '';

  passthru = {
    hardeningEnv = env;
    # An ACP client's first message: initialize must answer without any network or login.
    tests.smoke = runCommand "claude-agent-acp-smoke" { nativeBuildInputs = [ coreutils ]; } ''
      export HOME=$TMPDIR
      reply=$(printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{}}}' \
        | timeout 60 ${finalAttrs.finalPackage}/bin/claude-agent-acp 2>$TMPDIR/stderr | head -1 || true)
      case "$reply" in
        *'"protocolVersion"'*) echo "[+] initialize: $(echo "$reply" | cut -c1-160)" ;;
        *) echo "[-] no initialize reply: '$reply'"; cat $TMPDIR/stderr; exit 1 ;;
      esac
      touch $out
    '';
  };

  meta = {
    description = "ACP-compatible coding agent powered by the Claude Agent SDK";
    homepage = "https://github.com/agentclientprotocol/claude-agent-acp";
    changelog = "https://github.com/agentclientprotocol/claude-agent-acp/blob/main/CHANGELOG.md";
    license = lib.licenses.asl20;
    sourceProvenance = with lib.sourceTypes; [ fromSource ];
    platforms = lib.platforms.linux;
    mainProgram = "claude-agent-acp";
  };
})
