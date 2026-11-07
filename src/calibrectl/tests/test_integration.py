"""
Integration tests against a realistic Calibre library.

Uses the official Calibre metadata.db schema (with triggers, indexes, constraints)
and populates it with a multi-book library including real files on disk, covers,
ISBNs, tags, series, ratings, and multiple formats per book.
"""

import json
import os
import sqlite3
from pathlib import Path

import pytest
from click.testing import CliRunner

from calibrectl.cli import cli

SCHEMA_PATH = Path(__file__).parent / "calibre_schema.sql"


def _title_sort(title):
    """Mimic Calibre's title_sort for the books_insert_trg trigger."""
    if not title:
        return ""
    lower = title.lower()
    for article in ("the ", "a ", "an "):
        if lower.startswith(article):
            return lower[len(article) :]
    return lower


def _build_library(tmp_path: Path) -> tuple[Path, Path]:
    """
    Build a realistic Calibre library on disk with the official schema.

    Returns (db_path, storage_path).
    """
    storage = tmp_path / "library"
    storage.mkdir()
    db_path = storage / "metadata.db"

    conn = sqlite3.connect(str(db_path))
    conn.row_factory = sqlite3.Row
    conn.create_function("title_sort", 1, _title_sort)
    conn.create_function("uuid4", 0, lambda: "test-uuid-0001")

    schema_sql = SCHEMA_PATH.read_text()
    # The official schema has triggers that call title_sort() and uuid4() —
    # custom C functions only available inside Calibre. Strip those triggers
    # so executescript works with vanilla SQLite; we set sort/uuid manually.
    import re

    schema_sql = re.sub(
        r"CREATE TRIGGER books_insert_trg.*?END;",
        "",
        schema_sql,
        flags=re.DOTALL,
    )
    schema_sql = re.sub(
        r"CREATE TRIGGER books_update_trg.*?END;",
        "",
        schema_sql,
        flags=re.DOTALL,
    )
    schema_sql = re.sub(
        r"CREATE TRIGGER series_insert_trg.*?END;",
        "",
        schema_sql,
        flags=re.DOTALL,
    )
    schema_sql = re.sub(
        r"CREATE TRIGGER series_update_trg.*?END;",
        "",
        schema_sql,
        flags=re.DOTALL,
    )
    schema_sql = re.sub(
        r"CREATE TRIGGER books_pages_link_create_trigger.*?END;",
        "",
        schema_sql,
        flags=re.DOTALL,
    )
    conn.executescript(schema_sql)

    conn.execute("INSERT INTO library_id (uuid) VALUES ('test-library-uuid-0001')")

    # --- Authors ---
    conn.execute(
        "INSERT INTO authors (id, name, sort, link) VALUES (1, 'Isaac Asimov', 'Asimov, Isaac', '')"
    )
    conn.execute(
        "INSERT INTO authors (id, name, sort, link) VALUES (2, 'Frank Herbert', 'Herbert, Frank', '')"
    )
    conn.execute(
        "INSERT INTO authors (id, name, sort, link) VALUES (3, 'Arthur C. Clarke', 'Clarke, Arthur C.', '')"
    )
    conn.execute(
        "INSERT INTO authors (id, name, sort, link) VALUES (4, 'William Gibson', 'Gibson, William', '')"
    )

    # --- Publishers ---
    conn.execute(
        "INSERT INTO publishers (id, name, sort, link) VALUES (1, 'Gnome Press', 'Gnome Press', '')"
    )
    conn.execute(
        "INSERT INTO publishers (id, name, sort, link) VALUES (2, 'Chilton Books', 'Chilton Books', '')"
    )
    conn.execute(
        "INSERT INTO publishers (id, name, sort, link) VALUES (3, 'Harcourt', 'Harcourt', '')"
    )
    conn.execute(
        "INSERT INTO publishers (id, name, sort, link) VALUES (4, 'Ace Books', 'Ace Books', '')"
    )

    # --- Tags ---
    conn.execute("INSERT INTO tags (id, name, link) VALUES (1, 'Science Fiction', '')")
    conn.execute("INSERT INTO tags (id, name, link) VALUES (2, 'Classic', '')")
    conn.execute("INSERT INTO tags (id, name, link) VALUES (3, 'Space Opera', '')")
    conn.execute("INSERT INTO tags (id, name, link) VALUES (4, 'Cyberpunk', '')")
    conn.execute("INSERT INTO tags (id, name, link) VALUES (5, 'Hugo Award', '')")
    conn.execute("INSERT INTO tags (id, name, link) VALUES (6, 'Nebula Award', '')")
    conn.execute("INSERT INTO tags (id, name, link) VALUES (7, 'orphaned-empty-tag', '')")

    # --- Series ---
    conn.execute(
        "INSERT INTO series (id, name, sort, link) VALUES (1, 'Foundation', 'Foundation', '')"
    )
    conn.execute("INSERT INTO series (id, name, sort, link) VALUES (2, 'Dune', 'Dune', '')")
    conn.execute(
        "INSERT INTO series (id, name, sort, link) VALUES (3, 'Sprawl Trilogy', 'Sprawl Trilogy', '')"
    )

    # --- Ratings (Calibre uses even numbers 0-10) ---
    conn.execute("INSERT INTO ratings (id, rating, link) VALUES (1, 10, '')")
    conn.execute("INSERT INTO ratings (id, rating, link) VALUES (2, 8, '')")
    conn.execute("INSERT INTO ratings (id, rating, link) VALUES (3, 6, '')")

    # --- Languages ---
    conn.execute("INSERT INTO languages (id, lang_code, link) VALUES (1, 'eng', '')")

    books = [
        {
            "id": 1,
            "title": "Foundation",
            "author_sort": "Asimov, Isaac",
            "path": "Isaac Asimov/Foundation (1)",
            "pubdate": "1951-06-01 00:00:00+00:00",
            "timestamp": "2024-01-15 10:30:00+00:00",
            "last_modified": "2024-06-01 08:00:00+00:00",
            "has_cover": 1,
            "series_index": 1.0,
            "authors": [1],
            "publisher": 1,
            "tags": [1, 2, 3, 5],
            "series": 1,
            "rating": 1,
            "isbn": "9780553293357",
            "comment": "The first novel in Isaac Asimov's classic science-fiction masterpiece.",
            "formats": [("EPUB", 450000), ("PDF", 2100000)],
        },
        {
            "id": 2,
            "title": "Foundation and Empire",
            "author_sort": "Asimov, Isaac",
            "path": "Isaac Asimov/Foundation and Empire (2)",
            "pubdate": "1952-01-01 00:00:00+00:00",
            "timestamp": "2024-01-15 10:31:00+00:00",
            "last_modified": "2024-06-01 08:01:00+00:00",
            "has_cover": 1,
            "series_index": 2.0,
            "authors": [1],
            "publisher": 1,
            "tags": [1, 2, 3],
            "series": 1,
            "rating": 2,
            "isbn": "9780553293371",
            "comment": "The second novel in the Foundation series.",
            "formats": [("EPUB", 380000)],
        },
        {
            "id": 3,
            "title": "Dune",
            "author_sort": "Herbert, Frank",
            "path": "Frank Herbert/Dune (3)",
            "pubdate": "1965-08-01 00:00:00+00:00",
            "timestamp": "2024-02-10 14:00:00+00:00",
            "last_modified": "2024-07-15 12:00:00+00:00",
            "has_cover": 1,
            "series_index": 1.0,
            "authors": [2],
            "publisher": 2,
            "tags": [1, 2, 5, 6],
            "series": 2,
            "rating": 1,
            "isbn": "9780441172719",
            "comment": "A sweeping science fiction epic about the desert planet Arrakis.",
            "formats": [("EPUB", 520000), ("PDF", 3200000), ("MOBI", 490000)],
        },
        {
            "id": 4,
            "title": "Childhood's End",
            "author_sort": "Clarke, Arthur C.",
            "path": "Arthur C. Clarke/Childhood's End (4)",
            "pubdate": "1953-01-01 00:00:00+00:00",
            "timestamp": "2024-03-05 09:00:00+00:00",
            "last_modified": "2024-08-01 16:00:00+00:00",
            "has_cover": 0,
            "series_index": 1.0,
            "authors": [3],
            "publisher": 3,
            "tags": [1, 2],
            "series": None,
            "rating": 2,
            "isbn": None,
            "comment": None,
            "formats": [("PDF", 1800000)],
        },
        {
            "id": 5,
            "title": "Neuromancer",
            "author_sort": "Gibson, William",
            "path": "William Gibson/Neuromancer (5)",
            "pubdate": "1984-07-01 00:00:00+00:00",
            "timestamp": "2024-04-20 17:00:00+00:00",
            "last_modified": "2024-09-10 11:00:00+00:00",
            "has_cover": 1,
            "series_index": 1.0,
            "authors": [4],
            "publisher": 4,
            "tags": [1, 4, 5, 6],
            "series": 3,
            "rating": 1,
            "isbn": "9780441569595",
            "comment": "The groundbreaking cyberpunk novel that launched a genre.",
            "formats": [("EPUB", 310000)],
        },
        {
            "id": 6,
            "title": "Foundation",
            "author_sort": "Asimov, Isaac",
            "path": "Isaac Asimov/Foundation (6)",
            "pubdate": "1951-06-01 00:00:00+00:00",
            "timestamp": "2024-05-01 12:00:00+00:00",
            "last_modified": "2024-05-01 12:00:00+00:00",
            "has_cover": 0,
            "series_index": 1.0,
            "authors": [1],
            "publisher": 1,
            "tags": [1],
            "series": 1,
            "rating": None,
            "isbn": "9780553293357",
            "comment": None,
            "formats": [("EPUB", 450000)],
        },
    ]

    for b in books:
        conn.execute(
            """INSERT INTO books
               (id, title, sort, author_sort, path, pubdate, timestamp, last_modified, has_cover, series_index, uuid)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            (
                b["id"],
                b["title"],
                _title_sort(b["title"]),
                b["author_sort"],
                b["path"],
                b["pubdate"],
                b["timestamp"],
                b["last_modified"],
                b["has_cover"],
                b["series_index"],
                f"test-uuid-{b['id']:04d}",
            ),
        )

        for author_id in b["authors"]:
            conn.execute(
                "INSERT INTO books_authors_link (book, author) VALUES (?, ?)", (b["id"], author_id)
            )

        if b["publisher"]:
            conn.execute(
                "INSERT INTO books_publishers_link (book, publisher) VALUES (?, ?)",
                (b["id"], b["publisher"]),
            )

        for tag_id in b["tags"]:
            conn.execute("INSERT INTO books_tags_link (book, tag) VALUES (?, ?)", (b["id"], tag_id))

        if b["series"]:
            conn.execute(
                "INSERT INTO books_series_link (book, series) VALUES (?, ?)", (b["id"], b["series"])
            )

        if b["rating"]:
            conn.execute(
                "INSERT INTO books_ratings_link (book, rating) VALUES (?, ?)",
                (b["id"], b["rating"]),
            )

        if b["isbn"]:
            conn.execute(
                "INSERT INTO identifiers (book, type, val) VALUES (?, 'isbn', ?)",
                (b["id"], b["isbn"]),
            )

        if b["comment"]:
            conn.execute("INSERT INTO comments (book, text) VALUES (?, ?)", (b["id"], b["comment"]))

        conn.execute("INSERT INTO books_languages_link (book, lang_code) VALUES (?, 1)", (b["id"],))

        book_dir = storage / b["path"]
        book_dir.mkdir(parents=True, exist_ok=True)

        filename_base = f"{b['title']} - {b['author_sort'].split(',')[0].strip()}"
        for fmt, size in b["formats"]:
            file_path = book_dir / f"{filename_base}.{fmt.lower()}"
            file_path.write_bytes(os.urandom(min(size, 1024)))
            conn.execute(
                "INSERT INTO data (book, format, uncompressed_size, name) VALUES (?, ?, ?, ?)",
                (b["id"], fmt, size, filename_base),
            )

        if b["has_cover"]:
            cover = book_dir / "cover.jpg"
            cover.write_bytes(b"\xff\xd8\xff\xe0" + os.urandom(200))

    conn.commit()
    conn.close()
    return db_path, storage


@pytest.fixture
def calibre_library(tmp_path):
    """Full Calibre library with official schema, 6 books, real files."""
    db_path, storage = _build_library(tmp_path)
    return db_path, storage


@pytest.fixture
def runner():
    return CliRunner()


def _run(runner, args, db_path, storage=None, json_output=True):
    """Run a CLI command against the test library."""
    cmd = []
    if json_output:
        cmd.append("--json-output")
    cmd.extend(args)
    cmd.extend(["--database", str(db_path)])
    if storage:
        cmd.extend(["--storage", str(storage)])
    return runner.invoke(cli, cmd)


def _parse_json(output):
    """Extract JSON from output that may contain stderr progress lines."""
    decoder = json.JSONDecoder()
    for i, ch in enumerate(output):
        if ch in ("{", "["):
            try:
                obj, _ = decoder.raw_decode(output, i)
                return obj
            except json.JSONDecodeError:
                continue
    raise ValueError(f"No JSON in output: {output!r}")


# ==========================================================================
# get command
# ==========================================================================


class TestGetIntegration:
    def test_get_book_full_metadata(self, runner, calibre_library):
        db, storage = calibre_library
        result = _run(runner, ["get", "1"], db, storage)
        assert result.exit_code == 0
        data = _parse_json(result.output)
        assert data["title"] == "Foundation"
        assert "Isaac Asimov" in data["authors"]
        assert data["publisher"] == "Gnome Press"
        assert data["series"] == "Foundation"
        assert data["series_index"] == 1.0
        assert data["isbn"] == "9780553293357"
        assert data["has_cover"] is True
        assert data["rating"] == 10
        assert len(data["files"]) == 2
        formats = {f["format"] for f in data["files"]}
        assert formats == {"EPUB", "PDF"}

    def test_get_book_without_cover(self, runner, calibre_library):
        db, storage = calibre_library
        result = _run(runner, ["get", "4"], db, storage)
        assert result.exit_code == 0
        data = _parse_json(result.output)
        assert data["title"] == "Childhood's End"
        assert data["has_cover"] is False
        assert data["cover_path"] is None

    def test_get_book_no_isbn(self, runner, calibre_library):
        db, storage = calibre_library
        result = _run(runner, ["get", "4"], db, storage)
        data = _parse_json(result.output)
        assert data["isbn"] == "" or data["isbn"] is None

    def test_get_nonexistent(self, runner, calibre_library):
        db, storage = calibre_library
        result = _run(runner, ["get", "999"], db, storage)
        assert result.exit_code != 0


# ==========================================================================
# list command
# ==========================================================================


class TestListIntegration:
    def test_list_all_books(self, runner, calibre_library):
        db, storage = calibre_library
        result = _run(runner, ["list"], db)
        assert result.exit_code == 0
        data = _parse_json(result.output)
        assert len(data["books"]) == 6

    def test_list_by_tag(self, runner, calibre_library):
        db, _ = calibre_library
        result = _run(runner, ["list", "--tag", "Cyberpunk"], db)
        assert result.exit_code == 0
        data = _parse_json(result.output)
        assert len(data["books"]) == 1
        assert data["books"][0]["title"] == "Neuromancer"

    def test_list_by_author(self, runner, calibre_library):
        db, _ = calibre_library
        result = _run(runner, ["list", "--author", "Asimov"], db)
        assert result.exit_code == 0
        data = _parse_json(result.output)
        assert len(data["books"]) == 3

    def test_list_by_series(self, runner, calibre_library):
        db, _ = calibre_library
        result = _run(runner, ["list", "--series", "Foundation"], db)
        assert result.exit_code == 0
        data = _parse_json(result.output)
        assert all("Foundation" in b.get("series", "") for b in data["books"])

    def test_list_by_title(self, runner, calibre_library):
        db, _ = calibre_library
        result = _run(runner, ["list", "--title", "Dune"], db)
        assert result.exit_code == 0
        data = _parse_json(result.output)
        assert len(data["books"]) == 1

    def test_list_limit(self, runner, calibre_library):
        db, _ = calibre_library
        result = _run(runner, ["list", "--limit", "2"], db)
        assert result.exit_code == 0
        data = _parse_json(result.output)
        assert len(data["books"]) == 2

    def test_list_nonexistent_tag(self, runner, calibre_library):
        db, _ = calibre_library
        result = _run(runner, ["list", "--tag", "nonexistent-xyz"], db)
        assert result.exit_code == 0 or "No books" in (result.output + (result.stderr or ""))


# ==========================================================================
# tags command
# ==========================================================================


class TestTagsIntegration:
    def test_tags_list(self, runner, calibre_library):
        db, _ = calibre_library
        result = _run(runner, ["tags"], db)
        assert result.exit_code == 0
        data = _parse_json(result.output)
        names = [t["name"] for t in data["tags"]]
        assert "Science Fiction" in names
        assert "Cyberpunk" in names
        assert "Hugo Award" in names

    def test_tags_sort_by_count(self, runner, calibre_library):
        db, _ = calibre_library
        result = _run(runner, ["tags", "--sort-by", "count"], db)
        assert result.exit_code == 0
        data = _parse_json(result.output)
        counts = [t["book_count"] for t in data["tags"]]
        assert counts == sorted(counts, reverse=True)

    def test_tags_min_books_filter(self, runner, calibre_library):
        db, _ = calibre_library
        result = _run(runner, ["tags", "--min-books", "3"], db)
        assert result.exit_code == 0
        data = _parse_json(result.output)
        for tag in data["tags"]:
            assert tag["book_count"] >= 3


# ==========================================================================
# query command
# ==========================================================================


class TestQueryIntegration:
    def test_query_by_rating(self, runner, calibre_library):
        db, _ = calibre_library
        result = _run(runner, ["query", "r.rating = 10"], db)
        assert result.exit_code == 0
        data = _parse_json(result.output)
        for book in data["books"]:
            assert book["rating"] == 10

    def test_query_by_author_name(self, runner, calibre_library):
        db, _ = calibre_library
        result = _run(runner, ["query", "a.name = 'Frank Herbert'"], db)
        assert result.exit_code == 0
        data = _parse_json(result.output)
        assert len(data["books"]) == 1
        assert data["books"][0]["title"] == "Dune"

    def test_query_bad_sql(self, runner, calibre_library):
        db, _ = calibre_library
        result = _run(runner, ["query", "INVALID SQL !!!"], db)
        assert result.exit_code != 0


# ==========================================================================
# doctor command
# ==========================================================================


class TestDoctorIntegration:
    def test_doctor_healthy_library(self, runner, calibre_library):
        db, storage = calibre_library
        result = _run(runner, ["doctor"], db, storage)
        data = _parse_json(result.output)
        assert data["integrity"]["status"] == "ok"
        assert data["schema"]["status"] == "ok"
        assert data["overview"]["total_books"] == 6

    def test_doctor_detects_title_duplicates(self, runner, calibre_library):
        db, storage = calibre_library
        result = _run(runner, ["doctor"], db, storage)
        data = _parse_json(result.output)
        title_dupes = data.get("issues", {}).get("title_duplicates")
        assert title_dupes is not None
        assert title_dupes["count"] >= 1

    def test_doctor_detects_isbn_duplicates(self, runner, calibre_library):
        db, storage = calibre_library
        result = _run(runner, ["doctor"], db, storage)
        data = _parse_json(result.output)
        isbn_dupes = data.get("issues", {}).get("isbn_duplicates")
        assert isbn_dupes is not None

    def test_doctor_detects_empty_tags(self, runner, calibre_library):
        db, storage = calibre_library
        result = _run(runner, ["doctor"], db, storage)
        data = _parse_json(result.output)
        empty_tags = data.get("issues", {}).get("empty_tags")
        assert empty_tags is not None
        tag_names = [t["name"] for t in empty_tags["tags"]]
        assert "orphaned-empty-tag" in tag_names

    def test_doctor_detects_books_without_cover(self, runner, calibre_library):
        db, storage = calibre_library
        result = _run(runner, ["doctor"], db, storage)
        data = _parse_json(result.output)
        assert data["warnings"]["books_without_covers"] == 2

    def test_doctor_has_recommendations(self, runner, calibre_library):
        db, storage = calibre_library
        result = _run(runner, ["doctor"], db, storage)
        data = _parse_json(result.output)
        assert len(data["recommendations"]) > 0


# ==========================================================================
# export-bibtex command
# ==========================================================================


class TestExportBibtexIntegration:
    def test_export_single_book(self, runner, calibre_library):
        db, _ = calibre_library
        result = _run(runner, ["export-bibtex", "3"], db, json_output=False)
        assert result.exit_code == 0
        assert "@book{" in result.output
        assert "Dune" in result.output
        assert "Herbert" in result.output

    def test_export_all(self, runner, calibre_library):
        db, _ = calibre_library
        result = _run(runner, ["export-bibtex", "--all"], db, json_output=False)
        assert result.exit_code == 0
        entries = [line for line in result.output.splitlines() if line.startswith("@book{")]
        assert len(entries) == 6

    def test_export_by_tag(self, runner, calibre_library):
        db, _ = calibre_library
        result = _run(runner, ["export-bibtex", "--tag", "Cyberpunk"], db, json_output=False)
        assert result.exit_code == 0
        assert "Neuromancer" in result.output

    def test_export_unique_cite_keys(self, runner, calibre_library):
        db, _ = calibre_library
        result = _run(runner, ["export-bibtex", "--all"], db, json_output=False)
        keys = [
            line.split("{")[1].rstrip(",")
            for line in result.output.splitlines()
            if line.startswith("@book{")
        ]
        assert len(keys) == len(set(keys)), f"Duplicate keys: {keys}"

    def test_export_to_file(self, runner, calibre_library, tmp_path):
        db, _ = calibre_library
        outfile = tmp_path / "refs.bib"
        result = _run(
            runner,
            ["export-bibtex", "--all", "--output", str(outfile)],
            db,
            json_output=False,
        )
        assert result.exit_code == 0
        content = outfile.read_text()
        assert "@book{" in content


# ==========================================================================
# create + update + delete lifecycle
# ==========================================================================


class TestCRUDLifecycle:
    def test_create_update_delete(self, runner, calibre_library):
        db, storage = calibre_library

        # Create
        result = runner.invoke(
            cli,
            [
                "--quiet",
                "create",
                "--json-output",
                "--title",
                "The Left Hand of Darkness",
                "--authors",
                "Ursula K. Le Guin",
                "--isbn",
                "9780441478125",
                "--publisher",
                "Ace Books",
                "--pubdate",
                "1969-03-01",
                "--tags",
                "Science Fiction,Feminist SF",
                "--series",
                "Hainish Cycle",
                "--series-index",
                "4",
                "--rating",
                "10",
                "--abstract",
                "A groundbreaking exploration of gender and society.",
                "--database",
                str(db),
                "--storage",
                str(storage),
                "--no-backup",
            ],
        )
        assert result.exit_code == 0, result.output + (result.stderr or "")
        data = _parse_json(result.output)
        assert data["status"] == "success"
        new_id = data["book_id"]

        # Verify
        result = _run(runner, ["get", str(new_id)], db, storage)
        assert result.exit_code == 0
        book = _parse_json(result.output)
        assert book["title"] == "The Left Hand of Darkness"
        assert "Ursula K. Le Guin" in book["authors"]
        assert book["isbn"] == "9780441478125"
        assert book["publisher"] == "Ace Books"
        assert book["series"] == "Hainish Cycle"
        assert book["rating"] == 10

        # Update title
        result = runner.invoke(
            cli,
            [
                "--quiet",
                "update",
                str(new_id),
                "--json-output",
                "--title",
                "The Left Hand of Darkness (Revised)",
                "--database",
                str(db),
                "--no-backup",
            ],
        )
        assert result.exit_code == 0

        # Verify update
        result = _run(runner, ["get", str(new_id)], db, storage)
        book = _parse_json(result.output)
        assert "Revised" in book["title"]

        # Delete
        result = runner.invoke(
            cli,
            [
                "--quiet",
                "delete",
                str(new_id),
                "--json-output",
                "--force",
                "--files",
                "--database",
                str(db),
                "--storage",
                str(storage),
                "--no-backup",
            ],
        )
        assert result.exit_code == 0
        data = _parse_json(result.output)
        assert data["status"] == "success"

        # Verify deletion
        result = _run(runner, ["get", str(new_id)], db, storage)
        assert result.exit_code != 0


# ==========================================================================
# add-file command
# ==========================================================================


class TestAddFileIntegration:
    def test_add_pdf_to_book(self, runner, calibre_library, tmp_path):
        db, storage = calibre_library

        pdf = tmp_path / "new_book.pdf"
        pdf.write_bytes(b"%PDF-1.4 " + os.urandom(500))

        result = runner.invoke(
            cli,
            [
                "--json-output",
                "add-file",
                "4",
                str(pdf),
                "--format",
                "PDF",
                "--database",
                str(db),
                "--storage",
                str(storage),
                "--no-backup",
            ],
        )
        # Book 4 already has PDF — this should fail
        assert result.exit_code != 0

    def test_add_epub_to_pdf_only_book(self, runner, calibre_library, tmp_path):
        db, storage = calibre_library

        epub = tmp_path / "new_book.epub"
        epub.write_bytes(b"PK" + os.urandom(500))

        result = runner.invoke(
            cli,
            [
                "add-file",
                "4",
                str(epub),
                "--format",
                "EPUB",
                "--database",
                str(db),
                "--storage",
                str(storage),
                "--json-output",
                "--no-backup",
            ],
        )
        assert result.exit_code == 0, result.output + (result.stderr or "")
        data = _parse_json(result.output)
        assert data["format"] == "EPUB"


# ==========================================================================
# set-cover command
# ==========================================================================


class TestSetCoverIntegration:
    def test_set_cover_on_uncovered_book(self, runner, calibre_library, tmp_path):
        db, storage = calibre_library

        cover = tmp_path / "new_cover.jpg"
        cover.write_bytes(b"\xff\xd8\xff\xe0" + os.urandom(200))

        result = runner.invoke(
            cli,
            ["set-cover", "4", str(cover), "--database", str(db)],
            env={"CALIBRE_STORAGE": str(storage)},
        )
        assert result.exit_code == 0

        result = _run(runner, ["get", "4"], db, storage)
        book = _parse_json(result.output)
        assert book["has_cover"] is True


# ==========================================================================
# retag command
# ==========================================================================


class TestRetagIntegration:
    def test_retag_dry_run(self, runner, calibre_library, tmp_path):
        db, _ = calibre_library

        mapping_file = tmp_path / "mappings.yaml"
        mapping_file.write_text("Classic: Classics\nSpace Opera: null\n")

        result = runner.invoke(
            cli,
            [
                "retag",
                "--mapping",
                str(mapping_file),
                "--database",
                str(db),
                "--dry-run",
                "--no-backup",
            ],
        )
        assert result.exit_code == 0
        assert "Classic" in result.output or "Dry-run" in result.stderr

    def test_retag_live(self, runner, calibre_library, tmp_path):
        db, _ = calibre_library

        mapping_file = tmp_path / "mappings.yaml"
        mapping_file.write_text("Classic: Classics\n")

        result = runner.invoke(
            cli,
            [
                "retag",
                "--mapping",
                str(mapping_file),
                "--database",
                str(db),
                "--no-backup",
            ],
        )
        assert result.exit_code == 0

        result = _run(runner, ["tags"], db)
        data = _parse_json(result.output)
        names = [t["name"] for t in data["tags"]]
        assert "Classics" in names
        assert "Classic" not in names


# ==========================================================================
# dedupe command
# ==========================================================================


class TestDedupeIntegration:
    def test_dedupe_finds_identical_files(self, runner, calibre_library):
        db, storage = calibre_library

        b1_dir = storage / "Isaac Asimov/Foundation (1)"
        b6_dir = storage / "Isaac Asimov/Foundation (6)"
        shared_content = b"identical epub content for dedup test"
        (b1_dir / "Foundation - Asimov.epub").write_bytes(shared_content)
        (b6_dir / "Foundation - Asimov.epub").write_bytes(shared_content)

        result = _run(runner, ["dedupe", "--dry-run"], db, storage)
        assert result.exit_code == 0
        data = _parse_json(result.output)
        assert data["duplicate_groups"] >= 1

    def test_dedupe_no_duplicates_on_unique(self, runner, calibre_library):
        db, storage = calibre_library
        result = _run(runner, ["dedupe", "--dry-run", "--format", "MOBI"], db, storage)
        assert result.exit_code == 0
        assert "No duplicate" in result.output or "duplicate_groups" in result.output


# ==========================================================================
# YAML vs JSON output
# ==========================================================================


class TestOutputFormats:
    def test_yaml_output_default(self, runner, calibre_library):
        db, storage = calibre_library
        result = runner.invoke(cli, ["get", "1", "--database", str(db), "--storage", str(storage)])
        assert result.exit_code == 0
        assert "title:" in result.output
        assert "Foundation" in result.output

    def test_json_output_flag(self, runner, calibre_library):
        db, storage = calibre_library
        result = _run(runner, ["get", "1"], db, storage, json_output=True)
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["title"] == "Foundation"

    def test_quiet_mode(self, runner, calibre_library):
        db, storage = calibre_library
        result = runner.invoke(
            cli,
            [
                "--quiet",
                "--json-output",
                "get",
                "1",
                "--database",
                str(db),
                "--storage",
                str(storage),
            ],
        )
        assert result.exit_code == 0
        assert "[*]" not in result.output
