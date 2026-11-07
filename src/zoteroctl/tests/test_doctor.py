"""
Tests for zoteroctl.doctor module.
"""

import sqlite3

import pytest

from zoteroctl.cli import add_or_update_item
from zoteroctl.collections import add_item_to_collection, create_collection_record
from zoteroctl.doctor import (
    detect_broken_references,
    detect_missing_files,
    detect_orphaned_items,
    generate_diagnostic_report,
    run_database_diagnostics,
)


def _insert_book(conn, title="Book", with_collection=True):
    """Insert a book and optionally add it to a collection. Returns item_key."""
    item_key = add_or_update_item(
        conn,
        {
            "itemType": "book",
            "fields": {"title": title},
            "creators": [],
            "tags": [],
        },
    )
    if with_collection:
        coll_key = create_collection_record(conn, f"Coll-{title}")
        add_item_to_collection(conn, item_key, coll_key)
    return item_key


# ---------------------------------------------------------------------------
# detect_orphaned_items
# ---------------------------------------------------------------------------


class TestDetectOrphanedItems:
    def test_no_orphans(self):
        items = [{"itemKey": "A", "title": "T", "itemType": "book", "collections": ["COL1"]}]
        assert detect_orphaned_items(items) == []

    def test_empty_collections_is_orphan(self):
        items = [{"itemKey": "B", "title": "T", "itemType": "book", "collections": []}]
        orphans = detect_orphaned_items(items)
        assert len(orphans) == 1
        assert orphans[0]["itemKey"] == "B"

    def test_missing_collections_key_is_orphan(self):
        items = [{"itemKey": "C", "title": "T", "itemType": "book"}]
        orphans = detect_orphaned_items(items)
        assert len(orphans) == 1

    def test_mixed(self):
        items = [
            {"itemKey": "A", "title": "T", "itemType": "book", "collections": ["COL1"]},
            {"itemKey": "B", "title": "T", "itemType": "book", "collections": []},
            {"itemKey": "C", "title": "T", "itemType": "book"},
        ]
        orphan_keys = [o["itemKey"] for o in detect_orphaned_items(items)]
        assert "A" not in orphan_keys
        assert "B" in orphan_keys
        assert "C" in orphan_keys

    def test_orphan_reason(self):
        items = [{"itemKey": "X", "title": "T", "itemType": "book"}]
        orphans = detect_orphaned_items(items)
        assert orphans[0]["reason"] == "No parent collection"


# ---------------------------------------------------------------------------
# detect_missing_files
# ---------------------------------------------------------------------------


class TestDetectMissingFiles:
    def test_no_missing(self, tmp_path):
        storage = tmp_path / "storage"
        key_dir = storage / "ATT12345"
        key_dir.mkdir(parents=True)
        (key_dir / "file.pdf").write_bytes(b"content")

        attachments = [{"itemKey": "ATT12345", "title": "PDF", "path": "storage:file.pdf"}]
        assert detect_missing_files(attachments, storage) == []

    def test_missing_file(self, tmp_path):
        storage = tmp_path / "storage"
        storage.mkdir()

        attachments = [{"itemKey": "ATT12345", "title": "PDF", "path": "storage:file.pdf"}]
        missing = detect_missing_files(attachments, storage)
        assert len(missing) == 1
        assert missing[0]["itemKey"] == "ATT12345"
        assert "file.pdf" in missing[0]["expectedPath"]

    def test_absolute_path(self, tmp_path):
        existing = tmp_path / "file.pdf"
        existing.write_bytes(b"x")
        attachments = [{"itemKey": "ATT", "title": "F", "path": str(existing)}]
        assert detect_missing_files(attachments, tmp_path) == []

    def test_absolute_path_missing(self, tmp_path):
        missing_path = tmp_path / "gone.pdf"
        attachments = [{"itemKey": "ATT", "title": "F", "path": str(missing_path)}]
        result = detect_missing_files(attachments, tmp_path)
        assert len(result) == 1


# ---------------------------------------------------------------------------
# detect_broken_references
# ---------------------------------------------------------------------------


class TestDetectBrokenReferences:
    def test_no_broken(self):
        items = [{"itemKey": "A", "creators": [{"creatorID": 1}, {"creatorID": 2}]}]
        assert detect_broken_references(items, [1, 2, 3]) == []

    def test_broken_reference(self):
        items = [{"itemKey": "A", "creators": [{"creatorID": 1}, {"creatorID": 999}]}]
        broken = detect_broken_references(items, [1, 2, 3])
        assert len(broken) == 1
        assert broken[0]["creatorID"] == 999

    def test_no_creators(self):
        items = [{"itemKey": "A", "creators": []}]
        assert detect_broken_references(items, [1]) == []

    def test_creator_without_id(self):
        items = [{"itemKey": "A", "creators": [{"firstName": "J"}]}]
        assert detect_broken_references(items, [1]) == []


# ---------------------------------------------------------------------------
# generate_diagnostic_report
# ---------------------------------------------------------------------------


class TestGenerateDiagnosticReport:
    @staticmethod
    def _make_issues(**overrides):
        base = {
            "orphanedItems": [],
            "missingFiles": [],
            "brokenReferences": [],
            "hashDuplicates": {},
            "identifierDuplicates": {},
            "titleDuplicates": [],
        }
        base.update(overrides)
        return base

    def test_healthy_report(self):
        report = generate_diagnostic_report(self._make_issues(), 100, 50)
        assert report["summary"]["status"] == "healthy"
        assert report["summary"]["totalItems"] == 100
        assert report["summary"]["totalAttachments"] == 50

    def test_issues_found_with_orphans(self):
        report = generate_diagnostic_report(
            self._make_issues(orphanedItems=[{"itemKey": "A"}]), 10, 5
        )
        assert report["summary"]["status"] == "issues_found"

    def test_issues_found_with_missing_files(self):
        report = generate_diagnostic_report(
            self._make_issues(missingFiles=[{"itemKey": "A"}]), 10, 5
        )
        assert report["summary"]["status"] == "issues_found"

    def test_issues_found_with_broken_refs(self):
        report = generate_diagnostic_report(
            self._make_issues(brokenReferences=[{"itemKey": "A"}]), 10, 5
        )
        assert report["summary"]["status"] == "issues_found"

    def test_duplicates_found(self):
        report = generate_diagnostic_report(
            self._make_issues(hashDuplicates={"h1": [{"key": "A"}, {"key": "B"}]}), 10, 5
        )
        assert report["summary"]["status"] == "duplicates_found"

    def test_summary_counts(self):
        report = generate_diagnostic_report(
            self._make_issues(
                orphanedItems=[{"itemKey": "X"}],
                missingFiles=[{"itemKey": "Y"}, {"itemKey": "Z"}],
            ),
            50,
            20,
        )
        assert report["summary"]["orphanedItems"] == 1
        assert report["summary"]["missingFiles"] == 2

    def test_issues_in_report(self):
        issues = self._make_issues(orphanedItems=[{"itemKey": "A"}])
        report = generate_diagnostic_report(issues, 1, 0)
        assert report["issues"]["orphanedItems"][0]["itemKey"] == "A"


# ---------------------------------------------------------------------------
# run_database_diagnostics (integration)
# ---------------------------------------------------------------------------


class TestRunDatabaseDiagnostics:
    def test_healthy_empty_db(self, zotero_db, storage_dir):
        report = run_database_diagnostics(zotero_db, storage_dir)
        assert "summary" in report
        assert report["summary"]["status"] in ("healthy", "duplicates_found", "issues_found")

    def test_healthy_db_with_items_in_collections(self, zotero_db, storage_dir):
        _insert_book(zotero_db, "Book A", with_collection=True)
        _insert_book(zotero_db, "Book B", with_collection=True)

        report = run_database_diagnostics(zotero_db, storage_dir)
        assert report["summary"]["totalItems"] == 2
        assert report["summary"]["orphanedItems"] == 0

    def test_orphaned_item_detected(self, zotero_db, storage_dir):
        _insert_book(zotero_db, "Orphan Book", with_collection=False)

        report = run_database_diagnostics(zotero_db, storage_dir)
        assert report["summary"]["orphanedItems"] > 0
        assert report["summary"]["status"] == "issues_found"

    def test_broken_table_raises(self, zotero_db, storage_dir):
        """Dropping a required table causes SQL errors in diagnostics."""
        zotero_db.execute("DROP TABLE creators")
        with pytest.raises(sqlite3.Error):
            run_database_diagnostics(zotero_db, storage_dir)
