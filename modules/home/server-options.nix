# Options shared by every MCP server: the tools' `server` submodules (rafa.ai.tools.<x>.server)
# and the custom servers (rafa.ai.mcp.servers.<name>). `enable` is the systemd side only: the
# socket unit and the service. Installing the CLI is `rafa.ai.tools.<x>.enable`.
{ lib }:
let
  inherit (lib) mkOption types;
  m = import ../../lib/mcp.nix { inherit lib; };
  pattern = types.either types.str (types.attrsOf types.anything);
in
rec {
  rulesOptions = {
    include = mkOption { type = types.listOf pattern; default = [ ]; description = "Globs, `re:` regexes or `{ annotations = {...}; }`; empty means every tool."; };
    exclude = mkOption { type = types.listOf pattern; default = [ ]; description = "Hidden tools; exclude wins over include."; };
    rename = mkOption { type = types.attrsOf types.str; default = { }; description = "original name -> exposed name."; };
    describe = mkOption { type = types.attrsOf types.str; default = { }; description = "original name -> description."; };
  };

  # The options of one server called `name`.
  serverOptions = name: rulesOptions // {
    enable = lib.mkEnableOption "the ${name} MCP server as systemd user units (its own socket and service); the tool's own enable only installs the CLI";
    listen.socket = mkOption {
      type = types.str;
      default = m.poolSocket name;
      defaultText = lib.literalMD "`%t/rafa/mcp/servers/${name}.sock`";
      description = ''
        Unix socket the server listens on (systemd specifiers allowed). The default is in the pool
        directory the mcp proxy watches; a socket elsewhere is not discovered by the proxy and
        is reached directly, for example with `mcp client --unix PATH`.
      '';
    };
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
}
