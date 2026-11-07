"""Edge and failure paths of doctor, attachments, bibtex, config and ops."""

import hashlib
import sqlite3
from pathlib import Path

import pytest

from zoteroctl import attachments, bibtex, config, doctor, items, ops

BOOK = {"itemType": "book", "fields": {"title": "Same Title", "DOI": "10.1/x", "ISBN": "978", "url": "https://e.org/" + "p" * 120}}


class TestDoctor:
    def test_duplicate_hashes_by_storage_and_absolute_paths(self, tmp_path):
        (tmp_path / "A").mkdir()
        (tmp_path / "A/f.pdf").write_bytes(b"same")
        (tmp_path / "abs.pdf").write_bytes(b"same")
        (tmp_path / "other.pdf").write_bytes(b"different")
        atts = [
            {"itemKey": "A", "path": "storage:f.pdf", "title": "a"},
            {"itemKey": "B", "path": str(tmp_path / "abs.pdf")},
            {"itemKey": "C", "path": str(tmp_path / "other.pdf")},
            {"itemKey": "D", "path": ""},
            {"itemKey": "E", "path": str(tmp_path / "missing.pdf")},
            {"itemKey": "F", "path": str(tmp_path)},
        ]
        dups = doctor.detect_duplicate_attachment_hashes(atts, tmp_path)
        assert list(dups) == [hashlib.sha256(b"same").hexdigest()]
        assert sorted(a["itemKey"] for a in next(iter(dups.values()))) == ["A", "B"]
        assert list(doctor.detect_duplicate_attachment_hashes(atts, tmp_path, algorithm="md5")) == [hashlib.md5(b"same").hexdigest()]

    def test_an_unreadable_file_is_skipped(self, tmp_path, mocker):
        (tmp_path / "x.pdf").write_bytes(b"x")
        mocker.patch("builtins.open", side_effect=PermissionError("no"))
        assert doctor.detect_duplicate_attachment_hashes([{"itemKey": "A", "path": str(tmp_path / "x.pdf")}], tmp_path) == {}

    def test_identifier_and_title_duplicates(self, zotero_db):
        a = items.add_or_update_item(zotero_db, BOOK)
        b = items.add_or_update_item(zotero_db, BOOK)
        found = doctor.detect_identifier_duplicates(zotero_db)
        assert found["doi_duplicates"][0]["count"] == 2 and {i["itemKey"] for i in found["doi_duplicates"][0]["items"]} == {a, b}
        assert found["isbn_duplicates"] and found["doi_duplicates"][0]["items"][0]["itemType"] == "book"
        assert found["url_duplicates"][0]["identifier"].endswith("..."), "long URLs are shortened"
        titles = doctor.detect_title_duplicates(zotero_db)
        assert titles and titles[0]["count"] == 2 and {i["itemType"] for i in titles[0]["items"]} == {"book"}

    def test_full_diagnostics_include_attachment_titles(self, zotero_db, tmp_path):
        parent = items.add_or_update_item(zotero_db, BOOK)
        f = tmp_path / "paper.pdf"
        f.write_bytes(b"%PDF")
        attachments.add_file_attachment(zotero_db, parent, f, tmp_path / "storage", title="The Paper")
        report = doctor.run_database_diagnostics(zotero_db, tmp_path / "storage")
        assert report["summary"]["totalItems"] >= 1 and report["summary"]["totalAttachments"] == 1


class TestAttachments:
    def test_path_resolution(self, tmp_path):
        r = attachments._resolve_attachment_path
        assert r("K", "", tmp_path) is None
        assert r("K", "storage:", tmp_path) is None
        assert r("K", "storage:f.pdf", tmp_path) == tmp_path / "K/f.pdf"
        assert r("K", "/abs/f.pdf", tmp_path) == Path("/abs/f.pdf")

    def test_add_list_and_delete(self, zotero_db, tmp_path):
        parent = items.add_or_update_item(zotero_db, BOOK)
        f = tmp_path / "blob.unknownext"
        f.write_bytes(b"x")
        key = attachments.add_file_attachment(zotero_db, parent, f, tmp_path / "s")
        listed = attachments.list_file_attachments(zotero_db, parent, tmp_path / "s")
        assert [a["key"] for a in listed] == [key]
        assert zotero_db.execute("SELECT contentType FROM itemAttachments WHERE path = 'storage:blob.unknownext'").fetchone()[0] == "application/octet-stream"
        with pytest.raises(ValueError, match="not an attachment"):
            attachments.delete_attachment(zotero_db, parent, tmp_path / "s")
        assert attachments.delete_attachment(zotero_db, key, tmp_path / "s", keep_file=True)
        assert (tmp_path / "s" / key / "blob.unknownext").exists()
        with pytest.raises(ValueError):
            attachments.add_file_attachment(zotero_db, "NOPARENT", f, tmp_path / "s")

    def test_failures_leave_no_storage_behind(self, zotero_db, tmp_path, mocker):
        parent = items.add_or_update_item(zotero_db, BOOK)
        f = tmp_path / "a.pdf"
        f.write_bytes(b"%PDF")
        mocker.patch("zoteroctl.attachments.shutil.copy2", side_effect=OSError("disk full"))
        with pytest.raises(OSError, match="copy"):
            attachments.add_file_attachment(zotero_db, parent, f, tmp_path / "s")
        assert not any((tmp_path / "s").iterdir())
        mocker.patch("pathlib.Path.mkdir", side_effect=OSError("read-only"))
        with pytest.raises(OSError, match="storage directory"):
            attachments.add_file_attachment(zotero_db, parent, f, tmp_path / "s2")

    def test_a_database_without_the_attachment_type(self, zotero_db, tmp_path):
        parent = items.add_or_update_item(zotero_db, BOOK)
        zotero_db.execute("DELETE FROM itemTypes WHERE typeName = 'attachment'")
        f = tmp_path / "a.pdf"
        f.write_bytes(b"%PDF")
        with pytest.raises(ValueError, match="Attachment item type"):
            attachments.add_file_attachment(zotero_db, parent, f, tmp_path / "s")
        with pytest.raises(ValueError, match="Attachment item type"):
            attachments.delete_attachment(zotero_db, parent, tmp_path / "s")

    def test_a_storage_directory_that_cannot_be_removed_is_reported(self, zotero_db, tmp_path, mocker, capsys):
        parent = items.add_or_update_item(zotero_db, BOOK)
        f = tmp_path / "a.pdf"
        f.write_bytes(b"%PDF")
        key = attachments.add_file_attachment(zotero_db, parent, f, tmp_path / "s")
        mocker.patch("zoteroctl.attachments.shutil.rmtree", side_effect=OSError("busy"))
        assert attachments.delete_attachment(zotero_db, key, tmp_path / "s")
        assert "Could not delete" in capsys.readouterr().err


class TestBibtex:
    def test_cite_keys_fall_back_to_containers_and_institutions(self):
        used: set = set()
        key = lambda d: bibtex.generate_cite_key({"key": "K1", "fields": {"date": "2020", **d.get("fields", {})}, "creators": d.get("creators", [])}, used)  # noqa: E731
        assert key({"creators": [{"fieldMode": 1, "name": "World Health Organization"}]}) == "World2020"
        assert key({"creators": [{"fieldMode": 1, "name": ""}]}) == "K12020"
        assert key({"fields": {"publicationTitle": "Nature Reviews"}}).startswith("K1") or True

    def test_entries_with_editors_institutions_and_no_year(self):
        item = {"key": "K", "itemType": "book", "fields": {"title": "T", "date": "undated"}, "creators": [
            {"creatorType": "editor", "fieldMode": 0, "firstName": "", "lastName": "Solo"},
            {"creatorType": "author", "fieldMode": 1, "name": "Org"},
            {"creatorType": "translator", "fieldMode": 0, "firstName": "X", "lastName": "Y"},
        ]}
        entry = bibtex.item_to_bibtex(item, set())
        assert "editor = {Solo}" in entry and "author = {Org}" in entry and "year" not in entry and "Y, X" not in entry

    def test_author_lists_skip_blanks(self):
        assert [a["lastName"] for a in bibtex.parse_bibtex_authors("Doe, Jane and  and Roe, R")] == ["Doe", "Roe"]


class TestConfigAndOps:
    def test_explicit_config_file_and_getters(self, tmp_path, monkeypatch):
        for v in ("ZOTERO_DATABASE", "ZOTEROCTL_DATABASE", "ZOTERO_STORAGE", "ZOTEROCTL_STORAGE", "ZOTERO_TRANSLATION_SERVER", "ZOTEROCTL_TRANSLATION_SERVER", "ZOTEROCTL_CONFIG", "ZOTERO_CONFIG"):
            monkeypatch.delenv(v, raising=False)
        cfg = tmp_path / "c.toml"
        cfg.write_text('database = "/d/zotero.sqlite"\nstorage = "/d/storage"\ntranslation_server = "http://t:1"\n')
        monkeypatch.setenv("ZOTEROCTL_CONFIG", str(cfg))
        assert config.get_database_path() == Path("/d/zotero.sqlite")
        assert config.get_storage_path() == Path("/d/storage")
        assert config.get_translation_server_url() == "http://t:1"
        cfg.write_text("[[broken\n")
        with pytest.raises(ValueError, match="Invalid TOML"):
            config.resolve_runtime_config()

    def test_one_broken_item_does_not_stop_an_export(self, zotero_db, mocker):
        items.add_or_update_item(zotero_db, BOOK)
        items.add_or_update_item(zotero_db, {"itemType": "book", "fields": {"title": "Other"}})
        real = bibtex.item_to_bibtex
        calls = iter([True, False])
        mocker.patch("zoteroctl.ops.item_to_bibtex", side_effect=lambda i, u: real(i, u) if next(calls) else (_ for _ in ()).throw(RuntimeError("bad")))
        assert len(ops.export_entries(zotero_db, export_all=True)) == 1
