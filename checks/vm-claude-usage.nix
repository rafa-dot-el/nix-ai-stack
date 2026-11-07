# VM test of rafa.ai.agents.claude.usage.exporter: the user service starts for a lingering
# user, /metrics answers on loopback only (offline and without credentials: claude_fetch_ok 0),
# another path is a 404, and nothing is written under ~/.claude (claude-usage only reads it).
{ testers, homeManagerNixos, homeModule }:
testers.runNixOSTest {
  name = "vm-claude-usage";
  nodes.machine = {
    imports = [ homeManagerNixos ];
    users.users.alice = { isNormalUser = true; uid = 1000; linger = true; };
    home-manager.users.alice = {
      imports = [ homeModule ];
      home.stateVersion = "26.05";
      rafa.ai.voice.enable = false;
      rafa.ai.agents.claude.usage = { enable = true; exporter.enable = true; };
    };
  };
  testScript = ''
    def user(cmd):
        return machine.succeed(f"su - alice -c 'export XDG_RUNTIME_DIR=/run/user/1000; {cmd}'")

    machine.wait_for_unit("user@1000.service")
    machine.wait_until_succeeds("su - alice -c 'XDG_RUNTIME_DIR=/run/user/1000 systemctl --user is-active claude-usage-exporter.service'", timeout=120)
    machine.wait_for_open_port(9464)

    with subtest("/metrics answers, and says the fetch failed"):
        out = machine.succeed("curl -sf http://127.0.0.1:9464/metrics")
        assert "# TYPE claude_fetch_ok gauge" in out and "claude_fetch_ok 0" in out, out
        assert machine.succeed("curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:9464/other").strip() == "404"

    with subtest("loopback only"):
        # one listener, and its local address (column 4; column 5 is the peer) is loopback
        rows = machine.succeed("ss -Htln 'sport = :9464'").strip().splitlines()
        assert [r.split()[3] for r in rows] == ["127.0.0.1:9464"], rows

    with subtest("the service logs under its own unit, and ~/.claude is untouched"):
        user("journalctl --user -u claude-usage-exporter --no-pager | grep -q 'claude-usage exporter on'")
        machine.succeed("test ! -e /home/alice/.claude")
  '';
}
