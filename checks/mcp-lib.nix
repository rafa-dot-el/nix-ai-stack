# Unit tests (lib.runTests) for lib/mcp.nix: the rendered drop-in config, socket units and
# services of the rafa.ai.mcp module.
{ lib, runCommand }:
let
  m = import ../lib/mcp.nix { inherit lib; };
  mcp = { outPath = "/nix/store/x-mcp"; };
  server = overrides: {
    enable = true;
    exec = [ "/nix/store/y-srv/bin/srv" "--stdio" ];
    native = false;
    notify = true;
    environment = { };
    prefix = null;
    include = [ ];
    exclude = [ ];
    rename = { };
    describe = { };
    timeout = null;
    credentials = { };
    listen.socket = "%t/rafa/mcp/servers/web.sock";
    restartSec = 5;
    extraServiceConfig = { };
  } // overrides;
  cfg = {
    proxy = { separator = "-"; settings = { }; };
    servers = {
      web = server { prefix = "browser"; include = [ "browser_*" ]; exclude = [ { annotations.destructiveHint = true; } ]; timeout = 60; };
      off = server { enable = false; };
      plain = server { };
    };
    profiles.coding = { servers.web = { include = [ "browser_navigate" ]; exclude = [ ]; rename = { }; describe = { }; }; toon = true; timeout = null; };
  };
  results = lib.runTests {
    testExecLineQuotesAndEscapesSpecifiers = {
      expr = m.execLine [ "/bin/a" "100%" "$HOME" ''say "hi"'' ''back\slash'' ];
      expected = ''"/bin/a" "100%%" "$$HOME" "say \"hi\"" "back\\slash"'';
    };
    testConfigHasOnlyEnabledServersAndOnlySetKeys = {
      expr = m.renderConfig cfg;
      expected = {
        version = 1;
        proxy.namespace.separator = "-";
        servers = {
          web = { prefix = "browser"; include = [ "browser_*" ]; exclude = [ { annotations.destructiveHint = true; } ]; timeout = 60; };
          plain = { };
        };
        profiles.coding = { servers.web = { include = [ "browser_navigate" ]; }; toon = true; };
      };
    };
    testProxySettingsMergeIntoTheDropin = {
      expr = (m.renderConfig (cfg // { proxy = { separator = "_"; settings.log.level = "debug"; }; })).proxy;
      expected = { namespace.separator = "_"; log.level = "debug"; };
    };
    testSocketIsPrivateInThePool = {
      expr = (m.serverSocket "web" cfg.servers.web).Socket;
      expected = { ListenStream = "%t/rafa/mcp/servers/web.sock"; SocketMode = "0600"; DirectoryMode = "0700"; Accept = false; };
    };
    testDefaultSocketIsInThePool = {
      expr = m.poolSocket "web";
      expected = "%t/rafa/mcp/servers/web.sock";
    };
    testSocketPathIsTheServersOwnListenSocket = {
      expr = (m.serverSocket "web" (server { listen.socket = "%t/elsewhere/web.sock"; })).Socket.ListenStream;
      expected = "%t/elsewhere/web.sock";
    };
    testSocketJoinsTheTarget = {
      expr = let s = m.serverSocket "web" cfg.servers.web; in [ s.Unit.PartOf s.Install.WantedBy ];
      expected = [ [ "rafa-mcp.target" ] [ "rafa-mcp.target" ] ];
    };
    testStdioServersRunBehindTheSidecar = {
      expr = (m.serverService mcp "web" cfg.servers.web).Service.ExecStart;
      expected = ''"/nix/store/x-mcp/bin/mcp" "sidecar" "--name" "web" "--" "/nix/store/y-srv/bin/srv" "--stdio"'';
    };
    testNativeServersRunDirectly = {
      expr = (m.serverService mcp "n" (server { native = true; })).Service.ExecStart;
      expected = ''"/nix/store/y-srv/bin/srv" "--stdio"'';
    };
    testServiceIsActivatedHardenedAndLogsUnderItsName = {
      expr = let s = m.serverService mcp "web" (server { environment.A = "1"; credentials.token = "/run/agenix/t"; }); in {
        inherit (s.Unit) Requires After PartOf;
        inherit (s.Service) Type Restart RestartSec SyslogIdentifier NoNewPrivileges PrivateTmp Environment LoadCredential;
      };
      expected = {
        Requires = [ "mcp-web.socket" ];
        After = [ "mcp-web.socket" ];
        PartOf = [ "rafa-mcp.target" ];
        Type = "notify";
        Restart = "on-failure";
        RestartSec = "5s";
        SyslogIdentifier = "mcp-web";
        NoNewPrivileges = true;
        PrivateTmp = true;
        Environment = [ "A=1" ];
        LoadCredential = [ "token:/run/agenix/t" ];
      };
    };
    testNativeServersCanOptOutOfNotify = {
      expr = (m.serverService mcp "n" (server { native = true; notify = false; })).Service.Type;
      expected = "simple";
    };
    testExtraServiceConfigWins = {
      expr = (m.serverService mcp "web" (server { extraServiceConfig.MemoryMax = "1G"; extraServiceConfig.Restart = "always"; })).Service;
      expected = (m.serverService mcp "web" (server { })).Service // { MemoryMax = "1G"; Restart = "always"; };
    };
    testProxyIsANotifyServiceInTheTarget = {
      expr = let p = m.proxyService mcp cfg; in [ p.Service.Type p.Service.ExecStart p.Service.SyslogIdentifier p.Install.WantedBy ];
      expected = [ "notify" ''"/nix/store/x-mcp/bin/mcp" "proxy"'' "mcp-proxy" [ "rafa-mcp.target" ] ];
    };
  };
in
if results != [ ] then throw "mcp-lib tests failed: ${builtins.toJSON results}"
else runCommand "check-mcp-lib" { } ''
  echo "[+] lib/mcp.nix: all runTests cases pass"
  touch $out
''
