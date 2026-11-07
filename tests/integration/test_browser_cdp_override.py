"""Operator CDP overrides win over the managed Chromium, and a dead override fails clearly.

Precedence (tools/browser_use_cli.py:_resolve_backend_cdp): BU_CDP_URL/BU_CDP_WS in the
environment, then BROWSER_CDP_URL / browser.cdp_url, then the managed Chromium. Two
regressions in this repo came from getting that wrong, so each form is pinned here.
"""

import pytest

from browser_helpers import open_code, tool_message, turn_requests, watch_browser
from mock_openai.core import Step
from mock_openai.fixtures import text_response, tool_call_response

QUESTION = "Open the article and tell me its title."


# Lines appended under the profile's `browser:` block. A connected browser (BROWSER_CDP_URL /
# browser.cdp_url) gets Hermes's SSRF guard, which refuses loopback pages such as the local test
# article; the live tests opt out explicitly, and test_connected_browser_blocks_private_urls pins
# that the guard is on by default.
ALLOW_PRIVATE = "  allow_private_urls: true\n"


def override_kwargs(form: str, url: str, allow_private: bool = False) -> dict:
    config = ALLOW_PRIVATE if allow_private else ""
    if form == "BU_CDP_URL":
        return {"extra_env": {"BU_CDP_URL": url}, "extra_config": config}
    if form == "BROWSER_CDP_URL":
        return {"extra_env": {"BROWSER_CDP_URL": url}, "extra_config": config}
    return {"extra_config": f"  cdp_url: {url}\n" + config}


def run(mock_backend, client, local_article):
    mock_backend.script([
        Step(tool_call_response("browser_exec", {"code": open_code(local_article)}), advertises="browser_exec"),
        Step(text_response("done"), last_role="tool"),
    ])
    client.initialize()
    with watch_browser(client) as managed:
        result = client.prompt(client.new_session(), QUESTION, timeout=120)
    return result, tool_message(turn_requests(mock_backend)[1]), managed


@pytest.mark.parametrize("form", ["BU_CDP_URL", "BROWSER_CDP_URL", "browser.cdp_url"])
def test_live_override_is_used_and_no_managed_chromium_starts(form, mock_backend, acp_factory, local_article, operator_chromium):
    client = acp_factory(**override_kwargs(form, operator_chromium, allow_private=True))

    result, content, managed = run(mock_backend, client, local_article)

    assert result.stop_reason == "end_turn"
    assert "Cat - Wikipedia" in content, content
    assert managed == [], f"Hermes started its own Chromium despite {form}: {managed[:1]}"
    assert mock_backend.problems == []


@pytest.mark.parametrize("form", ["BU_CDP_URL", "BROWSER_CDP_URL", "browser.cdp_url"])
def test_dead_override_fails_clearly_without_falling_back(form, mock_backend, acp_factory, local_article, dead_cdp_url):
    client = acp_factory(**override_kwargs(form, dead_cdp_url))

    result, content, managed = run(mock_backend, client, local_article)

    assert result.stop_reason == "end_turn"
    assert "Cat - Wikipedia" not in content
    assert "error" in content.lower() or "refused" in content.lower() or "not running" in content.lower(), content
    assert managed == [], f"Hermes silently fell back to its own Chromium despite a dead {form}"
    assert mock_backend.problems == []


@pytest.mark.parametrize("form", ["BROWSER_CDP_URL", "browser.cdp_url"])
def test_connected_browser_blocks_private_urls_by_default(form, mock_backend, acp_factory, local_article, operator_chromium):
    client = acp_factory(**override_kwargs(form, operator_chromium))

    result, content, _ = run(mock_backend, client, local_article)

    assert result.stop_reason == "end_turn"
    assert "private or internal address" in content, content
    assert "Cat - Wikipedia" not in content
