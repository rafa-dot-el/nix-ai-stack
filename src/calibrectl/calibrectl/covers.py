"""
set-cover command and cover management helpers.
"""

import shutil
from pathlib import Path

import click

from calibrectl.config import get_storage_path
from calibrectl.db import connect_database, query_book_by_id
from calibrectl.formatters import echo_info

VALID_COVER_EXTENSIONS = {".jpg", ".jpeg", ".png", ".gif", ".bmp", ".webp"}


def set_cover_for_book(
    database_path: Path,
    book_id: int,
    cover_file: Path,
) -> None:
    """
    Set or update cover image for a book.

    Copies the image file to the book's storage directory as cover.jpg and
    updates has_cover=1 in the database.

    Args:
        database_path: Path to metadata.db
        book_id: Book ID
        cover_file: Path to the cover image file

    Raises:
        click.ClickException: On failure
    """
    if cover_file.suffix.lower() not in VALID_COVER_EXTENSIONS:
        raise click.ClickException(
            f"Invalid image file extension. Supported: {', '.join(sorted(VALID_COVER_EXTENSIONS))}"
        )

    storage_path = get_storage_path(database_path)

    book = query_book_by_id(database_path, book_id)
    if book is None:
        raise click.ClickException(f"Book {book_id} not found")

    book_dir = storage_path / book["storage_path"]
    if not book_dir.exists():
        raise click.ClickException(f"Book directory not found: {book_dir}")

    dest_cover = book_dir / "cover.jpg"
    shutil.copy2(cover_file, dest_cover)
    echo_info(f"Copied cover to: {dest_cover}")

    with connect_database(database_path, read_only=False) as conn:
        conn.execute("UPDATE books SET has_cover = 1 WHERE id = ?", (book_id,))
        conn.commit()
