# ollama from nixpkgs, wrapped so the binary (server and client) defaults the
# hardening environment. One function for every acceleration variant; models are
# never shipped (pulled with `ollama pull` into the service's models directory).
{ lib, symlinkJoin, makeWrapper, runCommand, ollama }:
let
  hardening = import ../../lib/hardening.nix { inherit lib; };
  wrapped = symlinkJoin {
    name = "${ollama.pname}-hardened-${ollama.version}";
    paths = [ ollama ];
    nativeBuildInputs = [ makeWrapper ];
    postBuild = ''
      wrapProgram $out/bin/ollama ${hardening.toWrapperArgsString hardening.env.ollama}
    '';
    inherit (ollama) version meta;
    passthru = (ollama.passthru or { }) // {
      unwrapped = ollama;
      hardeningEnv = hardening.env.ollama;
      tests.smoke = runCommand "ollama-smoke" { } ''
        export HOME=$TMPDIR
        v=$(${wrapped}/bin/ollama --version 2>&1)
        case "$v" in *${ollama.version}*) echo "[+] ollama --version: $v" ;; *) echo "[-] $v"; exit 1 ;; esac
        touch $out
      '';
    };
  };
in
wrapped
