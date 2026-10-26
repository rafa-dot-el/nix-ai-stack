# Wrapper over upstream hermes-agent with: Chromium, Piper TTS, browser-use CLI,
# LSP servers, and clipboard tools pre-baked. Upstream's own uv2nix handles CLI/TUI/web/ACP.
# Piper activation: config.yaml { tts: { provider: piper, piper: { voice: <passthru.piperVoicePath> } } }
#
{ lib, stdenv, makeWrapper, chromium, hermes, pkgsHermesRev
, pyright, typescript-language-server, gopls, rust-analyzer, nixd, clang-tools, xclip, xsel, pkgs
# Feature toggles — a service (NixOS module) that doesn't need every extra
# builds a smaller closure via these instead of always getting hermesFull's
# everything. Each maps to real pyproject.toml dependency groups (verified
# against upstream's own pyproject.toml, not guessed): "voice" -> withVoice,
# "computer-use" -> withBrowser (it's just the MCP client bits for
# cua-driver), "messaging"/"matrix" -> withGateway.
, withLsp ? true
, withVoice ? true
, withBrowser ? true
, withGateway ? true
, withDesktop ? true
}:

let
  inherit (pkgs.callPackage ./browser.nix { inherit chromium; })
    browserUse agentBrowser cuaDriver buzzCli;

  browserEnsureDebug = pkgs.callPackage ./browser-cdp.nix { inherit chromium; };

  # Cheap, instant --run hook for every `hermes`/`hermes-agent`/`hermes-acp`
  # launch: only computes and exports the CDP URL Hermes should aim at if it
  # ever needs one — never probes or launches Chromium here. Hermes's own
  # browser_exec resolution (tools/browser_use_cli.py:_resolve_backend_cdp)
  # reads BROWSER_CDP_URL from THIS process's env, but only when the agent
  # actually calls a browser tool, at which point it spawns the browser-use
  # CLI as a fresh subprocess — THAT wrapper (browser.nix) is where the real
  # probe-and-launch happens, lazily, exactly once per actual need. Doing the
  # heavy work here instead would launch a whole Chromium on every `hermes`
  # invocation, including ones that never touch a browser tool at all.
  browserCdpAutoRun = ''
    if [ -z "''${BROWSER_CDP_URL:-}" ] && [ -z "''${BU_CDP_URL:-}" ] && [ -z "''${BU_CDP_WS:-}" ]; then
      _hermes_cdp_url="$(${browserEnsureDebug}/bin/hermes-ensure-browser-debug --print-only 2>/dev/null)" || _hermes_cdp_url=""
      if [ -n "$_hermes_cdp_url" ]; then
        export BROWSER_CDP_URL="$_hermes_cdp_url"
        # Marks BROWSER_CDP_URL as this wrapper's own default, so the lazy
        # launch in browser.nix still runs and can make it live.
        export HERMES_BROWSER_CDP_AUTO=1
      fi
    fi
  '';

  inherit (pkgs.callPackage ./voice.nix { inherit pkgsHermesRev; })
    hermes-voice-dependencies piperVoiceDir piperVoiceName
    fasterWhisperModelDir fasterWhisperModelName;

  # Clipboard tools (xclip/xsel) stay unconditional — small, and not clearly
  # part of any one toggle. Everything else on PATH is gated by feature.
  extraPathTools =
    lib.optionals withBrowser [ chromium browserUse buzzCli cuaDriver agentBrowser ]
    ++ lib.optionals withLsp [
      pyright
      typescript-language-server
      gopls
      rust-analyzer
      nixd
      clang-tools # provides clangd
    ]
    ++ [ xclip xsel ];

  base = hermes.minimal.override {
    # Mirrors upstream's own "full" package (nix/packages.nix) minus
    # edge-tts/tts-premium, since Piper replaces the cloud TTS backends here.
    extraDependencyGroups = [
      "anthropic"
      "azure-identity"
      "bedrock"
      "daytona"
      "dingtalk"
      "exa"
      "fal"
      "feishu"
      "firecrawl"
      "modal"
      "parallel-web"
      "vercel"
    ]
    ++ lib.optionals withVoice [ "voice" ]
    # computer-use already in "all"; kept explicit to match upstream's name.
    ++ lib.optionals withBrowser [ "computer-use" ]
    ++ lib.optionals withGateway ([ "messaging" ] ++ lib.optionals stdenv.isLinux [ "matrix" ]);
    extraPythonPackages = lib.optionals withVoice hermes-voice-dependencies;
  };

  # self closes the loop: hermesDesktop must reference THIS wrapped binary,
  # not the pre-wrap base. Lazy evaluation ensures self is fully defined when forced.
  self = base.overrideAttrs (old: {
    nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ [ makeWrapper ];

    postFixup = ''
      ${old.postFixup or ""}
      for bin in hermes hermes-agent hermes-acp; do
        wrapProgram "$out/bin/$bin" \
          --suffix PATH : "${lib.makeBinPath extraPathTools}" \
          ${lib.optionalString withBrowser "--run ${lib.escapeShellArg browserCdpAutoRun}"}
      done
    ''
    # Baked into $out as actual build inputs, not just inert metadata.
    + lib.optionalString withVoice ''
      ln -s ${piperVoiceDir} "$out/share/hermes-agent/piper-voices"
      ln -s ${fasterWhisperModelDir} "$out/share/hermes-agent/faster-whisper-${fasterWhisperModelName}"
    '';

    passthru = (old.passthru or { })
      // lib.optionalAttrs withVoice {
        piperVoicePath = "${piperVoiceDir}/${piperVoiceName}.onnx";
        fasterWhisperModelPath = "${fasterWhisperModelDir}";
      }
      // {
        # Lazy — merely naming this attribute doesn't force building the
        # Electron app; only dereferencing its store path does (which
        # `final` below does, and only when withDesktop is true).
        hermesDesktop = old.passthru.hermesDesktop.override { hermesAgent = self; };
      };
  });

  # desktop.nix's runtime-detecting `hermes-desktop` launcher (x11/wayland
  # variants + a picker that execs the right one by WAYLAND_DISPLAY/
  # XDG_SESSION_TYPE), built here against THIS wrapper's
  # self.passthru.hermesDesktop rather than duplicating the platform-detection
  # logic — so hermesFull ships a working launcher + .desktop entry itself,
  # not just as the separate hermes-desktop-x11/-wayland/-desktop flake
  # outputs (which remain for a desktop-app-only install).
  desktopVariants = import ./desktop.nix {
    inherit pkgs;
    hermesDesktop = self.passthru.hermesDesktop;
  };

  # `hermes desktop` / `hermes gui` are source-tree workflows: they run
  # npm install + electron-builder inside PROJECT_ROOT (the read-only store
  # path), so they can never work from a Nix build — hence the bare
  # "Desktop GUI source not found" error. The desktop app on Nix is the
  # prebuilt `hermesDesktop` Electron package instead. This final layer
  # re-exposes self and rewrites those two subcommands to launch the
  # packaged app. It's a separate derivation (not an overrideAttrs on
  # self) precisely because it references self.passthru.hermesDesktop —
  # folding it into self would create a self→desktop→self cycle.
  #
  # Skipped entirely when withDesktop is false (e.g. the NixOS dashboard/
  # gateway services, which never need the Electron app on a headless
  # host) — self's own postFixup never dereferences hermesDesktop's store
  # path, so building `self` alone never pulls in the desktop closure.
  final = pkgs.runCommand "hermes-agent-with-desktop" { } ''
    mkdir -p $out

    # bin/ and share/ are rebuilt below (not one top-level symlink each) so
    # we can add/override individual entries (hermes, hermes-desktop,
    # share/applications, share/icons) while still exposing everything else
    # self provides.
    for d in ${self}/*; do
      case "$(basename "$d")" in
        bin|share) ;;
        *) ln -s "$d" "$out/$(basename "$d")" ;;
      esac
    done

    mkdir -p "$out/bin"
    for b in ${self}/bin/*; do
      ln -s "$b" "$out/bin/$(basename "$b")"
    done
    rm -f "$out/bin/hermes"

    mkdir -p "$out/share"
    if [ -d "${self}/share" ]; then
      for d in ${self}/share/*; do
        ln -s "$d" "$out/share/$(basename "$d")"
      done
    fi

    # Bundle the desktop launcher + a real application-menu entry so a
    # plain `pkgs.hermes` install has a working Exec/Icon out of the box —
    # previously nothing under share/applications shipped at all.
    ln -s ${desktopVariants.hermes-desktop}/bin/hermes-desktop "$out/bin/hermes-desktop"
    ln -s ${self.passthru.hermesDesktop}/share/icons "$out/share/icons"
    mkdir -p "$out/share/applications"
    cat > "$out/share/applications/hermes-desktop.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Hermes
GenericName=Hermes Desktop
Comment=Launch Hermes Desktop
Exec=$out/bin/hermes-desktop
Icon=hermes
Terminal=false
Categories=Utility;
StartupNotify=true
StartupWMClass=Hermes
DESKTOP

    cat > "$out/bin/hermes" <<'SHIM'
#!@shell@
# Nix builds ship the desktop app prebuilt; `hermes desktop`/`gui` delegate
# to the bundled hermes-desktop launcher next to this binary (the same
# platform auto-detection the .desktop entry's Exec goes through), rather
# than the in-CLI source build, which can't run inside the read-only store.
if [ "$1" = "desktop" ] || [ "$1" = "gui" ]; then
  shift
  exec "$(dirname "$0")/hermes-desktop" "$@"
fi
exec "@hermes@/bin/hermes" "$@"
SHIM
    substituteInPlace "$out/bin/hermes" \
      --replace-fail '@shell@' '${pkgs.runtimeShell}' \
      --replace-fail '@hermes@' '${self}'
    chmod +x "$out/bin/hermes"
  '';
in
# Laziness does the real work here: when withDesktop is false, `final` is
# simply never referenced, so its build script (which dereferences
# self.passthru.hermesDesktop's store path) never forces the Electron app
# to build at all.
if withDesktop then final // { passthru = self.passthru; } else self
