"""
Tests for calibrectl.db
"""

import pytest

from calibrectl.db import (
    REQUIRED_CALIBRE_TABLES,
    check_required_tables,
    compute_file_hash,
    connect_database,
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
)


class TestCheckRequiredTables:
    def test_all_present(self):
        assert check_required_tables(["books", "tags", "authors"], ["books", "tags"]) == []

    def test_some_missing(self):
        result = check_required_tables(["books"], ["books", "tags", "authors"])
        assert result == ["authors", "tags"]

    def test_empty_existing(self):
        assert check_required_tables([], ["books"]) == ["books"]

    def test_empty_required(self):
        assert check_required_tables(["books", "tags"], []) == []

    def test_returns_sorted(self):
        missing = check_required_tables([], ["z_table", "a_table", "m_table"])
        assert missing == sorted(missing)

    @pytest.mark.parametrize(
        "extra_table",
        ["books", "authors", "tags", "data", "identifiers", "comments"],
    )
    def test_all_calibre_tables_recognized(self, extra_table):
        # If we supply all required tables, nothing is missing
        result = check_required_tables(REQUIRED_CALIBRE_TABLES, REQUIRED_CALIBRE_TABLES)
        assert result == []


class TestConnectDatabase:
    def test_success_read_only(self, calibre_db_file):
        with connect_database(calibre_db_file, read_only=True) as conn:
            row = conn.execute("SELECT COUNT(*) FROM books").fetchone()
            assert row[0] == 2

    def test_missing_file_raises(self, tmp_path):
        import click

        with pytest.raises(click.ClickException), connect_database(tmp_path / "nonexistent.db"):
            pass

    def test_row_factory_set(self, calibre_db_file):
        with connect_database(calibre_db_file, read_only=True) as conn:
            row = conn.execute("SELECT id, title FROM books WHERE id = 1").fetchone()
            assert row["id"] == 1
            assert row["title"] == "Test Book"

    def test_write_connection(self, calibre_db_file):
        with connect_database(calibre_db_file, read_only=False) as conn:
            conn.execute("INSERT INTO tags (name) VALUES (?)", ("write-test-tag",))
            conn.commit()

        with connect_database(calibre_db_file, read_only=True) as conn:
            row = conn.execute("SELECT name FROM tags WHERE name = 'write-test-tag'").fetchone()
            assert row is not None


class TestSchemaValidation:
    def test_schema_all_tables_present(self, calibre_db):
        existing = [
            r[0]
            for r in calibre_db.execute(
                "SELECT name FROM sqlite_master WHERE type='table'"
            ).fetchall()
        ]
        missing = check_required_tables(existing, REQUIRED_CALIBRE_TABLES)
        assert missing == []

    @pytest.mark.parametrize("table", REQUIRED_CALIBRE_TABLES)
    def test_each_required_table_exists(self, calibre_db, table):
        row = calibre_db.execute(
            "SELECT name FROM sqlite_master WHERE type='table' AND name=?", (table,)
        ).fetchone()
        assert row is not None, f"Table '{table}' missing"


class TestGetOrCreate:
    def test_get_existing_author(self, calibre_db):
        assert get_or_create_author(calibre_db, "John Doe") == 1

    def test_create_new_author(self, calibre_db):
        author_id = get_or_create_author(calibre_db, "New Author")
        assert author_id > 2
        assert (
            calibre_db.execute("SELECT name FROM authors WHERE id = ?", (author_id,)).fetchone()[
                "name"
            ]
            == "New Author"
        )

    def test_get_existing_tag(self, calibre_db):
        assert get_or_create_tag(calibre_db, "fiction") == 1

    def test_create_new_tag(self, calibre_db):
        tag_id = get_or_create_tag(calibre_db, "new-tag")
        assert tag_id > 3

    def test_get_existing_series(self, calibre_db):
        assert get_or_create_series(calibre_db, "Test Series") == 1

    def test_create_new_series(self, calibre_db):
        sid = get_or_create_series(calibre_db, "New Series")
        assert sid > 1

    def test_get_existing_publisher(self, calibre_db):
        assert get_or_create_publisher(calibre_db, "Test Publisher") == 1

    def test_create_new_publisher(self, calibre_db):
        pid = get_or_create_publisher(calibre_db, "New Publisher")
        assert pid > 1

    def test_get_or_create_rating_idempotent(self, calibre_db):
        rid1 = get_or_create_rating(calibre_db, 8)
        rid2 = get_or_create_rating(calibre_db, 8)
        assert rid1 == rid2

    def test_get_or_create_rating_even_rounding(self, calibre_db):
        # Odd rating should be rounded to even
        rid = get_or_create_rating(calibre_db, 7)
        assert rid > 0
        row = calibre_db.execute("SELECT rating FROM ratings WHERE id = ?", (rid,)).fetchone()
        assert row["rating"] % 2 == 0

    def test_get_or_create_rating_clamp_high(self, calibre_db):
        rid = get_or_create_rating(calibre_db, 15)
        row = calibre_db.execute("SELECT rating FROM ratings WHERE id = ?", (rid,)).fetchone()
        assert row["rating"] <= 10

    def test_get_or_create_rating_clamp_low(self, calibre_db):
        rid = get_or_create_rating(calibre_db, -5)
        row = calibre_db.execute("SELECT rating FROM ratings WHERE id = ?", (rid,)).fetchone()
        assert row["rating"] == 0


class TestListAllTags:
    def test_lists_tags_with_books(self, calibre_db):
        tags = list_all_tags(calibre_db, sort_by="name", min_books=0)
        names = [t["name"] for t in tags]
        assert "fiction" in names
        assert "science" in names
        assert "orphaned-tag" in names

    def test_min_books_filters_orphaned(self, calibre_db):
        tags = list_all_tags(calibre_db, sort_by="name", min_books=1)
        names = [t["name"] for t in tags]
        assert "orphaned-tag" not in names
        assert "fiction" in names

    def test_sort_by_count(self, calibre_db):
        tags = list_all_tags(calibre_db, sort_by="count", min_books=0)
        counts = [t["book_count"] for t in tags]
        assert counts == sorted(counts, reverse=True)

    def test_book_count_field(self, calibre_db):
        tags = list_all_tags(calibre_db, sort_by="name", min_books=0)
        fiction = next(t for t in tags if t["name"] == "fiction")
        assert fiction["book_count"] == 1


class TestConnectDatabaseWriteMode:
    def test_write_connection_registers_custom_functions(self, calibre_db_file):
        with connect_database(calibre_db_file, read_only=False) as conn:
            # title_sort SQLite function should be registered
            row = conn.execute("SELECT title_sort('The Great Gatsby') as s").fetchone()
            assert row["s"] == "great gatsby"

    def test_title_sort_strips_article_a(self, calibre_db_file):
        with connect_database(calibre_db_file, read_only=False) as conn:
            row = conn.execute("SELECT title_sort('A Room with a View') as s").fetchone()
            assert row["s"] == "room with a view"

    def test_title_sort_empty_string(self, calibre_db_file):
        with connect_database(calibre_db_file, read_only=False) as conn:
            row = conn.execute("SELECT title_sort('') as s").fetchone()
            assert row["s"] == ""


class TestComputeFileHash:
    def test_sha256(self, tmp_path):
        f = tmp_path / "data.bin"
        f.write_bytes(b"hello world")
        result = compute_file_hash(f, "sha256")
        assert result is not None
        assert len(result) == 64

    def test_md5(self, tmp_path):
        f = tmp_path / "data.bin"
        f.write_bytes(b"hello world")
        result = compute_file_hash(f, "md5")
        assert result is not None
        assert len(result) == 32

    def test_missing_file_returns_none(self, tmp_path):
        assert compute_file_hash(tmp_path / "nonexistent.bin") is None

    def test_directory_returns_none(self, tmp_path):
        assert compute_file_hash(tmp_path) is None

    def test_same_content_same_hash(self, tmp_path):
        f1 = tmp_path / "a.bin"
        f2 = tmp_path / "b.bin"
        f1.write_bytes(b"identical")
        f2.write_bytes(b"identical")
        assert compute_file_hash(f1) == compute_file_hash(f2)

    def test_different_content_different_hash(self, tmp_path):
        f1 = tmp_path / "a.bin"
        f2 = tmp_path / "b.bin"
        f1.write_bytes(b"aaa")
        f2.write_bytes(b"bbb")
        assert compute_file_hash(f1) != compute_file_hash(f2)


class TestQueryBookById:
    def test_existing_book(self, calibre_db_file):
        book = query_book_by_id(calibre_db_file, 1)
        assert book is not None
        assert book["title"] == "Test Book"

    def test_nonexistent_book(self, calibre_db_file):
        book = query_book_by_id(calibre_db_file, 9999)
        assert book is None

    def test_book_has_authors(self, calibre_db_file):
        book = query_book_by_id(calibre_db_file, 1)
        assert book is not None
        assert "authors" in book


class TestListBooksQuery:
    def test_all_books(self, calibre_db):
        books = list_books_query(calibre_db)
        assert len(books) == 2

    def test_filter_by_tag(self, calibre_db):
        books = list_books_query(calibre_db, tag_name="fiction")
        assert len(books) == 1
        assert books[0]["title"] == "Test Book"

    def test_filter_by_nonexistent_tag(self, calibre_db):
        books = list_books_query(calibre_db, tag_name="nonexistent")
        assert books == []

    def test_filter_by_author(self, calibre_db):
        books = list_books_query(calibre_db, author_name="John Doe")
        assert len(books) == 1
        assert books[0]["title"] == "Test Book"

    def test_limit(self, calibre_db):
        books = list_books_query(calibre_db, limit=1)
        assert len(books) == 1


class TestDetectIsbnDuplicates:
    def test_no_duplicates(self, calibre_db):
        calibre_db.execute(
            "INSERT INTO identifiers (book, type, val) VALUES (1, 'isbn', '9780596005900')"
        )
        result = detect_isbn_duplicates(calibre_db)
        assert result == []

    def test_finds_isbn_duplicate(self, calibre_db):
        isbn = "9780596005900"
        calibre_db.execute(
            "INSERT INTO identifiers (book, type, val) VALUES (1, 'isbn', ?)", (isbn,)
        )
        calibre_db.execute(
            "INSERT INTO identifiers (book, type, val) VALUES (2, 'isbn', ?)", (isbn,)
        )
        result = detect_isbn_duplicates(calibre_db)
        assert len(result) == 1
        assert result[0]["isbn"] == isbn
        assert result[0]["count"] == 2

    def test_finds_isbn_duplicate_via_books_column(self, calibre_db):
        # Duplicate via books.isbn column (second query path)
        isbn = "9780596005901"
        calibre_db.execute("UPDATE books SET isbn = ? WHERE id = 1", (isbn,))
        calibre_db.execute("UPDATE books SET isbn = ? WHERE id = 2", (isbn,))
        result = detect_isbn_duplicates(calibre_db)
        assert any(r["isbn"] == isbn for r in result)

    def test_no_isbn_duplicates_returns_empty(self, calibre_db):
        # Each book has a unique ISBN in books.isbn column
        calibre_db.execute("UPDATE books SET isbn = '9780596005900' WHERE id = 1")
        calibre_db.execute("UPDATE books SET isbn = '9780596005918' WHERE id = 2")
        result = detect_isbn_duplicates(calibre_db)
        assert result == []


class TestGetBookFilePath:
    def test_returns_none_for_missing_book(self, calibre_db_file):
        result = get_book_file_path(calibre_db_file, 9999)
        assert result is None

    def test_returns_none_when_no_data(self, calibre_db_file):
        # Book exists but has no data entries
        result = get_book_file_path(calibre_db_file, 1)
        assert result is None

    def test_returns_epub_path(self, calibre_db_file, tmp_path):
        import sqlite3 as sq

        conn = sq.connect(str(calibre_db_file))
        conn.execute(
            "INSERT INTO data (book, format, uncompressed_size, name) VALUES (1, 'EPUB', 1000, 'Test Book')"
        )
        conn.commit()
        conn.close()
        result = get_book_file_path(calibre_db_file, 1)
        assert result is not None
        assert str(result).endswith(".epub")

    def test_prefers_preferred_format(self, calibre_db_file):
        import sqlite3 as sq

        conn = sq.connect(str(calibre_db_file))
        conn.execute(
            "INSERT INTO data (book, format, uncompressed_size, name) VALUES (1, 'EPUB', 1000, 'Test Book')"
        )
        conn.execute(
            "INSERT INTO data (book, format, uncompressed_size, name) VALUES (1, 'PDF', 2000, 'Test Book')"
        )
        conn.commit()
        conn.close()
        result = get_book_file_path(calibre_db_file, 1, preferred_format="PDF")
        assert result is not None
        assert str(result).endswith(".pdf")
