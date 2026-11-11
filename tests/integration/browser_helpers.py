"""Shared helpers for tests that drive Hermes's browser: scripted code, request lookups, Chromium sampling."""

import threading
from contextlib import contextmanager

from mock_openai.core import classify
from procs import argv_of, pids_for_session


def open_code(url: str) -> str:
    # wait_for_load() alone can return before new_tab()'s navigation starts (browser-harness race),
    # so wait for the article's own heading instead.
    return f'new_tab("{url}"); wait_for_element("#firstHeading", timeout=20); print(page_info())'


def turn_requests(mock_backend):
    return [r for r in mock_backend.requests if r.body and r.path.endswith("/chat/completions") and classify(r.body) == "turn"]


def tool_message(request) -> str:
    """Content of the last tool result Hermes sent back to the model in this request."""
    return [m for m in request.body["messages"] if m["role"] == "tool"][-1]["content"]


def browser_argvs(argvs: list) -> list:
    """The browser processes among `argvs`: has a profile dir and is not a Chromium child (--type=utility, renderer, gpu).

    Children carry --user-data-dir too but never --headless, so counting them fails the headless check at random.
    """
    return [
        argv for argv in argvs
        if any(a.startswith("--user-data-dir=") for a in argv) and not any(a.startswith("--type=") for a in argv)
    ]


def _chromium_argvs(client) -> list:
    return browser_argvs([argv_of(pid) for pid in pids_for_session(client.env["HOME"])])


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
