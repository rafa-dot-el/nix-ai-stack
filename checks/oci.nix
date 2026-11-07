# The OCI image's entrypoint, read from the image tarball itself, is the full
# Hermes and runs (`--version` inside the sandbox through that exact path).
{ runCommand, jq, gnutar, image, hermes }:
runCommand "check-oci" { nativeBuildInputs = [ jq gnutar ]; } ''
  config=$(tar -xOf ${image} manifest.json | jq -r '.[0].Config')
  entrypoint=$(tar -xOf ${image} "$config" | jq -r '.config.Entrypoint[0]')
  [ "$entrypoint" = "${hermes}/bin/hermes" ] || { echo "[-] entrypoint is $entrypoint"; exit 1; }
  export HOME=$TMPDIR HERMES_HOME=$TMPDIR/hermes
  version=$("$entrypoint" --version 2>&1) || { echo "[-] entrypoint failed: $version"; exit 1; }
  echo "[+] entrypoint $entrypoint runs: $(echo "$version" | head -1)"
  touch $out
''
