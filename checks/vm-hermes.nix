# VM test: rafa.ai.agents.hermes.server.dashboard runs as its own confined user and
# serves the web dashboard on the configured port.
{ testers, module }:
testers.runNixOSTest {
  name = "vm-hermes";
  nodes.machine = {
    imports = [ module ];
    rafa.ai.agents.hermes.server.dashboard = { enable = true; port = 8080; };
    virtualisation.memorySize = 3072;
    virtualisation.diskSize = 4096;
  };
  testScript = ''
    machine.wait_for_unit("hermes-agent-dashboard.service")
    machine.wait_for_open_port(8080, timeout=180)
    page = machine.succeed("curl -fsS http://127.0.0.1:8080/")
    assert "<html" in page.lower(), page[:300]
    user = machine.succeed("systemctl show -p User --value hermes-agent-dashboard.service").strip()
    assert user == "hermes-agent", user
  '';
}
