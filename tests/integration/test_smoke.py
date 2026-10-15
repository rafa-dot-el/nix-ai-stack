"""Boot check: hermes-acp starts, talks to the mock, and returns one plain answer."""

from mock_openai.core import classify
from mock_openai.fixtures import text_response


def turn_requests(mock_backend):
    return [r for r in mock_backend.requests if r.body and r.path.endswith("/chat/completions") and classify(r.body) == "turn"]


def test_hermes_acp_boots_and_completes_one_turn(mock_backend, acp_factory):
    # Arrange
    mock_backend.script([text_response("hello from the mock")])
    client = acp_factory()

    # Act
    client.initialize()
    session_id = client.new_session()
    result = client.prompt(session_id, "Say hi.")

    # Assert: what the agent returned
    assert result.stop_reason == "end_turn"
    assert result.text == "hello from the mock"

    # Assert: what the agent sent to the backend
    (turn,) = turn_requests(mock_backend)
    assert turn.body["model"] == "mock-1"
    assert turn.body["stream"] is True
    assert turn.body["messages"][-1] == {"role": "user", "content": "Say hi."}
    assert mock_backend.problems == []


def test_hermes_acp_reports_no_stderr_errors_on_a_clean_turn(mock_backend, acp_factory):
    mock_backend.script([text_response("ok")])
    client = acp_factory()
    client.initialize()
    client.prompt(client.new_session(), "Say hi.")
    client.close()

    assert "[ERROR]" not in client.stderr()
