"""
The MCP server (calibrectl serve, stdio): search, read, export BibTeX; update metadata and
attach files (from settings.attach_roots only).
"""

import json
from pathlib import Path
from typing import Any

from mcp.server.fastmcp import FastMCP
from mcp.types import ToolAnnotations

from calibrectl import ops
from calibrectl.bibtex import export_bibtex_books
from calibrectl.config import get_database_path, get_storage_path
from calibrectl.db import connect_database, list_all_tags, list_books_query, query_book_by_id
from calibrectl.settings import Settings

READ = ToolAnnotations(readOnlyHint=True, destructiveHint=False, idempotentHint=True, openWorldHint=False)


def _json(value: Any) -> str:
    return json.dumps(value, indent=2, default=str, ensure_ascii=False)


def inside(path: Path, roots: tuple[Path, ...]) -> Path:
    """The canonical path (symlinks and .. resolved) when it lies under one of the roots."""
    real = path.resolve(strict=True)
    for root in roots:
        try:
            real.relative_to(root.resolve())
            return real
        except (ValueError, FileNotFoundError):
            continue
    allowed = ", ".join(str(r) for r in roots) or "none configured"
    raise ops.Invalid(f"{path} is outside attach_roots ({allowed}; set them in ~/.config/rafa/calibrectl.yaml)")


def build(settings: Settings, env: dict[str, str]) -> FastMCP:
    database = settings.database or get_database_path()
    storage = settings.storage or get_storage_path(database)
    server = FastMCP("calibrectl", instructions="A Calibre library: search and read books, export BibTeX, edit metadata, attach files.")

    def db() -> Path:
        if not database.exists():
            raise ops.NotFound(f"no Calibre library at {database} (set database in ~/.config/rafa/calibrectl.yaml)")
        return database

    @server.tool(description="Books matching all given filters (title, author, series: substring; tag: exact).", annotations=READ)
    def search_books(title: str | None = None, author: str | None = None, tag: str | None = None, series: str | None = None, limit: int = 50) -> str:
        with connect_database(db()) as conn:
            return _json(list_books_query(conn, tag_name=tag, author_name=author, series_name=series, title_name=title, limit=limit))

    @server.tool(description="One book's full metadata, formats and file paths.", annotations=READ)
    def get_book(book_id: int) -> str:
        book = query_book_by_id(db(), book_id)
        if not book:
            raise ops.NotFound(f"book {book_id} not found")
        return _json(book)

    @server.tool(description="Tags with their book counts.", annotations=READ)
    def list_tags(min_books: int = 1) -> str:
        with connect_database(db()) as conn:
            return _json(list_all_tags(conn, min_books=min_books))

    @server.tool(description="BibTeX for one book, or for every book with a tag.", annotations=READ)
    def export_bibtex(book_id: int | None = None, tag: str | None = None) -> str:
        if (book_id is None) == (tag is None):
            raise ops.Invalid("give book_id or tag, one of them")
        return "\n\n".join(export_bibtex_books(db(), book_id=book_id, tag=tag))

    @server.tool(
        description="Change a book's metadata; authors and tags replace the existing ones (add_*/remove_* edit one). A database backup is made first unless backup is false.",
        annotations=ToolAnnotations(readOnlyHint=False, destructiveHint=True, idempotentHint=True, openWorldHint=False),
    )
    def update_metadata(
        book_id: int, title: str | None = None, authors: list[str] | None = None, add_author: str | None = None, remove_author: str | None = None,
        isbn: str | None = None, publisher: str | None = None, pubdate: str | None = None, abstract: str | None = None, tags: list[str] | None = None,
        add_tag: str | None = None, remove_tag: str | None = None, series: str | None = None, series_index: float | None = None, rating: int | None = None,
        backup: bool = True,
    ) -> str:
        fields = {k: v for k, v in locals().items() if k in ops.UPDATE_FIELDS}
        return _json(ops.update_metadata(db(), book_id, fields, backup=backup))

    @server.tool(
        description="Attach a file (from attach_roots only) to a book as a new format.",
        annotations=ToolAnnotations(readOnlyHint=False, destructiveHint=False, idempotentHint=False, openWorldHint=False),
    )
    def attach_file(book_id: int, path: str, file_format: str | None = None, backup: bool = True) -> str:
        source = inside(Path(path), settings.attach_roots)
        return _json(ops.attach_file(db(), storage, book_id, source, file_format, backup=backup))

    return server
