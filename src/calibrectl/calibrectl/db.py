"""
Database connection, schema validation, and Calibre metadata.db utilities.
"""

import shutil
import sqlite3
from contextlib import closing, contextmanager
from datetime import datetime
from pathlib import Path
from typing import Any

import click

from calibrectl.formatters import format_date_simple

# Tables required for calibrectl to operate correctly
REQUIRED_CALIBRE_TABLES = [
    "books",
    "authors",
    "books_authors_link",
    "tags",
    "books_tags_link",
    "series",
    "books_series_link",
    "publishers",
    "books_publishers_link",
    "ratings",
    "books_ratings_link",
    "data",
    "comments",
    "identifiers",
]


@contextmanager
def connect_database(database_path: Path, read_only: bool = True):
    """
    Context manager for database connections with read-only mode support.

    Args:
        database_path: Path to SQLite database file
        read_only: If True, opens database in read-only mode (default: True)

    Yields:
        sqlite3.Connection: Database connection with row factory

    Raises:
        click.ClickException: If database is locked or inaccessible
    """
    if not database_path.exists():
        raise click.ClickException(f"[-] Database file not found: {database_path}")

    conn = None
    try:
        if read_only:
            uri = f"file:{database_path}?mode=ro&immutable=1"
            conn = sqlite3.connect(uri, uri=True, timeout=1.0)
        else:
            conn = sqlite3.connect(str(database_path), timeout=0.1)
            conn.execute("BEGIN IMMEDIATE")
            conn.execute("ROLLBACK")
            conn.execute("PRAGMA foreign_keys = ON")

        conn.row_factory = sqlite3.Row

        if not read_only:
            import uuid as uuid_module

            def title_sort(title):
                if not title:
                    return ""
                title_lower = title.lower()
                for article in ["the ", "a ", "an "]:
                    if title_lower.startswith(article):
                        return title_lower[len(article) :]
                return title_lower

            def uuid4():
                return str(uuid_module.uuid4())

            conn.create_function("title_sort", 1, title_sort)
            conn.create_function("uuid4", 0, uuid4)

    except sqlite3.DatabaseError as e:  # OperationalError (locked) and "file is not a database"
        if conn is not None:
            conn.close()
        if "locked" in str(e).lower():
            raise click.ClickException(
                "[-] Database is locked. Close Calibre and try again."
            ) from e
        raise click.ClickException(f"[-] Database error: {e}") from e

    try:
        yield conn
    finally:
        if conn is not None:
            conn.close()


def backup_database(database_path: Path) -> Path:
    """
    Create a timestamped backup of the database file.

    Args:
        database_path: Path to database file to backup

    Returns:
        Path: Path to created backup file

    Raises:
        click.ClickException: If backup fails
    """
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    backup_path = (
        database_path.parent / f"{database_path.stem}_backup_{timestamp}{database_path.suffix}"
    )
    try:
        shutil.copy2(database_path, backup_path)
        return backup_path
    except Exception as e:
        raise click.ClickException(
            f"[-] Database backup failed: {e}. Use --no-backup to skip (NOT RECOMMENDED)."
        ) from e


def check_required_tables(existing_tables: list[str], required_tables: list[str]) -> list[str]:
    """
    Check which required tables are missing from the database.

    Pure function - no I/O, only set comparison.

    Args:
        existing_tables: List of table names found in the database
        required_tables: List of table names that must exist

    Returns:
        List of missing table names (empty if all present)

    Examples:
        >>> check_required_tables(['books', 'tags'], ['books', 'tags', 'authors'])
        ['authors']
        >>> check_required_tables(['books', 'tags', 'authors'], ['books', 'tags'])
        []
    """
    existing_set = set(existing_tables)
    return sorted([t for t in required_tables if t not in existing_set])


def detect_database_lock(database_path: Path) -> bool:
    """
    Detect if database file is locked by another process.

    Args:
        database_path: Path to database file

    Returns:
        bool: True if locked or inaccessible, False if accessible
    """
    if not database_path.exists():
        return True
    try:
        with closing(sqlite3.connect(str(database_path), timeout=0.1)) as conn:
            conn.execute("BEGIN IMMEDIATE")
            conn.execute("ROLLBACK")
            return False
    except (sqlite3.OperationalError, sqlite3.DatabaseError):
        return True


def get_or_create_author(conn: sqlite3.Connection, name: str) -> int:
    """Get existing author ID or create new author."""
    cursor = conn.execute("SELECT id FROM authors WHERE name = ?", (name,))
    row = cursor.fetchone()
    if row:
        return row["id"]
    sort_name = name
    cursor = conn.execute(
        "INSERT INTO authors (name, sort, link) VALUES (?, ?, '')",
        (name, sort_name),
    )
    assert cursor.lastrowid is not None
    return cursor.lastrowid


def get_or_create_tag(conn: sqlite3.Connection, name: str) -> int:
    """Get existing tag ID or create new tag."""
    cursor = conn.execute("SELECT id FROM tags WHERE name = ?", (name,))
    row = cursor.fetchone()
    if row:
        return row["id"]
    cursor = conn.execute("INSERT INTO tags (name) VALUES (?)", (name,))
    assert cursor.lastrowid is not None
    return cursor.lastrowid


def get_or_create_series(conn: sqlite3.Connection, name: str) -> int:
    """Get existing series ID or create new series."""
    cursor = conn.execute("SELECT id FROM series WHERE name = ?", (name,))
    row = cursor.fetchone()
    if row:
        return row["id"]
    cursor = conn.execute("INSERT INTO series (name, sort) VALUES (?, ?)", (name, name))
    assert cursor.lastrowid is not None
    return cursor.lastrowid


def get_or_create_publisher(conn: sqlite3.Connection, name: str) -> int:
    """Get existing publisher ID or create new publisher."""
    cursor = conn.execute("SELECT id FROM publishers WHERE name = ?", (name,))
    row = cursor.fetchone()
    if row:
        return row["id"]
    cursor = conn.execute("INSERT INTO publishers (name, sort) VALUES (?, ?)", (name, name))
    assert cursor.lastrowid is not None
    return cursor.lastrowid


def get_or_create_rating(conn: sqlite3.Connection, rating: int) -> int:
    """
    Get existing rating ID or create new rating.

    Note:
        Calibre stores ratings as even numbers (0, 2, 4, 6, 8, 10).
    """
    if rating < 0:
        rating = 0
    elif rating > 10:
        rating = 10
    if rating % 2 != 0:
        rating = rating + 1 if rating < 10 else rating - 1
    cursor = conn.execute("SELECT id FROM ratings WHERE rating = ?", (rating,))
    row = cursor.fetchone()
    if row:
        return row["id"]
    cursor = conn.execute("INSERT INTO ratings (rating) VALUES (?)", (rating,))
    assert cursor.lastrowid is not None
    return cursor.lastrowid


def compute_file_hash(file_path: Path, algorithm: str = "sha256") -> str | None:
    """
    Compute hash of a file using specified algorithm.

    Args:
        file_path: Path to file
        algorithm: Hash algorithm ('sha256', 'md5')

    Returns:
        Hex digest string, or None if file cannot be read
    """
    import hashlib

    if not file_path.exists() or not file_path.is_file():
        return None
    try:
        if algorithm == "sha256":
            hasher = hashlib.sha256()
        elif algorithm == "md5":
            hasher = hashlib.md5()
        else:
            raise ValueError(f"Unsupported hash algorithm: {algorithm}")
        chunk_size = 65536
        with open(file_path, "rb") as f:
            while True:
                chunk = f.read(chunk_size)
                if not chunk:
                    break
                hasher.update(chunk)
        return hasher.hexdigest()
    except Exception:
        return None


def detect_duplicate_files_by_hash(
    conn: sqlite3.Connection,
    storage_path: Path,
    algorithm: str = "sha256",
) -> dict[str, list[dict[str, Any]]]:
    """
    Detect duplicate files in Calibre library by computing file hashes.

    Args:
        conn: Database connection
        storage_path: Base storage directory
        algorithm: Hash algorithm (default: sha256)

    Returns:
        Dictionary mapping hash -> list of duplicate files
    """
    from collections import defaultdict

    cursor = conn.execute("""
        SELECT
            d.book as book_id,
            d.format,
            d.name as filename,
            b.path as book_path,
            b.title
        FROM data d
        JOIN books b ON d.book = b.id
        ORDER BY d.book, d.format
    """)

    hash_to_files = defaultdict(list)

    for row in cursor:
        file_path = storage_path / row["book_path"] / f"{row['filename']}.{row['format'].lower()}"
        if not file_path.exists() or not file_path.is_file():
            continue
        try:
            file_hash = compute_file_hash(file_path, algorithm)
            if file_hash is None:
                continue
            hash_to_files[file_hash].append(
                {
                    "book_id": row["book_id"],
                    "format": row["format"],
                    "title": row["title"],
                    "path": str(file_path),
                    "size": file_path.stat().st_size,
                }
            )
        except Exception:
            continue

    return {h: files for h, files in hash_to_files.items() if len(files) > 1}


def detect_isbn_duplicates(conn: sqlite3.Connection) -> list[dict[str, Any]]:
    """
    Detect books with duplicate ISBN identifiers.

    Returns:
        List of duplicate groups with ISBN and book details
    """
    cursor = conn.execute("""
        SELECT
            i.val as isbn,
            GROUP_CONCAT(b.id) as book_ids,
            COUNT(*) as count
        FROM identifiers i
        JOIN books b ON i.book = b.id
        WHERE i.type = 'isbn'
          AND i.val <> ''
        GROUP BY i.val
        HAVING count > 1
        ORDER BY count DESC
    """)

    duplicates = []
    processed_isbns: set = set()

    for row in cursor:
        isbn = row["isbn"]
        processed_isbns.add(isbn.lower())
        book_ids = [int(bid) for bid in row["book_ids"].split(",")]
        books = []
        for book_id in book_ids:
            book_cursor = conn.execute(
                """
                SELECT
                    b.id,
                    b.title,
                    (SELECT GROUP_CONCAT(a.name, ', ')
                     FROM books_authors_link bal
                     JOIN authors a ON bal.author = a.id
                     WHERE bal.book = b.id) as authors
                FROM books b
                WHERE b.id = ?
            """,
                (book_id,),
            )
            book_row = book_cursor.fetchone()
            if book_row:
                books.append(
                    {
                        "id": book_row["id"],
                        "title": book_row["title"],
                        "authors": book_row["authors"] or "Unknown",
                    }
                )
        duplicates.append({"isbn": isbn, "count": row["count"], "books": books})

    # Legacy fallback: older Calibre (<9) had an isbn column on books.
    # Calibre 9+ removed it. Skip gracefully if column is absent.
    has_isbn_col = any(r[1] == "isbn" for r in conn.execute("PRAGMA table_info(books)").fetchall())
    if has_isbn_col:
        cursor = conn.execute("""
            SELECT
                isbn,
                GROUP_CONCAT(id) as book_ids,
                COUNT(*) as count
            FROM books
            WHERE isbn <> ''
            GROUP BY isbn
            HAVING count > 1
            ORDER BY count DESC
        """)

        for row in cursor:
            isbn = row["isbn"]
            if isbn.lower() in processed_isbns:
                continue
            book_ids = [int(bid) for bid in row["book_ids"].split(",")]
            books = []
            for book_id in book_ids:
                book_cursor = conn.execute(
                    """
                    SELECT
                        b.id,
                        b.title,
                        (SELECT GROUP_CONCAT(a.name, ', ')
                         FROM books_authors_link bal
                         JOIN authors a ON bal.author = a.id
                         WHERE bal.book = b.id) as authors
                    FROM books b
                    WHERE b.id = ?
                """,
                    (book_id,),
                )
                book_row = book_cursor.fetchone()
                if book_row:
                    books.append(
                        {
                            "id": book_row["id"],
                            "title": book_row["title"],
                            "authors": book_row["authors"] or "Unknown",
                        }
                    )
            duplicates.append({"isbn": isbn, "count": row["count"], "books": books})

    return duplicates


def query_book_by_id(database_path: Path, book_id: int) -> dict[str, Any] | None:
    """
    Query complete book metadata by ID with joins to all related tables.

    Args:
        database_path: Path to Calibre metadata.db file
        book_id: Book ID to query

    Returns:
        Dictionary with all book metadata, or None if book not found
    """
    with connect_database(database_path, read_only=True) as conn:
        cursor = conn.execute(
            """
            SELECT
                b.id,
                b.title,
                b.pubdate as publication_date,
                b.timestamp as date_added,
                b.last_modified,
                b.path as storage_path,
                b.has_cover,
                b.series_index
            FROM books b
            WHERE b.id = ?
        """,
            (book_id,),
        )

        row = cursor.fetchone()
        if row is None:
            return None

        book: dict[str, Any] = {
            "id": row["id"],
            "title": row["title"],
            "isbn": "",
            "publication_date": format_date_simple(row["publication_date"]),
            "date_added": format_date_simple(row["date_added"]),
            "last_modified": format_date_simple(row["last_modified"]),
            "storage_path": row["storage_path"],
            "has_cover": bool(row["has_cover"]),
            "series_index": row["series_index"],
        }

        cursor = conn.execute(
            "SELECT val FROM identifiers WHERE book = ? AND type = 'isbn'", (book_id,)
        )
        identifier_row = cursor.fetchone()
        if identifier_row:
            book["isbn"] = identifier_row["val"]

        cursor = conn.execute(
            """
            SELECT a.name
            FROM authors a
            JOIN books_authors_link bal ON a.id = bal.author
            WHERE bal.book = ?
            ORDER BY bal.id
        """,
            (book_id,),
        )
        book["authors"] = [row["name"] for row in cursor.fetchall()]

        cursor = conn.execute(
            """
            SELECT t.name
            FROM tags t
            JOIN books_tags_link btl ON t.id = btl.tag
            WHERE btl.book = ?
            ORDER BY t.name
        """,
            (book_id,),
        )
        book["tags"] = [row["name"] for row in cursor.fetchall()]

        cursor = conn.execute(
            """
            SELECT p.name
            FROM publishers p
            JOIN books_publishers_link bpl ON p.id = bpl.publisher
            WHERE bpl.book = ?
        """,
            (book_id,),
        )
        row = cursor.fetchone()
        book["publisher"] = row["name"] if row else None

        cursor = conn.execute(
            """
            SELECT s.name
            FROM series s
            JOIN books_series_link bsl ON s.id = bsl.series
            WHERE bsl.book = ?
        """,
            (book_id,),
        )
        row = cursor.fetchone()
        book["series"] = row["name"] if row else None

        cursor = conn.execute(
            """
            SELECT r.rating
            FROM ratings r
            JOIN books_ratings_link brl ON r.id = brl.rating
            WHERE brl.book = ?
        """,
            (book_id,),
        )
        row = cursor.fetchone()
        book["rating"] = row["rating"] if row else None

        cursor = conn.execute("SELECT text FROM comments WHERE book = ?", (book_id,))
        row = cursor.fetchone()
        book["abstract"] = row["text"] if row else None

        cursor = conn.execute(
            """
            SELECT format, name, uncompressed_size
            FROM data
            WHERE book = ?
            ORDER BY format
        """,
            (book_id,),
        )
        files = []
        for row in cursor.fetchall():
            files.append(
                {
                    "format": row["format"],
                    "name": row["name"],
                    "size": row["uncompressed_size"],
                }
            )
        book["files"] = files
        book["cover_path"] = None

        return book


def list_all_tags(
    conn: sqlite3.Connection, sort_by: str = "name", min_books: int = 1
) -> list[dict[str, Any]]:
    """
    List all tags in the library with book counts.

    Args:
        conn: Database connection
        sort_by: Sort order - 'name' or 'count'
        min_books: Minimum number of books required

    Returns:
        List of tag dictionaries with 'name' and 'book_count' keys
    """
    order_clause = "book_count DESC, t.name ASC" if sort_by == "count" else "t.name ASC"

    query = f"""
        SELECT
            t.id,
            t.name,
            COUNT(btl.book) as book_count
        FROM tags t
        LEFT JOIN books_tags_link btl ON t.id = btl.tag
        GROUP BY t.id
        HAVING book_count >= ?
        ORDER BY {order_clause}
    """

    cursor = conn.execute(query, (min_books,))
    return [{"name": row["name"], "book_count": row["book_count"]} for row in cursor]


def query_books(
    conn: sqlite3.Connection, where_clause: str, limit: int | None = None
) -> list[dict[str, Any]]:
    """
    Query books with custom WHERE clause.

    Args:
        conn: Database connection
        where_clause: SQL WHERE condition
        limit: Maximum number of results

    Returns:
        List of book dictionaries with basic information
    """
    query = f"""
        SELECT DISTINCT
            b.id,
            b.title,
            b.timestamp as date_added,
            b.last_modified,
            b.series_index
        FROM books b
        LEFT JOIN books_authors_link bal ON b.id = bal.book
        LEFT JOIN authors a ON bal.author = a.id
        LEFT JOIN books_tags_link btl ON b.id = btl.book
        LEFT JOIN tags t ON btl.tag = t.id
        LEFT JOIN books_series_link bsl ON b.id = bsl.book
        LEFT JOIN series s ON bsl.series = s.id
        LEFT JOIN books_ratings_link brl ON b.id = brl.book
        LEFT JOIN ratings r ON brl.rating = r.id
        LEFT JOIN books_publishers_link bpl ON b.id = bpl.book
        LEFT JOIN publishers p ON bpl.publisher = p.id
        WHERE {where_clause}
        ORDER BY b.timestamp DESC
    """
    if limit is not None:
        query += " LIMIT ?"

    cursor = conn.execute(query, (limit,) if limit is not None else ())
    return _hydrate_book_rows(conn, cursor)


def list_books_query(
    conn: sqlite3.Connection,
    tag_name: str | None = None,
    author_name: str | None = None,
    series_name: str | None = None,
    title_name: str | None = None,
    limit: int | None = None,
) -> list[dict[str, Any]]:
    """
    Query books with optional filters.

    Args:
        conn: Database connection
        tag_name: Optional tag name to filter by
        author_name: Optional author name to filter by (partial match)
        series_name: Optional series name to filter by (partial match)
        title_name: Optional title to filter by (partial match, case-insensitive)
        limit: Maximum number of results

    Returns:
        List of book dictionaries with basic information
    """
    query = """
        SELECT DISTINCT
            b.id,
            b.title,
            b.timestamp as date_added,
            b.last_modified,
            b.series_index,
            b.has_cover,
            b.path
        FROM books b
        WHERE 1=1
    """
    params: list = []

    if title_name:
        query += " AND b.title LIKE ? COLLATE NOCASE"
        params.append(f"%{title_name}%")

    if tag_name:
        query += """
            AND b.id IN (
                SELECT btl.book
                FROM books_tags_link btl
                JOIN tags t ON btl.tag = t.id
                WHERE t.name = ?
            )
        """
        params.append(tag_name)

    if author_name:
        query += """
            AND b.id IN (
                SELECT bal.book
                FROM books_authors_link bal
                JOIN authors a ON bal.author = a.id
                WHERE a.name LIKE ?
            )
        """
        params.append(f"%{author_name}%")

    if series_name:
        query += """
            AND b.id IN (
                SELECT bsl.book
                FROM books_series_link bsl
                JOIN series s ON bsl.series = s.id
                WHERE s.name LIKE ?
            )
        """
        params.append(f"%{series_name}%")

    query += " ORDER BY b.timestamp DESC"
    if limit is not None:
        query += " LIMIT ?"
        params.append(limit)

    cursor = conn.execute(query, params)
    books = []

    for row in cursor:
        book_id = row["id"]
        authors, series, rating, tag_names, isbn = _fetch_book_related(conn, book_id, "")

        formats_dict: dict[str, list[str]] = {}
        for fmt_row in conn.execute(
            "SELECT format, name FROM data WHERE book = ? ORDER BY format", (book_id,)
        ):
            fmt = fmt_row["format"]
            formats_dict.setdefault(fmt, []).append(fmt_row["name"])

        books.append(
            {
                "id": book_id,
                "title": row["title"],
                "isbn": isbn,
                "authors": authors,
                "series": series,
                "series_index": row["series_index"] if series else None,
                "date_added": format_date_simple(row["date_added"]),
                "rating": rating,
                "tags": tag_names,
                "formats": formats_dict,
                "has_cover": bool(row["has_cover"]),
                "path": row["path"],
            }
        )

    return books


def get_book_file_path(
    database_path: Path,
    book_id: int,
    preferred_format: str | None = None,
) -> Path | None:
    """
    Get best file path for book.

    Priority: preferred_format > EPUB > PDF > first available

    Args:
        database_path: Path to Calibre metadata.db
        book_id: Book ID
        preferred_format: Preferred format (e.g., 'epub', 'pdf')

    Returns:
        Path to book file or None if no files found
    """
    from calibrectl.config import get_storage_path

    with connect_database(database_path, read_only=True) as conn:
        cursor = conn.execute(
            "SELECT b.path as storage_path FROM books b WHERE b.id = ?", (book_id,)
        )
        row = cursor.fetchone()
        if row is None:
            return None

        storage_path_rel = row["storage_path"]

        cursor = conn.execute(
            "SELECT format, name FROM data WHERE book = ? ORDER BY format", (book_id,)
        )
        files = {fr["format"].upper(): fr["name"] for fr in cursor.fetchall()}

        if not files:
            return None

        storage_root = get_storage_path(database_path)

        if preferred_format:
            fmt = preferred_format.upper()
            if fmt in files:
                return storage_root / storage_path_rel / f"{files[fmt]}.{fmt.lower()}"

        for fmt in ["EPUB", "PDF"]:
            if fmt in files:
                return storage_root / storage_path_rel / f"{files[fmt]}.{fmt.lower()}"

        first_format = next(iter(files))
        return storage_root / storage_path_rel / f"{files[first_format]}.{first_format.lower()}"


# ---------------------------------------------------------------------------
# Private helpers
# ---------------------------------------------------------------------------


def _fetch_book_related(conn, book_id, isbn_fallback):
    """Fetch authors, series, rating, tags and isbn for a book id."""
    authors_rows = conn.execute(
        """
        SELECT a.name FROM authors a
        JOIN books_authors_link bal ON a.id = bal.author
        WHERE bal.book = ? ORDER BY bal.id
    """,
        (book_id,),
    ).fetchall()
    authors = "; ".join(r["name"] for r in authors_rows) if authors_rows else ""

    series_row = conn.execute(
        """
        SELECT s.name FROM series s
        JOIN books_series_link bsl ON s.id = bsl.series
        WHERE bsl.book = ?
    """,
        (book_id,),
    ).fetchone()
    series = series_row["name"] if series_row else None

    rating_row = conn.execute(
        """
        SELECT r.rating FROM ratings r
        JOIN books_ratings_link brl ON r.id = brl.rating
        WHERE brl.book = ?
    """,
        (book_id,),
    ).fetchone()
    rating = rating_row["rating"] if rating_row else None

    tag_rows = conn.execute(
        """
        SELECT t.name FROM tags t
        JOIN books_tags_link btl ON t.id = btl.tag
        WHERE btl.book = ? ORDER BY t.name
    """,
        (book_id,),
    ).fetchall()
    tag_names = [r["name"] for r in tag_rows]

    isbn_row = conn.execute(
        "SELECT val FROM identifiers WHERE book = ? AND type = 'isbn'", (book_id,)
    ).fetchone()
    isbn = isbn_row["val"] if isbn_row else isbn_fallback

    return authors, series, rating, tag_names, isbn


def _hydrate_book_rows(conn, cursor) -> list[dict[str, Any]]:
    """Hydrate a cursor of basic book rows with related data."""
    books = []
    for row in cursor:
        book_id = row["id"]
        authors, series, rating, tag_names, isbn = _fetch_book_related(conn, book_id, "")

        path_row = conn.execute("SELECT path FROM books WHERE id = ?", (book_id,)).fetchone()
        book_path = path_row["path"] if path_row else None

        formats_dict: dict[str, list[str]] = {}
        for fmt_row in conn.execute(
            "SELECT format, name FROM data WHERE book = ? ORDER BY format", (book_id,)
        ):
            fmt = fmt_row["format"]
            formats_dict.setdefault(fmt, []).append(fmt_row["name"])

        books.append(
            {
                "id": book_id,
                "title": row["title"],
                "isbn": isbn,
                "authors": authors,
                "series": series,
                "series_index": row["series_index"] if series else None,
                "date_added": format_date_simple(row["date_added"]),
                "rating": rating,
                "tags": tag_names,
                "formats": formats_dict,
                "path": book_path,
            }
        )
    return books
