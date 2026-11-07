# M14 done-when, in a VM: `notebooklm login` as a normal user, with no network, makes no
# browser install attempt and downloads nothing (no ~/.cache/ms-playwright). Without a display
# Chromium itself cannot open; reaching "Opening Chromium" is the point past the install probe.
{ testers, notebooklm-py }:
testers.runNixOSTest {
  name = "vm-notebooklm-login";
  nodes.machine = {
    users.users.alice = { isNormalUser = true; uid = 1000; };
    environment.systemPackages = [ notebooklm-py ];
    networking.useDHCP = false;
  };
  testScript = ''
    machine.wait_for_unit("multi-user.target")
    out = machine.succeed("su - alice -c 'timeout 90 notebooklm login 2>&1 || true'")
    print(out)
    assert "Opening Chromium" in out, out
    assert "not installed" not in out and "Installing" not in out, out
    machine.succeed("test ! -e /home/alice/.cache/ms-playwright")
    machine.fail("ip route | grep -q default")
  '';
}
