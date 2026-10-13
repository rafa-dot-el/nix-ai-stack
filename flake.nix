{
  description = "hermes-agent — Nous Research AI agent framework (CLI, TUI, web dashboard, desktop, ACP)";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-25.11";
    hermes-agent-src = {
      url = "github:NousResearch/hermes-agent/v0.21.7";
      # Not `follows`-ed: upstream's uv2nix is pinned to its own nixpkgs-unstable revision.
    };
  };

  outputs =
    { self, nixpkgs, hermes-agent-src, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];

      forAllSystems = f: nixpkgs.lib.genAttrs systems (system:
        f {
          pkgs = import nixpkgs { inherit system; config.allowUnfree = true; };
          # Match upstream's nixpkgs revision for piper ABI compatibility.
          pkgsHermesRev = import hermes-agent-src.inputs.nixpkgs { inherit system; };
          hermes = hermes-agent-src.packages.${system};
        });
    in
    {
      # Exposed separately from `packages` so the NixOS module can build its
      # own tailored variant (feature toggles, always withDesktop = false)
      # using THIS flake's pinned nixpkgs/hermes/pkgsHermesRev, rather than
      # rebuilding against whatever nixpkgs the consuming system uses.
      lib = forAllSystems ({ pkgs, pkgsHermesRev, hermes }: {
        mkHermesPackage = args: pkgs.callPackage ./default.nix (
          { inherit hermes pkgsHermesRev pkgs; } // args
        );
      });

      packages = forAllSystems ({ pkgs, pkgsHermesRev, hermes }:
        let
          # hermesFull: Chromium, browser automation, voice (Piper/whisper/sherpa),
          # LSP servers, web dashboard, and the packaged Electron desktop app.
          hermesFull = pkgs.callPackage ./default.nix { inherit hermes pkgsHermesRev pkgs; };

          # hermes-slim: bare CLI+TUI, straight from upstream — no browser, no
          # voice, no LSP servers, no Chromium. Combines hermes.minimal (Python CLI)
          # and hermes.tui (Node.js TUI) into a single derivation.
          hermes-slim = pkgs.symlinkJoin {
            name = "hermes-slim";
            paths = [ hermes.minimal hermes.tui ];
          };

          desktopVariants = pkgs.callPackage ./desktop.nix {
            hermesDesktop = hermesFull.passthru.hermesDesktop;
          };

          ociImage = pkgs.dockerTools.buildLayeredImage {
            name = "hermes-agent";
            tag = "latest";
            contents = [ hermesFull ];
            config.Entrypoint = [ "${hermesFull}/bin/hermes" ];
            config.Cmd = [ "chat" "--cli" ];
          };
        in
        {
          inherit hermesFull hermes-slim ociImage;
          default = hermesFull;
          desktop = hermesFull.passthru.hermesDesktop;
          inherit (desktopVariants) hermes-desktop-x11 hermes-desktop-wayland hermes-desktop;
          inherit (hermes) minimal tui web;
          hermes-chromium = pkgs.chromium;
        });

      # Replaces (not extends) upstream's own module — see nixos/dashboard.nix
      # for why: dashboard/gateway services (not the full option surface),
      # feature-toggled package (lsp/voice/browser/gateway, always without
      # the Electron desktop app), firewall/TLS/auth.
      nixosModules.default = { config, lib, pkgs, ... }:
        import ./nixos/dashboard.nix {
          inherit config lib;
          mkHermesPackage = self.lib.${pkgs.stdenv.hostPlatform.system}.mkHermesPackage;
        };

      homeManagerModules.default = hermes-agent-src.homeManagerModules.default;

      devShells = forAllSystems ({ pkgs, ... }: {
        default = pkgs.mkShell {
          packages = [ pkgs.go-task pkgs.jq ];
        };
      });
    };
}
