{
  lib,
  stdenv,
  fetchFromGitHub,
  nodejs_22,
  pnpm_10_latest,
  npmHooks,
  makeWrapper,
  runCommand,
}:

let
  hardening = import ../../lib/hardening.nix { inherit lib; };
in
stdenv.mkDerivation (finalAttrs: {
  pname = "openspec";
  version = "1.17.0";

  src = fetchFromGitHub {
    owner = "Fission-AI";
    repo = "OpenSpec";
    tag = "v${finalAttrs.version}";
    hash = "sha256-UcPNglXW8pB67d1S4EcG1sn7ZOXLXSuLhoDcFjno2a0=";
  };

  pnpmDeps = pnpm_10_latest.fetchDeps {
    inherit (finalAttrs) pname version src;
    fetcherVersion = 3;
    hash = "sha256-+Hqb9DnxsCZkAjb3fOfYJiPpfoX+m4MDKlF02Uxs2bw=";
  };

  nativeBuildInputs = [
    nodejs_22
    npmHooks.npmInstallHook
    pnpm_10_latest.configHook
    pnpm_10_latest
    makeWrapper
  ];

  buildPhase = ''
    runHook preBuild
    pnpm run build
    runHook postBuild
  '';

  # Upstream's own npm packaging metadata already lists exactly what ships
  # (dist/bin/schemas); running npm prune would strip devDependencies used
  # only by the build step but also trips over the pnpm-managed store layout.
  dontNpmPrune = true;

  # Telemetry and the update check off (lib/hardening.nix).
  postFixup = ''
    wrapProgram $out/bin/openspec ${hardening.toWrapperArgsString hardening.env.openspec}
  '';

  passthru = {
    hardeningEnv = hardening.env.openspec;
    tests.smoke = runCommand "openspec-smoke" { } ''
      export HOME=$TMPDIR XDG_CONFIG_HOME=$TMPDIR/config
      out_text=$(${finalAttrs.finalPackage}/bin/openspec --version)
      [ "$out_text" = "${finalAttrs.version}" ] || { echo "[-] openspec --version: $out_text"; exit 1; }
      echo "[+] openspec --version: $out_text"
      touch $out
    '';
  };

  meta = {
    description = "AI-native system for spec-driven development";
    homepage = "https://github.com/Fission-AI/OpenSpec";
    changelog = "https://github.com/Fission-AI/OpenSpec/releases/tag/v${finalAttrs.version}";
    license = lib.licenses.mit;
    sourceProvenance = with lib.sourceTypes; [ fromSource ];
    platforms = lib.platforms.all;
    mainProgram = "openspec";
  };
})
