"""
BibTeX export: escape, cite key generation, and book-to-bibtex conversion.
"""

import re
from typing import Any

from calibrectl.db import connect_database, list_books_query, query_book_by_id
from calibrectl.formatters import echo_info


def escape_bibtex(text: str | None) -> str:
    """
    Escape special BibTeX characters.

    Args:
        text: Text to escape

    Returns:
        Escaped text safe for BibTeX
    """
    if not text:
        return ""

    _BS = "\x00BACKSLASH\x00"
    result = text.replace("\\", _BS)
    for char, replacement in [
        ("{", "\\{"),
        ("}", "\\}"),
        ("%", "\\%"),
        ("&", "\\&"),
        ("$", "\\$"),
        ("#", "\\#"),
        ("_", "\\_"),
        ("~", "\\textasciitilde{}"),
        ("^", "\\textasciicircum{}"),
    ]:
        result = result.replace(char, replacement)
    return result.replace(_BS, "\\textbackslash{}")


def generate_cite_key(book_data: dict[str, Any], used_keys: set) -> str:
    """
    Generate unique BibTeX cite key.

    Pattern: AuthorLastnameYYYY, with suffix (a, b, c) if duplicate.

    Args:
        book_data: Book data dictionary with 'authors', 'publication_date'
        used_keys: Set of already used cite keys

    Returns:
        Unique cite key
    """
    date_str = book_data.get("publication_date", "") or ""
    year_match = re.search(r"(\d{4})", date_str)
    year = year_match.group(1) if year_match else "NODATE"

    base_key = None
    authors = book_data.get("authors", [])

    if authors:
        first_author = authors[0] if isinstance(authors, list) else str(authors)
        parts = first_author.strip().split()
        if parts:
            last_name = re.sub(r"[^a-zA-Z]", "", parts[-1])
            if last_name:
                base_key = f"{last_name}{year}"

    if not base_key:
        base_key = f"Book{book_data.get('id', 'Unknown')}{year}"

    cite_key = base_key
    n = 1
    while cite_key in used_keys:
        suffix = chr(ord("a") + n - 1) if n <= 26 else str(n)
        cite_key = f"{base_key}{suffix}"
        n += 1

    used_keys.add(cite_key)
    return cite_key


def book_to_bibtex(book_data: dict[str, Any], used_keys: set) -> str:
    """
    Convert Calibre book to BibTeX entry.

    Args:
        book_data: Complete book data from query_book_by_id()
        used_keys: Set of already used cite keys

    Returns:
        BibTeX entry as string
    """
    cite_key = generate_cite_key(book_data, used_keys)

    lines = [f"@book{{{cite_key},"]

    title = book_data.get("title", "")
    if title:
        lines.append(f"  title = {{{escape_bibtex(title)}}},")

    date_str = book_data.get("publication_date", "") or ""
    if date_str:
        year_match = re.search(r"(\d{4})", date_str)
        if year_match:
            lines.append(f"  year = {{{year_match.group(1)}}},")

    authors = book_data.get("authors", [])
    if authors:
        author_str = " and ".join(authors) if isinstance(authors, list) else str(authors)
        lines.append(f"  author = {{{escape_bibtex(author_str)}}},")

    publisher = book_data.get("publisher", "")
    if publisher:
        lines.append(f"  publisher = {{{escape_bibtex(publisher)}}},")

    isbn = book_data.get("isbn", "")
    if isbn:
        lines.append(f"  isbn = {{{isbn}}},")

    series = book_data.get("series", "")
    if series:
        lines.append(f"  series = {{{escape_bibtex(series)}}},")

    abstract = book_data.get("abstract", "")
    if abstract:
        lines.append(f"  abstract = {{{escape_bibtex(abstract)}}},")

    if lines[-1].endswith(","):
        lines[-1] = lines[-1][:-1]

    lines.append("}")
    return "\n".join(lines)


def export_bibtex_books(database_path, book_id=None, tag=None, export_all=False) -> list[str]:
    """
    Export books to BibTeX entries.

    Args:
        database_path: Path to Calibre metadata.db
        book_id: Single book ID (optional)
        tag: Tag to export all books from (optional)
        export_all: Export entire library when True (optional)

    Returns:
        List of BibTeX entry strings
    """
    books_to_export = []

    if book_id:
        book_data = query_book_by_id(database_path, book_id)
        if book_data is None:
            return []
        books_to_export.append(book_data)
    elif tag:
        with connect_database(database_path, read_only=True) as conn:
            books_list = list_books_query(conn, tag_name=tag)
        for book_summary in books_list:
            book_data = query_book_by_id(database_path, book_summary["id"])
            if book_data:
                books_to_export.append(book_data)
    elif export_all:
        with connect_database(database_path, read_only=True) as conn:
            books_list = list_books_query(conn)
        for book_summary in books_list:
            book_data = query_book_by_id(database_path, book_summary["id"])
            if book_data:
                books_to_export.append(book_data)

    used_keys: set = set()
    entries = []
    for book in books_to_export:
        try:
            entries.append(book_to_bibtex(book, used_keys))
        except Exception as exc:
            echo_info(f"Warning: Failed to export book {book.get('id', 'unknown')}: {exc}")

    return entries
