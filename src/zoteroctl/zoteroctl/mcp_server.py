"""
The MCP server (zoteroctl serve, stdio): search, read and export BibTeX; import from a URL
(through the Zotero translation server) or from BibTeX text.
"""

import ipaddress
import json
import socket
from pathlib import Path
from urllib.parse import urlsplit
from typing import Any

from mcp.server.fastmcp import FastMCP
from mcp.types import ToolAnnotations

from zoteroctl import ops
from zoteroctl.config import RuntimeConfig
from zoteroctl.db import connect_database
from zoteroctl.items import get_item_by_key, list_items_query
from zoteroctl.translation import import_from_url as translate

READ = ToolAnnotations(readOnlyHint=True, destructiveHint=False, idempotentHint=True, openWorldHint=False)


def _json(value: Any) -> str:
    return json.dumps(value, indent=2, default=str, ensure_ascii=False)


def public_url(url: str) -> str:
    """The URL when it is http(s) and every address its host resolves to is public.

    The translation server resolves the name again when it fetches, so this does not stop
    DNS rebinding or redirects to internal addresses; it stops the direct cases.
    """
    parts = urlsplit(url)
    if parts.scheme not in ("http", "https"):
        raise ops.Invalid(f"{url!r}: only http:// and https:// URLs")
    host = (parts.hostname or "").rstrip(".").lower()
    if not host:
        raise ops.Invalid(f"{url!r}: no host")
    try:
        infos = socket.getaddrinfo(host, parts.port or (443 if parts.scheme == "https" else 80), proto=socket.IPPROTO_TCP)
    except (socket.gaierror, UnicodeError) as exc:
        raise ops.Invalid(f"{host}: the host does not resolve ({exc})") from exc
    for *_, sockaddr in infos:
        ip = ipaddress.ip_address(sockaddr[0].split("%")[0])
        if ip.is_private or ip.is_loopback or ip.is_link_local or ip.is_reserved or ip.is_multicast or ip.is_unspecified:
            raise ops.Invalid(f"{host} resolves to {ip}: only public addresses are handed to the translation server")
    return url


def build(config: RuntimeConfig) -> FastMCP:
    server = FastMCP("zoteroctl", instructions="A Zotero library: search and read items, export BibTeX, import from a URL or BibTeX.")

    def db() -> Path:
        if not config.database.exists():
            raise ops.NotFound(f"no Zotero library at {config.database} (set database in ~/.config/rafa/zoteroctl/config.toml)")
        return config.database

    @server.tool(description="Items matching all given filters (title: substring; tag, collection key: exact).", annotations=READ)
    def search_items(title: str | None = None, tag: str | None = None, collection: str | None = None, limit: int = 50) -> str:
        with connect_database(db()) as conn:
            return _json(list_items_query(conn, collection_key=collection, tag_name=tag, title_name=title, limit=limit))

    @server.tool(description="One item: fields, creators, tags, collections, attachments.", annotations=READ)
    def get_item(key: str) -> str:
        with connect_database(db()) as conn:
            item = get_item_by_key(conn, key)
        if item is None:
            raise ops.NotFound(f"item {key} not found")
        return _json(item)

    @server.tool(description="BibTeX for one item (key), a collection (collection key) or the whole library (all).", annotations=READ)
    def export_bibtex(key: str | None = None, collection: str | None = None, all: bool = False) -> str:  # noqa: A002
        if not (key or collection or all):
            raise ops.Invalid("give key, collection or all")
        with connect_database(db()) as conn:
            return "\n\n".join(ops.export_entries(conn, key=key, collection=collection, export_all=all))

    @server.tool(
        description="Import the item at a public http(s) URL through the Zotero translation server (internal addresses are refused).",
        annotations=ToolAnnotations(readOnlyHint=False, destructiveHint=False, idempotentHint=False, openWorldHint=True),
    )
    def import_from_url(url: str) -> str:
        item = translate(public_url(url), config.translation_server)
        data = {"itemType": item.get("itemType", "journalArticle"), "fields": {k: v for k, v in item.items() if k not in ("itemType", "creators", "tags", "key", "version")}, "creators": item.get("creators", [])}
        return _json({"keys": ops.import_items(db(), [data])})

    @server.tool(
        description="Import every entry of a BibTeX text as a new item (in one transaction).",
        annotations=ToolAnnotations(readOnlyHint=False, destructiveHint=False, idempotentHint=False, openWorldHint=False),
    )
    def import_bibtex(bibtex: str) -> str:
        return _json({"keys": ops.import_items(db(), ops.parse_bibtex(bibtex))})

    return server
