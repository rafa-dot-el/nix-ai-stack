# VM test: rafa.ai.runtimes.ollama starts the hardened service with no models and
# no network, answers /api/version on loopback, and runs with OLLAMA_NO_CLOUD.
{ testers, module }:
testers.runNixOSTest {
  name = "vm-ollama";
  nodes.machine = {
    imports = [ module ];
    rafa.ai.runtimes.ollama.enable = true;
    virtualisation.memorySize = 2048;
  };
  testScript = ''
    machine.wait_for_unit("ollama.service")
    machine.wait_for_open_port(11434)
    version = machine.succeed("curl -fsS http://127.0.0.1:11434/api/version")
    assert '"version"' in version, version
    env = machine.succeed("systemctl show -p Environment ollama.service")
    assert "OLLAMA_NO_CLOUD=1" in env, env
    tags = machine.succeed("curl -fsS http://127.0.0.1:11434/api/tags")
    assert '"models":[]' in tags.replace(" ", ""), tags
  '';
}
