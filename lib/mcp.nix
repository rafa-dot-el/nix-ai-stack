# Pure functions behind the rafa.ai.mcp Home Manager module (ADR-0004): options in,
# attrsets out (the 10-nix.yaml drop-in, one socket unit and one service per server, the
# proxy). Unit-tested by checks/mcp-lib.nix; the module only wires them up.
{ lib }:
let
  inherit (lib) filterAttrs mapAttrs mapAttrsToList optionalAttrs;
  enabled = servers: filterAttrs (_: s: s.enable) servers;
  # Drop unset keys (null, [], {}) so the drop-in only says what Nix decided.
  set = filterAttrs (_: v: v != null && v != [ ] && v != { });
  rules = r: set { inherit (r) include exclude rename describe; };
in
rec {
  poolDir = "%t/rafa/mcp/servers";
  # The proxy discovers every *.sock in the pool; a server listens there unless told otherwise.
  poolSocket = name: "${poolDir}/${name}.sock";
  target = "rafa-mcp.target";

  # One systemd command line from an argv list: each word double-quoted, `\` and `"`
  # escaped, `%` and `$` doubled so systemd expands neither specifiers nor variables.
  execLine = argv: lib.concatMapStringsSep " "
    (word: ''"${lib.replaceStrings [ "\\" "\"" "%" "$" ] [ "\\\\" "\\\"" "%%" "$$" ] word}"'')
    argv;

  # The drop-in config: proxy namespace plus per-server rules and the profiles.
  renderConfig = cfg: {
    version = 1;
    proxy = lib.recursiveUpdate { namespace.separator = cfg.proxy.separator; } cfg.proxy.settings;
    servers = mapAttrs (_: s: rules s // set { inherit (s) prefix timeout; }) (enabled cfg.servers);
    profiles = mapAttrs
      (_: p: { servers = mapAttrs (_: rules) p.servers; }
        // optionalAttrs p.toon { toon = true; }
        // set { inherit (p) timeout; })
      cfg.profiles;
  };

  serverSocket = name: server: {
    Unit = {
      Description = "rafa MCP server socket: ${name}";
      PartOf = [ target ];
    };
    Socket = {
      ListenStream = server.listen.socket;
      SocketMode = "0600";
      DirectoryMode = "0700";
      Accept = false;
    };
    Install.WantedBy = [ target ];
  };

  # A native server takes the activated socket itself; any other is wrapped by `mcp sidecar`.
  serverService = mcpPackage: name: server: {
    Unit = {
      Description = "rafa MCP server: ${name}";
      Requires = [ "mcp-${name}.socket" ];
      After = [ "mcp-${name}.socket" ];
      PartOf = [ target ];
      StartLimitIntervalSec = 60;
      StartLimitBurst = 5;
    };
    Service = {
      Type = if server.native && !server.notify then "simple" else "notify";
      ExecStart = execLine
        (if server.native then server.exec
        else [ "${mcpPackage}/bin/mcp" "sidecar" "--name" name "--" ] ++ server.exec);
      Restart = "on-failure";
      RestartSec = "${toString server.restartSec}s";
      SyslogIdentifier = "mcp-${name}";
      NoNewPrivileges = true;
      PrivateTmp = true;
    }
    // optionalAttrs (server.environment != { }) {
      Environment = mapAttrsToList (k: v: "${k}=${v}") server.environment;
    }
    // optionalAttrs (server.credentials != { }) {
      LoadCredential = mapAttrsToList (k: path: "${k}:${path}") server.credentials;
    }
    // server.extraServiceConfig;
  };

  proxyService = mcpPackage: cfg: {
    Unit = {
      Description = "rafa MCP proxy (discovers servers in ${poolDir})";
      PartOf = [ target ];
    };
    Service = {
      Type = "notify";
      ExecStart = execLine [ "${mcpPackage}/bin/mcp" "proxy" ];
      Restart = "on-failure";
      RestartSec = "2s";
      SyslogIdentifier = "mcp-proxy";
      NoNewPrivileges = true;
    };
    Install.WantedBy = [ target ];
  };
}
