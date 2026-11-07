"""Browser lifecycle: what Hermes leaves running, and when it starts a browser at all."""

import concurrent.futures
import secrets
import subprocess
import time

import pytest

from browser_helpers import open_code, tool_message, turn_requests, watch_browser
from mock_openai.core import Step
from mock_openai.fixtures import text_response, tool_call_response
from mock_openai.server import MockOpenAI
from procs import argv_of, pids_for_session

QUESTION = "Open the article and tell me its title."


def browser_script(local_article, answer="done"):
    return [
        Step(tool_call_response("browser_exec", {"code": open_code(local_article)}), advertises="browser_exec"),
        Step(text_response(answer), last_role="tool"),
    ]


def wait_for_no_processes(home: str, seconds: float) -> list:
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        left = pids_for_session(home)
        if not left:
            return []
        time.sleep(0.5)
    return pids_for_session(home)


# Known defect, tracked as GTD bug 66290e73-746b-40d5-bc4e-b9b3cb776e8b (upstream Hermes): the atexit
# cleanup stops harness daemons only when _active_sessions is non-empty, which browser_exec never fills.
# strict: the test fails loudly the day upstream fixes it, so this marker cannot go stale.
@pytest.mark.xfail(strict=True, reason="upstream: browser_harness.daemon outlives hermes-acp (bug 66290e73)")
def test_closing_the_acp_session_leaves_no_browser_processes(mock_backend, acp_factory, local_article):
    mock_backend.script(browser_script(local_article))
    client = acp_factory()
    client.initialize()
    with watch_browser(client) as seen:
        client.prompt(client.new_session(), QUESTION, timeout=120)
    assert seen, "the turn never started a Chromium, so there is nothing to clean up"

    client.close()

    left = wait_for_no_processes(client.env["HOME"], 30)
    assert left == [], f"still running 30s after hermes-acp exited: {[argv_of(p)[:3] for p in left]}"


def test_plain_text_turn_never_starts_a_browser(mock_backend, acp_factory):
    mock_backend.script([Step(text_response("hello"))])
    client = acp_factory()
    client.initialize()

    with watch_browser(client) as seen:
        result = client.prompt(client.new_session(), "Say hello.")

    assert result.text == "hello"
    assert seen == []


def test_hermes_help_never_starts_a_browser(hermes_out, tmp_path):
    home = tmp_path / "home"
    home.mkdir()
    env = {"PATH": "/run/current-system/sw/bin:/usr/bin:/bin", "HOME": str(home),
           "HERMES_HOME": str(tmp_path / "hermes"), "TMPDIR": str(tmp_path)}

    done = subprocess.run([str(hermes_out / "bin" / "hermes"), "--help"], env=env, capture_output=True,
                          text=True, timeout=120)

    assert done.returncode == 0, done.stderr
    assert "usage" in done.stdout.lower()
    assert pids_for_session(str(home)) == []
    assert not (tmp_path / "hermes" / "tools").exists() or not any((tmp_path / "hermes" / "tools").glob("chromium*"))


def test_two_concurrent_sessions_get_separate_browsers(mock_backend, acp_factory, local_article):
    # One scripted backend per client: a shared script would hand steps out in arrival order.
    with MockOpenAI(api_key=f"sk-test-{secrets.token_hex(12)}") as other_backend:
        backends = [mock_backend, other_backend]
        for backend in backends:
            backend.script(browser_script(local_article))
        clients = [acp_factory(), acp_factory(backend=other_backend)]
        for client in clients:
            client.initialize()
        sessions = [client.new_session() for client in clients]

        with watch_browser(clients[0]) as first, watch_browser(clients[1]) as second:
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                results = list(pool.map(lambda pair: pair[0].prompt(pair[1], QUESTION, timeout=180),
                                        zip(clients, sessions)))

        assert [r.stop_reason for r in results] == ["end_turn", "end_turn"]
        for backend in backends:
            assert "Cat - Wikipedia" in tool_message(turn_requests(backend)[1])
            assert backend.problems == []

    def profiles(argvs):
        return {a for argv in argvs for a in argv if a.startswith("--user-data-dir=")}

    assert profiles(first) and profiles(second), "each session must start its own Chromium"
    assert profiles(first).isdisjoint(profiles(second)), "both sessions drove the same Chromium profile"
