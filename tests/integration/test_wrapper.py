"""The hermes wrappers carry their own tools: nothing may depend on what the host has on PATH."""

import os
import re

import pytest

from browser_helpers import open_code, tool_message, turn_requests
from mock_openai.core import Step
from mock_openai.fixtures import text_response, tool_call_response

WRAPPED = ["hermes", "hermes-agent", "hermes-acp"]
# Every tool the full package promises on PATH (default.nix extraPathTools, all toggles on).
EXPECTED_TOOLS = [
    "chromium", "browser-use", "buzz", "cua-driver", "agent-browser",
    "pyright", "typescript-language-server", "gopls", "rust-analyzer", "nixd", "clangd",
    "xclip", "xsel",
]


def resolve_wrapper(path) -> str:
    """Text of the makeWrapper script behind ``path``, following the desktop shim's final exec."""
    text = path.read_text()
    shim_target = re.search(r'^exec "(/nix/store/[^"]+/bin/[^"]+)" "\$@"$', text, re.M)
    if "PATH=$PATH'" not in text and shim_target:
        return resolve_wrapper(type(path)(shim_target.group(1)))
    return text


def wrapper_path_dirs(wrapper_text: str) -> list:
    """Directories a makeWrapper ``--suffix PATH`` appends: one ``PATH=$PATH'<dir>'`` line each."""
    dirs = re.findall(r"PATH=\$PATH'([^']+)'", wrapper_text)
    assert dirs, "no PATH suffix found in the wrapper"
    return dirs


@pytest.mark.parametrize("name", WRAPPED)
def test_wrapper_puts_every_promised_tool_on_path(hermes_out, name):
    dirs = wrapper_path_dirs(resolve_wrapper(hermes_out / "bin" / name))

    missing = [tool for tool in EXPECTED_TOOLS
               if not any(os.access(os.path.join(d, tool), os.X_OK) for d in dirs)]

    assert missing == [], f"{name} wrapper lacks: {missing}"


def test_browser_works_with_an_empty_host_path(mock_backend, acp_factory, local_article):
    # PATH="" leaves only what the wrapper appends; if anything still came from the host, this breaks.
    mock_backend.script([
        Step(tool_call_response("browser_exec", {"code": open_code(local_article)}), advertises="browser_exec"),
        Step(text_response("done"), last_role="tool"),
    ])
    client = acp_factory(path="")
    client.initialize()

    result = client.prompt(client.new_session(), "Open the article.", timeout=120)

    assert result.stop_reason == "end_turn"
    assert "Cat - Wikipedia" in tool_message(turn_requests(mock_backend)[1])
    assert mock_backend.problems == []
