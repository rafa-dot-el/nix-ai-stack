# claude-code: the wrapper defaults every lib/hardening.nix variable, the binary
# still knows each name, and `claude --version` stays on-host.
{ callPackage, pkg, forbidden }:
(callPackage ./lib/hardened-wrapper.nix { }) {
  name = "claude-code";
  wrappers = [ "${pkg}/bin/claude" ];
  env = pkg.passthru.hardeningEnv;
  inherit forbidden;
  binary = "${pkg}/bin/.claude-wrapped";
  probe = "${pkg}/bin/claude --version";
}
