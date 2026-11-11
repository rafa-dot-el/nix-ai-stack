{ lib, stdenvNoCC, fetchurl, autoPatchelfHook, makeWrapper, procps
, ripgrep, bubblewrap, socat, runCommand }:

# Claude Code ships as a bun-compiled native binary via a GCS distribution.
# dontStrip is REQUIRED: stripping the ELF destroys the embedded bun bytecode,
# leaving only the raw bun runtime (which shows "Bun is a fast JS runtime...").
let
  hardening = import ../../lib/hardening.nix { inherit lib; };
in
stdenvNoCC.mkDerivation (finalAttrs: {
  pname = "claude-code";
  version = "2.1.322";

  # Same binary as downloads.claude.ai (claude.ai/install.sh): its manifest sha256 equals this hash (2.1.322, 2026-11-11).
  # The bucket name is undocumented; the hash pin is the guard.
  src = fetchurl {
    url = "https://storage.googleapis.com/claude-code-dist-86c565f3-f756-42ad-8dfa-d59b1c096819/claude-code-releases/${finalAttrs.version}/linux-x64/claude";
    hash = "sha256-v7NwOtU4AkIyhFm8Yq2kveiue6V8bh7q25wA4Ih0xHg=";
  };

  dontUnpack = true;
  dontBuild = true;
  dontStrip = true;

  # makeWrapper (a shell script, not makeBinaryWrapper) so checks can read the env it sets.
  nativeBuildInputs = [ autoPatchelfHook makeWrapper ];

  installPhase = ''
    runHook preInstall
    install -Dm755 $src $out/bin/claude
    runHook postInstall
  '';

  # Self-update, telemetry, error reporting and feedback prompts off (lib/hardening.nix);
  # CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC is deliberately not set: it disables Remote Control.
  postFixup = ''
    wrapProgram $out/bin/claude \
      ${hardening.toWrapperArgsString hardening.env.claude-code} \
      --set USE_BUILTIN_RIPGREP 0 \
      --unset DEV \
      --prefix PATH : ${lib.makeBinPath [ procps ripgrep bubblewrap socat ]}
  '';

  passthru = {
    hardeningEnv = hardening.env.claude-code;
    tests.smoke = runCommand "claude-code-smoke" { } ''
      export HOME=$TMPDIR
      out_text=$(${finalAttrs.finalPackage}/bin/claude --version)
      echo "$out_text" | grep -q '${finalAttrs.version}' || { echo "[-] claude --version: $out_text"; exit 1; }
      echo "[+] $out_text"
      touch $out
    '';
  };

  meta = {
    description =
      "Agentic coding tool that lives in your terminal, understands your codebase, and helps you code faster";
    homepage = "https://github.com/anthropics/claude-code";
    changelog = "https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md";
    license = lib.licenses.unfree;
    sourceProvenance = with lib.sourceTypes; [ binaryNativeCode ];
    mainProgram = "claude";
    platforms = [ "x86_64-linux" ];
  };
})
