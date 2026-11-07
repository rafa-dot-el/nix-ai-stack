"""Fixtures shared by the integration tests.

Every test gets its own mock backend, its own Hermes profile directory and its
own browser profile root, so tests cannot see each other's state. Nothing here
reads or writes ``~/.hermes``, and the environment handed to Hermes is built
from scratch, so no real provider key can leak in.
"""

import os
import secrets
import shutil
import signal
import tempfile
import threading
import time
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Optional

import pytest

from acp_client import AcpClient
from mock_openai.server import MockOpenAI
from procs import argv_of, pids_for_session

PROFILE_TEMPLATE = """\
model:
  default: mock-1
  provider: custom
  base_url: {base_url}
  api_key: {api_key}
browser:
  backend: browser-use
"""


@pytest.fixture(scope="session")
def hermes_out() -> Path:
    out = os.environ.get("HERMES_OUT")
    if not out:
        pytest.fail("HERMES_OUT is unset: run these tests through `task test:integration`, which builds the package and exports it")
    return Path(out)


@pytest.fixture
def mock_backend():
    with MockOpenAI(api_key=f"sk-test-{secrets.token_hex(12)}") as mock:
        yield mock


def _stop_session_processes(session_root: Path) -> None:
    """Stop everything the session started (Hermes, its agent-browser daemon, Chromium) and drop their profiles."""
    home = str(session_root / "home")
    profiles = {
        a.split("=", 1)[1]
        for pid in pids_for_session(home)
        for a in argv_of(pid)
        if a.startswith("--user-data-dir=")
    }
    for pid in pids_for_session(home):
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    deadline = time.monotonic() + 10
    while pids_for_session(home) and time.monotonic() < deadline:
        time.sleep(0.2)
    for pid in pids_for_session(home):
        os.kill(pid, signal.SIGKILL)
    for profile in profiles:
        shutil.rmtree(profile, ignore_errors=True)


@pytest.fixture
def acp_factory(hermes_out, mock_backend):
    """Start ``hermes-acp`` against the mock backend.

    ``api_key=`` overrides the profile's key, ``extra_env=`` adds or overrides environment variables
    (``None`` values remove one), ``extra_config=`` is appended to the profile's config.yaml, and
    ``path=`` replaces PATH (default: the test runner's PATH), and ``backend=`` points this client at
    another MockOpenAI (default: the test's ``mock_backend``). ``reuse_root=`` starts a new hermes-acp on
    an earlier client's session root (same HERMES_HOME), e.g. to load a stored session.
    """
    started = []

    def make(api_key: Optional[str] = None, extra_env: Optional[dict] = None,
             extra_config: str = "", path: Optional[str] = None,
             backend: Optional[MockOpenAI] = None, reuse_root: Optional[Path] = None) -> AcpClient:
        backend = backend or mock_backend
        # Short root on purpose: browser-harness puts an AF_UNIX socket under HERMES_HOME, and the
        # kernel caps socket paths at 108 bytes, which pytest's tmp_path is too long to leave room for.
        root = reuse_root or Path(tempfile.mkdtemp(prefix="hi-", dir="/tmp"))
        home = root / "h"
        home.mkdir(parents=True, exist_ok=True)
        config = PROFILE_TEMPLATE.format(base_url=backend.base_url, api_key=api_key or backend.api_key)
        (home / "config.yaml").write_text(config + extra_config)
        # TMPDIR inside the session root: the managed Chromium's profile (/tmp/agent-browser-chrome-*)
        # and Hermes's temp markers then belong to this session alone, so teardown can find and stop them.
        for name in ("tmp", "home", "run", "run/bh"):
            (root / name).mkdir(parents=True, exist_ok=True)
        env = {
            "PATH": os.environ["PATH"] if path is None else path,
            "HOME": str(root / "home"),
            "TMPDIR": str(root / "tmp"),
            # agent-browser keeps its daemon sockets under XDG_RUNTIME_DIR: point it at this session,
            # or the test would attach to the user's own browser daemon (/run/user/<uid>/agent-browser).
            "XDG_RUNTIME_DIR": str(root / "run"),
            # browser-harness's daemon (one per BU_NAME) lives in a runtime dir shared by the whole user
            # unless these are set; isolate it too, or a daemon from an earlier test answers this one.
            "BH_RUNTIME_DIR": str(root / "run" / "bh"),
            "BH_TMP_DIR": str(root / "tmp"),
            "HERMES_HOME": str(home),
            "SSH_ASKPASS": "",
            "GIT_ASKPASS": "",
        }
        for key, value in (extra_env or {}).items():
            if value is None:
                env.pop(key, None)
            else:
                env[key] = value
        client = AcpClient(
            argv=[str(hermes_out / "bin" / "hermes-acp")],
            env=env,
            cwd=str(root),
            stderr_path=str(root / "stderr.log"),
        ).start()
        client.session_root = root
        if reuse_root is None:
            started.append((client, root))
        else:
            started.insert(0, (client, root))  # torn down before the owner removes the root
        return client

    yield make

    for client, session_root in started:
        client.close()
        _stop_session_processes(session_root)
        # HERMES_IT_KEEP_SESSIONS=1 keeps each session's logs and profile for debugging a failure.
        if not os.environ.get("HERMES_IT_KEEP_SESSIONS"):
            shutil.rmtree(session_root, ignore_errors=True)


class _QuietHandler(SimpleHTTPRequestHandler):
    # The article has no file extension; without text/html Chromium downloads it instead of rendering it.
    extensions_map = {**SimpleHTTPRequestHandler.extensions_map, "": "text/html"}

    def log_message(self, format, *args):
        pass


@pytest.fixture
def local_article(tmp_path):
    """A static copy of a Wikipedia-shaped page, for the offline run."""
    site = tmp_path / "site" / "wiki"
    site.mkdir(parents=True)
    (site / "Cat").write_text(
        "<!doctype html><html><head><title>Cat - Wikipedia</title></head>"
        "<body><h1 id='firstHeading'>Cat</h1><p>The cat is a domestic species.</p></body></html>"
    )
    handler = partial(_QuietHandler, directory=str(tmp_path / "site"))
    server = ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        yield f"http://127.0.0.1:{server.server_address[1]}/wiki/Cat"
    finally:
        server.shutdown()
        server.server_close()


@pytest.fixture(scope="session")
def nix_chromium(hermes_out) -> str:
    """The Chromium the package wires in (read from the hermes-acp wrapper's default)."""
    import re

    text = (hermes_out / "bin" / "hermes-acp").read_text()
    match = re.search(r"AGENT_BROWSER_EXECUTABLE_PATH-'([^']+)'", text)
    if not match:
        pytest.fail("hermes-acp wrapper sets no default AGENT_BROWSER_EXECUTABLE_PATH")
    return match.group(1)


@pytest.fixture
def operator_chromium(nix_chromium, tmp_path):
    """A headless Chromium the operator started themselves, exposing CDP on a local port."""
    import json
    import socket
    import subprocess
    import urllib.request

    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
    profile = tmp_path / "operator-profile"
    log = tmp_path / "operator-chromium.log"
    # --no-sandbox: CI runners (Ubuntu's AppArmor userns restriction) cannot start Chromium's
    # sandbox; this browser only ever loads the test's own loopback page.
    proc = subprocess.Popen(
        [nix_chromium, "--headless=new", "--no-sandbox", f"--remote-debugging-port={port}",
         f"--user-data-dir={profile}", "--no-first-run", "--no-default-browser-check", "about:blank"],
        stdout=subprocess.DEVNULL, stderr=log.open("w"), start_new_session=True,
    )
    url = f"http://127.0.0.1:{port}"
    deadline = time.monotonic() + 20
    while True:
        try:
            with urllib.request.urlopen(f"{url}/json/version", timeout=1) as resp:
                json.load(resp)
            break
        except OSError:
            if time.monotonic() > deadline or proc.poll() is not None:
                proc.kill()
                pytest.fail(f"operator Chromium did not expose CDP within 20s:\n{log.read_text()[-2000:]}")
            time.sleep(0.2)
    try:
        yield url
    finally:
        os.killpg(proc.pid, signal.SIGTERM)
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(proc.pid, signal.SIGKILL)


@pytest.fixture
def dead_cdp_url() -> str:
    """A CDP URL on a port nothing listens on."""
    import socket

    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return f"http://127.0.0.1:{probe.getsockname()[1]}"
