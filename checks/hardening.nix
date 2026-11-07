# Hardening of the built Hermes: every hermes wrapper defaults lib/hardening.nix's
# Hermes variables, the config fragment ships, and `hermes --version` stays on-host.
{ callPackage, hermesPkg, forbidden }:
(callPackage ./lib/hardened-wrapper.nix { }) {
  name = "hermes";
  wrappers = map (b: "${hermesPkg}/bin/${b}") [ "hermes" "hermes-agent" "hermes-acp" ];
  env = hermesPkg.passthru.hardeningEnv;
  inherit forbidden;
  probe = "env HERMES_HOME=$TMPDIR/hermes ${hermesPkg}/bin/hermes --version";
  extraCheck = ''
    grep -q 'install_strategy: manual' ${hermesPkg}/share/hermes-agent/hardened-config.yaml \
      || { echo "[-] hardened-config.yaml missing or incomplete"; exit 1; }
    echo "[+] hardened-config.yaml ships"
  '';
}
