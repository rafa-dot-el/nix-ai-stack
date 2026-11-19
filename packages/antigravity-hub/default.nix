{
  lib,
  stdenv,
  fetchurl,
  autoPatchelfHook,
  makeWrapper,
  addDriverRunpath,
  alsa-lib,
  at-spi2-atk,
  at-spi2-core,
  atk,
  cairo,
  cups,
  dbus,
  expat,
  glib,
  gtk3,
  libgbm,
  libseccomp,
  libcap_ng,
  libxkbcommon,
  nspr,
  nss,
  pango,
  systemd,
  xorg,
  libGL,
  libnotify,
  libsecret,
  libuuid,
  pciutils,
  pipewire,
  wayland,
}:

# Google Antigravity hub (desktop app): an Electron 44 tree shipped as a
# tarball by the Antigravity CDN. Same technique as packages/claude-desktop:
# unpack the tree as-is, autoPatchelf the bundled Electron/Chromium ELFs, and
# wrap the main binary so resources/ and /proc/self/exe resolve in the store.
let
  hardening = import ../../lib/hardening.nix { inherit lib; };
in
stdenv.mkDerivation (finalAttrs: {
  pname = "antigravity-hub";
  version = "2.33.0";

  src = fetchurl {
    url = "https://storage.googleapis.com/antigravity-public/antigravity-hub/2.33.0-4731201358004224/linux-x64/Antigravity.tar.gz";
    hash = "sha256-pzOHsinGsfUoiGBzETPFsm4VM0Dq7ZbYZQMs2KhTUFA=";
  };

  sourceRoot = "Antigravity-x64";

  nativeBuildInputs = [
    autoPatchelfHook
    makeWrapper
  ];

  buildInputs = [
    alsa-lib
    at-spi2-atk
    at-spi2-core
    atk
    cairo
    cups
    dbus
    expat
    glib
    gtk3
    libcap_ng
    libgbm
    libseccomp
    libxkbcommon
    nspr
    nss
    pango
    stdenv.cc.cc.lib
    systemd
    xorg.libX11
    xorg.libXcomposite
    xorg.libXdamage
    xorg.libXext
    xorg.libXfixes
    xorg.libXrandr
    xorg.libxcb
    pipewire
  ];

  # dlopen'd at runtime, so autoPatchelf does not see them in DT_NEEDED.
  runtimeDependencies = map lib.getLib [
    libGL
    libnotify
    libsecret
    libuuid
    pciutils
    pipewire
    wayland
  ];

  dontConfigure = true;
  dontBuild = true;
  dontStrip = true;

  # The tarball carries chrome-sandbox with the SUID bit; the store cannot hold
  # it, and Chromium falls back to the user-namespace sandbox (the NixOS default).
  installPhase = ''
    runHook preInstall
    mkdir -p $out/lib/antigravity-hub
    cp -a . $out/lib/antigravity-hub/
    chmod u+w $out/lib/antigravity-hub/chrome-sandbox
    chmod u-s $out/lib/antigravity-hub/chrome-sandbox
    runHook postInstall
  '';

  preFixup = ''
    addAutoPatchelfSearchPath "$out/lib/antigravity-hub"
  '';

  appendRunpaths = [
    "${lib.getLib libGL}/lib"
    "${addDriverRunpath.driverLink}/lib"
  ];

  postFixup = ''
    makeWrapper $out/lib/antigravity-hub/antigravity $out/bin/antigravity-hub \
      --prefix VK_ADD_DRIVER_FILES : "${addDriverRunpath.driverLink}/share/vulkan/icd.d"
  '';

  passthru = {
    # No environment switch for the hub's updater or telemetry has been verified
    # yet; see the README note in this directory before shipping it.
    hardeningEnv = { };
  };

  meta = {
    description = "Google Antigravity hub: desktop app (Electron), repackaged from the official tarball";
    homepage = "https://antigravity.google";
    license = lib.licenses.unfree;
    sourceProvenance = with lib.sourceTypes; [ binaryNativeCode ];
    platforms = [ "x86_64-linux" ];
    mainProgram = "antigravity-hub";
  };
})
