# calibrectl (Python): every dependency from nixpkgs, built from source; the test suite
# (fixture metadata.db, no network) runs in the sandbox.
{ lib, python3Packages }:
python3Packages.buildPythonApplication {
  pname = "calibrectl";
  version = "0.1.0";
  pyproject = true;
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [ ./pyproject.toml ./calibrectl ./tests ];
  };
  build-system = [ python3Packages.hatchling ];
  dependencies = with python3Packages; [ click pyyaml bibtexparser pypdf ebooklib mcp ];
  nativeCheckInputs = with python3Packages; [ pytestCheckHook pytest-cov pytest-mock ];
  pythonImportsCheck = [ "calibrectl" ];
  meta = {
    description = "Calibre libraries from the command line and over MCP (direct metadata.db access)";
    license = lib.licenses.gpl3Plus;
    mainProgram = "calibrectl";
    platforms = lib.platforms.linux;
  };
}
