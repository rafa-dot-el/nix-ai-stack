"""Authentication between Hermes and the backend, and between Hermes and its ACP client."""

from acp_client import AcpError
from mock_openai.fixtures import text_response


def test_every_backend_request_carries_the_configured_bearer_key(mock_backend, acp_factory):
    mock_backend.script([text_response("ok")])
    client = acp_factory()
    client.initialize()
    client.prompt(client.new_session(), "Say hi.")

    chat = [r for r in mock_backend.requests if r.path.endswith("/chat/completions")]
    assert chat, "Hermes never reached the backend"
    for request in chat:
        assert request.headers.get("Authorization") == f"Bearer {mock_backend.api_key}"
        assert request.status == 200


def test_a_wrong_key_is_refused_and_the_agent_does_not_retry_forever(mock_backend, acp_factory):
    mock_backend.script([text_response("must never be reached")])
    client = acp_factory(api_key="sk-test-wrong")
    client.initialize()

    # v0.22 fails the turn with an ACP error (admitted_turn_failed); v0.21 ended it normally.
    try:
        text = client.prompt(client.new_session(), "Say hi.").text
    except AcpError as e:
        text = str(e)

    refused = [r for r in mock_backend.requests if r.path.endswith("/chat/completions")]
    assert refused and all(r.status == 401 for r in refused)
    assert len(refused) <= 3
    assert "must never be reached" not in text


def test_initialize_advertises_the_custom_runtime_auth_method(acp_factory, mock_backend):
    client = acp_factory()

    init = client.initialize()

    method_ids = [m["id"] for m in init["authMethods"]]
    assert "custom" in method_ids


def test_authenticate_accepts_the_advertised_method(acp_factory, mock_backend):
    client = acp_factory()
    client.initialize()

    assert client.authenticate("custom") == {}


def test_authenticate_gives_the_same_empty_result_for_an_unknown_method(acp_factory, mock_backend):
    # Observed behaviour: an unrecognised method returns the same empty result as an accepted one
    # (acp_adapter/server.py:553 returns None, which the ACP layer serialises as {}). So the reply
    # cannot signal rejection, and this test pins that contract rather than assuming a JSON-RPC error.
    client = acp_factory()
    client.initialize()

    assert client.authenticate("not-a-method") == {}
