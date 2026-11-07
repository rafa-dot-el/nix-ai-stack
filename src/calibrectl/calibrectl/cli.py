"""
Click root group and all subcommands. Entry point: main().
"""

import shutil
import sqlite3
import sys
from pathlib import Path

import click
import yaml

from calibrectl import ops
from calibrectl.bibtex import export_bibtex_books
from calibrectl.config import get_database_path, get_storage_path
from calibrectl.covers import set_cover_for_book
from calibrectl.db import (
    REQUIRED_CALIBRE_TABLES,
    backup_database,
    check_required_tables,
    connect_database,
    detect_duplicate_files_by_hash,
    detect_isbn_duplicates,
    get_book_file_path,
    get_or_create_author,
    get_or_create_publisher,
    get_or_create_rating,
    get_or_create_series,
    get_or_create_tag,
    list_all_tags,
    list_books_query,
    query_book_by_id,
    query_books,
)
from calibrectl.formatters import (
    convert_date_to_iso,
    echo_info,
    format_error,
    format_json,
    format_success,
    format_yaml,
    set_quiet,
)
from calibrectl.isbn import find_isbn_for_book
from calibrectl.maintenance import apply_retag, find_duplicates_by_hash
from calibrectl.metadata import fetch_metadata_from_calibre
from calibrectl.paths import (
    generate_storage_path,
    resolve_cover_path,
    resolve_file_paths,
    resolve_format_paths,
)
from calibrectl.validation import classify_integrity_result, validate_environment_paths

# ============================================================================
# Root group
# ============================================================================


@click.group()
@click.version_option(version="0.1.0", prog_name="calibrectl")
@click.option(
    "--json-output",
    is_flag=True,
    default=False,
    help="Output in JSON format instead of YAML (default: YAML)",
)
@click.option(
    "--quiet",
    "-q",
    is_flag=True,
    default=False,
    help="Suppress informational messages",
)
@click.pass_context
def cli(ctx, json_output, quiet):
    """Calibre Database CLI Tool — manage Calibre ebook libraries from the command line."""
    set_quiet(quiet)
    ctx.ensure_object(dict)
    ctx.obj["json_output"] = json_output
    ctx.obj["quiet"] = quiet


# ============================================================================
# get
# ============================================================================


@cli.command()
@click.argument("book_id", type=int)
@click.option("--database", type=click.Path(), help="Path to metadata.db file")
@click.option("--storage", type=click.Path(file_okay=False), help="Path to storage directory")
@click.pass_context
def get(ctx, book_id: int, database: str | None, storage: str | None):
    """Retrieve complete information about a book."""
    database_path = Path(database) if database else get_database_path()
    storage_path = Path(storage) if storage else get_storage_path(database_path)

    if not database_path.exists():
        click.echo(format_error(f"Database not found: {database_path}"), err=True)
        sys.exit(1)

    try:
        book = query_book_by_id(database_path, book_id)
        if book is None:
            click.echo(format_error(f"Book {book_id} not found"), err=True)
            sys.exit(1)

        book["files"] = resolve_file_paths(storage_path, book["storage_path"], book["files"])
        book["cover_path"] = resolve_cover_path(
            storage_path, book["storage_path"], book["has_cover"]
        )

        json_output = ctx.obj.get("json_output", False)
        click.echo(format_json(book) if json_output else format_yaml(book))

    except click.ClickException:
        raise
    except Exception as exc:
        click.echo(format_error(f"Error retrieving book: {exc}"), err=True)
        sys.exit(1)


# ============================================================================
# extract-text
# ============================================================================


@cli.command(name="extract-text")
@click.argument("book_id", type=int)
@click.option("--start-page", type=int, default=1, help="Starting page (default: 1)")
@click.option("--end-page", type=int, default=10, help="Ending page (default: 10)")
@click.option("--format", "preferred_format", type=str, help="Preferred format (epub/pdf)")
@click.option("--database", type=click.Path(), help="Path to metadata.db file")
@click.pass_context
def extract_text(
    ctx,
    book_id: int,
    start_page: int,
    end_page: int,
    preferred_format: str | None,
    database: str | None,
):
    """Extract text from book by ID."""
    from html.parser import HTMLParser

    from ebooklib import epub
    from pypdf import PdfReader

    database_path = Path(database) if database else get_database_path()
    if not database_path.exists():
        click.echo(format_error(f"Database not found: {database_path}"), err=True)
        sys.exit(1)

    try:
        echo_info(f"Resolving file path for book {book_id}...")
        file_path = get_book_file_path(database_path, book_id, preferred_format)

        if file_path is None:
            click.echo(format_error(f"Book {book_id} not found or has no files"), err=True)
            sys.exit(1)
        if not file_path.exists():
            click.echo(format_error(f"File not found: {file_path}"), err=True)
            sys.exit(1)

        echo_info(f"Using file: {file_path.name}")
        suffix = file_path.suffix.lower()

        if suffix == ".pdf":
            echo_info(f"Extracting pages {start_page}-{end_page} from PDF...")
            reader = PdfReader(str(file_path))
            total_pages = len(reader.pages)

            if start_page < 1:
                click.echo(format_error(f"Start page must be >= 1, got {start_page}"), err=True)
                sys.exit(1)
            if end_page > total_pages:
                click.echo(
                    format_error(f"End page {end_page} exceeds total pages {total_pages}"),
                    err=True,
                )
                sys.exit(1)
            if start_page > end_page:
                click.echo(
                    format_error(f"Start page {start_page} cannot exceed end page {end_page}"),
                    err=True,
                )
                sys.exit(1)

            text_parts = []
            for page_num in range(start_page - 1, end_page):
                text_parts.append(reader.pages[page_num].extract_text())
            output_text = "\n".join(text_parts)

        elif suffix == ".epub":
            echo_info(f"Extracting chapters {start_page}-{end_page} from EPUB...")
            book = epub.read_epub(str(file_path))

            spine_items = [
                book.get_item_with_id(item_id)
                for item_id, _ in book.spine
                if book.get_item_with_id(item_id)
            ]
            total_chapters = len(spine_items)

            if start_page < 1:
                click.echo(format_error(f"Start page must be >= 1, got {start_page}"), err=True)
                sys.exit(1)
            if end_page > total_chapters:
                click.echo(
                    format_error(f"End page {end_page} exceeds total chapters {total_chapters}"),
                    err=True,
                )
                sys.exit(1)
            if start_page > end_page:
                click.echo(
                    format_error(f"Start page {start_page} cannot exceed end page {end_page}"),
                    err=True,
                )
                sys.exit(1)

            class _SimpleHTMLParser(HTMLParser):
                def __init__(self):
                    super().__init__()
                    self.text_parts = []

                def handle_data(self, data):
                    self.text_parts.append(data)

                def get_text(self):
                    return "".join(self.text_parts)

            text_parts = []
            for chapter_num in range(start_page - 1, end_page):
                item = spine_items[chapter_num]
                content = item.get_content().decode("utf-8", errors="ignore")
                parser = _SimpleHTMLParser()
                parser.feed(content)
                text_parts.append(parser.get_text().strip())

            output_text = "\n\n".join(text_parts)

        else:
            click.echo(
                format_error(f"Unsupported format: {suffix}. Supported: PDF, EPUB"), err=True
            )
            sys.exit(1)

        click.echo(output_text)

    except click.ClickException:
        raise
    except Exception as exc:
        click.echo(format_error(f"Error extracting text: {exc}"), err=True)
        sys.exit(1)


# ============================================================================
# export-bibtex
# ============================================================================


@cli.command(name="export-bibtex")
@click.argument("book_id", type=int, required=False)
@click.option("--tag", type=str, help="Export all books with tag")
@click.option("--all", "export_all", is_flag=True, help="Export entire library")
@click.option("--output", type=click.Path(), help="Output file (default: stdout)")
@click.option("--database", type=click.Path(), help="Path to metadata.db file")
@click.pass_context
def export_bibtex(
    ctx,
    book_id: int | None,
    tag: str | None,
    export_all: bool,
    output: str | None,
    database: str | None,
):
    """Export book(s) to BibTeX format."""
    if not book_id and not tag and not export_all:
        click.echo(format_error("Specify BOOK_ID, --tag, or --all"), err=True)
        sys.exit(1)

    database_path = Path(database) if database else get_database_path()
    if not database_path.exists():
        click.echo(format_error(f"Database not found: {database_path}"), err=True)
        sys.exit(1)

    try:
        entries = export_bibtex_books(
            database_path, book_id=book_id, tag=tag, export_all=export_all
        )

        if not entries:
            echo_info("No books to export")
            sys.exit(0)

        output_str = "\n\n".join(entries)

        if output:
            Path(output).write_text(output_str)
            click.echo(format_success(f"Exported {len(entries)} book(s) to: {output}"), err=True)
        else:
            click.echo(output_str)

    except click.ClickException:
        raise
    except Exception as exc:
        click.echo(format_error(f"Error exporting BibTeX: {exc}"), err=True)
        sys.exit(1)


# ============================================================================
# list
# ============================================================================


@cli.command(name="list")
@click.option("--tag", type=str, help="Filter by tag name")
@click.option("--author", type=str, help="Filter by author name (partial match)")
@click.option("--series", type=str, help="Filter by series name (partial match)")
@click.option("--title", type=str, help="Filter by title (partial match, case-insensitive)")
@click.option("--limit", type=int, default=None, help="Maximum results (default: no limit)")
@click.option("--database", type=click.Path(), help="Path to metadata.db file")
@click.pass_context
def list_books(
    ctx,
    tag: str | None,
    author: str | None,
    series: str | None,
    title: str | None,
    limit: int | None,
    database: str | None,
):
    """List books with optional filters."""
    database_path = Path(database) if database else get_database_path()
    json_output = ctx.obj.get("json_output", False)

    if not database_path.exists():
        click.echo(format_error(f"Database not found: {database_path}"), err=True)
        sys.exit(1)

    try:
        storage_path = get_storage_path(database_path)

        with connect_database(database_path, read_only=True) as conn:
            books = list_books_query(conn, tag, author, series, title, limit)

            if not books:
                echo_info("No books found")
                if json_output:
                    click.echo(format_json([]))
                sys.exit(0)

            for book in books:
                book["cover_path"] = resolve_cover_path(
                    storage_path, book["path"], book["has_cover"]
                )
                if book.get("formats"):
                    book["formats"] = resolve_format_paths(
                        storage_path, book["path"], book["formats"]
                    )

            if json_output:
                click.echo(format_json({"books": books}))
            else:
                click.echo(format_yaml({"books": books}))

    except click.ClickException:
        raise
    except Exception as exc:
        click.echo(format_error(f"Error listing books: {exc}"), err=True)
        sys.exit(1)


# ============================================================================
# tags
# ============================================================================


@cli.command()
@click.option(
    "--sort-by",
    type=click.Choice(["name", "count"]),
    default="name",
    help="Sort by tag name (default) or book count",
)
@click.option(
    "--min-books",
    type=int,
    default=1,
    help="Only show tags with at least N books (default: 1)",
)
@click.option("--database", type=click.Path(), help="Path to metadata.db file")
@click.pass_context
def tags(ctx, sort_by: str, min_books: int, database: str | None):
    """List all tags in the library with book counts."""
    database_path = Path(database) if database else get_database_path()
    json_output = ctx.obj.get("json_output", False)

    if not database_path.exists():
        click.echo(format_error(f"Database not found: {database_path}"), err=True)
        sys.exit(1)

    try:
        with connect_database(database_path, read_only=True) as conn:
            tags_list = list_all_tags(conn, sort_by=sort_by, min_books=min_books)

            if not tags_list:
                echo_info("No tags found")
                sys.exit(0)

            total_books = conn.execute(
                "SELECT COUNT(DISTINCT book) FROM books_tags_link"
            ).fetchone()[0]
            echo_info(f"Found {len(tags_list)} tags across {total_books} books")

            if json_output:
                click.echo(format_json({"tags": tags_list}))
            else:
                click.echo(format_yaml({"tags": tags_list}))

    except click.ClickException:
        raise
    except Exception as exc:
        click.echo(format_error(f"Error listing tags: {exc}"), err=True)
        sys.exit(1)


# ============================================================================
# query
# ============================================================================


@cli.command()
@click.argument("where_clause", type=str)
@click.option("--limit", type=int, default=None, help="Maximum results (default: no limit)")
@click.option("--database", type=click.Path(), help="Path to metadata.db file")
@click.pass_context
def query(ctx, where_clause: str, limit: int | None, database: str | None):
    """Query books with SQL WHERE clause."""
    database_path = Path(database) if database else get_database_path()
    json_output = ctx.obj.get("json_output", False)

    if not database_path.exists():
        click.echo(format_error(f"Database not found: {database_path}"), err=True)
        sys.exit(1)

    try:
        storage_path = get_storage_path(database_path)

        with connect_database(database_path, read_only=True) as conn:
            books = query_books(conn, where_clause, limit)

            if not books:
                echo_info("No books found")
                sys.exit(0)

            for book in books:
                if book.get("path") and book.get("formats"):
                    book["formats"] = resolve_format_paths(
                        storage_path, book["path"], book["formats"]
                    )

            if json_output:
                click.echo(format_json({"books": books}))
            else:
                click.echo(format_yaml({"books": books}))

    except sqlite3.OperationalError as exc:
        click.echo(format_error(f"SQL error: {exc}"), err=True)
        echo_info("Check your WHERE clause syntax")
        sys.exit(1)
    except click.ClickException:
        raise
    except Exception as exc:
        click.echo(format_error(f"Error querying books: {exc}"), err=True)
        sys.exit(1)


# ============================================================================
# doctor
# ============================================================================


@cli.command()
@click.option("--database", type=click.Path(), help="Path to metadata.db file")
@click.option("--storage", type=click.Path(file_okay=False), help="Path to storage directory")
@click.pass_context
def doctor(ctx, database: str | None, storage: str | None):
    """Run comprehensive diagnostics on Calibre database."""
    database_path = Path(database) if database else get_database_path()
    storage_path = Path(storage) if storage else get_storage_path(database_path)
    json_output = ctx.obj.get("json_output", False)

    report: dict = {
        "environment": [],
        "integrity": {},
        "schema": {},
        "overview": {},
        "issues": {},
        "warnings": {},
        "recommendations": [],
    }
    has_errors = False

    echo_info("Checking environment paths...")
    env_checks = validate_environment_paths(database_path, storage_path)
    report["environment"] = env_checks

    for check in env_checks:
        if check["status"] == "error":
            has_errors = True
            click.echo(format_error(check["message"]), err=True)
        elif check["status"] == "warning":
            echo_info(check["message"])

    if not database_path.exists():
        click.echo(format_json(report) if json_output else format_yaml(report))
        click.echo(format_error("Cannot continue: database file not found"), err=True)
        sys.exit(1)

    echo_info("Running Calibre library diagnostics...")
    click.echo("", err=True)

    try:
        with connect_database(database_path, read_only=True) as conn:
            # Integrity check
            echo_info("Running SQLite integrity check...")
            integrity_result = conn.execute("PRAGMA integrity_check").fetchone()[0]
            integrity_status = classify_integrity_result(integrity_result)
            report["integrity"] = {
                "status": integrity_status,
                "detail": integrity_result if integrity_status != "ok" else "ok",
            }
            if integrity_status != "ok":
                has_errors = True
                click.echo(
                    format_error(f"SQLite integrity check failed: {integrity_result}"), err=True
                )

            # Schema validation
            echo_info("Validating database schema...")
            existing_tables = [
                row[0]
                for row in conn.execute(
                    "SELECT name FROM sqlite_master WHERE type='table' OR type='view'"
                ).fetchall()
            ]
            missing_tables = check_required_tables(existing_tables, REQUIRED_CALIBRE_TABLES)
            report["schema"] = {
                "status": "ok" if not missing_tables else "error",
                "tables_found": len(existing_tables),
                "tables_required": len(REQUIRED_CALIBRE_TABLES),
                "missing_tables": missing_tables,
            }
            if missing_tables:
                has_errors = True
                click.echo(
                    format_error(f"Missing required tables: {', '.join(missing_tables)}"),
                    err=True,
                )

            # Overview statistics
            total_books = conn.execute("SELECT COUNT(*) FROM books").fetchone()[0]
            report["overview"]["total_books"] = total_books
            report["overview"]["total_tags"] = conn.execute(
                "SELECT COUNT(DISTINCT tag) FROM books_tags_link"
            ).fetchone()[0]
            report["overview"]["total_series"] = conn.execute(
                "SELECT COUNT(*) FROM series"
            ).fetchone()[0]

            # Duplicate detection
            echo_info("Computing file hashes for duplicate detection...")
            hash_duplicates = detect_duplicate_files_by_hash(conn, storage_path)
            if hash_duplicates:
                report["issues"]["hash_duplicates"] = {
                    "count": len(hash_duplicates),
                    "note": "Byte-for-byte identical files (highest confidence)",
                    "groups": list(hash_duplicates.values())[:10],
                }

            echo_info("Checking for ISBN duplicates...")
            isbn_duplicates = detect_isbn_duplicates(conn)
            if isbn_duplicates:
                report["issues"]["isbn_duplicates"] = {
                    "count": len(isbn_duplicates),
                    "note": "Books with matching ISBN identifiers (high confidence)",
                    "groups": isbn_duplicates[:10],
                }

            echo_info("Checking for title duplicates...")
            dup_cursor = conn.execute("""
                SELECT
                    LOWER(TRIM(title)) as norm_title,
                    GROUP_CONCAT(id) as book_ids,
                    COUNT(*) as count,
                    title as original_title
                FROM books
                GROUP BY norm_title
                HAVING count > 1
                ORDER BY count DESC
            """)
            duplicate_groups = dup_cursor.fetchall()
            if duplicate_groups:
                duplicates_list = []
                total_duplicates = 0
                for group in duplicate_groups:
                    book_ids = [int(bid) for bid in group["book_ids"].split(",")]
                    total_duplicates += len(book_ids) - 1
                    formats_info = []
                    for bid in book_ids:
                        fmt_cursor = conn.execute(
                            "SELECT format, uncompressed_size FROM data WHERE book = ?", (bid,)
                        )
                        formats = [
                            {
                                "format": r["format"],
                                "size_mb": round(r["uncompressed_size"] / 1024 / 1024, 2)
                                if r["uncompressed_size"]
                                else 0,
                            }
                            for r in fmt_cursor.fetchall()
                        ]
                        formats_info.append({"id": bid, "formats": formats})
                    duplicates_list.append(
                        {
                            "title": group["original_title"],
                            "count": group["count"],
                            "book_ids": book_ids,
                            "formats": formats_info,
                        }
                    )
                report["issues"]["title_duplicates"] = {
                    "count": len(duplicate_groups),
                    "total_duplicate_books": total_duplicates,
                    "note": "Books with matching titles (needs review)",
                    "groups": duplicates_list[:10],
                }

            # Empty tags / series
            echo_info("Checking for empty tags...")
            empty_tags = [
                {"id": r["id"], "name": r["name"]}
                for r in conn.execute("""
                    SELECT t.id, t.name FROM tags t
                    LEFT JOIN books_tags_link btl ON t.id = btl.tag
                    WHERE btl.tag IS NULL
                """).fetchall()
            ]
            if empty_tags:
                report["issues"]["empty_tags"] = {"count": len(empty_tags), "tags": empty_tags}

            echo_info("Checking for empty series...")
            empty_series = [
                {"id": r["id"], "name": r["name"]}
                for r in conn.execute("""
                    SELECT s.id, s.name FROM series s
                    LEFT JOIN books_series_link bsl ON s.id = bsl.series
                    WHERE bsl.series IS NULL
                """).fetchall()
            ]
            if empty_series:
                report["issues"]["empty_series"] = {
                    "count": len(empty_series),
                    "series": empty_series,
                }

            # Metadata quality
            echo_info("Checking metadata quality...")
            no_authors = conn.execute("""
                SELECT COUNT(*) FROM books b
                WHERE NOT EXISTS (SELECT 1 FROM books_authors_link bal WHERE bal.book = b.id)
            """).fetchone()[0]
            if no_authors > 0:
                report["warnings"]["books_without_authors"] = no_authors

            no_tags = conn.execute("""
                SELECT COUNT(*) FROM books b
                WHERE NOT EXISTS (SELECT 1 FROM books_tags_link btl WHERE btl.book = b.id)
            """).fetchone()[0]
            if no_tags > 0:
                report["warnings"]["books_without_tags"] = no_tags

            no_ratings = conn.execute("""
                SELECT COUNT(*) FROM books b
                WHERE NOT EXISTS (SELECT 1 FROM books_ratings_link brl WHERE brl.book = b.id)
            """).fetchone()[0]
            if no_ratings > 0:
                report["warnings"]["books_without_ratings"] = {
                    "count": no_ratings,
                    "percentage": round(no_ratings / total_books * 100, 1)
                    if total_books > 0
                    else 0,
                }

            no_covers = conn.execute("SELECT COUNT(*) FROM books WHERE has_cover = 0").fetchone()[0]
            if no_covers > 0:
                report["warnings"]["books_without_covers"] = no_covers

            # Orphaned data
            echo_info("Checking for orphaned data...")
            orphaned_data = conn.execute("""
                SELECT COUNT(*) FROM data d
                WHERE NOT EXISTS (SELECT 1 FROM books b WHERE b.id = d.book)
            """).fetchone()[0]
            if orphaned_data > 0:
                report["issues"]["orphaned_data_entries"] = orphaned_data

            # Missing files
            echo_info("Checking file system consistency...")
            missing_files = []
            total_files_checked = 0
            for row in conn.execute("""
                SELECT d.book, d.format, d.name, b.path
                FROM data d JOIN books b ON d.book = b.id
                ORDER BY d.book, d.format
            """).fetchall():
                file_path = storage_path / row["path"] / f"{row['name']}.{row['format'].lower()}"
                total_files_checked += 1
                if not file_path.exists():
                    missing_files.append(
                        {
                            "book_id": row["book"],
                            "format": row["format"],
                            "expected_path": str(file_path),
                            "book_path": row["path"],
                        }
                    )
            if missing_files:
                report["issues"]["missing_files"] = {
                    "count": len(missing_files),
                    "total_checked": total_files_checked,
                    "files": missing_files[:20],
                }

            # Recommendations
            if "hash_duplicates" in report["issues"]:
                report["recommendations"].append(
                    {
                        "priority": "high",
                        "category": "duplicates",
                        "issue": f"Found {report['issues']['hash_duplicates']['count']} sets of byte-for-byte identical files",
                        "action": "These are confirmed duplicates - safe to remove to save storage space",
                    }
                )
            if "isbn_duplicates" in report["issues"]:
                report["recommendations"].append(
                    {
                        "priority": "high",
                        "category": "duplicates",
                        "issue": f"Found {report['issues']['isbn_duplicates']['count']} books with duplicate ISBN identifiers",
                        "action": "Review these books - same ISBN means same edition, likely true duplicates",
                    }
                )
            if "title_duplicates" in report["issues"]:
                report["recommendations"].append(
                    {
                        "priority": "medium",
                        "category": "duplicates",
                        "issue": f"Found {report['issues']['title_duplicates']['count']} duplicate title sets",
                        "action": "Review these books - may be different editions or formats",
                    }
                )
            if "empty_tags" in report["issues"]:
                report["recommendations"].append(
                    {
                        "priority": "low",
                        "category": "cleanup",
                        "issue": f"{report['issues']['empty_tags']['count']} unused tags",
                        "action": "Remove empty tags to clean up tag list",
                    }
                )
            if no_tags > 0:
                report["recommendations"].append(
                    {
                        "priority": "medium",
                        "category": "metadata",
                        "issue": f"{no_tags} books have no tags",
                        "action": "Add tags to improve organization and searchability",
                    }
                )
            if "orphaned_data_entries" in report["issues"]:
                report["recommendations"].append(
                    {
                        "priority": "high",
                        "category": "database",
                        "issue": f"{orphaned_data} orphaned data entries",
                        "action": "Clean up database to remove orphaned file references",
                    }
                )
            if "missing_files" in report["issues"]:
                report["recommendations"].append(
                    {
                        "priority": "high",
                        "category": "filesystem",
                        "issue": f"{report['issues']['missing_files']['count']} files referenced in database but missing from filesystem",
                        "action": "Check backups or remove broken references",
                    }
                )

            click.echo(format_json(report) if json_output else format_yaml(report))

            click.echo("", err=True)
            if has_errors or report["issues"] or report["warnings"]:
                click.echo(format_error("Issues detected in database"), err=True)
                echo_info(
                    f"Found {len(report.get('issues', {}))} issue categories"
                    f" and {len(report.get('warnings', {}))} warnings"
                )
                sys.exit(1)
            else:
                click.echo(format_success("Database is healthy - no issues found!"), err=True)

    except Exception as exc:
        click.echo(format_error(f"Error running diagnostics: {exc}"), err=True)
        sys.exit(1)


# ============================================================================
# create
# ============================================================================


@cli.command()
@click.option("--title", required=True, help="Book title")
@click.option("--authors", help="Comma-separated author names")
@click.option("--isbn", help="ISBN identifier")
@click.option("--publisher", help="Publisher name")
@click.option("--pubdate", help="Publication date (YYYY-MM-DD)")
@click.option("--abstract", help="Book description")
@click.option("--tags", "tags_str", help="Comma-separated tags")
@click.option("--series", help="Series name")
@click.option("--series-index", type=float, default=1.0, help="Position in series")
@click.option("--rating", type=int, help="Rating (0-10)")
@click.option("--database", type=click.Path(exists=True), help="Path to metadata.db file")
@click.option(
    "--storage",
    type=click.Path(exists=True, file_okay=False),
    help="Path to storage directory",
)
@click.option("--json-output", is_flag=True, help="Output in JSON format")
@click.option("--no-backup", is_flag=True, help="Skip database backup (NOT RECOMMENDED)")
def create(
    title: str,
    authors: str | None,
    isbn: str | None,
    publisher: str | None,
    pubdate: str | None,
    abstract: str | None,
    tags_str: str | None,
    series: str | None,
    series_index: float,
    rating: int | None,
    database: str | None,
    storage: str | None,
    json_output: bool,
    no_backup: bool,
):
    """Create a new book entry with metadata."""
    import uuid as uuid_module

    database_path = Path(database) if database else get_database_path()
    if not database_path.exists():
        click.echo(format_error(f"Database not found: {database_path}"), err=True)
        sys.exit(1)

    storage_path = Path(storage) if storage else get_storage_path(database_path)

    if not no_backup:
        backup_path = backup_database(database_path)
        echo_info(f"Database backed up to {backup_path}")

    try:
        echo_info("Creating new book entry...")

        with connect_database(database_path, read_only=False) as conn:
            conn.execute("BEGIN TRANSACTION")
            try:
                iso_pubdate = convert_date_to_iso(pubdate) if pubdate else None
                now = ops.utc_now()
                author_list = [a.strip() for a in authors.split(",")] if authors else ["Unknown"]
                first_author = author_list[0]
                book_uuid = str(uuid_module.uuid4())
                sort_title = title.lower()

                cursor = conn.execute(
                    """
                    INSERT INTO books (title, sort, pubdate, timestamp, last_modified,
                        series_index, path, has_cover, uuid)
                    VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?)
                    """,
                    (
                        title,
                        sort_title,
                        iso_pubdate or "",
                        now,
                        now,
                        series_index if series else 1.0,
                        "",
                        book_uuid,
                    ),
                )
                book_id = cursor.lastrowid
                echo_info(f"Book ID: {book_id}")

                book_path = generate_storage_path(first_author, title, book_id)
                conn.execute("UPDATE books SET path = ? WHERE id = ?", (book_path, book_id))

                full_storage_path = storage_path / book_path
                full_storage_path.mkdir(parents=True, exist_ok=True)
                echo_info(f"Created directory: {full_storage_path}")

                for author_name in author_list:
                    author_id = get_or_create_author(conn, author_name)
                    conn.execute(
                        "INSERT INTO books_authors_link (book, author) VALUES (?, ?)",
                        (book_id, author_id),
                    )
                echo_info(f"Linked {len(author_list)} author(s)")

                if isbn:
                    conn.execute(
                        "INSERT INTO identifiers (book, type, val) VALUES (?, 'isbn', ?)",
                        (book_id, isbn),
                    )

                if tags_str:
                    tag_list = [t.strip() for t in tags_str.split(",")]
                    for tag_name in tag_list:
                        tag_id = get_or_create_tag(conn, tag_name)
                        conn.execute(
                            "INSERT INTO books_tags_link (book, tag) VALUES (?, ?)",
                            (book_id, tag_id),
                        )
                    echo_info(f"Linked {len(tag_list)} tag(s)")

                if publisher:
                    publisher_id = get_or_create_publisher(conn, publisher)
                    conn.execute(
                        "INSERT INTO books_publishers_link (book, publisher) VALUES (?, ?)",
                        (book_id, publisher_id),
                    )

                if series:
                    series_id = get_or_create_series(conn, series)
                    conn.execute(
                        "INSERT INTO books_series_link (book, series) VALUES (?, ?)",
                        (book_id, series_id),
                    )

                if rating is not None:
                    rating_id = get_or_create_rating(conn, rating)
                    conn.execute(
                        "INSERT INTO books_ratings_link (book, rating) VALUES (?, ?)",
                        (book_id, rating_id),
                    )

                if abstract:
                    conn.execute(
                        "INSERT INTO comments (book, text) VALUES (?, ?)", (book_id, abstract)
                    )

                conn.commit()
                click.echo(format_success("Book created successfully!"), err=True)

                result = {
                    "status": "success",
                    "book_id": book_id,
                    "title": title,
                    "storage_path": str(full_storage_path),
                    "message": "Book created successfully",
                }
                click.echo(format_json(result) if json_output else format_yaml(result))

            except Exception:
                conn.rollback()
                if "full_storage_path" in dir() and full_storage_path.exists():
                    shutil.rmtree(full_storage_path, ignore_errors=True)
                raise

    except Exception as exc:
        click.echo(format_error(f"Error creating book: {exc}"), err=True)
        sys.exit(1)


# ============================================================================
# update
# ============================================================================


@cli.command()
@click.argument("book_id", type=int)
@click.option("--from-yaml", type=click.Path(exists=True), help="Load metadata from YAML file")
@click.option("--title", help="Update book title")
@click.option("--authors", help="Replace all authors")
@click.option("--add-author", help="Add author without removing existing")
@click.option("--remove-author", help="Remove specific author")
@click.option("--isbn", help="Update ISBN")
@click.option("--publisher", help="Update publisher")
@click.option("--pubdate", help="Update publication date")
@click.option("--abstract", help="Update abstract")
@click.option("--tags", "tags_str", help="Replace all tags")
@click.option("--add-tag", help="Add tag without removing existing")
@click.option("--remove-tag", help="Remove specific tag")
@click.option("--series", help="Update series")
@click.option("--series-index", type=float, help="Update series index")
@click.option("--rating", type=int, help="Update rating (0-10)")
@click.option("--cover", type=click.Path(exists=True), help="Replace cover image")
@click.option("--remove-cover", is_flag=True, help="Remove cover image")
@click.option("--database", type=click.Path(exists=True), help="Path to metadata.db file")
@click.option(
    "--storage",
    type=click.Path(exists=True, file_okay=False),
    help="Path to storage directory",
)
@click.option("--json-output", is_flag=True, help="Output in JSON format")
@click.option("--no-backup", is_flag=True, help="Skip database backup (NOT RECOMMENDED)")
def update(book_id: int, **kwargs):
    """Update metadata for an existing book."""
    from_yaml = kwargs.get("from_yaml")
    if from_yaml:
        try:
            with open(from_yaml) as f:
                yaml_data = yaml.safe_load(f)
            if "metadata" in yaml_data:
                metadata = yaml_data["metadata"]
                for field, kwarg in [
                    ("title", "title"),
                    ("isbn", "isbn"),
                    ("publisher", "publisher"),
                    ("pubdate", "pubdate"),
                    ("abstract", "abstract"),
                    ("series", "series"),
                    ("series_index", "series_index"),
                    ("rating", "rating"),
                ]:
                    if field in metadata and not kwargs.get(kwarg):
                        kwargs[kwarg] = metadata[field]
                if "authors" in metadata and not kwargs.get("authors"):
                    kwargs["authors"] = ", ".join(metadata["authors"])
                if "tags" in metadata and not kwargs.get("tags_str"):
                    kwargs["tags_str"] = ", ".join(metadata["tags"])
        except Exception as exc:
            click.echo(format_error(f"Failed to load YAML file: {exc}"), err=True)
            sys.exit(1)

    fields = {
        "title": kwargs.get("title"),
        "authors": kwargs.get("authors"),
        "add_author": kwargs.get("add_author"),
        "remove_author": kwargs.get("remove_author"),
        "isbn": kwargs.get("isbn"),
        "publisher": kwargs.get("publisher"),
        "pubdate": kwargs.get("pubdate"),
        "abstract": kwargs.get("abstract"),
        "tags": kwargs.get("tags_str"),
        "add_tag": kwargs.get("add_tag"),
        "remove_tag": kwargs.get("remove_tag"),
        "series": kwargs.get("series"),
        "series_index": kwargs.get("series_index"),
        "rating": kwargs.get("rating"),
    }
    database = kwargs.get("database")
    database_path = Path(database) if database else get_database_path()
    echo_info(f"Updating book {book_id}...")
    try:
        result = ops.update_metadata(database_path, book_id, fields, backup=not kwargs.get("no_backup", False))
    except ops.Invalid as exc:
        click.echo(format_error(f"{exc}. Use --help to see available options."), err=True)
        sys.exit(1)
    except ops.NotFound as exc:
        raise click.ClickException(str(exc)) from exc
    except click.ClickException:
        raise
    except Exception as exc:
        click.echo(format_error(f"Error updating book: {exc}"), err=True)
        sys.exit(1)
    if result.get("backup"):
        echo_info(f"Database backed up to {result['backup']}")
    click.echo(format_success("Book updated successfully!"), err=True)
    click.echo(format_json(result) if kwargs.get("json_output", False) else format_yaml(result))


# ============================================================================
# add-file
# ============================================================================


@cli.command(name="add-file")
@click.argument("book_id", type=int)
@click.argument("file_path", type=click.Path(exists=True))
@click.option("--format", "file_format", help="Override format detection")
@click.option("--database", type=click.Path(exists=True), help="Path to metadata.db file")
@click.option(
    "--storage",
    type=click.Path(exists=True, file_okay=False),
    help="Path to storage directory",
)
@click.option("--json-output", is_flag=True, help="Output in JSON format")
@click.option("--no-backup", is_flag=True, help="Skip database backup (NOT RECOMMENDED)")
def add_file(
    book_id: int,
    file_path: str,
    file_format: str | None,
    database: str | None,
    storage: str | None,
    json_output: bool,
    no_backup: bool,
):
    """Attach a file to an existing book."""
    database_path = Path(database) if database else get_database_path()
    storage_path = Path(storage) if storage else get_storage_path(database_path)
    try:
        result = ops.attach_file(database_path, storage_path, book_id, Path(file_path), file_format, backup=not no_backup)
    except (ops.Invalid, ops.NotFound) as exc:
        click.echo(format_error(str(exc)), err=True)
        sys.exit(1)
    except click.ClickException:
        raise
    except Exception as exc:
        click.echo(format_error(f"Error adding file: {exc}"), err=True)
        sys.exit(1)
    echo_info(f"Copied file to: {result['file_path']}")
    click.echo(format_success("File added successfully!"), err=True)
    click.echo(format_json(result) if json_output else format_yaml(result))


# ============================================================================
# delete
# ============================================================================


@cli.command()
@click.argument("book_id", type=int)
@click.option("--files", is_flag=True, help="Also delete physical files")
@click.option("--force", is_flag=True, help="Skip confirmation prompt")
@click.option("--database", type=click.Path(exists=True), help="Path to metadata.db file")
@click.option(
    "--storage",
    type=click.Path(exists=True, file_okay=False),
    help="Path to storage directory",
)
@click.option("--json-output", is_flag=True, help="Output in JSON format")
@click.option("--no-backup", is_flag=True, help="Skip database backup (NOT RECOMMENDED)")
def delete(
    book_id: int,
    files: bool,
    force: bool,
    database: str | None,
    storage: str | None,
    json_output: bool,
    no_backup: bool,
):
    """Delete a book entry from the database."""
    database_path = Path(database) if database else get_database_path()
    if not database_path.exists():
        click.echo(format_error(f"Database not found: {database_path}"), err=True)
        sys.exit(1)

    storage_path = Path(storage) if storage else get_storage_path(database_path)

    try:
        book = query_book_by_id(database_path, book_id)
        if not book:
            raise click.ClickException(f"Book ID {book_id} not found")

        echo_info("Book to delete:")
        click.echo(f"  ID: {book_id}", err=True)
        click.echo(f"  Title: {book['title']}", err=True)
        click.echo(f"  Authors: {', '.join(book['authors'])}", err=True)

        if not force:
            confirm = input("\nAre you sure you want to delete this book? [y/N]: ")
            if confirm.lower() != "y":
                echo_info("Deletion cancelled")
                sys.exit(0)

        if not no_backup:
            backup_path = backup_database(database_path)
            echo_info(f"Database backed up to {backup_path}")

        echo_info(f"Deleting book {book_id}...")

        with connect_database(database_path, read_only=False) as conn:
            conn.execute("BEGIN TRANSACTION")
            try:
                author_links = conn.execute(
                    "SELECT COUNT(*) FROM books_authors_link WHERE book = ?", (book_id,)
                ).fetchone()[0]
                tag_links = conn.execute(
                    "SELECT COUNT(*) FROM books_tags_link WHERE book = ?", (book_id,)
                ).fetchone()[0]
                file_count = conn.execute(
                    "SELECT COUNT(*) FROM data WHERE book = ?", (book_id,)
                ).fetchone()[0]

                conn.execute("DELETE FROM books WHERE id = ?", (book_id,))
                conn.commit()
                click.echo(format_success("Book deleted from database!"), err=True)

                deleted_files = []
                if files:
                    book_dir = storage_path / book["storage_path"]
                    if book_dir.exists():
                        try:
                            deleted_files = [f.name for f in book_dir.iterdir() if f.is_file()]
                            shutil.rmtree(book_dir)
                            click.echo(format_success(f"Deleted directory: {book_dir}"), err=True)
                        except Exception as exc:
                            click.echo(
                                format_error(f"Warning: Could not delete directory: {exc}"),
                                err=True,
                            )

                result = {
                    "status": "success",
                    "book_id": book_id,
                    "deleted_records": {
                        "author_links": author_links,
                        "tag_links": tag_links,
                        "file_records": file_count,
                    },
                    "files_deleted": deleted_files if files else [],
                    "message": "Book deleted successfully",
                }
                click.echo(format_json(result) if json_output else format_yaml(result))

            except Exception:
                conn.rollback()
                raise

    except click.ClickException:
        raise
    except Exception as exc:
        click.echo(format_error(f"Error deleting book: {exc}"), err=True)
        sys.exit(1)


# ============================================================================
# find-isbn
# ============================================================================


@cli.command(name="find-isbn")
@click.argument("book_id", type=int)
@click.option("--database", type=click.Path(exists=True), help="Path to metadata.db file")
@click.option(
    "--storage",
    type=click.Path(exists=True, file_okay=False),
    help="Path to storage directory",
)
@click.option(
    "--verify",
    is_flag=True,
    help="Verify even if ISBN already exists in database",
)
@click.option(
    "--max-chars",
    type=int,
    default=50000,
    help="Maximum characters to extract (default: 50000)",
)
@click.pass_context
def find_isbn_cmd(
    ctx,
    book_id: int,
    database: str | None,
    storage: str | None,
    verify: bool,
    max_chars: int,
):
    """Find ISBN for a book by extracting from file content and metadata."""
    database_path = Path(database) if database else get_database_path()
    storage_path = Path(storage) if storage else get_storage_path(database_path)
    json_output = ctx.obj.get("json_output", False)

    if not database_path.exists():
        click.echo(format_error(f"Database not found: {database_path}"), err=True)
        sys.exit(1)

    try:
        echo_info(f"Finding ISBN for book {book_id}...")
        book = query_book_by_id(database_path, book_id)

        if book is None:
            click.echo(format_error(f"Book {book_id} not found"), err=True)
            sys.exit(1)

        echo_info(f"Book: {book['title']}")

        if book.get("isbn") and book["isbn"].strip() and not verify:
            echo_info(f"Book already has ISBN: {book['isbn']}")
            result = {
                "status": "already_exists",
                "book_id": book_id,
                "isbn": book["isbn"],
                "message": "Book already has ISBN",
            }
        else:
            book["files"] = resolve_file_paths(storage_path, book["storage_path"], book["files"])
            echo_info(f"Searching for ISBN in book files (scanning up to {max_chars} chars)...")
            isbn_from_file = find_isbn_for_book(book, storage_path, max_chars=max_chars)

            if isbn_from_file:
                click.echo(format_success(f"Found ISBN in file: {isbn_from_file}"), err=True)

                if verify and book.get("isbn"):
                    db_isbn = book["isbn"].strip()
                    if isbn_from_file != db_isbn:
                        click.echo(
                            format_error(f"MISMATCH! Database: {db_isbn}, File: {isbn_from_file}"),
                            err=True,
                        )
                        result = {
                            "status": "mismatch",
                            "book_id": book_id,
                            "isbn_database": db_isbn,
                            "isbn_file": isbn_from_file,
                            "message": "ISBN mismatch",
                        }
                    else:
                        result = {
                            "status": "verified",
                            "book_id": book_id,
                            "isbn": isbn_from_file,
                            "message": "ISBN verified",
                        }
                else:
                    result = {
                        "status": "found",
                        "book_id": book_id,
                        "isbn": isbn_from_file,
                        "message": "ISBN found successfully",
                    }
            else:
                click.echo(format_error("ISBN not found in file content"), err=True)
                result = {
                    "status": "not_found",
                    "book_id": book_id,
                    "isbn": None,
                    "message": "ISBN not found in book files or metadata",
                }

        click.echo(format_json(result) if json_output else format_yaml(result))

        if result["status"] == "not_found":
            sys.exit(1)

    except click.ClickException:
        raise
    except Exception as exc:
        click.echo(format_error(f"Error finding ISBN: {exc}"), err=True)
        sys.exit(1)


# ============================================================================
# fetch-metadata
# ============================================================================


@cli.command(name="fetch-metadata")
@click.argument("book_id", type=int)
@click.option("--database", type=click.Path(exists=True), help="Path to metadata.db file")
@click.option(
    "--storage",
    type=click.Path(exists=True, file_okay=False),
    help="Path to storage directory",
)
@click.option("--timeout", type=int, default=30, help="Timeout in seconds (default: 30)")
@click.option("--isbn-only", is_flag=True, help="Only use ISBN for lookup")
@click.pass_context
def fetch_metadata_cmd(
    ctx,
    book_id: int,
    database: str | None,
    storage: str | None,
    timeout: int,
    isbn_only: bool,
):
    """Fetch comprehensive metadata for a book from online sources."""
    database_path = Path(database) if database else get_database_path()
    storage_path = Path(storage) if storage else get_storage_path(database_path)
    json_output = ctx.obj.get("json_output", False)

    if not database_path.exists():
        click.echo(format_error(f"Database not found: {database_path}"), err=True)
        sys.exit(1)

    try:
        echo_info(f"Fetching metadata for book {book_id}...")
        book = query_book_by_id(database_path, book_id)

        if book is None:
            click.echo(format_error(f"Book {book_id} not found"), err=True)
            sys.exit(1)

        echo_info(f"Book: {book['title']}")

        isbn = book.get("isbn", "").strip() if book.get("isbn") else None

        if not isbn:
            echo_info("No ISBN in database, searching book files...")
            book["files"] = resolve_file_paths(storage_path, book["storage_path"], book["files"])
            isbn = find_isbn_for_book(book, storage_path)

            if isbn:
                click.echo(format_success(f"Found ISBN in files: {isbn}"), err=True)
            else:
                click.echo(format_error("ISBN not found in files"), err=True)
                if isbn_only:
                    click.echo(
                        format_error("Cannot fetch metadata without ISBN (--isbn-only specified)"),
                        err=True,
                    )
                    sys.exit(1)
        else:
            echo_info(f"Using ISBN from database: {isbn}")

        echo_info("Fetching metadata from online sources...")

        if isbn:
            metadata = fetch_metadata_from_calibre(isbn=isbn, timeout=timeout)
        elif not isbn_only:
            book_authors = book.get("authors", [])
            metadata = fetch_metadata_from_calibre(
                title=book["title"],
                authors=book_authors if book_authors else None,
                timeout=timeout,
            )
        else:
            metadata = None

        if metadata:
            click.echo(format_success("Metadata fetched successfully!"), err=True)
            result = {
                "status": "found",
                "book_id": book_id,
                "metadata": metadata,
                "message": "Metadata fetched successfully",
            }
        else:
            click.echo(format_error("Failed to fetch metadata"), err=True)
            result = {
                "status": "not_found",
                "book_id": book_id,
                "metadata": {},
                "message": "Failed to fetch metadata from online sources",
            }

        click.echo(format_json(result) if json_output else format_yaml(result))

        if result["status"] == "not_found":
            sys.exit(1)

    except click.ClickException:
        raise
    except Exception as exc:
        click.echo(format_error(f"Error fetching metadata: {exc}"), err=True)
        sys.exit(1)


# ============================================================================
# set-cover
# ============================================================================


@cli.command(name="set-cover")
@click.argument("book_id", type=int)
@click.argument("cover_file", type=click.Path(exists=True, dir_okay=False))
@click.option("--database", type=click.Path(), help="Path to metadata.db file")
@click.pass_context
def set_cover(ctx, book_id: int, cover_file: str, database: str | None):
    """Set or update cover image for a book."""
    database_path = Path(database) if database else get_database_path()

    if not database_path.exists():
        click.echo(format_error(f"Database not found: {database_path}"), err=True)
        sys.exit(1)

    try:
        set_cover_for_book(database_path, book_id, Path(cover_file))
        click.echo(format_success(f"Cover set successfully for book {book_id}"), err=True)

    except click.ClickException:
        raise
    except Exception as exc:
        click.echo(format_error(f"Error setting cover: {exc}"), err=True)
        sys.exit(1)


# ============================================================================
# retag
# ============================================================================


@cli.command()
@click.option(
    "--mapping",
    "-m",
    type=click.Path(exists=True),
    default="tag-mappings.yaml",
    help="Path to YAML file with tag mappings",
)
@click.option("--database", type=click.Path(exists=True), help="Path to Calibre database")
@click.option("--dry-run", is_flag=True, help="Show what would change without modifying database")
@click.option("--backup/--no-backup", default=True, help="Create database backup before changes")
@click.pass_context
def retag(ctx, mapping: str, database: str | None, dry_run: bool, backup: bool):
    """Refactor library tags based on mapping file."""
    database_path = Path(database) if database else get_database_path()

    try:
        with open(mapping) as f:
            mappings = yaml.safe_load(f)
    except Exception as exc:
        click.echo(format_error(f"Failed to load mapping file: {exc}"), err=True)
        sys.exit(1)

    if not mappings:
        click.echo(format_error("Mapping file is empty"), err=True)
        sys.exit(1)

    echo_info(f"Loaded {len(mappings)} tag mappings from {mapping}")

    try:
        stats = apply_retag(database_path, mappings, dry_run=dry_run, backup=backup)

        if dry_run:
            click.echo(format_success("Dry-run complete"), err=True)
            click.echo(
                format_yaml(
                    {
                        "would_delete": stats.get("would_delete", []),
                        "would_consolidate": stats.get("would_consolidate", {}),
                    }
                )
            )
        else:
            click.echo("", err=True)
            click.echo(format_success("Tag refactoring completed successfully!"), err=True)
            click.echo(format_yaml(stats))

    except Exception as exc:
        click.echo(format_error(f"Error during tag refactoring: {exc}"), err=True)
        sys.exit(1)


# ============================================================================
# dedupe
# ============================================================================


@cli.command()
@click.option("--database", type=click.Path(exists=True), help="Path to metadata.db file")
@click.option(
    "--storage",
    type=click.Path(exists=True, file_okay=False),
    help="Path to storage directory",
)
@click.option("--dry-run", is_flag=True, help="Show what would be deleted without making changes")
@click.option(
    "--auto-delete", is_flag=True, help="Automatically delete duplicates keeping best copy"
)
@click.option(
    "--format",
    "formats",
    multiple=True,
    help="Only check specific formats (can specify multiple times)",
)
@click.option(
    "--algorithm",
    type=click.Choice(["sha256", "md5"]),
    default="sha256",
    help="Hash algorithm (default: sha256)",
)
@click.option("--no-backup", is_flag=True, help="Skip database backup (NOT RECOMMENDED)")
@click.pass_context
def dedupe(
    ctx,
    database: str | None,
    storage: str | None,
    dry_run: bool,
    auto_delete: bool,
    formats: tuple,
    algorithm: str,
    no_backup: bool,
):
    """Find and remove duplicate books based on file hash."""
    database_path = Path(database) if database else get_database_path()
    storage_path = Path(storage) if storage else get_storage_path(database_path)
    json_output = ctx.obj.get("json_output", False)

    if not database_path.exists():
        click.echo(format_error(f"Database not found: {database_path}"), err=True)
        sys.exit(1)

    check_formats = [f.upper() for f in formats] if formats else None

    echo_info(f"Computing file hashes using {algorithm.upper()}...")
    if check_formats:
        echo_info(f"Only checking formats: {', '.join(check_formats)}")
    click.echo("", err=True)

    try:
        duplicate_groups, total_files = find_duplicates_by_hash(
            database_path, storage_path, algorithm, check_formats
        )

        if not duplicate_groups:
            click.echo(format_success("No duplicate files found!"), err=True)
            sys.exit(0)

        total_duplicates = sum(len(files) - 1 for files in duplicate_groups.values())
        echo_info(f"Found {len(duplicate_groups)} hash groups with duplicates")
        echo_info(f"Total duplicate files: {total_duplicates}")
        click.echo("", err=True)

        format_preference = ["EPUB", "PDF", "MOBI", "AZW3", "TXT"]
        deletion_plan = []

        for file_hash, files in duplicate_groups.items():
            book_metadata = {}
            for file_info in files:
                book = query_book_by_id(database_path, file_info["book_id"])
                if book:
                    score = (
                        len(book.get("authors", []))
                        + len(book.get("tags", []))
                        + (1 if book.get("publisher") else 0)
                        + (1 if book.get("isbn") else 0)
                        + (1 if book.get("abstract") else 0)
                        + (1 if book.get("series") else 0)
                        + (1 if book.get("rating") else 0)
                    )
                    book_metadata[file_info["book_id"]] = {"metadata_score": score, "book": book}

            def sort_key(f, _bm=book_metadata):
                bid = f["book_id"]
                score = _bm.get(bid, {}).get("metadata_score", 0)
                fmt = f["format"]
                fmt_score = format_preference.index(fmt) if fmt in format_preference else 999
                return (-score, fmt_score, f.get("date_added", ""))

            sorted_files = sorted(files, key=sort_key)
            best_file = sorted_files[0]

            for dup in sorted_files[1:]:
                dup_score = book_metadata.get(dup["book_id"], {}).get("metadata_score", 0)
                best_score = book_metadata.get(best_file["book_id"], {}).get("metadata_score", 0)
                deletion_plan.append(
                    {
                        "hash": file_hash[:16],
                        "book_id": dup["book_id"],
                        "title": dup["title"],
                        "format": dup["format"],
                        "path": dup["path"],
                        "size_mb": round(dup["size"] / (1024 * 1024), 2) if dup["size"] else 0,
                        "metadata_score": dup_score,
                        "kept_book_id": best_file["book_id"],
                        "kept_format": best_file["format"],
                        "kept_metadata_score": best_score,
                    }
                )

        if dry_run or not auto_delete:
            click.echo(format_success(f"Would delete {len(deletion_plan)} files"), err=True)
            total_space = sum(item["size_mb"] for item in deletion_plan)
            click.echo(format_success(f"Potential space savings: {total_space:.2f} MB"), err=True)
            if not auto_delete:
                click.echo("Run with --auto-delete to apply changes", err=True)
        else:
            if not no_backup:
                backup_path = backup_database(database_path)
                echo_info(f"Database backed up to {backup_path}")

            echo_info("Deleting duplicate books...")
            deleted_count = 0
            for item in deletion_plan:
                try:
                    with connect_database(database_path, read_only=False) as conn:
                        conn.execute("BEGIN TRANSACTION")
                        try:
                            conn.execute("DELETE FROM books WHERE id = ?", (item["book_id"],))
                            conn.commit()
                        except Exception:
                            conn.rollback()
                            raise
                    file_path = Path(item["path"])
                    if file_path.exists():
                        file_path.unlink()
                    deleted_count += 1
                except Exception as exc:
                    click.echo(
                        format_error(f"Failed to delete book {item['book_id']}: {exc}"), err=True
                    )

            click.echo(format_success(f"Deleted {deleted_count} duplicate books"), err=True)

        result = {
            "status": "success",
            "mode": "dry_run" if dry_run else ("auto_delete" if auto_delete else "preview"),
            "duplicate_groups": len(duplicate_groups),
            "total_duplicates": total_duplicates,
            "files_to_delete": len(deletion_plan),
            "deletion_plan": deletion_plan,
        }

        click.echo(format_json(result) if json_output else format_yaml(result))

    except Exception as exc:
        click.echo(format_error(f"Error during deduplication: {exc}"), err=True)
        sys.exit(1)


# ============================================================================
# Entry point
# ============================================================================


@cli.command()
def serve():
    """MCP server on stdio (tools: search_books, get_book, list_tags, export_bibtex, update_metadata, attach_file)."""
    import os

    from calibrectl import mcp_server, settings

    mcp_server.build(settings.load(dict(os.environ)), dict(os.environ)).run("stdio")


def main():
    cli(auto_envvar_prefix="CALIBRECTL")
