# modules/nixos/hermes-server.nix (rafa.ai.agents.hermes.server) — a small, purpose-built module that runs the Hermes
# web dashboard and/or the messaging gateway as their own systemd services.
#
# This intentionally REPLACES upstream's own `services.hermes-agent` module
# (hermes-agent-src's nix/nixosModules.nix) rather than extending it. Upstream
# defaults `package` to upstream's own bare build rather than this flake's
# feature-toggled build, and has no firewall/TLS/auth story at all. Full
# parity with upstream's option surface (MCP servers, container mode,
# declarative config) is deferred — this covers: standing up the dashboard
# and/or gateway, with a dedicated user, a stateful HERMES_HOME (mirroring
# how upstream's own Home Manager module exposes `hermesHome` directly,
# unlike its NixOS module which only derives it from `stateDir`), an open
# firewall port, optional Let's Encrypt TLS via nginx, and optional HTTP
# Basic Auth or mutual TLS in front of the dashboard.
#
# The dashboard and the gateway are always two separate processes — verified
# against upstream's own module comments: "This is a different process from
# the gateway. Both use one HERMES_HOME" and "The backend does not run the
# messaging gateway... it does not contain a gateway." They coordinate purely
# through the shared HERMES_HOME state on disk, not direct IPC. Hence two
# independent systemd units below, `dashboard.enable`/`gateway.enable` each
# their own switch — no top-level `enable`, matching the split upstream uses
# between its shared options and its `backend.*` sub-namespace.
#
# `package`'s default is computed via `mkHermesPackage` (from flake.nix) with
# `lsp.enable`/`voice.enable`/`browser.enable`/`gateway.enable` as feature
# toggles, and ALWAYS `withDesktop = false` — neither service ever needs the
# Electron desktop app, so its build (and dependency closure — Electron,
# Chromium-for-Electron, npm) is skipped outright, not merely unused at
# runtime. See default.nix's own withDesktop laziness note for how that's
# guaranteed (not just "probably doesn't get built").
#
# Security note on `bindAddress`: when `dashboard.domain` is set, the
# dashboard process is ALWAYS forced to 127.0.0.1 regardless of
# `bindAddress` — the TLS+auth front door (nginx) must be the only path in.
# `bindAddress` only takes effect in the no-domain, direct-expose case. Only
# nginx ever binds a privileged port (its own systemd unit already grants
# itself CAP_NET_BIND_SERVICE); the dashboard process is confined to
# unprivileged ports and never runs with that capability, which is why the
# no-domain default port is 8080, not 80.
#
# TLS note: when `dashboard.domain` is set, this module enables
# `services.nginx` and `enableACME` on that vhost. NixOS itself requires the
# operator to also set, in their own configuration (deliberately not set
# here — accepting Let's Encrypt's subscriber agreement is not this module's
# decision to make):
#   security.acme.acceptTerms = true;
#   security.acme.defaults.email = "you@example.com";
#
# PAM auth was considered and deliberately left out: the nginx PAM module
# (nginxModules.pam, wrapping the third-party ngx_http_auth_pam_module) isn't
# in the default nginx build, and checking passwords against a shadow-backed
# PAM stack from inside an nginx worker means either root workers or granting
# the nginx user `shadow` group membership — a materially larger attack
# surface than basicAuth/mTLS for a network-facing daemon. Left as explicit
# future work if actually needed.
{ config, lib, mkHermesPackage, isx86_64 }:

with lib;

let
  cfg = config.rafa.ai.agents.hermes.server;

  # Only meaningful when domain is unset — see the security note above.
  effectiveBindAddress = if cfg.dashboard.domain != null then "127.0.0.1" else cfg.dashboard.bindAddress;

  # The port hermes itself binds to. When domain is set, this is an
  # internal-only loopback port that nginx proxies to — never exposed
  # directly, so it doesn't need its own option (kept as upstream's own
  # dashboard-port default purely for familiarity, not because it's
  # user-configurable in this mode).
  internalPort = if cfg.dashboard.domain != null then 9119 else cfg.dashboard.port;

  anyServiceEnabled = cfg.dashboard.enable || cfg.gateway.enable;
in
{
  options.rafa.ai.agents.hermes.server = {
    package = mkOption {
      type = types.package;
      default = mkHermesPackage {
        withLsp = cfg.lsp.enable;
        withVoice = cfg.voice.enable;
        withBrowser = cfg.browser.enable;
        withGateway = cfg.gateway.enable;
        withDesktop = false;
      };
      defaultText = literalExpression ''
        mkHermesPackage {
          withLsp = config.rafa.ai.agents.hermes.server.lsp.enable;
          withVoice = config.rafa.ai.agents.hermes.server.voice.enable;
          withBrowser = config.rafa.ai.agents.hermes.server.browser.enable;
          withGateway = config.rafa.ai.agents.hermes.server.gateway.enable;
          withDesktop = false;
        }
      '';
      description = ''
        The hermes-agent package to run the dashboard/gateway from. Built
        fresh per the lsp/voice/browser/gateway toggles below — override
        this directly if you want a specific package instead.
      '';
    };

    lsp.enable = mkOption {
      type = types.bool;
      default = true;
      description = "Include LSP servers (pyright, typescript-language-server, gopls, rust-analyzer, nixd, clangd) in `package`.";
    };

    voice.enable = mkOption {
      type = types.bool;
      # voice.nix pins x86_64-only wheels (sherpa-onnx, sentencepiece).
      default = isx86_64;
      defaultText = literalExpression "pkgs.stdenv.hostPlatform.isx86_64";
      description = "Include Piper TTS, faster-whisper (STT), and sherpa (wake word) in `package`.";
    };

    browser.enable = mkOption {
      type = types.bool;
      default = true;
      description = "Include Chromium, browser-use, agent-browser, cua-driver, and buzz CLI in `package`.";
    };

    gateway = {
      enable = mkOption {
        type = types.bool;
        # Never implied: importing the rafa.ai module (e.g. for ollama) must not start Hermes.
        default = false;
        description = ''
          Include the messaging/matrix extras (Telegram, Discord, Slack,
          Matrix) in `package`, and run the messaging gateway (`hermes
          gateway`) as its own systemd service. This is always a separate
          process from the dashboard — they coordinate only through the
          shared HERMES_HOME, never directly.
        '';
      };

      extraArgs = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "Extra command-line arguments for `hermes gateway`.";
      };
    };

    user = mkOption {
      type = types.str;
      default = "hermes-agent";
      description = "System user running the dashboard/gateway services.";
    };

    group = mkOption {
      type = types.str;
      default = "hermes-agent";
      description = "System group running the dashboard/gateway services.";
    };

    stateDir = mkOption {
      type = types.str;
      default = "/var/lib/hermes-agent";
      description = "Home directory for `user` and the services' WorkingDirectory.";
    };

    hermesHome = mkOption {
      type = types.str;
      default = "${cfg.stateDir}/.hermes";
      defaultText = literalExpression ''"''${config.rafa.ai.agents.hermes.server.stateDir}/.hermes"'';
      description = ''
        HERMES_HOME: config.yaml, .env, auth.json, sessions, skills, memory,
        cron — persistent application state, not a scratch/tmp directory.
        Mirrors upstream's own Home Manager module, which exposes this as a
        direct option; its NixOS module only ever derives it from `stateDir`.
        Point this at a separate volume if you don't want it living under
        `stateDir`. Shared between the dashboard and the gateway when both
        are enabled — that's how they coordinate.
      '';
    };

    dashboard = {
      enable = mkEnableOption "the Hermes Agent web dashboard";

      bindAddress = mkOption {
        type = types.str;
        default = "127.0.0.1";
        description = ''
          Address the dashboard process binds to. Only takes effect when
          `domain` is unset — whenever `domain` is set, the process is
          forced to 127.0.0.1 regardless of this value, so nginx's TLS+auth
          front door can't be bypassed on the local network.
          Hermes (v0.22+) itself refuses a non-loopback bind unless an auth
          provider is configured in its config.yaml (dashboard.basic_auth).
        '';
      };

      port = mkOption {
        type = types.port;
        default = if cfg.dashboard.domain != null then 443 else 8080;
        defaultText = literalExpression ''if domain != null then 443 else 8080'';
        description = ''
          When `domain` is set: the public HTTPS port nginx listens on. Only
          nginx ever binds a privileged port here — the dashboard process
          itself is confined to unprivileged ports and never runs with
          CAP_NET_BIND_SERVICE, so a `domain`-less `port` below 1024 will
          just fail to bind.
          When unset: the port the dashboard process itself binds to
          directly on `bindAddress`.
        '';
      };

      openFirewall = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Open the firewall: `port` when `domain` is unset, or 80 (always
          needed for ACME's HTTP-01 challenge) plus `port` when it is.
        '';
      };

      domain = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "hermes.example.com";
        description = ''
          Public domain name for the dashboard. When set, this module stands
          up an nginx virtual host for it with Let's Encrypt TLS
          (`enableACME`, `forceSSL`) reverse-proxying to the dashboard
          (forced to loopback), including WebSocket upgrade for /api/ws.
          When unset, the dashboard is reachable directly on
          `bindAddress`:`port` with no TLS layer.
        '';
      };

      basicAuth = {
        enable = mkEnableOption "HTTP Basic Auth in front of the dashboard (requires `domain`)";

        passwordFile = mkOption {
          # A `str`, not a `path`: a Nix path literal copies the file into the
          # Nix store, which every local user can read. Point this at a
          # runtime path instead (sops-nix, agenix, or a plain root-owned
          # file outside the store) — same convention upstream's own module
          # uses for environmentFiles/sessionTokenFile.
          type = types.nullOr types.str;
          default = null;
          description = ''
            Path to an htpasswd-format file (nginx's `basicAuthFile`),
            enforced for every request to `domain`. Generate one with:
              htpasswd -c /run/secrets/hermes-dashboard-htpasswd <username>
            Must be a runtime path, never a Nix store path.
          '';
          example = "/run/secrets/hermes-dashboard-htpasswd";
        };
      };

      mTLS = {
        enable = mkEnableOption "mutual TLS client-certificate verification in front of the dashboard (requires `domain`)";

        caFile = mkOption {
          # Unlike basicAuth.passwordFile, a CA certificate is public
          # information (used to verify client certs, not a secret) — a Nix
          # store path is fine here.
          type = types.nullOr types.path;
          default = null;
          description = "CA certificate (PEM) used to verify client certificates.";
        };

        verifyOptional = mkOption {
          type = types.bool;
          default = false;
          description = ''
            When true, a client certificate is optional
            (`ssl_verify_client optional`) rather than mandatory (`on`) —
            combine with basicAuth for a fallback. When false (default), a
            valid client certificate is required to reach the dashboard.
          '';
        };
      };
    };
  };

  config = mkMerge [
    (mkIf anyServiceEnabled {
      users.groups.${cfg.group} = { };
      users.users.${cfg.user} = {
        isSystemUser = true;
        group = cfg.group;
        home = cfg.stateDir;
        createHome = true;
      };

      systemd.tmpfiles.rules = [
        "d ${cfg.stateDir} 0750 ${cfg.user} ${cfg.group} - -"
        "d ${cfg.hermesHome} 0750 ${cfg.user} ${cfg.group} - -"
      ];
    })

    (mkIf cfg.gateway.enable {
      systemd.services.hermes-agent-gateway = {
        description = "Hermes Agent messaging gateway";
        wantedBy = [ "multi-user.target" ];
        after = [ "network-online.target" ];
        wants = [ "network-online.target" ];

        environment.HERMES_HOME = cfg.hermesHome;

        serviceConfig = {
          User = cfg.user;
          Group = cfg.group;
          WorkingDirectory = cfg.stateDir;
          ExecStart = lib.escapeShellArgs (
            [ "${cfg.package}/bin/hermes" "gateway" ] ++ cfg.gateway.extraArgs
          );
          Restart = "always";
          RestartSec = 5;

          NoNewPrivileges = true;
          ProtectSystem = "strict";
          ReadWritePaths = [ cfg.stateDir cfg.hermesHome ];
          PrivateTmp = true;
        };
      };
    })

    (mkIf cfg.dashboard.enable (mkMerge [
      {
        warnings = lib.optional (cfg.dashboard.domain == null && !(lib.elem cfg.dashboard.bindAddress [ "127.0.0.1" "::1" "localhost" ]))
          "rafa.ai.agents.hermes.server.dashboard.bindAddress = ${cfg.dashboard.bindAddress} without a domain: Hermes refuses to start unless its config.yaml sets dashboard.basic_auth (username + password_hash).";
        assertions = [
          {
            assertion = !cfg.dashboard.basicAuth.enable || cfg.dashboard.domain != null;
            message = "rafa.ai.agents.hermes.server.dashboard.basicAuth.enable requires rafa.ai.agents.hermes.server.dashboard.domain — Basic Auth is enforced at the nginx reverse-proxy layer, which only exists when a domain is configured.";
          }
          {
            assertion = !cfg.dashboard.basicAuth.enable || cfg.dashboard.basicAuth.passwordFile != null;
            message = "rafa.ai.agents.hermes.server.dashboard.basicAuth.enable requires rafa.ai.agents.hermes.server.dashboard.basicAuth.passwordFile.";
          }
          {
            assertion = !cfg.dashboard.mTLS.enable || cfg.dashboard.domain != null;
            message = "rafa.ai.agents.hermes.server.dashboard.mTLS.enable requires rafa.ai.agents.hermes.server.dashboard.domain — mTLS is enforced at the nginx reverse-proxy layer, which only exists when a domain is configured.";
          }
          {
            assertion = !cfg.dashboard.mTLS.enable || cfg.dashboard.mTLS.caFile != null;
            message = "rafa.ai.agents.hermes.server.dashboard.mTLS.enable requires rafa.ai.agents.hermes.server.dashboard.mTLS.caFile.";
          }
        ];

        systemd.services.hermes-agent-dashboard = {
          description = "Hermes Agent web dashboard";
          wantedBy = [ "multi-user.target" ];
          after = [ "network-online.target" ];
          wants = [ "network-online.target" ];

          environment.HERMES_HOME = cfg.hermesHome;

          serviceConfig = {
            User = cfg.user;
            Group = cfg.group;
            WorkingDirectory = cfg.stateDir;
            ExecStart = "${cfg.package}/bin/hermes dashboard --host ${effectiveBindAddress} --port ${toString internalPort} --no-open";
            Restart = "always";
            RestartSec = 5;

            NoNewPrivileges = true;
            ProtectSystem = "strict";
            ReadWritePaths = [ cfg.stateDir cfg.hermesHome ];
            PrivateTmp = true;

            # No CAP_NET_BIND_SERVICE, deliberately — see the security note
            # at the top of this file.
          };
        };
      }

      (mkIf (cfg.dashboard.domain == null) {
        networking.firewall.allowedTCPPorts = mkIf cfg.dashboard.openFirewall [ cfg.dashboard.port ];
      })

      (mkIf (cfg.dashboard.domain != null) {
        networking.firewall.allowedTCPPorts = mkIf cfg.dashboard.openFirewall [ 80 cfg.dashboard.port ];

        services.nginx.enable = true;
        services.nginx.virtualHosts.${cfg.dashboard.domain} = {
          enableACME = true;
          forceSSL = true;
          # A non-default public port needs its own explicit nginx listen/ACME
          # wiring beyond what forceSSL/enableACME set up automatically for
          # 443 — left as a manual follow-up if you ever change `port` here.
          basicAuthFile = mkIf cfg.dashboard.basicAuth.enable cfg.dashboard.basicAuth.passwordFile;
          # Guarded on caFile != null too (not just mTLS.enable) so a missing
          # caFile fails via the assertion above with a clear message instead
          # of an opaque "cannot coerce null to a string" from this string
          # interpolation trying to force it regardless of the assertion.
          extraConfig = optionalString (cfg.dashboard.mTLS.enable && cfg.dashboard.mTLS.caFile != null) ''
            ssl_client_certificate ${cfg.dashboard.mTLS.caFile};
            ssl_verify_client ${if cfg.dashboard.mTLS.verifyOptional then "optional" else "on"};
          '';
          locations."/" = {
            proxyPass = "http://127.0.0.1:${toString internalPort}";
            proxyWebsockets = true;
          };
        };
      })
    ]))
  ];
}
