{
  lib,
  python3Packages,
  fetchFromGitHub,
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

  # Built from the upstream git tag v0.8.6 (commit pinned), not from PyPI.
  src = fetchFromGitHub {
    owner = "teng-lin";
    repo = "notebooklm-py";
    rev = "956400aa7af6b61b4cb988a24c9a081a3abc4ee8";
    hash = "sha256-XH3VAxxSEt2xAn9MiRpTdeVa6Q5DWxH2H3JEReZF15U=";
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
