"""The MCP server through an in-memory client session; the translation server is mocked."""

import json

import pytest
import responses
from mcp.shared.memory import create_connected_server_and_client_session

from zoteroctl.config import RuntimeConfig
from zoteroctl.mcp_server import build
from tests.test_ops import BIB

pytestmark = pytest.mark.anyio


@pytest.fixture
def anyio_backend():
    return "asyncio"


def _config(db, storage):
    return RuntimeConfig(database=db, storage=storage, translation_server="http://translation.test:1969")


async def _call(s, name, args):
    r = await s.call_tool(name, args)
    return (not r.isError), "".join(c.text for c in r.content if hasattr(c, "text"))


async def test_tools_and_annotations(zotero_db_path, storage_dir):
    async with create_connected_server_and_client_session(build(_config(zotero_db_path, storage_dir))._mcp_server) as s:
        tools = {t.name: t for t in (await s.list_tools()).tools}
    assert set(tools) == {"search_items", "get_item", "export_bibtex", "import_from_url", "import_bibtex"}
    for ro in ("search_items", "get_item", "export_bibtex"):
        assert tools[ro].annotations.readOnlyHint is True, ro
    for w in ("import_from_url", "import_bibtex"):
        assert tools[w].annotations.readOnlyHint is False and tools[w].annotations.destructiveHint is False, w
    assert tools["import_from_url"].annotations.openWorldHint is True


async def test_search_get_export_and_bibtex_import(zotero_db_path, storage_dir):
    async with create_connected_server_and_client_session(build(_config(zotero_db_path, storage_dir))._mcp_server) as s:
        ok, text = await _call(s, "import_bibtex", {"bibtex": BIB})
        assert ok, text
        keys = json.loads(text)["keys"]
        ok, text = await _call(s, "search_items", {"title": "A Test Book"})
        assert ok and keys[0] in [i["key"] for i in json.loads(text)], text
        ok, text = await _call(s, "get_item", {"key": keys[0]})
        assert ok and json.loads(text)["key"] == keys[0], text
        ok, text = await _call(s, "get_item", {"key": "NOPE"})
        assert not ok and "NOPE" in text
        ok, text = await _call(s, "export_bibtex", {"key": keys[0]})
        assert ok and text.startswith("@book{") and "A Test Book" in text, text
        ok, text = await _call(s, "export_bibtex", {})
        assert not ok and "--all" not in text and "key, collection or all" in text


def _resolve_to(mocker, mapping):
    import socket

    def fake(host, port, *a, **kw):
        if host not in mapping:
            raise socket.gaierror("unknown host")
        return [(socket.AF_INET6 if ":" in ip else socket.AF_INET, socket.SOCK_STREAM, 6, "", (ip, port)) for ip in mapping[host]]

    mocker.patch("zoteroctl.mcp_server.socket.getaddrinfo", side_effect=fake)


@responses.activate
async def test_import_from_url_refuses_internal_destinations(zotero_db_path, storage_dir, mocker):
    # automated security review of f8f3a7d: any http(s) URL reached the translation server
    _resolve_to(mocker, {"intranet.test": ["10.0.0.17"], "mixed.test": ["93.184.215.14", "127.0.0.1"], "localhost": ["127.0.0.1"], "127.0.0.1": ["127.0.0.1"], "169.254.169.254": ["169.254.169.254"], "::1": ["::1"], "0.0.0.0": ["0.0.0.0"]})
    async with create_connected_server_and_client_session(build(_config(zotero_db_path, storage_dir))._mcp_server) as s:
        for url in ("http://127.0.0.1:8080/x", "http://localhost/", "http://169.254.169.254/latest/meta-data", "http://[::1]/", "http://0.0.0.0/", "https://intranet.test/doc", "https://mixed.test/doc", "https://nowhere.test/", "https:///nohost"):
            ok, text = await _call(s, "import_from_url", {"url": url})
            assert not ok and ("public" in text or "host" in text), (url, text)
    assert len(responses.calls) == 0, "nothing reached the translation server"


@responses.activate
async def test_import_from_url_goes_through_the_translation_server(zotero_db_path, storage_dir, mocker):
    _resolve_to(mocker, {"example.org": ["93.184.215.14"]})
    responses.post("http://translation.test:1969/web", json=[{"itemType": "book", "title": "From The Web", "creators": [{"creatorType": "author", "firstName": "A", "lastName": "B"}]}])
    async with create_connected_server_and_client_session(build(_config(zotero_db_path, storage_dir))._mcp_server) as s:
        ok, text = await _call(s, "import_from_url", {"url": "https://example.org/paper"})
        assert ok, text
        assert json.loads(responses.calls[0].request.body) == {"url": "https://example.org/paper"}
        ok, text = await _call(s, "import_from_url", {"url": "file:///etc/passwd"})
        assert not ok and "http" in text, "only http(s) URLs are handed to the translation server"
    responses.post("http://translation.test:1969/web", status=500)


async def test_without_a_library_the_tools_say_where_they_looked(tmp_path):
    async with create_connected_server_and_client_session(build(_config(tmp_path / "none/zotero.sqlite", tmp_path))._mcp_server) as s:
        ok, text = await _call(s, "search_items", {})
    assert not ok and "zotero.sqlite" in text
