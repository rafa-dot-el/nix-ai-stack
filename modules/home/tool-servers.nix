# rafa.ai.tools.<x>.server: the systemd side of a tool. `tools.<x>.enable` installs the CLI
# (run it yourself); `tools.<x>.server.enable` runs the same tool as an MCP server (socket unit
# plus service) and installs it too. Each enabled server is compiled into rafa.ai.mcp.servers,
# which renders the units (modules/home/mcp.nix, lib/mcp.nix).
{ packages }:
{ config, lib, ... }:
let
  inherit (lib) mkOption mkIf mkDefault types;
  so = import ./server-options.nix { inherit lib; };
  t = config.rafa.ai.tools;
  bin = pkg: name: "${pkg}/bin/${name}";
  gw = name: { tool = "gworkspace"; exec = [ (bin packages.gworkspace name) "serve" "--systemd-socket" ]; native = true; };

  # How each shipped server runs. native: takes the activated socket itself; otherwise it
  # speaks stdio behind `mcp sidecar`.
  presets = {
    playwright = { tool = "playwright"; exec = [ (bin packages.playwright-mcp "playwright-mcp") "--headless" "--isolated" ]; prefix = "web"; timeout = 60; };
    patternfly = { tool = "patternfly"; exec = [ (bin packages.patternfly-mcp "patternfly-mcp") ]; prefix = "pf"; };
    # zoteroctl and calibrectl serve stdio only; settings via rafa.ai.tools.<x>.settings.
    zotero = { tool = "zotero"; exec = [ (bin packages.zoteroctl "zoteroctl") "serve" ]; prefix = "zotero"; };
    calibre = { tool = "calibre"; exec = [ (bin packages.calibrectl "calibrectl") "serve" ]; prefix = "calibre"; };
    tmpmail = { tool = "tmpmail"; exec = [ (bin packages.tmpmail "tmpmail") "serve" "--systemd-socket" ]; native = true; };
    # say/--say uses the shared voice by its store path (a user service's PATH is not the
    # profile's); empty when voice is off.
    slack = {
      tool = "slack";
      exec = [ (bin packages.slack "slack") "serve" "--systemd-socket" ];
      native = true;
      environment.RAFA_VOICE = if config.rafa.ai.voice.enable then "${config.rafa.ai.voice.package}/bin/rafa-voice" else "";
    };
    gtasks = gw "gtasks";
    gdocs = gw "gdocs";
    gsheets = gw "gsheets";
    gcal = gw "gcal";
    gdrive = gw "gdrive";
    gmail = gw "gmail";
    gmeet = gw "gmeet";
  };

  gworkspaceServers = [ "gtasks" "gdocs" "gsheets" "gcal" "gdrive" "gmail" "gmeet" ];
  single = [ "playwright" "patternfly" "zotero" "calibre" "tmpmail" "slack" ];

  # The user's options for each shipped server, by server name.
  declared = lib.genAttrs single (name: t.${name}.server) // lib.genAttrs gworkspaceServers (name: t.gworkspace.servers.${name});

  compile = name: s:
    let p = presets.${name}; in {
      inherit (s) enable listen include exclude rename describe credentials restartSec extraServiceConfig;
      exec = p.exec;
      native = p.native or false;
      environment = (p.environment or { }) // s.environment;
      prefix = if s.prefix != null then s.prefix else p.prefix or null;
      timeout = if s.timeout != null then s.timeout else p.timeout or null;
    };
in
{
  options.rafa.ai.tools =
    lib.genAttrs single (name: { server = mkOption { type = types.submodule { options = so.serverOptions name; }; default = { }; description = "Run ${name} as an MCP server (systemd units)."; }; })
    // {
      gworkspace.servers = lib.genAttrs gworkspaceServers (name: mkOption {
        type = types.submodule { options = so.serverOptions name; };
        default = { };
        description = "Run ${name} as an MCP server (systemd units).";
      });
    };

  config = {
    rafa.ai.mcp.servers = lib.mapAttrs compile declared;

    # A running server needs its CLI installed.
    rafa.ai.tools = lib.genAttrs single (name: { enable = mkIf t.${name}.server.enable (mkDefault true); })
      // { gworkspace.enable = mkIf (lib.any (n: t.gworkspace.servers.${n}.enable) gworkspaceServers) (mkDefault true); };
  };
}
