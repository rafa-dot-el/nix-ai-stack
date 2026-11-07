"""The MCP server through an in-memory client session."""

import json

import pytest
from mcp.shared.memory import create_connected_server_and_client_session

from calibrectl.mcp_server import build
from calibrectl.settings import Settings
from tests.test_integration import _build_library

pytestmark = pytest.mark.anyio


@pytest.fixture
def anyio_backend():
    return "asyncio"


@pytest.fixture
def library(tmp_path):
    db, storage = _build_library(tmp_path)
    share = tmp_path / "share"
    share.mkdir()
    return db, storage, share


async def _call(session, name, args):
    r = await session.call_tool(name, args)
    text = "".join(c.text for c in r.content if hasattr(c, "text"))
    return (not r.isError), text


async def test_tools_and_annotations(library):
    db, storage, share = library
    server = build(Settings(database=db, storage=storage, attach_roots=(share,)), {})
    async with create_connected_server_and_client_session(server._mcp_server) as s:
        tools = {t.name: t for t in (await s.list_tools()).tools}
    assert set(tools) == {"search_books", "get_book", "list_tags", "export_bibtex", "update_metadata", "attach_file"}
    for ro in ("search_books", "get_book", "list_tags", "export_bibtex"):
        assert tools[ro].annotations.readOnlyHint is True, ro
    assert tools["update_metadata"].annotations.destructiveHint is True, "it replaces fields"
    assert tools["attach_file"].annotations.readOnlyHint is False and tools["attach_file"].annotations.destructiveHint is False


async def test_reads_and_writes(library):
    db, storage, share = library
    server = build(Settings(database=db, storage=storage, attach_roots=(share,)), {})
    async with create_connected_server_and_client_session(server._mcp_server) as s:
        ok, text = await _call(s, "search_books", {"title": "", "limit": 3})
        assert ok and len(json.loads(text)) == 3, text
        ok, text = await _call(s, "get_book", {"book_id": 1})
        assert ok and json.loads(text)["id"] == 1, text
        ok, text = await _call(s, "get_book", {"book_id": 999})
        assert not ok and "999" in text
        ok, text = await _call(s, "list_tags", {})
        assert ok and isinstance(json.loads(text), list)
        ok, text = await _call(s, "export_bibtex", {"book_id": 1})
        assert ok and text.lstrip().startswith("@"), text
        ok, text = await _call(s, "update_metadata", {"book_id": 1, "title": "Via MCP", "backup": False})
        assert ok and "title" in json.loads(text)["updated_fields"], text
        ok, text = await _call(s, "update_metadata", {"book_id": 1, "backup": False})
        assert not ok and "no fields" in text


async def test_attach_is_confined_to_attach_roots(library, tmp_path):
    db, storage, share = library
    (share / "inside.txt").write_text("in")
    (tmp_path / "outside.txt").write_text("out")
    (share / "link.txt").symlink_to(tmp_path / "outside.txt")
    server = build(Settings(database=db, storage=storage, attach_roots=(share,)), {})
    async with create_connected_server_and_client_session(server._mcp_server) as s:
        ok, text = await _call(s, "attach_file", {"book_id": 2, "path": str(share / "inside.txt"), "backup": False})
        assert ok, text
        for p in (tmp_path / "outside.txt", share / "link.txt", share / ".." / "outside.txt"):
            ok, text = await _call(s, "attach_file", {"book_id": 3, "path": str(p), "backup": False})
            assert not ok and "attach_roots" in text, (p, text)
    none = build(Settings(database=db, storage=storage), {})
    async with create_connected_server_and_client_session(none._mcp_server) as s:
        ok, text = await _call(s, "attach_file", {"book_id": 3, "path": str(share / "inside.txt"), "backup": False})
        assert not ok and "attach_roots" in text, "no roots configured: nothing can be attached"


async def test_without_a_library_the_tools_say_where_it_was_looked_for(tmp_path):
    server = build(Settings(database=tmp_path / "none/metadata.db"), {})
    async with create_connected_server_and_client_session(server._mcp_server) as s:
        ok, text = await _call(s, "search_books", {})
    assert not ok and "metadata.db" in text
