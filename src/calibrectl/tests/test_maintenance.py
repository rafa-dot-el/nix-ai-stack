"""
Tests for calibrectl.maintenance
"""

import sqlite3
from pathlib import Path

from calibrectl.maintenance import apply_retag, find_duplicates_by_hash


def _make_db(tmp_path: Path) -> Path:
    """Create a minimal file-backed Calibre DB."""
    db_path = tmp_path / "metadata.db"
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

        INSERT INTO books (id, title, path, timestamp, last_modified)
            VALUES (1, 'Book A', 'A/Book A (1)', '2024-01-01 00:00:00+00:00', '2024-01-01 00:00:00+00:00');
        INSERT INTO books (id, title, path, timestamp, last_modified)
            VALUES (2, 'Book B', 'B/Book B (2)', '2024-01-02 00:00:00+00:00', '2024-01-02 00:00:00+00:00');
        INSERT INTO books (id, title, path, timestamp, last_modified)
            VALUES (3, 'Book C', 'C/Book C (3)', '2024-01-03 00:00:00+00:00', '2024-01-03 00:00:00+00:00');

        INSERT INTO authors (id, name, sort) VALUES (1, 'Author A', 'A');
        INSERT INTO books_authors_link (book, author) VALUES (1, 1);
        INSERT INTO books_authors_link (book, author) VALUES (2, 1);
        INSERT INTO books_authors_link (book, author) VALUES (3, 1);

        INSERT INTO tags (id, name) VALUES (1, 'horror');
        INSERT INTO tags (id, name) VALUES (2, 'ghost');
        INSERT INTO tags (id, name) VALUES (3, 'fiction');
        INSERT INTO tags (id, name) VALUES (4, 'rare-tag');
        INSERT INTO books_tags_link (book, tag) VALUES (1, 1);
        INSERT INTO books_tags_link (book, tag) VALUES (2, 2);
        INSERT INTO books_tags_link (book, tag) VALUES (3, 3);
        INSERT INTO books_tags_link (book, tag) VALUES (1, 4);
    """)
    conn.commit()
    conn.close()
    return db_path


class TestApplyRetag:
    def test_dry_run_no_changes(self, tmp_path):
        db = _make_db(tmp_path)
        mappings = {"horror": "Horror", "rare-tag": None}
        stats = apply_retag(db, mappings, dry_run=True, backup=False)

        assert stats["dry_run"] is True
        # DB should be unchanged
        conn = sqlite3.connect(str(db))
        tags = [r[0] for r in conn.execute("SELECT name FROM tags").fetchall()]
        conn.close()
        assert "horror" in tags
        assert "rare-tag" in tags

    def test_dry_run_reports_would_delete(self, tmp_path):
        db = _make_db(tmp_path)
        stats = apply_retag(db, {"rare-tag": None}, dry_run=True, backup=False)
        assert "rare-tag" in stats["would_delete"]

    def test_dry_run_reports_would_consolidate(self, tmp_path):
        db = _make_db(tmp_path)
        stats = apply_retag(db, {"ghost": "Horror"}, dry_run=True, backup=False)
        assert "ghost" in stats.get("would_consolidate", {})

    def test_delete_tag(self, tmp_path):
        db = _make_db(tmp_path)
        apply_retag(db, {"rare-tag": None}, dry_run=False, backup=False)

        conn = sqlite3.connect(str(db))
        tags = [r[0] for r in conn.execute("SELECT name FROM tags").fetchall()]
        conn.close()
        assert "rare-tag" not in tags

    def test_delete_removes_links(self, tmp_path):
        db = _make_db(tmp_path)
        # Verify link exists before
        conn = sqlite3.connect(str(db))
        link_before = conn.execute(
            "SELECT COUNT(*) FROM books_tags_link btl JOIN tags t ON btl.tag=t.id WHERE t.name='rare-tag'"
        ).fetchone()[0]
        conn.close()
        assert link_before == 1

        apply_retag(db, {"rare-tag": None}, dry_run=False, backup=False)

        conn = sqlite3.connect(str(db))
        link_after = conn.execute(
            "SELECT COUNT(*) FROM books_tags_link btl JOIN tags t ON btl.tag=t.id WHERE t.name='rare-tag'"
        ).fetchone()[0]
        conn.close()
        assert link_after == 0

    def test_consolidate_tag(self, tmp_path):
        db = _make_db(tmp_path)
        # ghost -> horror; both books 1 (horror) and 2 (ghost) should end up with Horror
        apply_retag(db, {"ghost": "Horror", "horror": "Horror"}, dry_run=False, backup=False)

        conn = sqlite3.connect(str(db))
        tags = [r[0] for r in conn.execute("SELECT name FROM tags").fetchall()]
        # ghost and horror should be gone
        assert "ghost" not in tags
        assert "horror" not in tags
        # Horror should exist
        assert "Horror" in tags
        conn.close()

    def test_consolidate_moves_books(self, tmp_path):
        db = _make_db(tmp_path)
        apply_retag(db, {"ghost": "Horror"}, dry_run=False, backup=False)

        conn = sqlite3.connect(str(db))
        # Book 2 had tag ghost; now should have Horror
        tags_for_book2 = [
            r[0]
            for r in conn.execute("""
                SELECT t.name FROM tags t
                JOIN books_tags_link btl ON t.id = btl.tag
                WHERE btl.book = 2
            """).fetchall()
        ]
        conn.close()
        assert "Horror" in tags_for_book2
        assert "ghost" not in tags_for_book2

    def test_unknown_tag_in_mapping_ignored(self, tmp_path):
        db = _make_db(tmp_path)
        # non-existent tag in mapping — should not raise
        stats = apply_retag(db, {"nonexistent-tag": "Horror"}, dry_run=False, backup=False)
        assert stats["tags_consolidated"] == 0

    def test_stats_returned(self, tmp_path):
        db = _make_db(tmp_path)
        stats = apply_retag(db, {"rare-tag": None}, dry_run=False, backup=False)
        assert stats["tags_deleted"] == 1
        assert stats["tag_associations_deleted"] == 1


class TestFindDuplicatesByHash:
    def _make_db_with_data(self, tmp_path: Path) -> Path:
        import sqlite3 as sq

        db_path = tmp_path / "metadata.db"
        conn = sq.connect(str(db_path))
        conn.row_factory = sq.Row
        conn.executescript("""
            CREATE TABLE books (
                id INTEGER PRIMARY KEY,
                title TEXT NOT NULL DEFAULT '',
                path TEXT NOT NULL DEFAULT '',
                timestamp TEXT DEFAULT ''
            );
            CREATE TABLE data (
                id INTEGER PRIMARY KEY,
                book INTEGER,
                format TEXT,
                uncompressed_size INTEGER DEFAULT 0,
                name TEXT
            );
        """)
        conn.execute(
            "INSERT INTO books (id, title, path) VALUES (1, 'Book A', 'AuthorA/Book A (1)')"
        )
        conn.execute(
            "INSERT INTO books (id, title, path) VALUES (2, 'Book B', 'AuthorB/Book B (2)')"
        )
        conn.execute(
            "INSERT INTO data (book, format, uncompressed_size, name) VALUES (1, 'EPUB', 1000, 'Book A')"
        )
        conn.execute(
            "INSERT INTO data (book, format, uncompressed_size, name) VALUES (2, 'EPUB', 1000, 'Book B')"
        )
        conn.commit()
        conn.close()
        return db_path

    def test_no_duplicates_when_files_differ(self, tmp_path):
        db = self._make_db_with_data(tmp_path)
        storage = tmp_path
        # Create distinct files
        book_dir_a = storage / "AuthorA" / "Book A (1)"
        book_dir_b = storage / "AuthorB" / "Book B (2)"
        book_dir_a.mkdir(parents=True)
        book_dir_b.mkdir(parents=True)
        (book_dir_a / "Book A.epub").write_bytes(b"content_a")
        (book_dir_b / "Book B.epub").write_bytes(b"content_b")

        dupes, total = find_duplicates_by_hash(db, storage)
        assert dupes == {}
        assert total == 2

    def test_finds_duplicates_when_content_identical(self, tmp_path):
        db = self._make_db_with_data(tmp_path)
        storage = tmp_path
        book_dir_a = storage / "AuthorA" / "Book A (1)"
        book_dir_b = storage / "AuthorB" / "Book B (2)"
        book_dir_a.mkdir(parents=True)
        book_dir_b.mkdir(parents=True)
        identical_content = b"same content for both"
        (book_dir_a / "Book A.epub").write_bytes(identical_content)
        (book_dir_b / "Book B.epub").write_bytes(identical_content)

        dupes, total = find_duplicates_by_hash(db, storage)
        assert len(dupes) == 1
        assert total == 2

    def test_missing_files_are_skipped(self, tmp_path):
        db = self._make_db_with_data(tmp_path)
        storage = tmp_path
        # Don't create any actual files
        dupes, total = find_duplicates_by_hash(db, storage)
        assert dupes == {}
        assert total == 2
