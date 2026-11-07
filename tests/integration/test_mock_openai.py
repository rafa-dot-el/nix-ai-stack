"""Unit tests for the pure core of the mock OpenAI backend (no sockets, no nix)."""

import json

from mock_openai.core import (
    Step,
    check_bearer,
    classify,
    next_step,
    next_turn,
    to_sse,
)
from mock_openai.fixtures import (
    error_response,
    raw_tool_call_response,
    text_response,
    title_response,
    tool_call_response,
)


def chat_request(*, messages=None, tools=None, response_format=None, stream=False):
    body = {"model": "mock-1", "messages": messages or [{"role": "user", "content": "hi"}], "stream": stream}
    if tools is not None:
        body["tools"] = tools
    if response_format is not None:
        body["response_format"] = response_format
    return body


def tool(name):
    return {"type": "function", "function": {"name": name, "parameters": {"type": "object"}}}


TITLE_FORMAT = {"type": "json_schema", "json_schema": {"name": "session_title", "strict": True, "schema": {}}}


class TestFixtures:
    def test_text_response_is_a_chat_completion_with_stop(self):
        resp = text_response("Cat - Wikipedia")

        choice = resp["choices"][0]
        assert resp["object"] == "chat.completion"
        assert choice["message"] == {"role": "assistant", "content": "Cat - Wikipedia"}
        assert choice["finish_reason"] == "stop"

    def test_tool_call_response_serialises_arguments_as_json_string(self):
        resp = tool_call_response("browser_exec", {"code": "print(1)"})

        choice = resp["choices"][0]
        call = choice["message"]["tool_calls"][0]
        assert choice["finish_reason"] == "tool_calls"
        assert call["type"] == "function"
        assert call["function"]["name"] == "browser_exec"
        assert json.loads(call["function"]["arguments"]) == {"code": "print(1)"}

    def test_title_response_is_json_content(self):
        resp = title_response("Cat visit")

        assert json.loads(resp["choices"][0]["message"]["content"]) == {"title": "Cat visit"}


class TestToSse:
    def test_text_stream_reassembles_to_the_original_text(self):
        raw = to_sse(text_response("Cat - Wikipedia")).decode()

        lines = [l[len("data: "):] for l in raw.split("\n\n") if l.startswith("data: ")]
        assert lines[-1] == "[DONE]"
        chunks = [json.loads(l) for l in lines[:-1]]
        text = "".join(c["choices"][0]["delta"].get("content", "") for c in chunks)
        assert text == "Cat - Wikipedia"
        assert chunks[-1]["choices"][0]["finish_reason"] == "stop"

    def test_tool_call_stream_carries_name_and_arguments_by_index(self):
        raw = to_sse(tool_call_response("browser_exec", {"code": "x"})).decode()

        chunks = [json.loads(l[6:]) for l in raw.split("\n\n") if l.startswith("data: ") and l != "data: [DONE]"]
        call_delta = chunks[0]["choices"][0]["delta"]["tool_calls"][0]
        assert call_delta["index"] == 0
        assert call_delta["function"]["name"] == "browser_exec"
        assert json.loads(call_delta["function"]["arguments"]) == {"code": "x"}
        assert chunks[-1]["choices"][0]["finish_reason"] == "tool_calls"


class TestClassify:
    def test_title_side_call_is_recognised_by_schema_name(self):
        assert classify(chat_request(response_format=TITLE_FORMAT)) == "title"

    def test_other_response_formats_are_turns(self):
        other = {"type": "json_schema", "json_schema": {"name": "other"}}
        assert classify(chat_request(response_format=other)) == "turn"

    def test_plain_request_is_a_turn(self):
        assert classify(chat_request()) == "turn"


class TestCheckBearer:
    def test_exact_bearer_matches(self):
        assert check_bearer({"Authorization": "Bearer sk-1"}, "sk-1")

    def test_wrong_key_does_not_match(self):
        assert not check_bearer({"Authorization": "Bearer sk-2"}, "sk-1")

    def test_missing_header_does_not_match(self):
        assert not check_bearer({}, "sk-1")

    def test_non_bearer_scheme_does_not_match(self):
        assert not check_bearer({"Authorization": "Basic sk-1"}, "sk-1")


class TestNextTurn:
    def test_steps_are_consumed_in_order(self):
        script = [Step(text_response("one")), Step(text_response("two"))]

        resp, cursor, problems = next_turn(script, 0, chat_request())
        assert resp["choices"][0]["message"]["content"] == "one"
        assert cursor == 1 and problems == []

        resp, cursor, problems = next_turn(script, cursor, chat_request())
        assert resp["choices"][0]["message"]["content"] == "two"
        assert cursor == 2 and problems == []

    def test_exhausted_script_is_reported_as_a_problem(self):
        resp, cursor, problems = next_turn([], 0, chat_request())

        assert cursor == 0
        assert problems == ["script exhausted at step 0"]
        assert resp is None

    def test_last_role_expectation_mismatch_is_reported(self):
        script = [Step(text_response("late"), last_role="tool")]

        _, _, problems = next_turn(script, 0, chat_request())

        assert problems == ["expected last message role 'tool', got 'user'"]

    def test_last_role_expectation_match_is_clean(self):
        script = [Step(text_response("ok"), last_role="tool")]
        request = chat_request(messages=[{"role": "user", "content": "q"}, {"role": "tool", "content": "r"}])

        _, _, problems = next_turn(script, 0, request)

        assert problems == []

    def test_advertised_tool_expectation_mismatch_is_reported(self):
        script = [Step(text_response("ok"), advertises="browser_exec")]

        _, _, problems = next_turn(script, 0, chat_request(tools=[tool("terminal")]))

        assert problems == ["expected tool 'browser_exec' to be advertised"]

    def test_advertised_tool_expectation_match_is_clean(self):
        script = [Step(text_response("ok"), advertises="browser_exec")]

        _, _, problems = next_turn(script, 0, chat_request(tools=[tool("browser_exec")]))

        assert problems == []

    def test_mismatch_still_answers_so_the_agent_does_not_hang(self):
        script = [Step(text_response("ok"), last_role="tool")]

        resp, cursor, problems = next_turn(script, 0, chat_request())

        assert resp["choices"][0]["message"]["content"] == "ok"
        assert cursor == 1
        assert len(problems) == 1


class TestNextStep:
    def test_returns_the_step_with_its_status_and_delay(self):
        step = Step(error_response(500, "boom"), status=500, delay=1.5)
        got, cursor, problems = next_step([step], 0, chat_request())
        assert got is step and cursor == 1 and problems == []
        assert (got.status, got.delay) == (500, 1.5)

    def test_default_step_is_an_immediate_200(self):
        got, _, _ = next_step([Step(text_response("x"))], 0, chat_request())
        assert (got.status, got.delay) == (200, 0.0)

    def test_exhausted_script_returns_no_step(self):
        got, cursor, problems = next_step([], 0, chat_request())
        assert got is None and cursor == 0 and problems == ["script exhausted at step 0"]


class TestFaultFixtures:
    def test_error_response_is_openai_shaped(self):
        assert error_response(503, "overloaded") == {
            "error": {"message": "overloaded", "type": "server_error", "code": 503}}

    def test_raw_tool_call_keeps_the_arguments_string_verbatim(self):
        resp = raw_tool_call_response("terminal", "{not json")
        call = resp["choices"][0]["message"]["tool_calls"][0]
        assert call["function"] == {"name": "terminal", "arguments": "{not json"}
