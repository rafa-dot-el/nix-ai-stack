"""Minimal stdio client for the Agent Client Protocol (ACP), stdlib only.

Speaks newline-delimited JSON-RPC 2.0 with a ``hermes-acp`` child process, the
same front door an editor such as Zed uses. Everything the agent streams back
is kept in ``updates`` so tests can assert on what Hermes did, not just on the
final answer.
"""

import json
import queue
import subprocess
import threading
from dataclasses import dataclass, field
from typing import Optional

PROTOCOL_VERSION = 1


class AcpError(RuntimeError):
    def __init__(self, method: str, error: dict):
        super().__init__(f"{method} failed: {error}")
        self.method = method
        self.error = error


@dataclass
class PromptResult:
    stop_reason: str
    text: str


@dataclass
class AcpClient:
    argv: list
    env: dict
    cwd: str
    stderr_path: str
    timeout: float = 60.0
    permission_option_kind: str = "allow_once"
    updates: list = field(default_factory=list)
    permission_requests: list = field(default_factory=list)
    session_root: Optional[object] = None
    _proc: Optional[subprocess.Popen] = None
    _stderr_file: Optional[object] = None
    _lines: "queue.Queue[str]" = field(default_factory=queue.Queue)
    _next_id: int = 0
    _write_lock: threading.Lock = field(default_factory=threading.Lock)

    def start(self) -> "AcpClient":
        # stderr goes to a file, not a pipe: Hermes logs at INFO and an undrained pipe would block it.
        self._stderr_file = open(self.stderr_path, "w")
        self._proc = subprocess.Popen(
            self.argv,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=self._stderr_file,
            env=self.env,
            cwd=self.cwd,
            text=True,
            bufsize=1,
        )
        threading.Thread(target=self._pump, daemon=True).start()
        return self

    def _pump(self) -> None:
        for line in self._proc.stdout:
            self._lines.put(line)
        self._lines.put("")

    def _send(self, message: dict) -> None:
        with self._write_lock:
            self._proc.stdin.write(json.dumps(message) + "\n")
            self._proc.stdin.flush()

    def notify(self, method: str, params: dict) -> None:
        """Send a JSON-RPC notification (no id, no response)."""
        self._send({"jsonrpc": "2.0", "method": method, "params": params})

    def _answer_permission(self, request: dict) -> None:
        self.permission_requests.append(request["params"])
        options = request["params"].get("options", [])
        option = next((o for o in options if o.get("kind") == self.permission_option_kind), None)
        if option is None:
            outcome = {"outcome": "cancelled"}
        else:
            outcome = {"outcome": "selected", "optionId": option["optionId"]}
        self._send({"jsonrpc": "2.0", "id": request["id"], "result": {"outcome": outcome}})

    def request(self, method: str, params: dict, timeout: Optional[float] = None) -> dict:
        self._next_id += 1
        request_id = self._next_id
        self._send({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
        wait = timeout if timeout is not None else self.timeout
        while True:
            try:
                line = self._lines.get(timeout=wait)
            except queue.Empty:
                raise TimeoutError(f"no response to {method} within {wait}s") from None
            if not line:
                raise RuntimeError(f"hermes-acp exited while waiting for {method}: {self.stderr()}")
            message = json.loads(line)
            if message.get("id") == request_id and ("result" in message or "error" in message):
                if "error" in message:
                    raise AcpError(method, message["error"])
                return message["result"]
            if message.get("method") == "session/update":
                self.updates.append(message["params"]["update"])
            elif message.get("method") == "session/request_permission":
                self._answer_permission(message)

    def initialize(self) -> dict:
        return self.request(
            "initialize",
            {"protocolVersion": PROTOCOL_VERSION, "clientCapabilities": {}, "clientInfo": {"name": "hermes-integration", "version": "0"}},
        )

    def authenticate(self, method_id: str) -> dict:
        return self.request("authenticate", {"methodId": method_id})

    def cancel(self, session_id: str) -> None:
        self.notify("session/cancel", {"sessionId": session_id})

    def load_session(self, session_id: str) -> dict:
        """``session/load``: Hermes replays the stored history as session/update notifications."""
        self.updates.clear()
        return self.request("session/load", {"sessionId": session_id, "cwd": self.cwd, "mcpServers": []})

    def new_session(self) -> str:
        return self.request("session/new", {"cwd": self.cwd, "mcpServers": []})["sessionId"]

    def prompt(self, session_id: str, text: str, timeout: Optional[float] = None) -> PromptResult:
        self.updates.clear()
        result = self.request(
            "session/prompt",
            {"sessionId": session_id, "prompt": [{"type": "text", "text": text}]},
            timeout=timeout,
        )
        return PromptResult(stop_reason=result["stopReason"], text=self.agent_text())

    def agent_text(self) -> str:
        # Hermes separates text from tool activity with blank lines, so the final answer is trimmed.
        return "".join(
            u["content"]["text"] for u in self.updates if u.get("sessionUpdate") == "agent_message_chunk"
        ).strip()

    def tool_updates(self) -> list:
        return [u for u in self.updates if u.get("sessionUpdate") in ("tool_call", "tool_call_update")]

    def stderr(self) -> str:
        with open(self.stderr_path) as f:
            return f.read()

    def close(self) -> int:
        if self._proc is None:
            return 0
        if not self._proc.stdin.closed:
            self._proc.stdin.close()
        try:
            code = self._proc.wait(timeout=30)
        except subprocess.TimeoutExpired:
            self._proc.kill()
            code = self._proc.wait()
        self._stderr_file.close()
        return code
