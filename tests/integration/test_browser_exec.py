"""The browser path end to end: the model picks browser_exec, hermes-acp drives a headless Chromium.

The model is a fixture: only its turns are scripted. What is asserted is what
Hermes sent the backend (the tool it advertised, the real page data it sent
back) and what the Chromium it started was doing while the turn ran. Hermes
closes its browser when the turn ends, so the browser is sampled during the turn.
"""

import re
import threading
from contextlib import contextmanager
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

import pytest

from mock_openai.core import Step, classify
from mock_openai.fixtures import text_response, tool_call_response
from procs import argv_of, pids_for_session

CAT_URL = "https://en.wikipedia.org/wiki/Cat"
CAT_QUESTION = "Open the English Wikipedia article about cats and tell me the title of the page."


def open_code(url: str) -> str:
    # wait_for_load() alone can return before new_tab()'s navigation starts (browser-harness race),
    # so wait for the article's own heading instead.
    return f'new_tab("{url}"); wait_for_element("#firstHeading", timeout=20); print(page_info())'


def turn_requests(mock_backend):
    return [r for r in mock_backend.requests if r.body and r.path.endswith("/chat/completions") and classify(r.body) == "turn"]


def tool_message(request) -> str:
    """Content of the last tool result Hermes sent back to the model in this request."""
    return [m for m in request.body["messages"] if m["role"] == "tool"][-1]["content"]


def _chromium_argvs(client) -> list:
    argvs = [argv_of(pid) for pid in pids_for_session(client.env["HOME"])]
    return [argv for argv in argvs if any(a.startswith("--user-data-dir=") for a in argv)]


@contextmanager
def watch_browser(client):
    """Record every Chromium argv this session runs, sampled on a thread while the block runs.

    Sampled during the turn because Hermes closes its browser when the turn ends.
    """
    seen = []
    stop = threading.Event()

    def poll():
        while not stop.is_set():
            seen.extend(_chromium_argvs(client))
            stop.wait(0.2)

    thread = threading.Thread(target=poll, daemon=True)
    thread.start()
    try:
        yield seen
    finally:
        stop.set()
        thread.join()


def assert_headless_chromium(argvs: list) -> None:
    """The browser Hermes used was a real Chromium, started headless (no display in the test environment)."""
    assert argvs, "Hermes never started a Chromium for this session"
    for argv in argvs:
        assert any(a == "--headless" or a.startswith("--headless=") for a in argv), f"Chromium is not headless: {argv}"


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


@pytest.mark.network
def test_browser_exec_visits_the_cat_article_in_headless_chromium(mock_backend, acp_factory):
    mock_backend.script([
        Step(tool_call_response("browser_exec", {"code": open_code(CAT_URL)}), advertises="browser_exec"),
        Step(text_response("Cat - Wikipedia"), last_role="tool"),
    ])
    client = acp_factory()
    client.initialize()

    with watch_browser(client) as snapshots:
        result = client.prompt(client.new_session(), CAT_QUESTION)

    assert result.stop_reason == "end_turn"
    assert result.text == "Cat - Wikipedia"
    first, second = turn_requests(mock_backend)
    assert "browser_exec" in {t["function"]["name"] for t in first.body["tools"]}
    content = tool_message(second)
    assert "en.wikipedia.org/wiki/Cat" in content
    assert re.search(r"Cat.*Wikipedia", content)
    assert "chrome-not-running" not in content
    assert mock_backend.problems == []
    assert_headless_chromium(snapshots)


def test_browser_exec_visits_a_local_article_offline(mock_backend, acp_factory, local_article):
    mock_backend.script([
        Step(tool_call_response("browser_exec", {"code": open_code(local_article)}), advertises="browser_exec"),
        Step(text_response("Cat - Wikipedia"), last_role="tool"),
    ])
    client = acp_factory()
    client.initialize()

    with watch_browser(client) as snapshots:
        result = client.prompt(client.new_session(), CAT_QUESTION)

    assert result.stop_reason == "end_turn"
    content = tool_message(turn_requests(mock_backend)[1])
    assert "Cat - Wikipedia" in content
    assert mock_backend.problems == []
    assert_headless_chromium(snapshots)


def test_follow_up_query_reads_the_article_heading_in_the_same_session(mock_backend, acp_factory, local_article):
    # A second browser_exec does not inherit the first call's tab (observed), so the follow-up opens the page again.
    read_heading = (
        f'new_tab("{local_article}"); wait_for_element("#firstHeading", timeout=20); '
        "print(js('document.querySelector(\"#firstHeading\").innerText'))"
    )
    mock_backend.script([
        Step(tool_call_response("browser_exec", {"code": open_code(local_article)}), advertises="browser_exec"),
        Step(text_response("Cat - Wikipedia"), last_role="tool"),
        Step(tool_call_response("browser_exec", {"code": read_heading}), last_role="user"),
        Step(text_response("The heading is Cat."), last_role="tool"),
    ])
    client = acp_factory()
    client.initialize()
    session_id = client.new_session()
    client.prompt(session_id, CAT_QUESTION)

    with watch_browser(client) as snapshots:
        result = client.prompt(session_id, "What is the main heading on that page?")

    assert result.stop_reason == "end_turn"
    assert result.text == "The heading is Cat."
    follow_up_tool = tool_message(turn_requests(mock_backend)[3])
    assert "Cat" in follow_up_tool
    assert mock_backend.problems == []
    assert_headless_chromium(snapshots)
