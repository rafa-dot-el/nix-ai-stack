{
  description = "nix-ai-stack — AI agents, tools and MCP servers for NixOS/home-manager (Hermes, Claude Code, goose, Antigravity, the mcp proxy and first-party servers)";

  # Every input follows one of two nixpkgs (ADR-0001, task check:inputs):
  # `nixpkgs` (stable) for this flake's own packages, `nixpkgs-unstable` for
  # what upstream Hermes builds against (its uv2nix set and pkgsHermesRev).
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-26.05";
    # Pinned to the exact nixpkgs upstream Hermes locks and tests against
    # (task hermes:update keeps them in sync): its prebuilt pm node segfaults on newer
    # unstable revisions (GTD bug, hermes-tui `npm config set` exit 139).
    nixpkgs-unstable.url = "github:nixos/nixpkgs/02c087f3fe673c9e317f206a627cb6b910336dbe";
    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };
    pyproject-nix = {
      url = "github:pyproject-nix/pyproject.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    uv2nix = {
      url = "github:pyproject-nix/uv2nix";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.pyproject-nix.follows = "pyproject-nix";
    };
    pyproject-build-systems = {
      url = "github:pyproject-nix/build-system-pkgs";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.pyproject-nix.follows = "pyproject-nix";
      inputs.uv2nix.follows = "uv2nix";
    };
    home-manager = {
      url = "github:nix-community/home-manager/release-26.05";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # plannotator's builder. The fork carries one commit on top of nix-community/bun2nix
    # master (structured-attrs hook fix, upstream PR #106, still open); locked by rev.
    bun2nix = {
      url = "github:Mic92/bun2nix/5765b0614591f75ee8ba5596e81ae85c167d1071"; # branch fix-structured-attrs-hook
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.flake-parts.follows = "flake-parts";
    };
    hermes-agent-src = {
      url = "github:NousResearch/hermes-agent/v0.23.1";
      inputs.nixpkgs.follows = "nixpkgs-unstable";
      inputs.flake-parts.follows = "flake-parts";
      inputs.pyproject-nix.follows = "pyproject-nix";
      inputs.uv2nix.follows = "uv2nix";
      inputs.pyproject-build-systems.follows = "pyproject-build-systems";
      inputs.home-manager.follows = "home-manager";
    };
  };

  outputs =
    { self, nixpkgs, hermes-agent-src, bun2nix, home-manager, ... }:
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
            name = "ai";
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

          # packages/: third-party AI tools, one directory each (ADR-0001).
          claude-code = pkgs.callPackage ./packages/claude-code { };
          antigravity-cli = pkgs.callPackage ./packages/antigravity-cli { };
          openspec = pkgs.callPackage ./packages/openspec { };
          patternfly-mcp = pkgs.callPackage ./packages/patternfly-mcp { };
          notebooklm-py = pkgs.callPackage ./packages/notebooklm-py { };
          goose-cli = pkgs.callPackage ./packages/goose-cli { };
          goose-desktop = pkgs.callPackage ./packages/goose-desktop {
            goose-cli = self.packages.${pkgs.stdenv.hostPlatform.system}.goose-cli;
          };
          ollama = pkgs.callPackage ./packages/ollama { ollama = pkgs.ollama-cpu; };
          ollama-rocm = pkgs.callPackage ./packages/ollama { ollama = pkgs.ollama-rocm; };
          ollama-cuda = pkgs.callPackage ./packages/ollama { ollama = pkgs.ollama-cuda; };
          ollama-vulkan = pkgs.callPackage ./packages/ollama { ollama = pkgs.ollama-vulkan; };
          playwright-mcp = pkgsHermesRev.callPackage ./packages/playwright-mcp { };
          likec4 = pkgs.callPackage ./packages/likec4 { };
          plannotator = pkgs.callPackage ./packages/plannotator {
            # bun2nix's overlay applied directly (final = prev = pkgs), not pkgs.extend.
            bun2nixLib = (bun2nix.overlays.default pkgs pkgs).bun2nix;
          };
          claude-desktop = pkgs.callPackage ./packages/claude-desktop { };
          claude-desktop-fhs = pkgs.callPackage ./packages/claude-desktop/fhs.nix {
            claude-desktop = self.packages.${pkgs.stdenv.hostPlatform.system}.claude-desktop;
          };
          claude-agent-acp = pkgs.callPackage ./packages/claude-agent-acp {
            claude-code = self.packages.${pkgs.stdenv.hostPlatform.system}.claude-code;
          };

          # src/: first-party Rust, one workspace (ADR-0001, ADR-0004).
          mcp = (pkgs.callPackage ./src/package.nix { }) {
            pname = "mcp";
            description = "One MCP entry point: proxy, stdio client, sidecar, doctor";
          };
          rafa-voice = pkgs.callPackage ./packages/voice { };
          gworkspace = (pkgs.callPackage ./src/package.nix { }) {
            pname = "gworkspace";
            mainProgram = "gtasks";
            description = "Google Workspace CLIs and MCP servers (gtasks, gdocs, gsheets, gcal, gdrive, gmail, gmeet)";
          };
          calibrectl = pkgs.callPackage ./src/calibrectl/package.nix { };
          zoteroctl = pkgs.callPackage ./src/zoteroctl/package.nix { };
          claude-usage = (pkgs.callPackage ./src/package.nix { }) {
            pname = "claude-usage";
            description = "Claude Code usage: CLI, polybar format and Prometheus exporter";
          };
          tmpmail = (pkgs.callPackage ./src/package.nix { }) {
            pname = "tmpmail";
            description = "Disposable GuerrillaMail inboxes: CLI and MCP server";
          };
          slack = (pkgs.callPackage ./src/package.nix { }) {
            pname = "slack";
            description = "Slack CLI and MCP server; every write goes through the slack.yaml policy (live, draft, deny)";
          };
        });

      # rafa.ai tree (ADR-0006), system level: the Hermes server (services.hermes-agent
      # is kept as a renamed alias), Claude Desktop managed settings, ollama.
      nixosModules.default = { pkgs, ... }: {
        imports = [
          (import ./modules/nixos {
            packages = self.packages.${pkgs.stdenv.hostPlatform.system};
            mkHermesPackage = self.lib.${pkgs.stdenv.hostPlatform.system}.mkHermesPackage;
            inherit (pkgs.stdenv.hostPlatform) isx86_64;
          })
        ];
      };

      # rafa.ai tree (ADR-0006). Upstream Hermes's own module stays available as `hermes`.
      homeManagerModules = {
        default = { pkgs, ... }: {
          imports = [ (import ./modules/home { packages = self.packages.${pkgs.stdenv.hostPlatform.system}; }) ];
        };
        hermes = hermes-agent-src.homeManagerModules.default;
      };

      checks = forAllSystems ({ pkgs, ... }:
        let mkHermesPackage = self.lib.${pkgs.stdenv.hostPlatform.system}.mkHermesPackage;
        in {
          wrapper-toggles = pkgs.callPackage ./checks/wrapper-toggles.nix { inherit mkHermesPackage; };
          module = pkgs.callPackage ./checks/module.nix {
            inherit (nixpkgs.lib) nixosSystem;
            system = pkgs.stdenv.hostPlatform.system;
            module = self.nixosModules.default;
            packages = self.packages.${pkgs.stdenv.hostPlatform.system};
          };
          desktop-launcher = pkgs.callPackage ./checks/desktop-launcher.nix { };
          vm-ollama = pkgs.callPackage ./checks/vm-ollama.nix { module = self.nixosModules.default; };
          vm-mcp-stack = pkgs.callPackage ./checks/vm-mcp-stack.nix {
            homeManagerNixos = home-manager.nixosModules.home-manager;
            homeModule = self.homeManagerModules.default;
          };
          vm-hermes = pkgs.callPackage ./checks/vm-hermes.nix { module = self.nixosModules.default; };
          hardening-lib = pkgs.callPackage ./checks/hardening-lib.nix { };
          home-module = pkgs.callPackage ./checks/home-module.nix {
            inherit (home-manager.lib) homeManagerConfiguration;
            module = self.homeManagerModules.default;
            packages = self.packages.${pkgs.stdenv.hostPlatform.system};
          };
          claude-code-smoke = self.packages.${pkgs.stdenv.hostPlatform.system}.claude-code.passthru.tests.smoke;
          antigravity-cli-smoke = self.packages.${pkgs.stdenv.hostPlatform.system}.antigravity-cli.passthru.tests.smoke;
          antigravity-cli-hardening = pkgs.callPackage ./checks/hardening-antigravity-cli.nix {
            pkg = self.packages.${pkgs.stdenv.hostPlatform.system}.antigravity-cli;
            inherit ((import ./lib/hardening.nix { inherit (nixpkgs) lib; })) forbidden;
          };
          claude-desktop-smoke = pkgs.callPackage ./checks/claude-desktop-smoke.nix {
            pkg = self.packages.${pkgs.stdenv.hostPlatform.system}.claude-desktop;
          };
          claude-desktop-hardening = (pkgs.callPackage ./checks/lib/hardened-wrapper.nix { }) {
            name = "claude-desktop";
            wrappers = [ "${self.packages.${pkgs.stdenv.hostPlatform.system}.claude-desktop}/bin/claude-desktop" ];
            env = self.packages.${pkgs.stdenv.hostPlatform.system}.claude-desktop.passthru.hardeningEnv;
            inherit ((import ./lib/hardening.nix { inherit (nixpkgs) lib; })) forbidden;
          };
          plannotator-smoke = pkgs.runCommand "plannotator-smoke" { } ''
            export HOME=$TMPDIR
            v=$(${self.packages.${pkgs.stdenv.hostPlatform.system}.plannotator}/bin/plannotator --version)
            case "$v" in *${self.packages.${pkgs.stdenv.hostPlatform.system}.plannotator.version}*) echo "[+] plannotator --version: $v" ;;
              *) echo "[-] plannotator --version: $v"; exit 1 ;; esac
            touch $out
          '';
          plannotator-hardening = (pkgs.callPackage ./checks/lib/hardened-wrapper.nix { }) {
            name = "plannotator";
            wrappers = [ "${self.packages.${pkgs.stdenv.hostPlatform.system}.plannotator}/bin/plannotator" ];
            env = self.packages.${pkgs.stdenv.hostPlatform.system}.plannotator.passthru.hardeningEnv;
            inherit ((import ./lib/hardening.nix { inherit (nixpkgs) lib; })) forbidden;
            binary = "${self.packages.${pkgs.stdenv.hostPlatform.system}.plannotator}/bin/.plannotator-wrapped";
            probe = "${self.packages.${pkgs.stdenv.hostPlatform.system}.plannotator}/bin/plannotator --version";
          };
          notebooklm-py-smoke = pkgs.runCommand "notebooklm-py-smoke" { } ''
            export HOME=$TMPDIR
            v=$(${self.packages.${pkgs.stdenv.hostPlatform.system}.notebooklm-py}/bin/notebooklm --version)
            case "$v" in *${self.packages.${pkgs.stdenv.hostPlatform.system}.notebooklm-py.version}*) echo "[+] notebooklm --version: $v" ;;
              *) echo "[-] notebooklm --version: $v"; exit 1 ;; esac
            touch $out
          '';
          # M14: `notebooklm login` installs Chromium itself when Playwright's bundled browser is
          # missing. With the wrapper's PLAYWRIGHT_BROWSERS_PATH (nixpkgs' playwright-driver
          # browsers, matching the Python playwright) it must find it and download nothing;
          # pointed at an empty directory, the same run must try to install (so the check can fail).
          vm-notebooklm-login = pkgs.callPackage ./checks/vm-notebooklm-login.nix {
            inherit (self.packages.${pkgs.stdenv.hostPlatform.system}) notebooklm-py;
          };
          notebooklm-py-login-offline = pkgs.runCommand "notebooklm-py-login-offline" { nativeBuildInputs = [ pkgs.strace ]; } ''
            nb=${self.packages.${pkgs.stdenv.hostPlatform.system}.notebooklm-py}/bin/notebooklm
            export HOME=$TMPDIR/home
            mkdir -p $HOME
            strace -f -qq -e trace=connect -o $TMPDIR/connect timeout 60 $nb login > $TMPDIR/ok 2>&1 || true
            if grep -q "not installed" $TMPDIR/ok; then echo "[-] login tried to install Chromium:"; cat $TMPDIR/ok; exit 1; fi
            grep -q "Opening Chromium" $TMPDIR/ok || { echo "[-] login did not reach the browser:"; cat $TMPDIR/ok; exit 1; }
            [ ! -e $HOME/.cache/ms-playwright ] || { echo "[-] a browser was downloaded"; exit 1; }
            if grep -E 'sin_addr|sin6_addr' $TMPDIR/connect | grep -vE '127\.0\.0\.1|::1|0\.0\.0\.0' | grep -q .; then
              echo "[-] off-host connect():"; grep -E 'sin_addr|sin6_addr' $TMPDIR/connect; exit 1
            fi
            echo "[+] login: no install attempt, no ~/.cache/ms-playwright, no off-host connect"
            # Why, robustly: login's probe child runs the bare sys.executable, which cannot import
            # playwright, so a failed probe skips the install today. Prove the version match too:
            # with the package's own module path, Playwright resolves Chromium inside the Nix
            # browsers (the probe would say "present"), and only an empty directory reads as missing.
            nbpy=$(head -1 ${self.packages.${pkgs.stdenv.hostPlatform.system}.notebooklm-py}/bin/.notebooklm-wrapped | sed 's/^#!//')
            sitepath=$(grep -o "'/nix/store/[^']*/site-packages'" ${self.packages.${pkgs.stdenv.hostPlatform.system}.notebooklm-py}/bin/.notebooklm-wrapped | tr -d "'" | paste -sd:)
            probe='import os; from playwright.sync_api import sync_playwright
            with sync_playwright() as p: path = p.chromium.executable_path
            print("present" if path and os.path.exists(path) else "missing", path)'
            browsers=$(grep -o "PLAYWRIGHT_BROWSERS_PATH='[^']*'" $nb | cut -d"'" -f2)
            got=$(PYTHONPATH=$sitepath PLAYWRIGHT_BROWSERS_PATH=$browsers $nbpy -c "$probe")
            case "$got" in present*) echo "[+] version match: $got" ;; *) echo "[-] the Nix browsers do not match the Python playwright: $got"; exit 1 ;; esac
            mkdir -p $TMPDIR/empty
            got=$(PYTHONPATH=$sitepath PLAYWRIGHT_BROWSERS_PATH=$TMPDIR/empty $nbpy -c "$probe")
            case "$got" in missing*) echo "[+] an empty browser directory reads as missing: the check can fail" ;; *) echo "[-] negative case: $got"; exit 1 ;; esac
            touch $out
          '';
          notebooklm-py-hardening = (pkgs.callPackage ./checks/lib/hardened-wrapper.nix { }) {
            name = "notebooklm-py";
            wrappers = [ "${self.packages.${pkgs.stdenv.hostPlatform.system}.notebooklm-py}/bin/notebooklm" ];
            env = self.packages.${pkgs.stdenv.hostPlatform.system}.notebooklm-py.passthru.hardeningEnv;
            inherit ((import ./lib/hardening.nix { inherit (nixpkgs) lib; })) forbidden;
            probe = "${self.packages.${pkgs.stdenv.hostPlatform.system}.notebooklm-py}/bin/notebooklm --version";
          };
          goose-desktop-smoke = self.packages.${pkgs.stdenv.hostPlatform.system}.goose-desktop.passthru.tests.smoke;
          # src/: the package build runs the crate's tests (nextest) in the sandbox.
          mcp = self.packages.${pkgs.stdenv.hostPlatform.system}.mcp;
          mcp-lib = pkgs.callPackage ./checks/mcp-lib.nix { };
          gworkspace = self.packages.${pkgs.stdenv.hostPlatform.system}.gworkspace;
          gworkspace-hardening = (pkgs.callPackage ./checks/lib/hardened-wrapper.nix { }) {
            name = "gworkspace";
            wrappers = [ ];
            env = { };
            probe = toString (pkgs.writeShellScript "gworkspace-probe" ''
              g=${self.packages.${pkgs.stdenv.hostPlatform.system}.gworkspace}/bin
              init='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"probe","version":"1"}}}'
              for bin in gtasks gdocs gsheets gcal gdrive gmail gmeet; do
                $g/$bin --help
                $g/$bin status || true
                echo "$init" | $g/$bin serve
              done
            '');
          };
          zoteroctl = self.packages.${pkgs.stdenv.hostPlatform.system}.zoteroctl;
          # Offline: --help, a list against no library, and an MCP initialize + tools/list.
          zoteroctl-hardening = (pkgs.callPackage ./checks/lib/hardened-wrapper.nix { }) {
            name = "zoteroctl";
            wrappers = [ ];
            env = { };
            probe = toString (pkgs.writeShellScript "zoteroctl-probe" ''
              z=${self.packages.${pkgs.stdenv.hostPlatform.system}.zoteroctl}/bin/zoteroctl
              export HOME=$TMPDIR
              $z --help
              $z list-items || true
              printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"probe","version":"1"}}}' '{"jsonrpc":"2.0","method":"notifications/initialized"}' '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | timeout 20 $z serve | grep -q search_items
            '');
          };
          calibrectl = self.packages.${pkgs.stdenv.hostPlatform.system}.calibrectl;
          calibrectl-hardening = (pkgs.callPackage ./checks/lib/hardened-wrapper.nix { }) {
            name = "calibrectl";
            wrappers = [ ];
            env = { };
            probe = toString (pkgs.writeShellScript "calibrectl-probe" ''
              c=${self.packages.${pkgs.stdenv.hostPlatform.system}.calibrectl}/bin/calibrectl
              export HOME=$TMPDIR
              $c --help
              $c list || true
              printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"probe","version":"1"}}}' '{"jsonrpc":"2.0","method":"notifications/initialized"}' '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | timeout 20 $c serve | grep -q search_books
            '');
          };
          claude-usage = self.packages.${pkgs.stdenv.hostPlatform.system}.claude-usage;
          # Offline and unconfigured: --help and a run that fails on the missing credentials file.
          claude-usage-hardening = (pkgs.callPackage ./checks/lib/hardened-wrapper.nix { }) {
            name = "claude-usage";
            wrappers = [ ];
            env = { };
            probe = toString (pkgs.writeShellScript "claude-usage-probe" ''
              c=${self.packages.${pkgs.stdenv.hostPlatform.system}.claude-usage}/bin/claude-usage
              export HOME=$TMPDIR
              $c --help
              $c --json || true
            '');
          };
          vm-claude-usage = pkgs.callPackage ./checks/vm-claude-usage.nix {
            homeManagerNixos = home-manager.nixosModules.home-manager;
            homeModule = self.homeManagerModules.default;
          };
          tmpmail = self.packages.${pkgs.stdenv.hostPlatform.system}.tmpmail;
          # Offline: only --help, a list of the (empty) saved inboxes and an MCP initialize.
          tmpmail-hardening = (pkgs.callPackage ./checks/lib/hardened-wrapper.nix { }) {
            name = "tmpmail";
            wrappers = [ ];
            env = { };
            probe = toString (pkgs.writeShellScript "tmpmail-probe" ''
              t=${self.packages.${pkgs.stdenv.hostPlatform.system}.tmpmail}/bin/tmpmail
              export HOME=$TMPDIR
              $t --help
              $t list-identities
              echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"probe","version":"1"}}}' | $t serve
            '');
          };
          slack = self.packages.${pkgs.stdenv.hostPlatform.system}.slack;
          slack-hardening = (pkgs.callPackage ./checks/lib/hardened-wrapper.nix { }) {
            name = "slack";
            wrappers = [ ];
            env = { };
            probe = toString (pkgs.writeShellScript "slack-probe" ''
              s=${self.packages.${pkgs.stdenv.hostPlatform.system}.slack}/bin/slack
              export HOME=$TMPDIR
              $s --help
              $s status
              echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"probe","version":"1"}}}' | $s serve
            '');
          };
          voice = pkgs.callPackage ./checks/voice.nix { inherit (self.packages.${pkgs.stdenv.hostPlatform.system}) rafa-voice; };
          voice-hardening = (pkgs.callPackage ./checks/lib/hardened-wrapper.nix { }) {
            name = "rafa-voice";
            wrappers = [ "${self.packages.${pkgs.stdenv.hostPlatform.system}.rafa-voice}/bin/rafa-voice" ];
            env = self.packages.${pkgs.stdenv.hostPlatform.system}.rafa-voice.passthru.hardeningEnv;
            probe = toString (pkgs.writeShellScript "voice-probe" ''
              v=${self.packages.${pkgs.stdenv.hostPlatform.system}.rafa-voice}/bin/rafa-voice
              $v say "network probe" -o $TMPDIR/p.wav
              $v transcribe $TMPDIR/p.wav --language en
            '');
          };
          mcp-hardening = (pkgs.callPackage ./checks/lib/hardened-wrapper.nix { }) {
            name = "mcp";
            wrappers = [ ];
            env = { };
            probe = toString (pkgs.writeShellScript "mcp-probe" ''
              mcp=${self.packages.${pkgs.stdenv.hostPlatform.system}.mcp}/bin/mcp
              $mcp --version
              $mcp --help
              XDG_RUNTIME_DIR=$TMPDIR/run $mcp doctor || true
            '');
          };
          goose-hardening = (pkgs.callPackage ./checks/lib/hardened-wrapper.nix { }) {
            name = "goose";
            wrappers = [
              "${self.packages.${pkgs.stdenv.hostPlatform.system}.goose-cli}/bin/goose"
              "${self.packages.${pkgs.stdenv.hostPlatform.system}.goose-desktop}/bin/goose-desktop"
            ];
            env = self.packages.${pkgs.stdenv.hostPlatform.system}.goose-cli.passthru.hardeningEnv;
            inherit ((import ./lib/hardening.nix { inherit (nixpkgs) lib; })) forbidden;
            binary = "${self.packages.${pkgs.stdenv.hostPlatform.system}.goose-cli}/bin/.goose-wrapped";
            probe = "${self.packages.${pkgs.stdenv.hostPlatform.system}.goose-cli}/bin/goose --version";
          };
          ollama-smoke = self.packages.${pkgs.stdenv.hostPlatform.system}.ollama.passthru.tests.smoke;
          ollama-hardening = (pkgs.callPackage ./checks/lib/hardened-wrapper.nix { }) {
            name = "ollama";
            wrappers = [ "${self.packages.${pkgs.stdenv.hostPlatform.system}.ollama}/bin/ollama" ];
            env = self.packages.${pkgs.stdenv.hostPlatform.system}.ollama.passthru.hardeningEnv;
            inherit ((import ./lib/hardening.nix { inherit (nixpkgs) lib; })) forbidden;
            binary = "${self.packages.${pkgs.stdenv.hostPlatform.system}.ollama.passthru.unwrapped}/bin/ollama";
            probe = "${self.packages.${pkgs.stdenv.hostPlatform.system}.ollama}/bin/ollama --version";
          };
          playwright-mcp-smoke = self.packages.${pkgs.stdenv.hostPlatform.system}.playwright-mcp.passthru.tests.smoke;
          patternfly-mcp-smoke = self.packages.${pkgs.stdenv.hostPlatform.system}.patternfly-mcp.passthru.tests.smoke;
          likec4-smoke = self.packages.${pkgs.stdenv.hostPlatform.system}.likec4.passthru.tests.smoke;
          likec4-hardening = (pkgs.callPackage ./checks/lib/hardened-wrapper.nix { }) {
            name = "likec4";
            wrappers = map (b: "${self.packages.${pkgs.stdenv.hostPlatform.system}.likec4}/bin/${b}") [ "likec4" "likec4-mcp" ];
            env = self.packages.${pkgs.stdenv.hostPlatform.system}.likec4.passthru.hardeningEnv;
            inherit ((import ./lib/hardening.nix { inherit (nixpkgs) lib; })) forbidden;
            probe = "${self.packages.${pkgs.stdenv.hostPlatform.system}.likec4}/bin/likec4 --version";
          };
          openspec-smoke = self.packages.${pkgs.stdenv.hostPlatform.system}.openspec.passthru.tests.smoke;
          openspec-hardening = (pkgs.callPackage ./checks/lib/hardened-wrapper.nix { }) {
            name = "openspec";
            wrappers = [ "${self.packages.${pkgs.stdenv.hostPlatform.system}.openspec}/bin/openspec" ];
            env = self.packages.${pkgs.stdenv.hostPlatform.system}.openspec.passthru.hardeningEnv;
            inherit ((import ./lib/hardening.nix { inherit (nixpkgs) lib; })) forbidden;
            probe = "${self.packages.${pkgs.stdenv.hostPlatform.system}.openspec}/bin/openspec --version";
            # The switches are read in the bundled JS, so check the names are still there.
            extraCheck = ''
              for key in OPENSPEC_TELEMETRY OPENSPEC_NO_UPDATE_CHECK DO_NOT_TRACK; do
                grep -rqF "$key" ${self.packages.${pkgs.stdenv.hostPlatform.system}.openspec}/lib/node_modules/@fission-ai/openspec/dist \
                  || { echo "[-] openspec no longer reads $key"; exit 1; }
              done
              echo "[+] openspec still reads every switch"
            '';
          };
          claude-agent-acp-smoke = self.packages.${pkgs.stdenv.hostPlatform.system}.claude-agent-acp.passthru.tests.smoke;
          claude-agent-acp-hardening = (pkgs.callPackage ./checks/lib/hardened-wrapper.nix { }) {
            name = "claude-agent-acp";
            wrappers = [ "${self.packages.${pkgs.stdenv.hostPlatform.system}.claude-agent-acp}/bin/claude-agent-acp" ];
            env = self.packages.${pkgs.stdenv.hostPlatform.system}.claude-agent-acp.passthru.hardeningEnv;
            inherit ((import ./lib/hardening.nix { inherit (nixpkgs) lib; })) forbidden;
          };
          claude-code-hardening = pkgs.callPackage ./checks/hardening-claude-code.nix {
            pkg = self.packages.${pkgs.stdenv.hostPlatform.system}.claude-code;
            inherit ((import ./lib/hardening.nix { inherit (nixpkgs) lib; })) forbidden;
          };
          hardening = pkgs.callPackage ./checks/hardening.nix {
            hermesPkg = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
            inherit ((import ./lib/hardening.nix { inherit (nixpkgs) lib; })) forbidden;
          };
          oci = pkgs.callPackage ./checks/oci.nix {
            image = self.packages.${pkgs.stdenv.hostPlatform.system}.ociImage;
            hermes = self.packages.${pkgs.stdenv.hostPlatform.system}.hermesFull;
          };
          # Eval only: aarch64-linux must instantiate (no builder here to build it).
          aarch64-eval = pkgs.writeText "aarch64-eval" (builtins.unsafeDiscardStringContext
            (builtins.concatStringsSep "\n" (map (name: self.packages.aarch64-linux.${name}.drvPath)
              [ "default" "hermes-slim" ])));
          extras = pkgs.callPackage ./checks/extras.nix {
            hermesPkg = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
            hermesSrc = hermes-agent-src;
          };
        });

      devShells = forAllSystems ({ pkgs, ... }: {
        default = pkgs.mkShell {
          packages = [ pkgs.go-task pkgs.jq pkgs.actionlint pkgs.python3 ];
        };
        # Rust workspace (src/): build, nextest, clippy, coverage.
        rust = pkgs.mkShell {
          packages = with pkgs; [ cargo rustc clippy rustfmt cargo-nextest cargo-llvm-cov ];
          # cargo-llvm-cov looks for llvm-cov/llvm-profdata from rustup's llvm-tools.
          LLVM_COV = "${pkgs.llvmPackages.llvm}/bin/llvm-cov";
          LLVM_PROFDATA = "${pkgs.llvmPackages.llvm}/bin/llvm-profdata";
        };
        # First-party Python (src/calibrectl, src/zoteroctl): the packages' own dependencies
        # and the test tools, all from the locked nixpkgs.
        python = pkgs.mkShell {
          packages = [
            (pkgs.python3.withPackages (ps: with ps; [ click pyyaml requests bibtexparser pypdf ebooklib mcp pytest pytest-cov pytest-mock pytest-asyncio responses hatchling ]))
          ];
        };
        # Package update tooling (task pkg:*:update), kept out of the CI shell.
        update = pkgs.mkShell {
          packages = [ pkgs.bun bun2nix.packages.${pkgs.stdenv.hostPlatform.system}.default ];
        };
        # Test runner: pytest and jq from the locked nixpkgs, nothing
        # installed with pip/uv (see tests/).
        itest = pkgs.mkShell {
          packages = [ (pkgs.python3.withPackages (ps: [ ps.pytest ps.pytest-cov ps.packaging ])) pkgs.jq ];
        };
      });
    };
}
