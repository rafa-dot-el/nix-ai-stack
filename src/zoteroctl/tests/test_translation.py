"""
Tests for zoteroctl.translation module.
"""

import json

import pytest
import responses as responses_lib

from zoteroctl.translation import import_from_url

TRANSLATION_SERVER = "http://localhost:1969"


class TestImportFromUrl:
    @responses_lib.activate
    def test_success(self):
        item = {"title": "Test Paper", "itemType": "journalArticle"}
        responses_lib.add(
            responses_lib.POST,
            f"{TRANSLATION_SERVER}/web",
            json=[item],
            status=200,
        )

        result = import_from_url("https://example.com/paper", TRANSLATION_SERVER)
        assert result["title"] == "Test Paper"
        assert result["itemType"] == "journalArticle"

    @responses_lib.activate
    def test_returns_first_item(self):
        items = [
            {"title": "First", "itemType": "journalArticle"},
            {"title": "Second", "itemType": "book"},
        ]
        responses_lib.add(
            responses_lib.POST,
            f"{TRANSLATION_SERVER}/web",
            json=items,
            status=200,
        )

        result = import_from_url("https://example.com", TRANSLATION_SERVER)
        assert result["title"] == "First"

    @responses_lib.activate
    def test_empty_response_raises(self):
        responses_lib.add(
            responses_lib.POST,
            f"{TRANSLATION_SERVER}/web",
            json=[],
            status=200,
        )

        with pytest.raises(ValueError, match="No items found"):
            import_from_url("https://example.com", TRANSLATION_SERVER)

    @responses_lib.activate
    def test_404_raises(self):
        responses_lib.add(
            responses_lib.POST,
            f"{TRANSLATION_SERVER}/web",
            status=404,
        )

        import requests as req

        with pytest.raises(req.exceptions.HTTPError):
            import_from_url("https://example.com/notfound", TRANSLATION_SERVER)

    @responses_lib.activate
    def test_500_raises(self):
        responses_lib.add(
            responses_lib.POST,
            f"{TRANSLATION_SERVER}/web",
            status=500,
        )

        import requests as req

        with pytest.raises(req.exceptions.HTTPError):
            import_from_url("https://example.com", TRANSLATION_SERVER)

    @responses_lib.activate
    def test_sends_url_in_payload(self):
        item = {"title": "Paper", "itemType": "journalArticle"}
        responses_lib.add(
            responses_lib.POST,
            f"{TRANSLATION_SERVER}/web",
            json=[item],
            status=200,
        )

        import_from_url("https://arxiv.org/abs/2301.12345", TRANSLATION_SERVER)

        assert len(responses_lib.calls) == 1
        body = json.loads(responses_lib.calls[0].request.body)
        assert body["url"] == "https://arxiv.org/abs/2301.12345"

    def test_timeout(self):
        import responses as resp_lib

        with resp_lib.RequestsMock() as rsps:
            rsps.add(
                resp_lib.POST,
                f"{TRANSLATION_SERVER}/web",
                body=__import__("requests").exceptions.ConnectTimeout(),
            )
            import requests as req

            with pytest.raises(req.exceptions.ConnectionError):
                import_from_url("https://example.com", TRANSLATION_SERVER)

    @responses_lib.activate
    def test_malformed_json_body(self):
        responses_lib.add(
            responses_lib.POST,
            f"{TRANSLATION_SERVER}/web",
            body=b"not json at all",
            status=200,
            content_type="application/json",
        )
        import requests as req

        with pytest.raises(req.exceptions.JSONDecodeError):
            import_from_url("https://example.com", TRANSLATION_SERVER)

    @responses_lib.activate
    def test_uses_correct_endpoint(self):
        item = {"title": "X", "itemType": "book"}
        responses_lib.add(
            responses_lib.POST,
            "http://myserver:8080/web",
            json=[item],
            status=200,
        )

        result = import_from_url("https://example.com", "http://myserver:8080")
        assert result["title"] == "X"
