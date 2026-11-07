"""items.py: the helpers that moved out of cli.py, now measured on their own."""

import io
import os
import time
from datetime import datetime, timezone

import pytest

from zoteroctl import items

FULL = {
    "itemType": "book",
    "fields": {"title": "Full Book", "date": "2020", "publisher": "P"},
    "url": "https://example.org",
    "noSuchField": "x",
    "creators": [
        {"creatorType": "author", "firstName": "Jane", "lastName": "Doe"},
        {"creatorType": "editor", "fieldMode": 1, "name": "The Committee"},
        {"creatorType": "translator", "firstName": "T", "lastName": "R"},
    ],
    "tags": ["one", "two"],
    "collections": ["NOCOLL"],
}


@pytest.fixture
def warsaw():
    old = os.environ.get("TZ")
    os.environ["TZ"] = "Europe/Warsaw"
    time.tzset()
    yield
    if old is None:
        del os.environ["TZ"]
    else:
        os.environ["TZ"] = old
    time.tzset()


def _collection(conn, name="C", key="COLL0001"):
    conn.execute("INSERT INTO collections (collectionName, key) VALUES (?, ?)", (name, key))
    return key


def test_dates_are_written_in_utc(zotero_db, warsaw):
    # bug ce138ee6: local wall-clock time; Zotero stores UTC
    key = items.add_or_update_item(zotero_db, {"itemType": "book", "fields": {"title": "T"}})
    stamp = zotero_db.execute("SELECT dateAdded FROM items WHERE key = ?", (key,)).fetchone()[0]
    age = abs((datetime.now(timezone.utc) - datetime.strptime(stamp, "%Y-%m-%d %H:%M:%S").replace(tzinfo=timezone.utc)).total_seconds())
    assert age < 60, stamp


def test_add_and_read_back_everything(zotero_db):
    _collection(zotero_db)
    key = items.add_or_update_item(zotero_db, {**FULL, "collections": ["COLL0001", "NOCOLL"]})
    got = items.get_item_by_key(zotero_db, key)
    assert got["fields"] == {"title": "Full Book", "date": "2020", "publisher": "P", "url": "https://example.org"}
    assert [c.get("lastName") or c.get("name") for c in got["creators"]] == ["Doe", "The Committee", "R"]
    assert sorted(got["tags"]) == ["one", "two"]
    assert got["collections"]
    assert items.get_item_by_key(zotero_db, "NOPE") is None


def test_update_replaces_the_item_and_bumps_its_version(zotero_db):
    key = items.add_or_update_item(zotero_db, FULL)
    items.add_or_update_item(zotero_db, {"key": key, "itemType": "book", "fields": {"title": "Renamed"}, "tags": ["three"]})
    got = items.get_item_by_key(zotero_db, key)
    assert got["fields"] == {"title": "Renamed"} and got["tags"] == ["three"] and got["creators"] == []
    assert zotero_db.execute("SELECT version FROM items WHERE key = ?", (key,)).fetchone()[0] == 1
    with pytest.raises(ValueError, match="not found for update"):
        items.add_or_update_item(zotero_db, {"key": "MISSING1", "itemType": "book"})
    with pytest.raises(ValueError, match="Unknown item type"):
        items.add_or_update_item(zotero_db, {"itemType": "spaceship"})


def test_existing_values_and_creators_are_reused(zotero_db):
    items.add_or_update_item(zotero_db, FULL)
    items.add_or_update_item(zotero_db, FULL)
    assert zotero_db.execute("SELECT count(*) FROM itemDataValues WHERE value = 'Full Book'").fetchone()[0] == 1
    assert zotero_db.execute("SELECT count(*) FROM creators WHERE lastName = 'Doe'").fetchone()[0] == 1
    assert zotero_db.execute("SELECT count(*) FROM tags WHERE name = 'one'").fetchone()[0] == 1


def _child(conn, parent_key, kind, **kw):
    type_id = {"attachment": 3, "note": 4}[kind]
    key = items.generate_zotero_key(conn)
    parent = conn.execute("SELECT itemID FROM items WHERE key = ?", (parent_key,)).fetchone()[0] if parent_key else None
    item_id = conn.execute("INSERT INTO items (itemTypeID, key) VALUES (?, ?)", (type_id, key)).lastrowid
    if kind == "attachment":
        conn.execute("INSERT INTO itemAttachments (itemID, parentItemID, linkMode, contentType, path) VALUES (?, ?, ?, ?, ?)", (item_id, parent, kw["mode"], kw.get("ctype"), kw.get("path")))
    else:
        conn.execute("INSERT INTO itemNotes (itemID, parentItemID, note) VALUES (?, ?, ?)", (item_id, parent, kw.get("note")))
    return key


def test_attachments_notes_and_their_nesting(zotero_db):
    key = items.add_or_update_item(zotero_db, FULL)
    imported = _child(zotero_db, key, "attachment", mode=0, ctype="application/pdf", path="paper.pdf")
    _child(zotero_db, key, "attachment", mode=0, path="storage:already.pdf")
    _child(zotero_db, key, "attachment", mode=1, path="/abs/linked.pdf")
    _child(zotero_db, key, "attachment", mode=3, path=None)
    _child(zotero_db, key, "note", note="<p>hi</p>")
    _child(zotero_db, None, "note", note=None)
    got = items.get_item_by_key(zotero_db, key)
    assert [a["path"] for a in got["attachments"]] == ["storage:paper.pdf", "storage:already.pdf", "/abs/linked.pdf", ""]
    assert got["notes"] == [{"key": got["notes"][0]["key"], "note": "<p>hi</p>"}]
    children = items.query_child_items(zotero_db)
    assert len(children["attachments"]) == 4 and len(children["notes"]) == 2
    nested = items.nest_child_items({"key": key}, children["attachments"], children["notes"])
    assert len(nested["attachments"]) == 4 and len(nested["notes"]) == 1
    mixed = [{"itemType": "book"}, {"itemType": "attachment"}, {"itemType": "note"}]
    assert items.filter_bibliographic_items(mixed) == [{"itemType": "book"}]
    assert imported in [a["key"] for a in children["attachments"]]


def test_list_filters(zotero_db):
    coll = _collection(zotero_db)
    a = items.add_or_update_item(zotero_db, {**FULL, "collections": [coll]})
    b = items.add_or_update_item(zotero_db, {"itemType": "journalArticle", "fields": {"title": "Other"}, "tags": ["z"]})
    _child(zotero_db, a, "attachment", mode=0, path="x.pdf")
    keys = lambda **kw: [i["key"] for i in items.list_items_query(zotero_db, **kw)]  # noqa: E731
    assert set(keys()) == {a, b}, "attachments and notes are not listed"
    assert keys(collection_key=coll) == [a]
    assert keys(tag_name="z") == [b]
    assert keys(title_name="full") == [a]
    assert len(keys(limit=1)) == 1
    row = next(i for i in items.list_items_query(zotero_db) if i["key"] == a)
    assert row["creators"] == "Jane Doe; The Committee; T R" and row["date"] == "2020"
    zotero_db.execute("INSERT INTO deletedItems (itemID) SELECT itemID FROM items WHERE key = ?", (b,))
    assert keys() == [a], "trashed items are not listed"


def test_delete_removes_imported_files_unless_kept(zotero_db, tmp_path):
    storage = tmp_path / "storage"
    key = items.add_or_update_item(zotero_db, FULL)
    att = _child(zotero_db, key, "attachment", mode=0, path="storage:paper.pdf")
    (storage / att).mkdir(parents=True)
    (storage / att / "paper.pdf").write_bytes(b"%PDF")
    keep = items.add_or_update_item(zotero_db, FULL)
    att2 = _child(zotero_db, keep, "attachment", mode=0, path="kept.pdf")
    (storage / att2).mkdir()
    (storage / att2 / "kept.pdf").write_bytes(b"%PDF")
    assert items.delete_item(zotero_db, keep, storage, keep_files=True)
    assert (storage / att2 / "kept.pdf").exists()
    assert items.delete_item(zotero_db, key, storage)
    assert not (storage / att).exists()
    assert not items.delete_item(zotero_db, key, storage), "already gone"
    standalone = _child(zotero_db, None, "attachment", mode=0, path="storage:solo.pdf")
    (storage / standalone).mkdir()
    (storage / standalone / "solo.pdf").write_bytes(b"x")
    assert items.delete_item(zotero_db, standalone, storage)
    assert not (storage / standalone).exists(), "a standalone attachment's own directory goes too"


def test_item_files_and_yaml_input(tmp_path, monkeypatch):
    (tmp_path / "i.json").write_text('{"itemType": "book"}')
    (tmp_path / "i.yml").write_text("itemType: book\n")
    (tmp_path / "i.txt").write_text("")
    assert items.load_item_data(tmp_path / "i.json") == {"itemType": "book"}
    assert items.load_item_data(tmp_path / "i.yml") == {"itemType": "book"}
    with pytest.raises(ValueError, match="Unsupported"):
        items.load_item_data(tmp_path / "i.txt")
    assert items.read_yaml_input(tmp_path / "i.yml") == {"itemType": "book"}
    with pytest.raises(FileNotFoundError):
        items.read_yaml_input(tmp_path / "missing.yml")
    monkeypatch.setattr("sys.stdin", io.StringIO("a: 1\n"))
    assert items.read_yaml_input(None) == {"a": 1}
    monkeypatch.setattr("sys.stdin", io.StringIO(""))
    with pytest.raises(ValueError, match="Empty"):
        items.read_yaml_input(None)
    monkeypatch.setattr("sys.stdin", io.StringIO("- a\n"))
    with pytest.raises(ValueError, match="dictionary"):
        items.read_yaml_input(None)


def test_partial_updates_merge_fields():
    old = {"itemType": "book", "fields": {"title": "A", "date": "1"}, "tags": ["x"]}
    new = items.merge_partial_updates(old, {"itemKey": "K", "fields": {"title": "B"}, "tags": ["y"]})
    assert new == {"itemType": "book", "fields": {"title": "B", "date": "1"}, "tags": ["y"]}
    assert old["fields"]["title"] == "A", "the original is untouched"
    assert items.merge_partial_updates({}, {"fields": {"t": 1}}) == {"fields": {"t": 1}}


def test_keys_avoid_collisions(zotero_db, monkeypatch):
    zotero_db.execute("INSERT INTO items (itemTypeID, key) VALUES (1, 'AAAAAAAA')")
    seq = iter(["AAAAAAAA", "BBBBBBBB"])
    monkeypatch.setattr(items.random, "choices", lambda chars, k: list(next(seq)))
    assert items.generate_zotero_key(zotero_db) == "BBBBBBBB"


def _cli(db_path, storage, *args):
    import json as _json

    from click.testing import CliRunner

    from zoteroctl.cli import cli

    r = CliRunner().invoke(cli, ["--json-output", "list-items", *args], env={"ZOTERO_DATABASE": str(db_path), "ZOTERO_STORAGE": str(storage), "XDG_CONFIG_HOME": str(storage)})
    out = r.output[r.output.find("{"):] if "{" in r.output else ""
    return r.exit_code, (_json.loads(out)["items"] if out else [])


def test_list_items_parent_shows_the_children_and_items_carry_theirs(zotero_db_path, tmp_path):
    # bug: --parent was always empty, and children were nested only under --no-attachments
    import sqlite3
    from contextlib import closing

    with closing(sqlite3.connect(zotero_db_path)) as c:
        c.row_factory = sqlite3.Row
        parent = items.add_or_update_item(c, FULL)
        plain = items.add_or_update_item(c, {"itemType": "book", "fields": {"title": "No files"}})
        att = _child(c, parent, "attachment", mode=1, path="/abs/a.pdf")
        note = _child(c, parent, "note", note="n")
        c.commit()
    code, children = _cli(zotero_db_path, tmp_path, "--parent", parent)
    assert code == 0 and sorted(i["key"] for i in children) == sorted([att, note]), children
    code, listed = _cli(zotero_db_path, tmp_path)
    by_key = {i["key"]: i for i in listed}
    assert set(by_key) == {parent, plain}
    assert [a["key"] for a in by_key[parent]["attachments"]] == [att] and [n["key"] for n in by_key[parent]["notes"]] == [note]
    code, bare = _cli(zotero_db_path, tmp_path, "--no-attachments")
    assert [i["key"] for i in bare] == [plain]
    code, both = _cli(zotero_db_path, tmp_path, "--parent", parent, "--no-attachments")
    assert code == 0, "no NameError"
