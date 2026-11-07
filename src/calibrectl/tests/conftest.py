"""
Shared pytest fixtures for calibrectl tests.
"""

import sqlite3

import pytest


def _create_test_calibre_db(conn: sqlite3.Connection) -> None:
    """Populate a Calibre-like schema in the given connection."""
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
        CREATE TABLE authors (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            sort TEXT,
            link TEXT DEFAULT ''
        );
        CREATE TABLE books_authors_link (
            id INTEGER PRIMARY KEY,
            book INTEGER,
            author INTEGER
        );
        CREATE TABLE tags (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL
        );
        CREATE TABLE books_tags_link (
            id INTEGER PRIMARY KEY,
            book INTEGER,
            tag INTEGER
        );
        CREATE TABLE series (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            sort TEXT
        );
        CREATE TABLE books_series_link (
            id INTEGER PRIMARY KEY,
            book INTEGER,
            series INTEGER
        );
        CREATE TABLE publishers (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            sort TEXT
        );
        CREATE TABLE books_publishers_link (
            id INTEGER PRIMARY KEY,
            book INTEGER,
            publisher INTEGER
        );
        CREATE TABLE ratings (
            id INTEGER PRIMARY KEY,
            rating INTEGER
        );
        CREATE TABLE books_ratings_link (
            id INTEGER PRIMARY KEY,
            book INTEGER,
            rating INTEGER
        );
        CREATE TABLE data (
            id INTEGER PRIMARY KEY,
            book INTEGER,
            format TEXT,
            uncompressed_size INTEGER,
            name TEXT
        );
        CREATE TABLE comments (
            id INTEGER PRIMARY KEY,
            book INTEGER,
            text TEXT
        );
        CREATE TABLE identifiers (
            id INTEGER PRIMARY KEY,
            book INTEGER,
            type TEXT,
            val TEXT
        );
        CREATE TABLE languages (
            id INTEGER PRIMARY KEY,
            lang_code TEXT
        );
        CREATE TABLE books_languages_link (
            id INTEGER PRIMARY KEY,
            book INTEGER,
            lang_code INTEGER
        );
        CREATE TABLE library_id (
            id INTEGER PRIMARY KEY,
            uuid TEXT
        );

        INSERT INTO books (id, title, path, timestamp, last_modified)
            VALUES (1, 'Test Book', 'Author/Test Book (1)',
                    '2024-01-01 00:00:00+00:00', '2024-06-15 10:00:00+00:00');
        INSERT INTO books (id, title, path, timestamp, last_modified)
            VALUES (2, 'Another Book', 'Author/Another Book (2)',
                    '2024-03-01 00:00:00+00:00', '2024-07-01 12:00:00+00:00');

        INSERT INTO authors (id, name, sort) VALUES (1, 'John Doe', 'Doe, John');
        INSERT INTO authors (id, name, sort) VALUES (2, 'Jane Smith', 'Smith, Jane');
        INSERT INTO books_authors_link (book, author) VALUES (1, 1);
        INSERT INTO books_authors_link (book, author) VALUES (2, 2);

        INSERT INTO tags (id, name) VALUES (1, 'fiction');
        INSERT INTO tags (id, name) VALUES (2, 'science');
        INSERT INTO tags (id, name) VALUES (3, 'orphaned-tag');
        INSERT INTO books_tags_link (book, tag) VALUES (1, 1);
        INSERT INTO books_tags_link (book, tag) VALUES (1, 2);

        INSERT INTO series (id, name, sort) VALUES (1, 'Test Series', 'Test Series');
        INSERT INTO books_series_link (book, series) VALUES (1, 1);

        INSERT INTO publishers (id, name, sort) VALUES (1, 'Test Publisher', 'Test Publisher');
        INSERT INTO books_publishers_link (book, publisher) VALUES (1, 1);
    """)


@pytest.fixture
def calibre_db():
    """In-memory Calibre-like SQLite database for testing."""
    conn = sqlite3.connect(":memory:")
    conn.row_factory = sqlite3.Row
    _create_test_calibre_db(conn)
    yield conn
    conn.close()


@pytest.fixture
def calibre_db_file(tmp_path):
    """File-backed Calibre-like SQLite database for testing."""
    db_path = tmp_path / "metadata.db"
    conn = sqlite3.connect(str(db_path))
    conn.row_factory = sqlite3.Row
    _create_test_calibre_db(conn)
    conn.commit()
    conn.close()
    return db_path
