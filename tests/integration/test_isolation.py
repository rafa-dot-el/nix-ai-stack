"""Isolation: a Hermes run sees only its own HERMES_HOME and never leaks host credentials.

Canary provider keys stand in for keys a real user has exported in their shell.
Hermes is configured with a custom provider and its own key, so a canary must
never reach the backend and never be written anywhere under the session.
"""

import secrets

from mock_openai.core import Step
from mock_openai.fixtures import text_response

CANARY_VARS = ["OPENROUTER_API_KEY", "ANTHROPIC_API_KEY", "OPENAI_API_KEY", "GEMINI_API_KEY"]


def test_host_provider_keys_never_reach_the_backend_or_disk(mock_backend, acp_factory):
    canaries = {name: f"canary-{name.lower()}-{secrets.token_hex(8)}" for name in CANARY_VARS}
    mock_backend.script([Step(text_response("hello"))])
    client = acp_factory(extra_env=canaries)
    client.initialize()

    result = client.prompt(client.new_session(), "Say hello.")

    assert result.text == "hello"
    sent = [r for r in mock_backend.requests if r.path.endswith("/chat/completions")]
    assert sent, "no request reached the backend"
    for request in mock_backend.requests:
        blob = repr(request.headers) + repr(request.body)
        leaked = [name for name, value in canaries.items() if value in blob]
        assert leaked == [], f"host keys sent to the backend: {leaked}"
    for request in sent:
        # Model calls carry the profile's key; a metadata GET may carry none, never a host key.
        auth = next((v for k, v in request.headers.items() if k.lower() == "authorization"), None)
        assert auth == f"Bearer {mock_backend.api_key}"
    client.close()
    written = []
    for path in client.session_root.rglob("*"):
        if path.is_file() and not path.is_symlink() and path.stat().st_size < 50_000_000:
            data = path.read_bytes()
            written += [f"{path.relative_to(client.session_root)}: {name}"
                        for name, value in canaries.items() if value.encode() in data]
    assert written == [], f"host keys written to disk: {written}"


def test_hermes_writes_only_inside_its_session(mock_backend, acp_factory):
    mock_backend.script([Step(text_response("hello"))])
    client = acp_factory()
    client.initialize()

    client.prompt(client.new_session(), "Say hello.")

    root = client.session_root
    home = root / "h"
    assert (home / "config.yaml").is_file()
    # Everything Hermes created lives under the session root; the real user home is not in its env.
    assert all(str(v).startswith(str(root)) for k, v in client.env.items() if k in ("HOME", "HERMES_HOME", "TMPDIR"))
    created = {p.name for p in home.iterdir()}
    assert "logs" in created, f"Hermes kept no state in its HERMES_HOME: {sorted(created)}"
