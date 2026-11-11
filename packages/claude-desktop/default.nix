{
  lib,
  stdenv,
  fetchurl,
  dpkg,
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
  libayatana-appindicator,
  libnotify,
  libpulseaudio,
  libsecret,
  libuuid,
  libxtst,
  pciutils,
  pipewire,
  wayland,
  # Chromium's OSCrypt backend picks a secret-service implementation by
  # sniffing XDG_CURRENT_DESKTOP/DESKTOP_SESSION for known strings
  # ("GNOME", "KDE", ...); on anything else (bspwm, i3, sway, riced
  # window managers in general) it can't tell which backend to use and
  # silently falls back to plaintext storage, even when a real secret
  # service (gnome-keyring, kwallet) is running and reachable over
  # D-Bus. Force the backend explicitly so login sessions persist.
  # Override to `[ "--password-store=kwallet6" ]` (or "kwallet5") on a
  # KDE-only host, or `[ ]` to defer to Chromium's own detection.
  commandLineArgs ? [ "--password-store=gnome-libsecret" ],
}:

# Claude Desktop (Chat, Cowork, and Code tabs) ships for Linux only as an
# official .deb from Anthropic's apt.stable repo -- Debian/Ubuntu only, no
# Fedora/Arch/NixOS support upstream, and no source published (Electron app;
# see the add-nix-package skill's Phase 0 supply-chain notes). We never run
# dpkg's maintainer scripts (postinst registers an apt repo + AppArmor
# profile that make no sense outside dpkg); instead we unpack the .deb's
# data.tar directly and autoPatchelf the bundled Electron/Chromium tree --
# same technique nixpkgs uses for discord/vscode-bin.
let
  hardening = import ../../lib/hardening.nix { inherit lib; };
in
stdenv.mkDerivation rec {
  pname = "claude-desktop";
  version = "5.1231.2";

  src = fetchurl {
    url = "https://downloads.claude.ai/claude-desktop/apt/stable/pool/main/c/claude-desktop/claude-desktop_${version}_amd64.deb";
    hash = "sha256-600Wya/wppY2bYhyj3T6VRNr58/ePB/jWvQGK0WVW28=";
  };

  nativeBuildInputs = [
    dpkg
    autoPatchelfHook
    makeWrapper
  ];

  # DT_NEEDED of the main Electron ELF (verified with patchelf --print-needed
  # on the unpacked v1.24012.9 binary; re-verified with a full-tree ldd scan
  # for missing dependencies on v1.52386.3 -- same set holds).
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
    stdenv.cc.cc.lib # libstdc++/libgcc_s (node-pty prebuild)
    systemd # libudev.so.1
    xorg.libX11
    xorg.libXcomposite
    xorg.libXdamage
    xorg.libXext
    xorg.libXfixes
    xorg.libXrandr
    xorg.libxcb
    # DT_NEEDED by claude-native-binding.node since 4.x (was dlopen-only before).
    pipewire
  ];

  # dlopen'd at runtime, not in DT_NEEDED, so autoPatchelf's automatic
  # runtime-dep detection won't find these on its own.
  runtimeDependencies = map lib.getLib [
    libGL
    libayatana-appindicator
    libnotify
    libpulseaudio
    libsecret
    libuuid
    pciutils
    pipewire
    wayland
    libxtst
  ];

  # chrome-sandbox ships SUID 4755 in the official tree; the Nix store
  # cannot represent SUID bits. dpkg-deb --fsys-tarfile + `tar
  # --no-same-permissions` applies the umask instead of failing the build.
  # Chromium falls back to the unprivileged user-namespace sandbox on
  # kernels with it enabled (the NixOS default), so this is not a sandbox
  # downgrade on this repo's hosts.
  unpackPhase = ''
    runHook preUnpack
    dpkg-deb --fsys-tarfile "$src" | tar -x --no-same-owner --no-same-permissions
    runHook postUnpack
  '';

  dontConfigure = true;
  dontBuild = true;
  dontStrip = true; # already stripped upstream

  installPhase = ''
    runHook preInstall
    mkdir -p $out
    cp -a usr/lib $out/
    mkdir -p $out/share
    cp -a usr/share/applications $out/share/
    cp -a usr/share/icons $out/share/
    runHook postInstall
  '';

  # usr/bin/claude-desktop upstream is a relative symlink into usr/lib; we
  # replace it with a makeWrapper script so /proc/self/exe still resolves
  # inside this store path (process.resourcesPath stays correct) while
  # adding the NixOS Vulkan ICD dir to the loader search path. The shipped
  # .desktop uses `Exec=claude-desktop %U` (PATH-relative), so it resolves
  # this wrapper from any profile without patching.
  postInstall = ''
    makeWrapper $out/lib/claude-desktop/claude-desktop $out/bin/claude-desktop \
      --add-flags ${lib.escapeShellArg (lib.concatStringsSep " " commandLineArgs)} \
      --prefix VK_ADD_DRIVER_FILES : "${addDriverRunpath.driverLink}/share/vulkan/icd.d" \
      ${hardening.toWrapperArgsString hardening.env.claude-desktop}
  '';

  # The co-located libs (libffmpeg.so, libEGL.so, libGLESv2.so,
  # libvk_swiftshader.so, virtiofsd, chrome-native-host, node-pty/pty.node,
  # claude-native-binding.node) live under lib/claude-desktop/{.,resources,
  # app.asar.unpacked/...}; autoPatchelf needs the explicit search path
  # since RPATH on the main ELF is only $ORIGIN.
  preFixup = ''
    addAutoPatchelfSearchPath "$out/lib/claude-desktop"
  '';

  # Chromium's bundled ANGLE (libEGL.so/libGLESv2.so) dlopen()s the glvnd
  # dispatcher by soname at GPU-init time; a dlopen resolves against the
  # *calling* object's runpath, not the main ELF's, so this must land on
  # every patched ELF, not just executables (runtimeDependencies only
  # reaches executables).
  appendRunpaths = [
    "${lib.getLib libGL}/lib"
    "${addDriverRunpath.driverLink}/lib"
  ];

  passthru = {
    hardeningEnv = hardening.env.claude-desktop;
    managedSettings = hardening.claudeDesktopManagedSettings;
  };

  meta = {
    description = "Claude Desktop for Linux (official .deb, repackaged) -- Chat, Cowork, and Code tabs";
    homepage = "https://claude.ai";
    downloadPage = "https://downloads.claude.ai/claude-desktop/apt/stable";
    changelog = "https://code.claude.com/docs/en/desktop-linux";
    license = lib.licenses.unfree;
    sourceProvenance = with lib.sourceTypes; [ binaryNativeCode ];
    platforms = [ "x86_64-linux" ];
    mainProgram = "claude-desktop";
  };
}
