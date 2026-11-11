# VM test of rafa.ai.mcp end to end, as a lingering user with Home Manager:
# rafa-mcp.target brings up the proxy and the playwright socket; the server starts only on
# first use (socket activation, behind `mcp sidecar`); it logs under its own identifier; a
# killed server is restarted by systemd and the proxy reconnects on the next call; gtasks is
# a native server (it takes the activated socket itself) and, unconfigured, explains itself.
{ testers, homeManagerNixos, homeModule }:
testers.runNixOSTest {
  name = "vm-mcp-stack";
  nodes.machine = {
    imports = [ homeManagerNixos ];
    virtualisation.memorySize = 3072;
    users.users.alice = { isNormalUser = true; uid = 1000; linger = true; };
    # initialize, initialized, one tools/call; stdin stays open until the answer is in
    environment.etc."mcp-vm/gtasks.sh".text = ''
      {
        cat <<'JSON'
      {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"vm","version":"1"}}}
      {"jsonrpc":"2.0","method":"notifications/initialized"}
      {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"gtasks-list","arguments":{}}}
      JSON
        sleep 5
      } | timeout 10 mcp client --profile tasks || true
    '';
    environment.etc."mcp-vm/call.sh".text = ''
      {
        cat <<'JSON'
      {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"vm","version":"1"}}}
      {"jsonrpc":"2.0","method":"notifications/initialized"}
      {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"web-browser_close","arguments":{}}}
      JSON
        sleep 20
      } | timeout 25 mcp client --profile web || true
    '';
    home-manager.users.alice = {
      imports = [ homeModule ];
      home.stateVersion = "26.05";
      rafa.ai.tools.playwright.server.enable = true;
      rafa.ai.tools.gworkspace.servers.gtasks.enable = true;
      rafa.ai.mcp = {
        enable = true;
        profiles.web.servers.playwright.include = [ "browser_close" "browser_tabs" ];
        profiles.tasks.servers.gtasks = { };
      };
    };
  };
  testScript = ''
    def user(cmd):
        return machine.succeed(f"su - alice -c 'export XDG_RUNTIME_DIR=/run/user/1000; {cmd}'")

    def call_close():
        return user("sh /etc/mcp-vm/call.sh")

    machine.wait_for_unit("user@1000.service")
    machine.wait_until_succeeds("su - alice -c 'XDG_RUNTIME_DIR=/run/user/1000 systemctl --user is-active rafa-mcp.target'", timeout=120)
    user("systemctl --user is-active mcp-proxy.service")
    user("systemctl --user is-active mcp-playwright.socket")
    user("test -S /run/user/1000/rafa/mcp/servers/playwright.sock")
    assert user("stat -c %a /run/user/1000/rafa/mcp/proxy.sock").strip() == "600"

    with subtest("the proxy found the socket; the first connection activated the server"):
        machine.wait_until_succeeds("su - alice -c 'XDG_RUNTIME_DIR=/run/user/1000 mcp doctor --profile web'", timeout=120)
        report = user("mcp doctor --json --profile web")
        print(report)
        assert '"name": "playwright"' in report, report
        assert '"web-browser_close"' in report and '"web-browser_tabs"' in report, report
        user("systemctl --user is-active mcp-playwright.service")
        logs = user("journalctl --user -t mcp-playwright --no-pager")
        assert "mcp sidecar playwright: serving" in logs, logs

    with subtest("a tool call goes agent -> mcp client -> proxy -> sidecar -> playwright-mcp"):
        out = call_close()
        print(out)
        assert '"id":2' in out and '"result"' in out, out

    with subtest("gtasks: a native server, activated by its socket, explains it is not configured"):
        user("systemctl --user is-active mcp-gtasks.socket")
        report = user("mcp doctor --json --profile tasks")
        print(report)
        assert '"name": "gtasks"' in report and '"tools": 7' in report, report
        unit = user("systemctl --user show -p ExecStart mcp-gtasks.service")
        assert "sidecar" not in unit, unit
        user("systemctl --user is-active mcp-gtasks.service")
        out = user("sh /etc/mcp-vm/gtasks.sh")
        print(out)
        assert '"id":2' in out and '"isError":true' in out and "gworkspace.yaml" in out, out

    with subtest("a killed server is restarted and the proxy reconnects"):
        before = user("systemctl --user show -p MainPID --value mcp-playwright.service").strip()
        user("systemctl --user kill -s KILL mcp-playwright.service")
        machine.wait_until_succeeds("su - alice -c 'XDG_RUNTIME_DIR=/run/user/1000 systemctl --user is-active mcp-playwright.socket'", timeout=30)
        out = call_close()
        print(out)
        assert '"id":2' in out and '"result"' in out, out
        after = user("systemctl --user show -p MainPID --value mcp-playwright.service").strip()
        assert after not in ("", "0", before), (before, after)
  '';
}
