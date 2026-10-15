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
import time
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
    """Start ``hermes-acp`` against the mock backend. Call with ``api_key=`` to override the profile's key."""
    started = []

    def make(api_key: Optional[str] = None) -> AcpClient:
        # Short root on purpose: browser-harness puts an AF_UNIX socket under HERMES_HOME, and the
        # kernel caps socket paths at 108 bytes, which pytest's tmp_path is too long to leave room for.
        root = Path(tempfile.mkdtemp(prefix="hi-", dir="/tmp"))
        home = root / "h"
        home.mkdir(parents=True)
        config = PROFILE_TEMPLATE.format(base_url=mock_backend.base_url, api_key=api_key or mock_backend.api_key)
        (home / "config.yaml").write_text(config)
        # TMPDIR inside the session root: the managed Chromium's profile (/tmp/agent-browser-chrome-*)
        # and Hermes's temp markers then belong to this session alone, so teardown can find and stop them.
        for name in ("tmp", "home", "run", "run/bh"):
            (root / name).mkdir(parents=True)
        env = {
            "PATH": os.environ["PATH"],
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
        client = AcpClient(
            argv=[str(hermes_out / "bin" / "hermes-acp")],
            env=env,
            cwd=str(root),
            stderr_path=str(root / "stderr.log"),
        ).start()
        started.append((client, root))
        return client

    yield make

    for client, session_root in started:
        client.close()
        _stop_session_processes(session_root)
        # HERMES_IT_KEEP_SESSIONS=1 keeps each session's logs and profile for debugging a failure.
        if not os.environ.get("HERMES_IT_KEEP_SESSIONS"):
            shutil.rmtree(session_root, ignore_errors=True)
