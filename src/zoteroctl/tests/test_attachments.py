"""
Tests for zoteroctl.attachments module.
"""

import pytest

from zoteroctl.attachments import (
    add_file_attachment,
    delete_attachment,
    list_file_attachments,
)
from zoteroctl.cli import add_or_update_item


def _insert_book(conn, title="Test Book"):
    """Insert a book item and return its key."""
    item = {
        "itemType": "book",
        "fields": {"title": title},
        "creators": [],
        "tags": [],
    }
    return add_or_update_item(conn, item)


class TestAddFileAttachment:
    def test_success(self, zotero_db, storage_dir, tmp_path):
        parent_key = _insert_book(zotero_db)
        pdf = tmp_path / "paper.pdf"
        pdf.write_bytes(b"%PDF-1.4 test")

        attachment_key = add_file_attachment(zotero_db, parent_key, pdf, storage_dir)
        assert len(attachment_key) == 8
        assert attachment_key.isupper() or attachment_key.isalnum()

        # File should exist in storage
        dest = storage_dir / attachment_key / "paper.pdf"
        assert dest.exists()

    def test_creates_db_record(self, zotero_db, storage_dir, tmp_path):
        parent_key = _insert_book(zotero_db)
        pdf = tmp_path / "doc.pdf"
        pdf.write_bytes(b"content")

        att_key = add_file_attachment(zotero_db, parent_key, pdf, storage_dir)

        row = zotero_db.execute(
            "SELECT * FROM itemAttachments ia JOIN items i ON ia.itemID = i.itemID WHERE i.key = ?",
            (att_key,),
        ).fetchone()
        assert row is not None
        assert row["linkMode"] == 0

    def test_custom_title(self, zotero_db, storage_dir, tmp_path):
        parent_key = _insert_book(zotero_db)
        pdf = tmp_path / "file.pdf"
        pdf.write_bytes(b"x")

        att_key = add_file_attachment(zotero_db, parent_key, pdf, storage_dir, title="My Doc")

        title_row = zotero_db.execute(
            """
            SELECT idv.value FROM itemData id
            JOIN fieldsCombined f ON id.fieldID = f.fieldID
            JOIN itemDataValues idv ON id.valueID = idv.valueID
            JOIN items i ON id.itemID = i.itemID
            WHERE i.key = ? AND f.fieldName = 'title'
            """,
            (att_key,),
        ).fetchone()
        assert title_row is not None
        assert title_row["value"] == "My Doc"

    def test_missing_file_raises(self, zotero_db, storage_dir, tmp_path):
        parent_key = _insert_book(zotero_db)
        missing = tmp_path / "nonexistent.pdf"

        with pytest.raises(ValueError, match="File not found"):
            add_file_attachment(zotero_db, parent_key, missing, storage_dir)

    def test_missing_parent_raises(self, zotero_db, storage_dir, tmp_path):
        pdf = tmp_path / "file.pdf"
        pdf.write_bytes(b"x")

        with pytest.raises(ValueError, match="Parent item not found"):
            add_file_attachment(zotero_db, "NOTFOUND", pdf, storage_dir)

    def test_path_uses_storage_prefix(self, zotero_db, storage_dir, tmp_path):
        parent_key = _insert_book(zotero_db)
        pdf = tmp_path / "report.pdf"
        pdf.write_bytes(b"x")

        att_key = add_file_attachment(zotero_db, parent_key, pdf, storage_dir)
        row = zotero_db.execute(
            """
            SELECT ia.path
            FROM itemAttachments ia
            JOIN items i ON ia.itemID = i.itemID
            WHERE i.key = ?
            """,
            (att_key,),
        ).fetchone()
        assert row["path"].startswith("storage:")


class TestListFileAttachments:
    def test_empty_list(self, zotero_db, storage_dir):
        parent_key = _insert_book(zotero_db)
        result = list_file_attachments(zotero_db, parent_key, storage_dir)
        assert result == []

    def test_one_attachment(self, zotero_db, storage_dir, tmp_path):
        parent_key = _insert_book(zotero_db)
        pdf = tmp_path / "paper.pdf"
        pdf.write_bytes(b"content")
        add_file_attachment(zotero_db, parent_key, pdf, storage_dir)

        result = list_file_attachments(zotero_db, parent_key, storage_dir)
        assert len(result) == 1
        assert result[0]["exists"] is True

    def test_multiple_attachments(self, zotero_db, storage_dir, tmp_path):
        parent_key = _insert_book(zotero_db)
        for i in range(3):
            f = tmp_path / f"file{i}.pdf"
            f.write_bytes(b"x")
            add_file_attachment(zotero_db, parent_key, f, storage_dir)

        result = list_file_attachments(zotero_db, parent_key, storage_dir)
        assert len(result) == 3

    def test_attachment_fields(self, zotero_db, storage_dir, tmp_path):
        parent_key = _insert_book(zotero_db)
        pdf = tmp_path / "paper.pdf"
        pdf.write_bytes(b"x")
        add_file_attachment(zotero_db, parent_key, pdf, storage_dir)

        result = list_file_attachments(zotero_db, parent_key, storage_dir)
        att = result[0]
        assert "key" in att
        assert "title" in att
        assert "path" in att
        assert "contentType" in att
        assert "exists" in att

    def test_not_found_raises(self, zotero_db, storage_dir):
        with pytest.raises(ValueError, match="Item not found"):
            list_file_attachments(zotero_db, "NOTFOUND", storage_dir)

    def test_file_not_on_disk_shows_false(self, zotero_db, storage_dir, tmp_path):
        parent_key = _insert_book(zotero_db)
        pdf = tmp_path / "paper.pdf"
        pdf.write_bytes(b"x")
        att_key = add_file_attachment(zotero_db, parent_key, pdf, storage_dir)

        # Remove file from storage
        (storage_dir / att_key / "paper.pdf").unlink()

        result = list_file_attachments(zotero_db, parent_key, storage_dir)
        assert result[0]["exists"] is False

    def test_storage_prefix_with_link_mode_one_resolves_real_file(
        self, zotero_db, storage_dir, tmp_path
    ):
        parent_key = _insert_book(zotero_db)
        pdf = tmp_path / "paper.pdf"
        pdf.write_bytes(b"x")
        att_key = add_file_attachment(zotero_db, parent_key, pdf, storage_dir)

        zotero_db.execute(
            """
            UPDATE itemAttachments
            SET linkMode = 1
            WHERE itemID = (SELECT itemID FROM items WHERE key = ?)
            """,
            (att_key,),
        )
        zotero_db.commit()

        result = list_file_attachments(zotero_db, parent_key, storage_dir)
        assert result[0]["exists"] is True
        assert result[0]["path"] == str(storage_dir / att_key / "paper.pdf")


class TestDeleteAttachment:
    def test_delete_success(self, zotero_db, storage_dir, tmp_path):
        parent_key = _insert_book(zotero_db)
        pdf = tmp_path / "paper.pdf"
        pdf.write_bytes(b"x")
        att_key = add_file_attachment(zotero_db, parent_key, pdf, storage_dir)

        result = delete_attachment(zotero_db, att_key, storage_dir)
        assert result is True

        row = zotero_db.execute("SELECT 1 FROM items WHERE key = ?", (att_key,)).fetchone()
        assert row is None

    def test_delete_removes_file(self, zotero_db, storage_dir, tmp_path):
        parent_key = _insert_book(zotero_db)
        pdf = tmp_path / "paper.pdf"
        pdf.write_bytes(b"x")
        att_key = add_file_attachment(zotero_db, parent_key, pdf, storage_dir)

        delete_attachment(zotero_db, att_key, storage_dir, keep_file=False)
        assert not (storage_dir / att_key).exists()

    def test_delete_keeps_file(self, zotero_db, storage_dir, tmp_path):
        parent_key = _insert_book(zotero_db)
        pdf = tmp_path / "paper.pdf"
        pdf.write_bytes(b"x")
        att_key = add_file_attachment(zotero_db, parent_key, pdf, storage_dir)

        delete_attachment(zotero_db, att_key, storage_dir, keep_file=True)
        # File should remain
        assert (storage_dir / att_key / "paper.pdf").exists()

    def test_delete_storage_prefix_link_mode_one_removes_storage_dir(
        self, zotero_db, storage_dir, tmp_path
    ):
        parent_key = _insert_book(zotero_db)
        pdf = tmp_path / "paper.pdf"
        pdf.write_bytes(b"x")
        att_key = add_file_attachment(zotero_db, parent_key, pdf, storage_dir)

        zotero_db.execute(
            """
            UPDATE itemAttachments
            SET linkMode = 1
            WHERE itemID = (SELECT itemID FROM items WHERE key = ?)
            """,
            (att_key,),
        )
        zotero_db.commit()

        delete_attachment(zotero_db, att_key, storage_dir, keep_file=False)
        assert not (storage_dir / att_key).exists()

    def test_not_found_returns_false(self, zotero_db, storage_dir):
        result = delete_attachment(zotero_db, "NOTFOUND", storage_dir)
        assert result is False

    def test_delete_non_attachment_raises(self, zotero_db, storage_dir):
        parent_key = _insert_book(zotero_db)
        with pytest.raises(ValueError, match="not an attachment"):
            delete_attachment(zotero_db, parent_key, storage_dir)
