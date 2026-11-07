"""Browser failure paths: every failure must come back to the model as a tool result, never a hung turn.

The model is scripted to call browser_exec once and then answer. What is
asserted is that the turn ends normally and that the tool message Hermes sent
back on the second request describes the failure, so a real model could react.
"""

import socket
import threading

import pytest

from browser_helpers import open_code, tool_message, turn_requests
from mock_openai.core import Step
from mock_openai.fixtures import text_response, tool_call_response

QUESTION = "Use the browser for this."
TURN_TIMEOUT = 120


def run_one_browser_call(mock_backend, client, code: str) -> tuple:
    mock_backend.script([
        Step(tool_call_response("browser_exec", {"code": code}), advertises="browser_exec"),
        Step(text_response("done"), last_role="tool"),
    ])
    client.initialize()
    result = client.prompt(client.new_session(), QUESTION, timeout=TURN_TIMEOUT)
    requests = turn_requests(mock_backend)
    assert len(requests) == 2, f"expected the model to be asked twice, got {len(requests)}"
    return result, tool_message(requests[1])


@pytest.fixture
def failing_chromium(tmp_path):
    """A 'Chromium' that exits at once, as a broken or missing browser install would."""
    fake = tmp_path / "fake-chromium"
    fake.write_text("#!/bin/sh\necho 'fake chromium: refusing to start' >&2\nexit 1\n")
    fake.chmod(0o755)
    return str(fake)


@pytest.fixture
def silent_server():
    """Accepts TCP connections and never answers: a page whose load never completes."""
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen(16)
    held = []
    stop = threading.Event()

    def accept():
        listener.settimeout(0.2)
        while not stop.is_set():
            try:
                held.append(listener.accept()[0])
            except OSError:
                continue

    thread = threading.Thread(target=accept, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{listener.getsockname()[1]}/never"
    finally:
        stop.set()
        thread.join()
        for conn in held:
            conn.close()
        listener.close()


def test_chromium_that_fails_to_start_is_reported_to_the_model(mock_backend, acp_factory, local_article, failing_chromium):
    client = acp_factory(extra_env={"AGENT_BROWSER_EXECUTABLE_PATH": failing_chromium})

    result, content = run_one_browser_call(mock_backend, client, open_code(local_article))

    assert result.stop_reason == "end_turn"
    assert "Cat - Wikipedia" not in content
    assert "could not be started" in content or "error" in content.lower(), content
    assert mock_backend.problems == []


def test_missing_element_wait_returns_false_and_the_model_sees_it(mock_backend, acp_factory, local_article):
    # browser-harness's wait_for_element returns False on timeout (documented), it does not raise.
    code = (f'new_tab("{local_article}"); wait_for_element("#firstHeading", timeout=20); '
            'print("missing:", wait_for_element("#does-not-exist", timeout=3))')
    client = acp_factory()

    result, content = run_one_browser_call(mock_backend, client, code)

    assert result.stop_reason == "end_turn"
    assert "missing: False" in content, content
    assert mock_backend.problems == []


def test_page_that_never_finishes_loading_reaches_the_model(mock_backend, acp_factory, silent_server):
    code = f'new_tab("{silent_server}"); wait_for_element("body", timeout=3); print("loaded")'
    client = acp_factory()

    result, content = run_one_browser_call(mock_backend, client, code)

    assert result.stop_reason == "end_turn"
    assert "loaded" not in content
    assert "timeout" in content.lower() or "timed out" in content.lower() or "error" in content.lower(), content
    assert mock_backend.problems == []


def test_python_exception_in_browser_code_reaches_the_model(mock_backend, acp_factory):
    client = acp_factory()

    result, content = run_one_browser_call(mock_backend, client, 'raise ValueError("boom-4711")')

    assert result.stop_reason == "end_turn"
    assert "ValueError" in content and "boom-4711" in content, content
    assert mock_backend.problems == []
