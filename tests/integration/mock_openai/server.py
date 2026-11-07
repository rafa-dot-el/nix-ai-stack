"""HTTP boundary of the mock backend: stdlib server around the pure core.

Every request is recorded, including the rejected ones, so tests can assert on
what Hermes actually sent (auth header, tools, message roles) rather than on
what the fake chose to answer.
"""

import json
import threading
import time
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Optional

from .core import Step, check_bearer, classify, next_step, to_sse
from .fixtures import MODEL, title_response

SESSION_TITLE = "Mock session"


@dataclass
class Recorded:
    method: str
    path: str
    headers: dict
    body: Optional[dict]
    status: int


@dataclass
class _State:
    api_key: str
    script: list = field(default_factory=list)
    cursor: int = 0
    requests: list = field(default_factory=list)
    problems: list = field(default_factory=list)
    lock: threading.Lock = field(default_factory=threading.Lock)


def _make_handler(state: _State):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, format, *args):  # keep pytest output clean
            pass

        def _json(self, status: int, payload: dict) -> None:
            raw = json.dumps(payload).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)

        def _record(self, status: int, body: Optional[dict]) -> None:
            state.requests.append(Recorded(self.command, self.path, dict(self.headers), body, status))

        def do_GET(self):
            if self.path == "/v1/models":
                self._record(200, None)
                return self._json(200, {"object": "list", "data": [{"id": MODEL, "object": "model"}]})
            self._record(404, None)
            self._json(404, {"error": {"message": f"no route {self.path}", "type": "invalid_request_error"}})

        def do_POST(self):
            if self.path != "/v1/chat/completions":
                self._record(404, None)
                return self._json(404, {"error": {"message": f"no route {self.path}", "type": "invalid_request_error"}})

            length = int(self.headers.get("Content-Length", 0))
            body = json.loads(self.rfile.read(length) or b"{}")
            with state.lock:
                if not check_bearer(dict(self.headers), state.api_key):
                    self._record(401, body)
                    return self._json(401, {"error": {"message": "Incorrect API key provided", "type": "invalid_request_error", "code": "invalid_api_key"}})

                if classify(body) == "title":
                    step, problems = Step(title_response(SESSION_TITLE)), []
                else:
                    step, state.cursor, problems = next_step(state.script, state.cursor, body)
                state.problems.extend(problems)

                if step is None:
                    self._record(500, body)
                    return self._json(500, {"error": {"message": "; ".join(problems), "type": "mock_script_error"}})

                self._record(step.status, body)

            # Outside the lock: a held-back answer must not block other requests.
            if step.delay:
                time.sleep(step.delay)
            if step.status != 200:
                return self._json(step.status, step.response)
            response = step.response
            if body.get("stream"):
                raw = to_sse(response)
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("Content-Length", str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)
                return
            self._json(200, response)

    return Handler


class MockOpenAI:
    """Context-managed fake backend; use ``with MockOpenAI(key) as mock``."""

    def __init__(self, api_key: str, host: str = "127.0.0.1", port: int = 0):
        self._state = _State(api_key=api_key)
        self._server = ThreadingHTTPServer((host, port), _make_handler(self._state))
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)

    @property
    def base_url(self) -> str:
        host, port = self._server.server_address[:2]
        return f"http://{host}:{port}/v1"

    @property
    def api_key(self) -> str:
        return self._state.api_key

    @property
    def requests(self) -> list:
        return list(self._state.requests)

    @property
    def problems(self) -> list:
        return list(self._state.problems)

    def script(self, steps: list) -> None:
        """Set the model's turns in order. A bare response dict is shorthand for an unconstrained Step."""
        with self._state.lock:
            self._state.script = [s if isinstance(s, Step) else Step(s) for s in steps]
            self._state.cursor = 0

    def __enter__(self):
        self._thread.start()
        return self

    def __exit__(self, *exc):
        self._server.shutdown()
        self._server.server_close()
        return False
