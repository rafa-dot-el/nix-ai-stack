{
  lib,
  stdenv,
  callPackage,
  fetchFromGitHub,
  rustPlatform,
  cmake,
  dbus,
  libxcb,
  pkg-config,
  protobuf,
  openssl,
  versionCheckHook,
  llvmPackages,
  makeWrapper,
  librusty_v8 ? callPackage ./librusty_v8.nix {
    inherit (callPackage ./fetchers.nix { }) fetchLibrustyV8;
  },

  # Extension(s) Dependencies
  python3,
  bash,
  # X11
  xdotool,
  wmctrl,
  xclip,
  xwininfo,
  # Wayland
  wtype,
  wl-clipboard,
}:

# Vendored from nixpkgs (pkgs/by-name/go/goose-cli at 941a8e3e, 1.55.0) and bumped:
# neither of this flake's channels carries a current goose. Changes from nixpkgs:
# version, hashes, the hardening environment (lib/hardening.nix), no updateScript.
let
  hardening = import ../../lib/hardening.nix { inherit lib; };
in
rustPlatform.buildRustPackage (finalAttrs: {
  pname = "goose-cli";
  version = "1.59.0";

  src = fetchFromGitHub {
    owner = "aaif-goose";
    repo = "goose";
    tag = "v${finalAttrs.version}";
    hash = "sha256-0fAyv3ppEbe2Z2FAg8Y4NS4blEwpVbDd+Bz/ie1nwhk=";
  };

  cargoHash = "sha256-L7SP+UuLpNAkC9hQigz8zHC9g/TrY183lpwy81V698I=";

  cargoBuildFlags = [
    "--bin"
    "goose"
  ];

  nativeBuildInputs = [
    cmake
    pkg-config
    protobuf
    rustPlatform.bindgenHook
    makeWrapper
  ];

  buildInputs = [
    dbus
    openssl
  ]
  ++ lib.optionals stdenv.hostPlatform.isLinux [ libxcb ];

  env = {
    LIBCLANG_PATH = "${lib.getLib llvmPackages.libclang}/lib";
    RUSTY_V8_ARCHIVE = librusty_v8;
  };

  postFixup = ''
    wrapProgram $out/bin/goose \
      ${hardening.toWrapperArgsString hardening.env.goose} \
      --prefix PATH : ${
        lib.makeBinPath (
          [
            bash
            python3
          ]
          ++ lib.optionals stdenv.hostPlatform.isLinux [
            # X11
            xdotool
            wmctrl
            xclip
            xwininfo
            # Wayland
            wtype
            wl-clipboard
          ]
        )
      }
  '';

  # Upstream does not gate releases on this test suite, and keeping it running
  # here means maintaining a long list of per-test skips on every release: the
  # tests race on process-global state, need dbus/keychain, network or loopback
  # access, and several are flaky on aarch64-darwin. Skipping the check phase
  # on all platforms makes tracking upstream releases feasible; the version
  # install check below still exercises the binary.
  # https://github.com/NixOS/nixpkgs/pull/569940
  doCheck = false;

  nativeInstallCheckInputs = [ versionCheckHook ];
  versionCheckProgramArg = "--version";
  doInstallCheck = true;

  passthru.hardeningEnv = hardening.env.goose;

  meta = {
    description = "Open-source, extensible AI agent that goes beyond code suggestions - install, execute, edit, and test with any LLM";
    homepage = "https://github.com/aaif-goose/goose";
    changelog = "https://github.com/aaif-goose/goose/releases/tag/${finalAttrs.src.tag}";
    mainProgram = "goose";
    license = lib.licenses.asl20;
    maintainers = with lib.maintainers; [
      asiantuntija
      cloudripper
      thardin
      brittonr
      miniharinn
      caniko
    ];
    platforms = [
      "x86_64-linux"
      "aarch64-linux"
      "aarch64-darwin"
    ];
  };
})
