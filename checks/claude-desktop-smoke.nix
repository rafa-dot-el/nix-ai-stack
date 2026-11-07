# Electron cannot start in the build sandbox, so this checks what the wrapper and the
# hardening rely on: the launcher, the desktop entry, and that the app bundle still
# reads every switch (environment names and managed-settings keys).
{ lib, runCommand, pkg }:
let
  keys = lib.attrNames pkg.passthru.hardeningEnv ++ lib.attrNames pkg.passthru.managedSettings;
in
runCommand "claude-desktop-smoke" { } ''
  test -x ${pkg}/bin/claude-desktop
  asar=${pkg}/lib/claude-desktop/resources/app.asar
  test -s "$asar" || { echo "[-] no app.asar"; exit 1; }
  for key in ${lib.escapeShellArgs keys}; do
    grep -aqF "$key" "$asar" || { echo "[-] app.asar never mentions $key"; exit 1; }
  done
  ls ${pkg}/share/applications/*.desktop >/dev/null
  echo "[+] claude-desktop ${pkg.version}: launcher, desktop entry, every hardening switch read by the app"
  touch $out
''
