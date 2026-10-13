# browser.nix — browser automation stack: browser-use CLI, agent-browser, cua-driver, buzz CLI.
#
# All Python packages here are built from their real upstream source (git tag
# or PyPI sdist), not prebuilt wheels. A tag was used wherever the upstream
# repo tags releases; three packages (uuid7, cdp-use, fetch-use) don't tag
# releases (or the repo listed on PyPI is gone), so those use the PyPI sdist
# instead — still a from-source build via hatchling/setuptools, just not a git
# checkout.
{ lib, fetchurl, fetchFromGitHub, python3Packages, rustPlatform, makeWrapper, chromium, pkgs }:

let
  # uuid7: no tags on stevesimmons/uuid7 — sdist build (classic setup.py).
  uuid7 = python3Packages.buildPythonPackage {
    pname = "uuid7";
    version = "0.1.0";
    format = "setuptools";
    src = fetchurl {
      url = "https://files.pythonhosted.org/packages/5c/19/7472bd526591e2192926247109dbf78692e709d3e56775792fec877a7720/uuid7-0.1.0.tar.gz";
      hash = "sha256-jFeqMu50VtPMaMlcRTC8VxZG3vrAGJXPxzVFRJiUpjw=";
    };
    doCheck = false;
  };

  # cdp-use: browser-use/cdp-use has no tags — sdist build.
  cdpUse = python3Packages.buildPythonPackage {
    pname = "cdp-use";
    version = "1.4.5";
    format = "pyproject";
    src = fetchurl {
      url = "https://files.pythonhosted.org/packages/f7/7a/c549417e8c5e4dface6d5d828cd7dc72502dcea33a99f5324abf5a853ce9/cdp_use-1.4.5.tar.gz";
      hash = "sha256-DaOjLfRjNqA/9aIrxrxELNfS8tUKEY/UhW8p039tJqA=";
    };
    nativeBuildInputs = [ python3Packages.hatchling ];
    dependencies = with python3Packages; [ httpx typing-extensions websockets ];
    dontCheckRuntimeDeps = true;
    doCheck = false;
  };

  # fetch-use: browser-use/fetch-use (linked from PyPI metadata) is gone from
  # GitHub as of this writing — sdist build instead of a dead git ref.
  fetchUse = python3Packages.buildPythonPackage {
    pname = "fetch-use";
    version = "0.4.0";
    format = "pyproject";
    src = fetchurl {
      url = "https://files.pythonhosted.org/packages/5d/2d/66784fa8b66a04f170ad8f6598688b30b3a194dad4185b36d53da4ae1505/fetch_use-0.4.0.tar.gz";
      hash = "sha256-lRGYfUkH7G2sUB4h1mlG0QCY9mtdIbwqukGJzYG6GJo=";
    };
    nativeBuildInputs = [ python3Packages.hatchling ];
    dontCheckRuntimeDeps = true;
    doCheck = false;
  };

  # browser-use-sdk: published from the browser-use/sdk monorepo, whose tags
  # (vX.Y.Z) don't correspond to this subpackage's own PyPI version numbers —
  # pinning a git rev here would risk silently building the wrong version.
  # sdist build instead.
  browserUseSdk = python3Packages.buildPythonPackage {
    pname = "browser-use-sdk";
    version = "3.4.2";
    format = "pyproject";
    src = fetchurl {
      url = "https://files.pythonhosted.org/packages/1d/f0/e897f4b75d76c96017f0cff4d7264426ae5f7e26ad68656e2731b9c166a7/browser_use_sdk-3.4.2.tar.gz";
      hash = "sha256-vgULyAOzHsTp8j39cdncXxFg197AuWIyeRXK90OhAgg=";
    };
    nativeBuildInputs = [ python3Packages.hatchling ];
    dependencies = with python3Packages; [ httpx pydantic ];
    dontCheckRuntimeDeps = true;
    doCheck = false;
  };

  bubus = python3Packages.buildPythonPackage {
    pname = "bubus";
    version = "1.5.6";
    format = "pyproject";
    src = fetchFromGitHub {
      owner = "browser-use";
      repo = "bubus";
      rev = "7c09342724feabee7785f99e60e583d54bf6882c"; # tag 1.5.6
      hash = "sha256-zpVf8Ws0zjI6P9M0+8tWvRwykPKp64cwiIOxdiFZIGo=";
    };
    nativeBuildInputs = [ python3Packages.hatchling ];
    dependencies = with python3Packages; [ aiofiles anyio portalocker pydantic typing-extensions uuid7 ];
    dontCheckRuntimeDeps = true;
    doCheck = false;
  };

  browserHarness = python3Packages.buildPythonPackage {
    pname = "browser-harness";
    version = "0.1.9";
    format = "pyproject";
    src = fetchFromGitHub {
      owner = "browser-use";
      repo = "browser-harness";
      rev = "41108b8676d4bdb58b26ab3b079c0b7b0f8f3926"; # tag v0.1.9
      hash = "sha256-jL9qpR9igfDgpybgsLTlgGaThRd4QTIdPoE9V6VtuGg=";
    };
    nativeBuildInputs = [ python3Packages.setuptools ];
    dependencies = with python3Packages; [ cdpUse fetchUse pillow websockets ];
    dontCheckRuntimeDeps = true;
    doCheck = false;
  };

  browserUseDeps = with python3Packages; [
    aiohttp
    anthropic
    anyio
    click
    cloudpickle
    google-api-core
    google-api-python-client
    google-auth
    google-auth-oauthlib
    google-genai
    groq
    httpx
    inquirerpy
    markdownify
    mcp
    ollama
    openai
    pillow
    posthog
    psutil
    pydantic
    pyotp
    pypdf
    python-docx
    python-dotenv
    reportlab
    requests
    rich
    screeninfo
    typing-extensions
    browserHarness
    browserUseSdk
    bubus
    cdpUse
  ];

  # browser_harness re-execs as daemon via subprocess.Popen; requires explicit
  # PYTHONPATH since addsitedir() doesn't propagate to child processes.
  browserUsePythonPath = lib.makeSearchPath python3Packages.python.sitePackages
    (python3Packages.requiredPythonModules browserUseDeps);

  browserEnsureDebug = pkgs.callPackage ./browser-cdp.nix { inherit chromium; };

  # The real, lazy auto-launch trigger: this binary is only ever spawned as a
  # fresh subprocess when Hermes's browser_exec tool actually runs (see
  # tools/browser_use_cli.py:_find_cli/_route_backend in hermes-agent's own
  # source), never at `hermes` process start — so hanging the probe-and-launch
  # here (full mode: no `--print-only`) means Chromium only ever starts on
  # real, first use. See browser-cdp.nix for what this backs off from and why
  # a bare launch can't rely on browser-harness's own discovery here.
  browserCdpAutoRun = ''
    # Launch unless an operator override is set. A BROWSER_CDP_URL that the
    # top-level --run hook exported (HERMES_BROWSER_CDP_AUTO) is not one.
    # browser-harness reads BU_CDP_URL, so export both or the launch is wasted.
    if [ -z "''${BU_CDP_URL:-}" ] && [ -z "''${BU_CDP_WS:-}" ] \
       && { [ -z "''${BROWSER_CDP_URL:-}" ] || [ -n "''${HERMES_BROWSER_CDP_AUTO:-}" ]; }; then
      _hermes_cdp_url="$(${browserEnsureDebug}/bin/hermes-ensure-browser-debug 2>/dev/null)" || _hermes_cdp_url=""
      if [ -n "$_hermes_cdp_url" ]; then
        export BROWSER_CDP_URL="$_hermes_cdp_url" BU_CDP_URL="$_hermes_cdp_url"
      fi
    fi
  '';

  browserUse = python3Packages.buildPythonApplication {
    pname = "browser-use";
    version = "0.13.8";
    format = "pyproject";
    src = fetchFromGitHub {
      owner = "browser-use";
      repo = "browser-use";
      rev = "eb4126921bea3373f91afc49fb4b59d6eda7fed6"; # tag 0.13.8
      hash = "sha256-ysHmVM2ImZb8CZUG5DTqx141MpnBfdPB8K37XdenvkM=";
    };
    nativeBuildInputs = [ python3Packages.hatchling makeWrapper ];
    dependencies = browserUseDeps;
    makeWrapperArgs = [ "--set" "PYTHONPATH" browserUsePythonPath ];
    dontCheckRuntimeDeps = true;
    doCheck = false;
    # A second, explicit wrapProgram pass, not folded into makeWrapperArgs
    # above: buildPythonApplication's own wrap-python-hook builds its
    # wrapper via unquoted word-splitting internally, which chokes
    # ("array assign: ... syntax error near unexpected token `&&'") on a
    # multi-line --run script containing shell metacharacters. A plain
    # postFixup wrapProgram call (same pattern as hermes/hermes-agent/
    # hermes-acp in default.nix, and agent-browser below) re-wraps the
    # already-wrapped binary safely instead.
    postFixup = ''
      wrapProgram "$out/bin/browser-use" --run ${lib.escapeShellArg browserCdpAutoRun}
    '';
  };

  # buzz CLI is a Rust binary from block/buzz monorepo used by buzz/adapter.py.
  # Built from source (no standalone release); two git deps in Cargo.lock need outputHashes.
  buzzCliSrc = fetchFromGitHub {
    owner = "block";
    repo = "buzz";
    rev = "3e48f1b2365d326ee1c9582448d86a99b44ecd5d"; # v0.5.2
    hash = "sha256-VWqoIS5FMyou6fEuuUq1OUIPycAtn0kVLbm5yCQAsOs=";
  };
  buzzCli = rustPlatform.buildRustPackage {
    pname = "buzz-cli";
    version = "0.5.2";
    src = buzzCliSrc;
    cargoLock = {
      lockFile = "${buzzCliSrc}/Cargo.lock";
      outputHashes = {
        # tlongwell-block/rust-s3 fork, pinned by rev in buzz's own Cargo.toml
        "aws-creds-0.39.1" = "sha256-QAAm1phmeLFtDRgfDCoHijN1ce/rYzh18KziOUbL+hw=";
        # Mesh-LLM/mesh-llm — dozens of crates share this one repo+rev (see
        # buzz-agent's local-LLM runtime, unrelated to buzz-cli itself, but
        # still present in the shared workspace Cargo.lock); one outputHashes
        # entry covers the whole checkout for Nix's vendoring purposes.
        "mesh-llm-api-client-0.73.1" = "sha256-2ArkxK7Ze13mqkQB+JkuqVSCLeHpdxXHMZ0592VyEWw=";
      };
    };
    # Only build the CLI binary, not the rest of this large workspace
    # (admin-web/desktop/mobile/relay/agent/... aren't Nix concerns here).
    cargoBuildFlags = [ "-p" "buzz-cli" ];
    doCheck = false;
  };

  # cua-driver (MCP backend for computer_use). Built directly with
  # rustPlatform.buildRustPackage against libs/cua-driver/rust, rather than
  # importing trycua's own nix/cua-driver/package.nix — that import is
  # import-from-derivation (IFD): it reads a .nix file out of cuaDriverSrc at
  # eval time, which forces realising cuaDriverSrc during evaluation and
  # breaks `nix flake check --no-build` in any consumer flake ("path
  # '.../<hash>-source.drv' is not valid" while realising the context of the
  # import). Building inline here — same pattern as agentBrowser right below —
  # avoids IFD entirely. libs/cua-driver/rust/Cargo.lock has no git deps, so
  # cargoLock.lockFile needs no outputHashes (verified against
  # cua-driver-rs-v0.23.2).
  cuaDriverSrc = fetchFromGitHub {
    owner = "trycua";
    repo = "cua";
    # trycua/cua tags the cua-driver-rs component separately from the
    # repo-wide "vX.Y.Z" tags (those are the Python SDK's); nix/cua-driver/
    # only exists as of this component-specific tag.
    rev = "e88e9d899ac5effaeae38619527ebaa46b26ce72"; # cua-driver-rs-v0.23.2
    hash = "sha256-zM1hKX4Ri4j7sFbly14fBxGQ3/YDdtdsidvlWipcDMw=";
  };
  cuaDriver = rustPlatform.buildRustPackage {
    pname = "cua-driver";
    version = "0.23.2";
    src = "${cuaDriverSrc}/libs/cua-driver/rust";
    cargoLock.lockFile = "${cuaDriverSrc}/libs/cua-driver/rust/Cargo.lock";

    # Build only the main binary crate. The workspace also contains
    # platform-macos, platform-windows, and cua-driver-uia, gated behind
    # cfg(target_os) and not buildable on Linux.
    cargoBuildFlags = [ "-p" "cua-driver" "--features" "portal-input,portal-capture" ];
    doCheck = false;

    nativeBuildInputs = [ pkgs.pkg-config pkgs.rustPlatform.bindgenHook ];
    buildInputs = with pkgs; [
      libx11
      libxi
      libxtst
      libxext
      pipewire
      libei
      libxkbcommon
    ];

    meta = with lib; {
      description = "Cross-platform MCP server for computer-use automation";
      homepage = "https://github.com/trycua/cua";
      license = licenses.mit;
      mainProgram = "cua-driver";
      platforms = platforms.linux;
    };
  };

  # agent-browser is the CDP CLI for browser_tool.py's built-in browser toolset.
  # Built from source (Rust, Apache-2.0, no git deps). AGENT_BROWSER_EXECUTABLE_PATH
  # wired to the given chromium.
  agentBrowserSrc = fetchFromGitHub {
    owner = "vercel-labs";
    repo = "agent-browser";
    rev = "118af8eef1eebe21b949c3cf677d468330da7a46"; # v0.35.2
    hash = "sha256-7fNCG3Gu8a/93suVyDSxsOY2olWjPBaCDr8zvHrcWuY=";
  };
  agentBrowser = rustPlatform.buildRustPackage {
    pname = "agent-browser";
    version = "0.35.2";
    src = "${agentBrowserSrc}/cli";
    cargoLock.lockFile = "${agentBrowserSrc}/cli/Cargo.lock";
    doCheck = false;
    postFixup = ''
      wrapProgram "$out/bin/agent-browser" --set AGENT_BROWSER_EXECUTABLE_PATH "${chromium}/bin/chromium"
    '';
    nativeBuildInputs = [ makeWrapper ];
  };
in
{
  inherit browserUse agentBrowser cuaDriver buzzCli;
}
