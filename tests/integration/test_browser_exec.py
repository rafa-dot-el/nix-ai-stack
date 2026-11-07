"""The browser path end to end: the model picks browser_exec, hermes-acp drives a headless Chromium.

The model is a fixture: only its turns are scripted. What is asserted is what
Hermes sent the backend (the tool it advertised, the real page data it sent
back) and what the Chromium it started was doing while the turn ran. Hermes
closes its browser when the turn ends, so the browser is sampled during the turn.
"""

import re

import pytest

from browser_helpers import assert_headless_chromium, open_code, tool_message, turn_requests, watch_browser
from mock_openai.core import Step
from mock_openai.fixtures import text_response, tool_call_response

CAT_URL = "https://en.wikipedia.org/wiki/Cat"
CAT_QUESTION = "Open the English Wikipedia article about cats and tell me the title of the page."


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


def test_browser_use_never_downloads_a_chromium_at_runtime(mock_backend, acp_factory, local_article):
    # Hermes's package manager installs a generic (non-Nix) Chromium into HERMES_HOME/tools unless it is
    # told which browser to use; the Nix build must supply its own so nothing is fetched at runtime.
    mock_backend.script([
        Step(tool_call_response("browser_exec", {"code": open_code(local_article)}), advertises="browser_exec"),
        Step(text_response("Cat - Wikipedia"), last_role="tool"),
    ])
    client = acp_factory()
    client.initialize()

    client.prompt(client.new_session(), CAT_QUESTION)

    tools = client.session_root / "h" / "tools"
    downloaded = sorted(p.name for p in tools.glob("chromium*")) if tools.is_dir() else []
    assert downloaded == [], f"Hermes downloaded a browser at runtime: {downloaded}"
    assert "installing PM's pinned Chromium" not in client.stderr()
    assert "Cat - Wikipedia" in tool_message(turn_requests(mock_backend)[1])
