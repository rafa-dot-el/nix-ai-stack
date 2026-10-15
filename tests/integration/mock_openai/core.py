"""Pure logic of the mock backend: no sockets, no clock, no globals.

The HTTP boundary (server.py) feeds requests in and sends what comes out.
"""

import json
from dataclasses import dataclass
from typing import Optional

TITLE_SCHEMA_NAME = "session_title"


@dataclass(frozen=True)
class Step:
    """One scripted model turn, plus what the request must look like to get it."""

    response: dict
    last_role: Optional[str] = None
    advertises: Optional[str] = None


def classify(request: dict) -> str:
    """``title`` for Hermes's session-title side call, ``turn`` for everything else."""
    fmt = request.get("response_format") or {}
    name = (fmt.get("json_schema") or {}).get("name")
    return "title" if name == TITLE_SCHEMA_NAME else "turn"


def check_bearer(headers: dict, expected_key: str) -> bool:
    value = next((v for k, v in headers.items() if k.lower() == "authorization"), "")
    return value == f"Bearer {expected_key}"


def _problems_for(step: Step, request: dict) -> list:
    problems = []
    if step.last_role is not None:
        got = request["messages"][-1]["role"]
        if got != step.last_role:
            problems.append(f"expected last message role '{step.last_role}', got '{got}'")
    if step.advertises is not None:
        names = {t["function"]["name"] for t in request.get("tools", [])}
        if step.advertises not in names:
            problems.append(f"expected tool '{step.advertises}' to be advertised")
    return problems


def next_turn(script: list, cursor: int, request: dict):
    """Return ``(response, next_cursor, problems)``.

    A mismatch is recorded as a problem but still answered, so the agent never
    hangs waiting on the fake; the test asserts on the problems afterwards.
    """
    if cursor >= len(script):
        return None, cursor, [f"script exhausted at step {cursor}"]
    step = script[cursor]
    return step.response, cursor + 1, _problems_for(step, request)


def to_sse(response: dict) -> bytes:
    """Encode a complete chat.completion as the streamed chunk sequence."""
    choice = response["choices"][0]
    message = choice["message"]
    base = {
        "id": response["id"],
        "object": "chat.completion.chunk",
        "created": response["created"],
        "model": response["model"],
    }
    chunks = []
    if message.get("tool_calls"):
        deltas = [{"index": i, **call} for i, call in enumerate(message["tool_calls"])]
        chunks.append({**base, "choices": [{"index": 0, "delta": {"role": "assistant", "tool_calls": deltas}, "finish_reason": None}]})
    elif message.get("content") is not None:
        chunks.append({**base, "choices": [{"index": 0, "delta": {"role": "assistant", "content": message["content"]}, "finish_reason": None}]})
    chunks.append({**base, "choices": [{"index": 0, "delta": {}, "finish_reason": choice["finish_reason"]}]})
    body = "".join(f"data: {json.dumps(c)}\n\n" for c in chunks)
    return (body + "data: [DONE]\n\n").encode()
