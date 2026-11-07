# zoteroctl (Python): every dependency from nixpkgs, built from source; the test suite
# (fixture zotero.sqlite, a mocked translation server) runs in the sandbox.
{ lib, python3Packages }:
python3Packages.buildPythonApplication {
  pname = "zoteroctl";
  version = "0.1.0";
  pyproject = true;
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [ ./pyproject.toml ./zoteroctl ./tests ];
  };
  build-system = [ python3Packages.hatchling ];
  dependencies = with python3Packages; [ click pyyaml requests bibtexparser mcp ];
  nativeCheckInputs = with python3Packages; [ pytestCheckHook pytest-cov pytest-mock responses ];
  pythonImportsCheck = [ "zoteroctl" ];
  meta = {
    description = "Zotero libraries from the command line and over MCP (direct zotero.sqlite access)";
    license = lib.licenses.gpl3Plus;
    mainProgram = "zoteroctl";
    platforms = lib.platforms.linux;
  };
}
