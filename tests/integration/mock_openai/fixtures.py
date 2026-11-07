"""Canned Chat Completions responses for scripting the model side of a test."""

import json

MODEL = "mock-1"


def _completion(message: dict, finish_reason: str) -> dict:
    return {
        "id": "chatcmpl-mock",
        "object": "chat.completion",
        "created": 0,
        "model": MODEL,
        "choices": [{"index": 0, "message": message, "finish_reason": finish_reason}],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
    }


def text_response(text: str) -> dict:
    return _completion({"role": "assistant", "content": text}, "stop")


def tool_call_response(name: str, arguments: dict) -> dict:
    return raw_tool_call_response(name, json.dumps(arguments))


def raw_tool_call_response(name: str, arguments: str) -> dict:
    """A tool call whose ``arguments`` string is sent verbatim (e.g. malformed JSON)."""
    message = {
        "role": "assistant",
        "content": None,
        "tool_calls": [
            {"id": "call_1", "type": "function", "function": {"name": name, "arguments": arguments}}
        ],
    }
    return _completion(message, "tool_calls")


def error_response(status: int, message: str) -> dict:
    """OpenAI-shaped error body; send it with ``Step(..., status=status)``."""
    return {"error": {"message": message, "type": "server_error", "code": status}}


def title_response(title: str) -> dict:
    """Answer to Hermes's session-title side call (json_schema ``session_title``)."""
    return _completion({"role": "assistant", "content": json.dumps({"title": title})}, "stop")
