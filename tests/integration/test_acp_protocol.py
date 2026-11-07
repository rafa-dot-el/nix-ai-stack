"""ACP protocol behaviour beyond a happy turn: faults, cancel, load, permissions, tool updates."""

import threading
import time

import pytest

from acp_client import AcpError
from browser_helpers import tool_message, turn_requests
from mock_openai.core import Step
from mock_openai.fixtures import error_response, raw_tool_call_response, text_response, tool_call_response

MANUAL_APPROVALS = "approvals:\n  mode: manual\n  timeout: 30\n"


def terminal_call(command: str) -> dict:
    return tool_call_response("terminal", {"command": command})


# Hermes retries a 5xx for minutes by default (agent.api_max_retries 3, then auto_recovery_cycles 5
# with 15-60s waits: 23 attempts in ~300s observed). With both turned down the turn must fail fast.
FAST_FAIL = "agent:\n  api_max_retries: 1\n  auto_recovery_cycles: 0\n"


def test_backend_500_fails_the_turn_fast_when_retries_are_off(mock_backend, acp_factory):
    mock_backend.script([Step(error_response(500, "upstream exploded"), status=500)] * 10)
    client = acp_factory(extra_config=FAST_FAIL)
    client.initialize()
    started = time.monotonic()

    try:
        result = client.prompt(client.new_session(), "hi", timeout=120)
        outcome = (result.stop_reason, result.text)
    except AcpError as e:
        outcome = ("acp-error", str(e))

    assert time.monotonic() - started < 90
    attempts = len(turn_requests(mock_backend))
    assert 1 <= attempts <= 4, f"{attempts} attempts with retries turned off"
    assert outcome[1], f"the failure reached the client as nothing: {outcome}"


def test_malformed_tool_arguments_never_run_the_tool(mock_backend, acp_factory):
    # Hermes replaces unparseable arguments and asks the model again instead of running the tool.
    mock_backend.script([
        Step(raw_tool_call_response("terminal", "{not json"), advertises="terminal"),
        Step(text_response("sorry")),
    ])
    client = acp_factory()
    client.initialize()

    result = client.prompt(client.new_session(), "Run something.", timeout=120)

    assert result.stop_reason == "end_turn"
    assert result.text == "sorry"
    assert len(turn_requests(mock_backend)) == 2
    assert client.tool_updates() == [], "a tool ran with arguments the model never sent"
    assert mock_backend.problems == []


# Known defect, GTD bug 7184625e (upstream Hermes): acp_adapter never applies agent.max_turns.
@pytest.mark.xfail(strict=True, reason="upstream: ACP ignores agent.max_turns (bug 7184625e)")
def test_turn_cap_stops_a_model_that_never_stops_calling_tools(mock_backend, acp_factory):
    mock_backend.script([Step(terminal_call("echo loop"))] * 8 + [Step(text_response("finally"))])
    client = acp_factory(extra_config="agent:\n  max_turns: 2\n")
    client.initialize()

    client.prompt(client.new_session(), "Loop.", timeout=180)

    assert len(turn_requests(mock_backend)) <= 3, "the turn cap did not stop the loop"


def test_cancel_stops_a_turn_waiting_on_the_model(mock_backend, acp_factory):
    mock_backend.script([Step(text_response("too late"), delay=20)])
    client = acp_factory()
    client.initialize()
    session_id = client.new_session()
    outcome = {}

    def run():
        try:
            outcome["result"] = client.prompt(session_id, "Take your time.", timeout=60)
        except Exception as e:  # recorded and asserted below
            outcome["error"] = e

    worker = threading.Thread(target=run)
    worker.start()
    deadline = time.monotonic() + 30
    while not turn_requests(mock_backend) and time.monotonic() < deadline:
        time.sleep(0.1)
    assert turn_requests(mock_backend), "the turn never reached the model"
    cancelled_at = time.monotonic()
    client.cancel(session_id)
    worker.join(timeout=30)

    assert not worker.is_alive(), "session/prompt did not return after session/cancel"
    assert "error" not in outcome, outcome.get("error")
    assert outcome["result"].stop_reason == "cancelled"
    assert time.monotonic() - cancelled_at < 15, "cancel waited for the model's delayed answer"


def test_load_session_replays_the_stored_conversation(mock_backend, acp_factory):
    mock_backend.script([Step(text_response("The number is 4711."))])
    first = acp_factory()
    init = first.initialize()
    assert init.get("agentCapabilities", {}).get("loadSession") is True
    session_id = first.new_session()
    first.prompt(session_id, "Remember the number 4711.")
    first.close()

    second = acp_factory(reuse_root=first.session_root)
    second.initialize()
    second.load_session(session_id)

    replayed = " ".join(
        u.get("content", {}).get("text", "") for u in second.updates
        if u.get("sessionUpdate") in ("user_message_chunk", "agent_message_chunk")
    )
    assert "Remember the number 4711." in replayed
    assert "The number is 4711." in replayed


@pytest.mark.parametrize("choice,should_run", [("allow_once", True), ("reject_once", False)])
def test_dangerous_command_asks_permission_and_obeys_the_answer(choice, should_run, mock_backend, acp_factory):
    mock_backend.script([
        Step(terminal_call("rm -r ./victim"), advertises="terminal"),
        Step(text_response("done"), last_role="tool"),
    ])
    client = acp_factory(extra_config=MANUAL_APPROVALS)
    victim = client.session_root / "victim"
    victim.mkdir()
    (victim / "file").write_text("keep me unless approved")
    client.permission_option_kind = choice
    client.initialize()

    result = client.prompt(client.new_session(), "Clean up.", timeout=120)

    assert result.stop_reason == "end_turn"
    assert client.permission_requests, "Hermes ran a recursive delete without asking"
    assert victim.exists() is (not should_run)


def test_tool_calls_stream_a_start_and_a_completed_update(mock_backend, acp_factory):
    mock_backend.script([
        Step(terminal_call("echo hello-from-tool"), advertises="terminal"),
        Step(text_response("done"), last_role="tool"),
    ])
    client = acp_factory()
    client.initialize()

    client.prompt(client.new_session(), "Echo.", timeout=120)

    starts = [u for u in client.tool_updates() if u["sessionUpdate"] == "tool_call"]
    assert len(starts) == 1, client.tool_updates()
    start = starts[0]
    assert start["toolCallId"] and start.get("title")
    assert start.get("status", "pending") in ("pending", "in_progress")
    finals = [u for u in client.tool_updates()
              if u["sessionUpdate"] == "tool_call_update" and u["toolCallId"] == start["toolCallId"]
              and u.get("status") in ("completed", "failed")]
    assert finals and finals[-1]["status"] == "completed", client.tool_updates()
    assert "hello-from-tool" in tool_message(turn_requests(mock_backend)[1])
