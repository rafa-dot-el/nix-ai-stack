"""
Tests for zoteroctl.db module.
"""

import sqlite3

import pytest

from zoteroctl.db import (
    REQUIRED_ZOTERO_TABLES,
    check_required_tables,
    connect_database,
    load_schema,
)
from zoteroctl.validation import classify_integrity_result


class TestCheckRequiredTables:
    def test_all_present(self):
        assert check_required_tables(["items", "tags", "creators"], ["items", "tags"]) == []

    def test_some_missing(self):
        missing = check_required_tables(["items"], ["items", "tags", "creators"])
        assert "creators" in missing
        assert "tags" in missing
        assert "items" not in missing

    def test_empty_existing(self):
        assert check_required_tables([], ["items"]) == ["items"]

    def test_empty_required(self):
        assert check_required_tables(["items", "tags"], []) == []

    def test_result_sorted(self):
        missing = check_required_tables([], ["z_table", "a_table", "m_table"])
        assert missing == sorted(missing)

    def test_full_schema_against_in_memory_db(self, zotero_db):
        existing = [
            r[0]
            for r in zotero_db.execute(
                "SELECT name FROM sqlite_master WHERE type='table'"
            ).fetchall()
        ]
        # The in-memory test DB should have all required tables
        missing = check_required_tables(existing, REQUIRED_ZOTERO_TABLES)
        assert missing == []


class TestConnectDatabase:
    def test_read_only_connection(self, zotero_db_path):
        with connect_database(zotero_db_path, read_only=True) as conn:
            result = conn.execute("SELECT COUNT(*) FROM itemTypes").fetchone()[0]
            assert result > 0

    def test_read_only_blocks_writes(self, zotero_db_path):
        with (
            pytest.raises(sqlite3.OperationalError),
            connect_database(zotero_db_path, read_only=True) as conn,
        ):
            conn.execute("INSERT INTO tags (name) VALUES ('test')")

    def test_write_connection_commits(self, tmp_path):
        db_path = tmp_path / "test.sqlite"
        conn_init = sqlite3.connect(str(db_path))
        conn_init.execute("CREATE TABLE tags (tagID INTEGER PRIMARY KEY, name TEXT NOT NULL)")
        conn_init.commit()
        conn_init.close()

        with connect_database(db_path, read_only=False) as conn:
            conn.execute("INSERT INTO tags (name) VALUES ('hello')")

        verify = sqlite3.connect(str(db_path))
        count = verify.execute("SELECT COUNT(*) FROM tags").fetchone()[0]
        verify.close()
        assert count == 1

    def test_row_factory_set(self, zotero_db_path):
        with connect_database(zotero_db_path, read_only=True) as conn:
            row = conn.execute("SELECT typeName FROM itemTypes LIMIT 1").fetchone()
            assert row["typeName"] is not None

    def test_missing_file_raises(self, tmp_path):
        missing = tmp_path / "nonexistent.sqlite"
        with (
            pytest.raises(sqlite3.OperationalError),
            connect_database(missing, read_only=True) as conn,
        ):
            conn.execute("SELECT 1")

    def test_write_rollback_on_exception(self, tmp_path):
        db_path = tmp_path / "test.sqlite"
        conn_init = sqlite3.connect(str(db_path))
        conn_init.execute("CREATE TABLE tags (tagID INTEGER PRIMARY KEY, name TEXT NOT NULL)")
        conn_init.commit()
        conn_init.close()

        with pytest.raises(ValueError), connect_database(db_path, read_only=False) as conn:
            conn.execute("INSERT INTO tags (name) VALUES ('before_error')")
            raise ValueError("intentional error")

        verify = sqlite3.connect(str(db_path))
        count = verify.execute("SELECT COUNT(*) FROM tags").fetchone()[0]
        verify.close()
        assert count == 0


class TestLoadSchema:
    def test_loads_item_types(self, zotero_db_path):
        from zoteroctl import db as db_module

        db_module._schema_cache = None  # clear cache
        schema = load_schema(zotero_db_path)
        assert "book" in schema["itemTypes"]
        assert "journalArticle" in schema["itemTypes"]

    def test_loads_fields(self, zotero_db_path):
        from zoteroctl import db as db_module

        db_module._schema_cache = None
        schema = load_schema(zotero_db_path)
        assert "title" in schema["fields"]
        assert "date" in schema["fields"]

    def test_loads_creator_types(self, zotero_db_path):
        from zoteroctl import db as db_module

        db_module._schema_cache = None
        schema = load_schema(zotero_db_path)
        assert "author" in schema["creatorTypes"]

    def test_loads_item_type_fields(self, zotero_db_path):
        from zoteroctl import db as db_module

        db_module._schema_cache = None
        schema = load_schema(zotero_db_path)
        # book typeID is 1 in test fixture
        assert 1 in schema["itemTypeFields"]

    def test_schema_cache(self, zotero_db_path):
        from zoteroctl import db as db_module

        db_module._schema_cache = None
        s1 = load_schema(zotero_db_path)
        s2 = load_schema(zotero_db_path)
        assert s1 is s2  # same object due to cache

    def test_integrity_check_passes(self, zotero_db):
        result = zotero_db.execute("PRAGMA integrity_check").fetchone()[0]
        assert classify_integrity_result(result) == "ok"

    def teardown_method(self, method):
        """Clear schema cache after each test."""
        from zoteroctl import db as db_module

        db_module._schema_cache = None
