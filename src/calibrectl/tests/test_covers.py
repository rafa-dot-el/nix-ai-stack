"""
Tests for calibrectl.covers
"""

import sqlite3

import click
import pytest

from calibrectl.covers import set_cover_for_book


class TestSetCoverForBook:
    def _setup_db_and_storage(self, tmp_path):
        """Create a minimal database and book directory."""
        db_path = tmp_path / "metadata.db"
        storage_path = tmp_path / "storage"
        book_dir = storage_path / "Author" / "Test Book (1)"
        book_dir.mkdir(parents=True)

        conn = sqlite3.connect(str(db_path))
        conn.row_factory = sqlite3.Row
        conn.executescript("""
            CREATE TABLE books (
                id INTEGER PRIMARY KEY,
                title TEXT NOT NULL,
                sort TEXT,
                isbn TEXT DEFAULT '',
                pubdate TEXT,
                timestamp TEXT,
                last_modified TEXT,
                path TEXT NOT NULL DEFAULT '',
                has_cover INTEGER DEFAULT 0,
                series_index REAL DEFAULT 1.0,
                uuid TEXT DEFAULT ''
            );
            CREATE TABLE authors (id INTEGER PRIMARY KEY, name TEXT NOT NULL, sort TEXT, link TEXT DEFAULT '');
            CREATE TABLE books_authors_link (id INTEGER PRIMARY KEY, book INTEGER, author INTEGER);
            CREATE TABLE tags (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
            CREATE TABLE books_tags_link (id INTEGER PRIMARY KEY, book INTEGER, tag INTEGER);
            CREATE TABLE series (id INTEGER PRIMARY KEY, name TEXT NOT NULL, sort TEXT);
            CREATE TABLE books_series_link (id INTEGER PRIMARY KEY, book INTEGER, series INTEGER);
            CREATE TABLE publishers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, sort TEXT);
            CREATE TABLE books_publishers_link (id INTEGER PRIMARY KEY, book INTEGER, publisher INTEGER);
            CREATE TABLE ratings (id INTEGER PRIMARY KEY, rating INTEGER);
            CREATE TABLE books_ratings_link (id INTEGER PRIMARY KEY, book INTEGER, rating INTEGER);
            CREATE TABLE data (id INTEGER PRIMARY KEY, book INTEGER, format TEXT, uncompressed_size INTEGER, name TEXT);
            CREATE TABLE comments (id INTEGER PRIMARY KEY, book INTEGER, text TEXT);
            CREATE TABLE identifiers (id INTEGER PRIMARY KEY, book INTEGER, type TEXT, val TEXT);
            INSERT INTO books (id, title, path, timestamp, last_modified, has_cover)
                VALUES (1, 'Test Book', 'Author/Test Book (1)',
                        '2024-01-01 00:00:00+00:00', '2024-01-01 00:00:00+00:00', 0);
            INSERT INTO authors (id, name, sort) VALUES (1, 'Author', 'Author');
            INSERT INTO books_authors_link (book, author) VALUES (1, 1);
        """)
        conn.commit()
        conn.close()

        return db_path, storage_path, book_dir

    def test_success_jpg(self, tmp_path, monkeypatch):
        db_path, storage_path, book_dir = self._setup_db_and_storage(tmp_path)

        # Patch get_storage_path to return our storage_path
        import calibrectl.covers as covers_module

        monkeypatch.setattr(covers_module, "get_storage_path", lambda _: storage_path)

        cover = tmp_path / "cover.jpg"
        cover.write_bytes(b"\xff\xd8\xff")  # minimal JPEG header

        set_cover_for_book(db_path, 1, cover)

        dest = book_dir / "cover.jpg"
        assert dest.exists()

        # Verify database updated
        conn = sqlite3.connect(str(db_path))
        row = conn.execute("SELECT has_cover FROM books WHERE id = 1").fetchone()
        conn.close()
        assert row[0] == 1

    def test_success_png(self, tmp_path, monkeypatch):
        db_path, storage_path, book_dir = self._setup_db_and_storage(tmp_path)

        import calibrectl.covers as covers_module

        monkeypatch.setattr(covers_module, "get_storage_path", lambda _: storage_path)

        cover = tmp_path / "cover.png"
        cover.write_bytes(b"\x89PNG")

        set_cover_for_book(db_path, 1, cover)

        dest = book_dir / "cover.jpg"
        assert dest.exists()

    def test_book_not_found(self, tmp_path, monkeypatch):
        db_path, storage_path, book_dir = self._setup_db_and_storage(tmp_path)

        import calibrectl.covers as covers_module

        monkeypatch.setattr(covers_module, "get_storage_path", lambda _: storage_path)

        cover = tmp_path / "cover.jpg"
        cover.write_bytes(b"\xff\xd8\xff")

        with pytest.raises(click.ClickException, match="not found"):
            set_cover_for_book(db_path, 999, cover)

    def test_invalid_extension_raises(self, tmp_path, monkeypatch):
        db_path, storage_path, book_dir = self._setup_db_and_storage(tmp_path)

        import calibrectl.covers as covers_module

        monkeypatch.setattr(covers_module, "get_storage_path", lambda _: storage_path)

        cover = tmp_path / "cover.txt"
        cover.write_text("not an image")

        with pytest.raises(click.ClickException, match="Invalid image"):
            set_cover_for_book(db_path, 1, cover)

    def test_book_directory_missing(self, tmp_path, monkeypatch):
        db_path, storage_path, book_dir = self._setup_db_and_storage(tmp_path)

        import calibrectl.covers as covers_module

        # Point to a storage dir where the book subdir doesn't exist
        empty_storage = tmp_path / "empty"
        empty_storage.mkdir()
        monkeypatch.setattr(covers_module, "get_storage_path", lambda _: empty_storage)

        cover = tmp_path / "cover.jpg"
        cover.write_bytes(b"\xff\xd8\xff")

        with pytest.raises(click.ClickException, match="directory not found"):
            set_cover_for_book(db_path, 1, cover)
