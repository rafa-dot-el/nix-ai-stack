# One builder for every first-party Rust binary in the src/ workspace (ADR-0001, ADR-0005):
# one Cargo.lock, offline vendoring from it, the crate's own tests run under nextest.
{ lib, rustPlatform, util-linux, cacert }:
{ pname, description, crate ? pname, mainProgram ? pname }:
rustPlatform.buildRustPackage {
  inherit pname;
  version = (lib.importTOML ./Cargo.toml).workspace.package.version;
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.difference
      (lib.fileset.unions [ ./Cargo.toml ./Cargo.lock (lib.fileset.fileFilter (f: true) ./.) ])
      (lib.fileset.unions [ (lib.fileset.maybeMissing ./target) ./package.nix ]);
  };
  cargoLock.lockFile = ./Cargo.lock;
  cargoBuildFlags = [ "-p" crate ];
  cargoTestFlags = [ "-p" crate ];
  useNextest = true;
  # `script`: the TUI's end-to-end test runs it on a pseudo-terminal. cacert: a trust
  # store for the reqwest (rustls) clients the HTTP tests build.
  nativeCheckInputs = [ util-linux cacert ];
  meta = {
    inherit description mainProgram;
    license = lib.licenses.gpl3Plus;
    platforms = lib.platforms.linux;
  };
}
