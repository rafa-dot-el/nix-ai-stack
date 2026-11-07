# rafa.ai.mcp (ADR-0004): the `mcp` binary, the proxy service, and one socket unit plus one
# service per enabled server in the pool directory the proxy watches. The logic is in
# lib/mcp.nix (pure, unit-tested); this file declares options and wires them up.
{ packages }:
{ config, lib, ... }:
let
  inherit (lib) mkEnableOption mkOption mkIf mkDefault types;
  cfg = config.rafa.ai.mcp;
  m = import ../../lib/mcp.nix { inherit lib; };
  enabled = lib.filterAttrs (_: s: s.enable) cfg.servers;
  perServer = f: lib.mapAttrs' (name: s: lib.nameValuePair "mcp-${name}" (f name s)) enabled;

  pattern = types.either types.str (types.attrsOf types.anything);
  rulesOptions = {
    include = mkOption { type = types.listOf pattern; default = [ ]; description = "Globs, `re:` regexes or `{ annotations = {...}; }`; empty means every tool."; };
    exclude = mkOption { type = types.listOf pattern; default = [ ]; description = "Hidden tools; exclude wins over include."; };
    rename = mkOption { type = types.attrsOf types.str; default = { }; description = "original name -> exposed name."; };
    describe = mkOption { type = types.attrsOf types.str; default = { }; description = "original name -> description."; };
  };

  server = { name, ... }: {
    options = rulesOptions // {
      enable = mkEnableOption "the ${name} MCP server (its own socket unit and service)";
      exec = mkOption { type = types.listOf types.str; default = [ ]; description = "argv of the server."; };
      native = mkOption {
        type = types.bool;
        default = false;
        description = "The server takes the systemd-activated socket itself; otherwise it speaks stdio behind `mcp sidecar`.";
      };
      notify = mkOption { type = types.bool; default = true; description = "A native server sends READY=1 (Type=notify)."; };
      environment = mkOption { type = types.attrsOf types.str; default = { }; description = "Service environment. No secrets: use credentials."; };
      credentials = mkOption {
        type = types.attrsOf types.str;
        default = { };
        description = "name -> file, passed with LoadCredential (read from $CREDENTIALS_DIRECTORY/name); never in the store or the unit.";
      };
      prefix = mkOption { type = types.nullOr types.str; default = null; description = "Exposed as <prefix><separator><tool>; default: the server name."; };
      timeout = mkOption { type = types.nullOr types.ints.positive; default = null; description = "Per-call timeout in seconds (proxy default 120)."; };
      restartSec = mkOption { type = types.ints.positive; default = 5; description = "RestartSec."; };
      extraServiceConfig = mkOption { type = types.attrsOf types.anything; default = { }; description = "Merged last into [Service]."; };
    };
  };

  profile = {
    options = {
      servers = mkOption { type = types.attrsOf (types.submodule { options = rulesOptions; }); default = { }; description = "Servers in this profile and their client-side rules."; };
      toon = mkOption { type = types.bool; default = false; description = "TOON-encode JSON results."; };
      timeout = mkOption { type = types.nullOr types.ints.positive; default = null; description = "Client-side per-call timeout in seconds."; };
    };
  };
in
{
  options.rafa.ai.mcp = {
    enable = mkEnableOption "the rafa MCP stack: `mcp`, the proxy service and rafa-mcp.target";
    package = mkOption {
      type = types.package;
      default = packages.mcp;
      defaultText = lib.literalMD "this flake's mcp";
      description = "The mcp package.";
    };
    proxy = {
      separator = mkOption { type = types.str; default = "-"; description = "Between prefix and tool name."; };
      settings = mkOption { type = types.attrsOf types.anything; default = { }; description = "Merged into `proxy:` of the drop-in."; };
    };
    profiles = mkOption {
      type = types.attrsOf (types.submodule profile);
      default = { };
      description = "Profiles for `mcp client --profile NAME`.";
    };
    servers = mkOption {
      type = types.attrsOf (types.submodule server);
      default = { };
      description = "MCP servers. Presets exist for playwright, patternfly, gtasks, gdocs, gsheets, gcal, gdrive, gmail and gmeet; each is off until enabled.";
    };
  };

  config = lib.mkMerge [
    {
      # Presets: package and argv for servers this flake ships; still off by default.
      rafa.ai.mcp.servers = {
        playwright = {
          exec = mkDefault [ "${packages.playwright-mcp}/bin/playwright-mcp" "--headless" "--isolated" ];
          prefix = mkDefault "web";
          timeout = mkDefault 60;
        };
        patternfly = {
          exec = mkDefault [ "${packages.patternfly-mcp}/bin/patternfly-mcp" ];
          prefix = mkDefault "pf";
        };
        # gworkspace: native (socket-activated) servers; credentials via rafa.ai.tools.gworkspace.settings.
        gtasks = {
          exec = mkDefault [ "${packages.gworkspace}/bin/gtasks" "serve" "--systemd-socket" ];
          native = mkDefault true;
        };
        gdocs = {
          exec = mkDefault [ "${packages.gworkspace}/bin/gdocs" "serve" "--systemd-socket" ];
          native = mkDefault true;
        };
        gsheets = {
          exec = mkDefault [ "${packages.gworkspace}/bin/gsheets" "serve" "--systemd-socket" ];
          native = mkDefault true;
        };
        gcal = {
          exec = mkDefault [ "${packages.gworkspace}/bin/gcal" "serve" "--systemd-socket" ];
          native = mkDefault true;
        };
        gdrive = {
          exec = mkDefault [ "${packages.gworkspace}/bin/gdrive" "serve" "--systemd-socket" ];
          native = mkDefault true;
        };
        gmail = {
          exec = mkDefault [ "${packages.gworkspace}/bin/gmail" "serve" "--systemd-socket" ];
          native = mkDefault true;
        };
        gmeet = {
          exec = mkDefault [ "${packages.gworkspace}/bin/gmeet" "serve" "--systemd-socket" ];
          native = mkDefault true;
        };
        # zoteroctl serves stdio only: wrapped by `mcp sidecar`; settings via rafa.ai.tools.zotero.
        zotero = {
          exec = mkDefault [ "${packages.zoteroctl}/bin/zoteroctl" "serve" ];
          native = mkDefault false;
          prefix = mkDefault "zotero";
        };
        # calibrectl serves stdio only: wrapped by `mcp sidecar`; settings via rafa.ai.tools.calibre.
        calibre = {
          exec = mkDefault [ "${packages.calibrectl}/bin/calibrectl" "serve" ];
          native = mkDefault false;
          prefix = mkDefault "calibre";
        };
        tmpmail = {
          exec = mkDefault [ "${packages.tmpmail}/bin/tmpmail" "serve" "--systemd-socket" ];
          native = mkDefault true;
        };
        # slack: settings via rafa.ai.tools.slack.settings; say/--say uses the shared voice by
        # its store path (a user service's PATH is not the profile's), empty when voice is off.
        slack = {
          exec = mkDefault [ "${packages.slack}/bin/slack" "serve" "--systemd-socket" ];
          native = mkDefault true;
          environment.RAFA_VOICE = mkDefault (if config.rafa.ai.voice.enable then "${config.rafa.ai.voice.package}/bin/rafa-voice" else "");
        };
      };
    }
    (mkIf cfg.enable {
      assertions = lib.mapAttrsToList (name: s: {
        assertion = s.exec != [ ];
        message = "rafa.ai.mcp.servers.${name}: enabled with no exec";
      }) enabled;
      home.packages = [ cfg.package ];
      xdg.configFile."rafa/mcp.d/10-nix.yaml".text = builtins.toJSON (m.renderConfig cfg);
      systemd.user.targets.rafa-mcp = {
        Unit.Description = "rafa MCP stack (proxy and servers)";
        Install.WantedBy = [ "default.target" ];
      };
      systemd.user.sockets = perServer m.serverSocket;
      systemd.user.services = { mcp-proxy = m.proxyService cfg.package cfg; } // perServer (m.serverService cfg.package);
    })
  ];
}
