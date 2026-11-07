# Home Manager side of the rafa.ai tree (ADR-0006): agents, generic tools, hardening.
# One root switch per agent enables its default stack; every sub-option defaults to
# that switch. Long-running services never follow a root switch.
#
# `packages` is this flake's packages.<system>, passed by flake.nix.
{ packages }:
{ config, lib, pkgs, ... }:
let
  inherit (lib) mkEnableOption mkOption mkIf mkDefault types optional optionals;
  cfg = config.rafa.ai;
  hardening = import ../../lib/hardening.nix { inherit lib; };

  # { enable; package; } for one installable piece, enabled by `default`.
  component = description: default: package: {
    enable = mkOption {
      type = types.bool;
      inherit default;
      defaultText = lib.literalMD "the agent's root switch";
      description = "Install ${description}.";
    };
    package = mkOption {
      type = types.package;
      default = package;
      defaultText = lib.literalMD "this flake's ${package.pname or package.name}";
      description = "Package for ${description}.";
    };
  };
  tool = name: description: {
    enable = mkEnableOption description;
    package = mkOption {
      type = types.package;
      default = packages.${name};
      defaultText = lib.literalMD "this flake's ${name}";
      description = "Package for ${description}.";
    };
  };
in
{
  imports = [ (import ./mcp.nix { inherit packages; }) (import ./voice.nix { inherit packages; }) ];

  options.rafa.ai = {
    hardening.enable = mkOption {
      type = types.bool;
      default = true;
      description = ''
        Ship the config-only hardening (no self-update, telemetry or runtime installs).
        Wrappers are hardened at build time regardless; this adds what lives in config files.
      '';
    };

    agents = {
      claude = {
        enable = mkEnableOption "the Claude stack: claude-code, claude-agent-acp, claude-desktop";
        code = component "claude-code" cfg.agents.claude.enable packages.claude-code;
        acp = component "claude-agent-acp (ACP adapter driving claude-code)" cfg.agents.claude.enable packages.claude-agent-acp;
        desktop = component "Claude Desktop" cfg.agents.claude.enable packages.claude-desktop // {
          fhs = mkOption {
            type = types.bool;
            default = false;
            description = "Use the FHS variant (Cowork VM helper, npx/uvx/docker MCP servers).";
          };
        };
        usage = component "claude-usage (CLI and polybar format)" cfg.agents.claude.enable packages.claude-usage // {
          polybar.enable = mkOption {
            type = types.bool;
            default = cfg.agents.claude.usage.enable;
            defaultText = lib.literalMD "agents.claude.usage.enable";
            description = "Add services.polybar.settings.\"module/claude-usage\" (a custom/script module).";
          };
          polybar.format = mkOption {
            type = types.str;
            default = "{PCT_5H}% {COUNTDOWN_5H}";
            description = "The template the polybar module prints (claude-usage --format).";
          };
          exporter = {
            enable = mkEnableOption "the Prometheus exporter as a user service (a long-running service: never implied)";
            bind = mkOption { type = types.str; default = "127.0.0.1"; description = "Listen address; /metrics is unauthenticated."; };
            port = mkOption { type = types.port; default = 9464; description = "Listen port."; };
          };
        };
      };

      hermes = {
        enable = mkEnableOption "Hermes: CLI, TUI, ACP and the desktop app";
        package = mkOption {
          type = types.package;
          default = packages.default;
          defaultText = lib.literalMD "this flake's hermesFull";
          description = "Hermes package (or a lib.mkHermesPackage build with fewer features).";
        };
        hardenedConfig.enable = mkOption {
          type = types.bool;
          default = cfg.agents.hermes.enable && cfg.hardening.enable;
          defaultText = lib.literalMD "agents.hermes.enable && hardening.enable";
          description = ''
            Install the config.yaml-only hardening fragment as
            ~/.local/share/rafa/hermes/hardened-config.yaml, to merge into profiles.
          '';
        };
      };

      antigravity = {
        enable = mkEnableOption "the Antigravity CLI";
        cli = component "antigravity-cli (agy)" cfg.agents.antigravity.enable packages.antigravity-cli;
      };
    };

    tools = {
      plannotator = tool "plannotator" "plannotator (plan and code review UI for agents)";
      openspec = tool "openspec" "openspec (spec-driven development CLI)";
      likec4 = tool "likec4" "likec4 (architecture as code, with likec4-mcp)";
      notebooklm = tool "notebooklm-py" "notebooklm (NotebookLM CLI)";
      gworkspace = tool "gworkspace" "the Google Workspace CLIs (gtasks, ...)" // {
        settings = mkOption {
          type = types.attrsOf types.anything;
          default = { };
          example = lib.literalExpression ''{ auth = { client.file = "/run/agenix/google-client"; refresh_token.command = [ "pass" "show" "google/refresh-token" ]; store_refresh_token = [ "pass" "insert" "-mf" "google/refresh-token" ]; }; }'';
          description = "Rendered to ~/.config/rafa/gworkspace.yaml (secret refs only, never secrets). Shared by the CLIs and the MCP servers.";
        };
      };
      tmpmail = tool "tmpmail" "tmpmail (disposable GuerrillaMail inboxes)";
      zotero = tool "zoteroctl" "zoteroctl (Zotero libraries from the command line)" // {
        settings = mkOption {
          type = (pkgs.formats.toml { }).type;
          default = { };
          example = lib.literalExpression ''{ database = "''${config.home.homeDirectory}/Zotero/zotero.sqlite"; translation_server = "http://127.0.0.1:1969"; }'';
          description = "Rendered to ~/.config/rafa/zoteroctl/config.toml (zoteroctl's own TOML format): database, storage, translation_server.";
        };
      };
      calibre = tool "calibrectl" "calibrectl (Calibre libraries from the command line)" // {
        settings = mkOption {
          type = types.attrsOf types.anything;
          default = { };
          example = lib.literalExpression ''{ database = "''${config.home.homeDirectory}/Calibre Library/metadata.db"; attach_roots = [ "''${config.home.homeDirectory}/share" ]; }'';
          description = "Rendered to ~/.config/rafa/calibrectl.yaml: database, storage, attach_roots (the only directories the MCP server attaches files from).";
        };
      };
      slack = tool "slack" "the slack CLI" // {
        settings = mkOption {
          type = types.attrsOf types.anything;
          default = { };
          example = lib.literalExpression ''{ credentials.token.command = [ "pass" "show" "slack/token" ]; policy.rules = [ { match.self_dm = true; action = "live"; } ]; }'';
          description = "Rendered to ~/.config/rafa/slack.yaml (secret refs only, never secrets): credentials, policy, attachments. Shared by the CLI and the MCP server.";
        };
      };
    };
  };

  config = {
    home.packages =
      let a = cfg.agents; t = cfg.tools; in
      optional a.claude.code.enable a.claude.code.package
      ++ optional a.claude.usage.enable a.claude.usage.package
      ++ optional a.claude.acp.enable a.claude.acp.package
      ++ optional a.claude.desktop.enable
        (if a.claude.desktop.fhs then packages.claude-desktop-fhs else a.claude.desktop.package)
      ++ optional a.hermes.enable a.hermes.package
      ++ optional a.antigravity.cli.enable a.antigravity.cli.package
      ++ lib.concatMap (name: optional t.${name}.enable t.${name}.package) (lib.attrNames t);

    xdg.configFile."rafa/gworkspace.yaml" = mkIf (cfg.tools.gworkspace.settings != { }) {
      text = builtins.toJSON ({ version = 1; } // cfg.tools.gworkspace.settings);
    };

    services.polybar.settings."module/claude-usage" = mkIf (cfg.agents.claude.usage.enable && cfg.agents.claude.usage.polybar.enable) {
      type = "custom/script";
      exec = "${cfg.agents.claude.usage.package}/bin/claude-usage --format ${lib.escapeShellArg cfg.agents.claude.usage.polybar.format} --color-mode polybar";
      interval = 120;
    };

    xdg.configFile."rafa/claude-usage.yaml" = mkIf cfg.agents.claude.usage.exporter.enable {
      text = builtins.toJSON { version = 1; exporter = { inherit (cfg.agents.claude.usage.exporter) bind port; }; };
    };

    systemd.user.services.claude-usage-exporter = mkIf cfg.agents.claude.usage.exporter.enable {
      Unit.Description = "Claude Code usage, as Prometheus metrics";
      Service = {
        ExecStart = "${cfg.agents.claude.usage.package}/bin/claude-usage exporter";
        Restart = "on-failure";
        RestartSec = "5s";
        NoNewPrivileges = true;
        PrivateTmp = true;
        RestrictAddressFamilies = [ "AF_INET" "AF_INET6" "AF_UNIX" ];
      };
      Install.WantedBy = [ "default.target" ];
    };

    xdg.configFile."rafa/zoteroctl/config.toml" = mkIf (cfg.tools.zotero.settings != { }) {
      source = (pkgs.formats.toml { }).generate "zoteroctl-config.toml" cfg.tools.zotero.settings;
    };

    xdg.configFile."rafa/calibrectl.yaml" = mkIf (cfg.tools.calibre.settings != { }) {
      text = builtins.toJSON ({ version = 1; } // cfg.tools.calibre.settings);
    };

    xdg.configFile."rafa/slack.yaml" = mkIf (cfg.tools.slack.settings != { }) {
      text = builtins.toJSON ({ version = 1; } // cfg.tools.slack.settings);
    };

    xdg.dataFile."rafa/hermes/hardened-config.yaml" = mkIf cfg.agents.hermes.hardenedConfig.enable {
      source = "${cfg.agents.hermes.package}/share/hermes-agent/hardened-config.yaml";
    };
  };
}
