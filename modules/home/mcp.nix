# rafa.ai.mcp (ADR-0004): the `mcp` binary, the proxy service, and one socket unit plus one
# service per enabled server in the pool directory the proxy watches. The logic is in
# lib/mcp.nix (pure, unit-tested); this file declares options and wires them up.
{ packages }:
{ config, lib, ... }:
let
  inherit (lib) mkEnableOption mkOption mkIf types;
  cfg = config.rafa.ai.mcp;
  m = import ../../lib/mcp.nix { inherit lib; };
  enabled = lib.filterAttrs (_: s: s.enable) cfg.servers;
  perServer = f: lib.mapAttrs' (name: s: lib.nameValuePair "mcp-${name}" (f name s)) enabled;

  so = import ./server-options.nix { inherit lib; };
  inherit (so) rulesOptions;

  # A custom server: not shipped by this flake, so it also says what to run. Shipped servers
  # are enabled with rafa.ai.tools.<x>.server (tool-servers.nix).
  server = { name, ... }: {
    options = so.serverOptions name // {
      exec = mkOption { type = types.listOf types.str; default = [ ]; description = "argv of the server."; };
      native = mkOption {
        type = types.bool;
        default = false;
        description = "The server takes the systemd-activated socket itself; otherwise it speaks stdio behind `mcp sidecar`.";
      };
      notify = mkOption { type = types.bool; default = true; description = "A native server sends READY=1 (Type=notify)."; };
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
      description = "Custom MCP servers (your own binary). The servers this flake ships are enabled with rafa.ai.tools.<x>.server.enable.";
    };
  };

  config = lib.mkMerge [
    # Servers: units exist for every enabled server, with or without the proxy.
    (mkIf (enabled != { }) {
      assertions = lib.mapAttrsToList (name: s: {
        assertion = s.exec != [ ];
        message = "rafa.ai.mcp.servers.${name}: enabled with no exec";
      }) enabled;
      systemd.user.targets.rafa-mcp = {
        Unit.Description = "rafa MCP stack (proxy and servers)";
        Install.WantedBy = [ "default.target" ];
      };
      systemd.user.sockets = perServer m.serverSocket;
      systemd.user.services = perServer (m.serverService cfg.package);
    })
    # The proxy, the `mcp` binary and the profiles.
    (mkIf cfg.enable {
      home.packages = [ cfg.package ];
      xdg.configFile."rafa/mcp.d/10-nix.yaml".text = builtins.toJSON (m.renderConfig cfg);
      systemd.user.targets.rafa-mcp = {
        Unit.Description = "rafa MCP stack (proxy and servers)";
        Install.WantedBy = [ "default.target" ];
      };
      systemd.user.services.mcp-proxy = m.proxyService cfg.package cfg;
    })
  ];
}
