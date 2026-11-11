{ lib, stdenvNoCC, fetchurl, autoPatchelfHook, makeWrapper, runCommand }:

let
  hardening = import ../../lib/hardening.nix { inherit lib; };
in
stdenvNoCC.mkDerivation (finalAttrs: {
  pname = "antigravity-cli";
  version = "1.5.5";

  src = fetchurl {
    url = "https://storage.googleapis.com/antigravity-public/antigravity-cli/1.5.5-6740358537805824/linux-x64/cli_linux_x64.tar.gz";
    hash = "sha256-v+LCXt6t2YwpQkF8mXLuDh+qPwr9yQcR6E1fOmM7UHg=";
  };

  sourceRoot = ".";

  nativeBuildInputs = [ autoPatchelfHook makeWrapper ];

  dontBuild = true;
  dontStrip = true;

  installPhase = ''
    runHook preInstall
    install -Dm755 antigravity $out/bin/antigravity
    runHook postInstall
  '';

  # AGY_CLI_DISABLE_AUTO_UPDATE is the documented switch; the old
  # ANTIGRAVITY_DISABLE_AUTOUPDATE never appears in the binary.
  postFixup = ''
    wrapProgram $out/bin/antigravity ${hardening.toWrapperArgsString hardening.env.antigravity-cli}
    ln -s $out/bin/antigravity $out/bin/agy
  '';

  passthru = {
    hardeningEnv = hardening.env.antigravity-cli;
    tests.smoke = runCommand "antigravity-cli-smoke" { } ''
      export HOME=$TMPDIR
      out_text=$(${finalAttrs.finalPackage}/bin/agy --version 2>&1) || { echo "[-] agy --version: $out_text"; exit 1; }
      echo "[+] agy --version: $out_text"
      touch $out
    '';
  };

  meta = {
    description = "Google Antigravity CLI — agentic coding tool for your terminal";
    homepage = "https://antigravity.google";
    license = lib.licenses.unfree;
    sourceProvenance = with lib.sourceTypes; [ binaryNativeCode ];
    platforms = [ "x86_64-linux" ];
    mainProgram = "antigravity";
  };
})
