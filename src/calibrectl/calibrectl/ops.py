"""
Library operations shared by the CLI and the MCP server. They raise NotFound or Invalid and
never print or exit: the callers decide how to report.
"""

import re
import shutil
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from calibrectl.db import (
    backup_database,
    connect_database,
    get_or_create_author,
    get_or_create_publisher,
    get_or_create_rating,
    get_or_create_series,
    get_or_create_tag,
    query_book_by_id,
)
from calibrectl.formatters import convert_date_to_iso
from calibrectl.paths import detect_file_format, sanitize_path_component


class NotFound(LookupError):
    """A book (or file) that does not exist."""


class Invalid(ValueError):
    """A request that cannot be carried out as given."""


UPDATE_FIELDS = (
    "title", "authors", "add_author", "remove_author", "isbn", "publisher", "pubdate",
    "abstract", "tags", "add_tag", "remove_tag", "series", "series_index", "rating",
)


def utc_now() -> str:
    """Calibre's timestamp format, in UTC."""
    return datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S") + "+00:00"


def _names(value: Any) -> list[str]:
    items = value.split(",") if isinstance(value, str) else list(value)
    return [str(i).strip() for i in items if str(i).strip()]


def _check(fields: dict[str, Any]) -> dict[str, Any]:
    unknown = sorted(set(fields) - set(UPDATE_FIELDS))
    if unknown:
        raise Invalid(f"unknown fields {unknown}; allowed: {', '.join(UPDATE_FIELDS)}")
    given = {k: v for k, v in fields.items() if v is not None}
    if not given:
        raise Invalid("no fields to update")
    rating = given.get("rating")
    if rating is not None and not (isinstance(rating, int) and 0 <= rating <= 10):
        raise Invalid(f"rating {rating!r}: an integer from 0 to 10")
    return given


def _database(database_path: Path) -> Path:
    if not database_path.exists():
        raise NotFound(f"database not found: {database_path}")
    return database_path


def _book(database_path: Path, book_id: int) -> dict[str, Any]:
    book = query_book_by_id(database_path, book_id)
    if not book:
        raise NotFound(f"book {book_id} not found")
    return book


def _relink(conn, book_id: int, table: str, column: str, ids: list[int]) -> None:
    conn.execute(f"DELETE FROM books_{table}_link WHERE book = ?", (book_id,))
    for i in ids:
        conn.execute(f"INSERT INTO books_{table}_link (book, {column}) VALUES (?, ?)", (book_id, i))


def _add_link(conn, book_id: int, table: str, column: str, i: int) -> bool:
    if conn.execute(f"SELECT 1 FROM books_{table}_link WHERE book = ? AND {column} = ?", (book_id, i)).fetchone():
        return False
    conn.execute(f"INSERT INTO books_{table}_link (book, {column}) VALUES (?, ?)", (book_id, i))
    return True


def _remove_link(conn, book_id: int, table: str, column: str, names_table: str, name: str) -> bool:
    row = conn.execute(f"SELECT id FROM {names_table} WHERE name = ?", (name,)).fetchone()
    if not row:
        return False
    conn.execute(f"DELETE FROM books_{table}_link WHERE book = ? AND {column} = ?", (book_id, row["id"]))
    return True


def update_metadata(database_path: Path, book_id: int, fields: dict[str, Any], *, backup: bool = True) -> dict[str, Any]:
    """Apply `fields` (keys from UPDATE_FIELDS; authors and tags as lists or comma strings)."""
    f = _check(fields)
    db = _database(database_path)
    backup_path = str(backup_database(db)) if backup else None
    with connect_database(db, read_only=False) as conn:
        conn.execute("BEGIN TRANSACTION")
        try:
            book = _book(db, book_id)
            updated: list[str] = []
            if "title" in f:
                conn.execute("UPDATE books SET title = ? WHERE id = ?", (f["title"], book_id))
                updated.append("title")
            if "isbn" in f:
                conn.execute("DELETE FROM identifiers WHERE book = ? AND type = 'isbn'", (book_id,))
                conn.execute("INSERT INTO identifiers (book, type, val) VALUES (?, 'isbn', ?)", (book_id, f["isbn"]))
                updated.append("isbn")
            if "pubdate" in f:
                conn.execute("UPDATE books SET pubdate = ? WHERE id = ?", (convert_date_to_iso(f["pubdate"]) if f["pubdate"] else "", book_id))
                updated.append("pubdate")
            if "series_index" in f:
                conn.execute("UPDATE books SET series_index = ? WHERE id = ?", (float(f["series_index"]), book_id))
                updated.append("series_index")
            if "authors" in f:
                _relink(conn, book_id, "authors", "author", [get_or_create_author(conn, n) for n in _names(f["authors"])])
                updated.append("authors")
            if "add_author" in f and _add_link(conn, book_id, "authors", "author", get_or_create_author(conn, f["add_author"])):
                updated.append("add_author")
            if "remove_author" in f and _remove_link(conn, book_id, "authors", "author", "authors", f["remove_author"]):
                updated.append("remove_author")
            if "tags" in f:
                _relink(conn, book_id, "tags", "tag", [get_or_create_tag(conn, n) for n in _names(f["tags"])])
                updated.append("tags")
            if "add_tag" in f and _add_link(conn, book_id, "tags", "tag", get_or_create_tag(conn, f["add_tag"])):
                updated.append("add_tag")
            if "remove_tag" in f and _remove_link(conn, book_id, "tags", "tag", "tags", f["remove_tag"]):
                updated.append("remove_tag")
            if "publisher" in f:
                _relink(conn, book_id, "publishers", "publisher", [get_or_create_publisher(conn, f["publisher"])])
                updated.append("publisher")
            if "series" in f:
                _relink(conn, book_id, "series", "series", [get_or_create_series(conn, f["series"])])
                updated.append("series")
            if "rating" in f:
                _relink(conn, book_id, "ratings", "rating", [get_or_create_rating(conn, f["rating"])])
                updated.append("rating")
            if "abstract" in f:
                conn.execute("DELETE FROM comments WHERE book = ?", (book_id,))
                conn.execute("INSERT INTO comments (book, text) VALUES (?, ?)", (book_id, f["abstract"]))
                updated.append("abstract")
            conn.execute("UPDATE books SET last_modified = ? WHERE id = ?", (utc_now(), book_id))
            conn.commit()
        except Exception:
            conn.rollback()
            raise
    return {"status": "success", "book_id": book_id, "title": book["title"], "updated_fields": updated, "backup": backup_path, "message": "Book updated successfully"}


def attach_file(database_path: Path, storage_path: Path, book_id: int, source: Path, file_format: str | None = None, *, backup: bool = True) -> dict[str, Any]:
    """Copy `source` into the book's directory and register it as a format."""
    db = _database(database_path)
    if not source.is_file():
        raise NotFound(f"no such file: {source}")
    fmt = (file_format or detect_file_format(str(source)) or "").upper()
    if not fmt:
        raise Invalid(f"could not detect the format of {source.name}; give it explicitly")
    if not re.fullmatch(r"[A-Z0-9]{1,10}", fmt):
        raise Invalid(f"invalid format {fmt!r}: 1-10 letters or digits")
    backup_path = str(backup_database(db)) if backup else None
    dest: Path | None = None
    with connect_database(db, read_only=False) as conn:
        conn.execute("BEGIN TRANSACTION")
        try:
            book = _book(db, book_id)
            if conn.execute("SELECT id FROM data WHERE book = ? AND format = ?", (book_id, fmt)).fetchone():
                raise Invalid(f"book {book_id} already has a {fmt} file; delete it first")
            first_author = book["authors"][0] if book["authors"] else "Unknown"
            base = sanitize_path_component(f"{book['title']} - {first_author}")
            size = source.stat().st_size
            book_dir = (storage_path / book["storage_path"]).resolve()
            target = (book_dir / f"{base}.{fmt.lower()}").resolve()
            if target.parent != book_dir or not book_dir.is_relative_to(storage_path.resolve()):
                raise Invalid(f"the destination {target} leaves the book directory under {storage_path}")
            if target.exists():
                raise Invalid(f"{target} exists already and is not registered; refusing to overwrite it")
            shutil.copy2(source, target)
            dest = target
            conn.execute("INSERT INTO data (book, format, uncompressed_size, name) VALUES (?, ?, ?, ?)", (book_id, fmt, size, base))
            conn.execute("UPDATE books SET last_modified = ? WHERE id = ?", (utc_now(), book_id))
            conn.commit()
        except Exception:
            conn.rollback()
            if dest is not None:
                dest.unlink(missing_ok=True)
            raise
    return {"status": "success", "book_id": book_id, "format": fmt, "file_path": str(dest), "size_bytes": size, "size_mb": round(size / (1024 * 1024), 2), "backup": backup_path, "message": "File added successfully"}
