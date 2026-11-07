# antigravity-cli: the wrapper defaults the documented update switch, the binary
# still knows its name, and `agy --version` stays on-host.
{ callPackage, pkg, forbidden }:
(callPackage ./lib/hardened-wrapper.nix { }) {
  name = "antigravity-cli";
  wrappers = [ "${pkg}/bin/antigravity" ];
  env = pkg.passthru.hardeningEnv;
  inherit forbidden;
  binary = "${pkg}/bin/.antigravity-wrapped";
  probe = "${pkg}/bin/agy --version";
}
