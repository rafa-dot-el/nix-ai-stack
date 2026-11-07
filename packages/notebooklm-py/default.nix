{
  lib,
  python3Packages,
  fetchPypi,
  playwright-driver,
  makeWrapper,
}:

let
  hardening = import ../../lib/hardening.nix { inherit lib; };
in
python3Packages.buildPythonApplication rec {
  pname = "notebooklm-py";
  version = "0.8.6";
  pyproject = true;

  src = fetchPypi {
    pname = "notebooklm_py";
    inherit version;
    hash = "sha256-UaUKi0y5QlFHk+zSdoIonbme/OR1jRyzC+Is9SouSSU=";
  };

  build-system = with python3Packages; [ hatchling hatch-fancy-pypi-readme ];

  nativeBuildInputs = [ makeWrapper ];

  # Core requirements plus the `browser` extra (playwright, for `notebooklm login`).
  # The `mcp` extra (notebooklm-mcp) needs fastmcp==4.1.0, which needs mcp 2.x, mcp-types,
  # httpx2 and more that nixpkgs does not carry yet: deferred until nixpkgs has fastmcp 4.x
  # (2026-11-01). `notebooklm login` needs no patch: checks.notebooklm-py-login-offline.
  dependencies = with python3Packages; [
    httpx
    click
    rich
    filelock
    idna
    typing-extensions
    playwright
  ];

  doCheck = false;

  pythonImportsCheck = [
    "notebooklm"
    "notebooklm.client"
    "notebooklm.auth"
  ];

  makeWrapperArgs = [
    "--set" "PLAYWRIGHT_BROWSERS_PATH" "${playwright-driver.browsers}"
  ] ++ hardening.toWrapperArgs hardening.env.notebooklm-py;

  passthru.hardeningEnv = hardening.env.notebooklm-py;

  meta = {
    description = "Unofficial Python API and CLI for Google NotebookLM";
    homepage = "https://github.com/teng-lin/notebooklm-py";
    license = lib.licenses.mit;
    mainProgram = "notebooklm";
    platforms = lib.platforms.linux;
  };
}
