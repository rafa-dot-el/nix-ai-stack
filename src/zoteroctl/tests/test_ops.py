"""ops and the export path: no silent truncation, BibTeX import."""

import re
import sqlite3
from contextlib import closing

import pytest
from click.testing import CliRunner

from zoteroctl import ops
from zoteroctl.cli import cli
from zoteroctl.db import connect_database

BIB = """@book{doe2024,
  title = {A Test Book},
  author = {Doe, Jane and Roe, Rick},
  year = {2024},
  publisher = {Press}
}

@article{x2023,
  title = {An Article},
  author = {Xu, Li},
  journal = {J},
  year = {2023}
}
"""


def _env(db, storage):
    return {"ZOTERO_DATABASE": str(db), "ZOTERO_STORAGE": str(storage), "XDG_CONFIG_HOME": str(storage)}


def test_export_all_is_not_cut_at_ten_thousand(zotero_db_path, storage_dir):
    # bug dff75e96: --all and --collection passed limit=10000
    with closing(sqlite3.connect(zotero_db_path)) as c:
        book = c.execute("SELECT itemTypeID FROM itemTypes WHERE typeName = 'book'").fetchone()[0]
        start = c.execute("SELECT count(*) FROM items i JOIN itemTypes t ON i.itemTypeID = t.itemTypeID WHERE t.typeName NOT IN ('attachment', 'note')").fetchone()[0]
        c.executemany("INSERT INTO items (itemTypeID, key) VALUES (?, ?)", [(book, f"B{i:07d}") for i in range(10_001)])
        c.commit()
    r = CliRunner().invoke(cli, ["export", "--all"], env=_env(zotero_db_path, storage_dir))
    assert r.exit_code == 0, r.output
    assert len(re.findall(r"^@\w+\{", r.output, re.M)) == start + 10_001


def test_export_choices(zotero_db_path):
    key = ops.import_items(zotero_db_path, ops.parse_bibtex(BIB))[0]
    with connect_database(zotero_db_path) as conn:
        everything = ops.export_entries(conn, export_all=True)
        one = ops.export_entries(conn, key=key)
        with pytest.raises(ops.NotFound, match="NOPE"):
            ops.export_entries(conn, key="NOPE")
        with pytest.raises(ops.Invalid, match="KEY, --collection, or --all"):
            ops.export_entries(conn)
        assert ops.export_entries(conn, collection="NOCOLL") == []
    assert len(one) == 1 and everything and one[0] in everything


def test_parse_bibtex():
    items = ops.parse_bibtex(BIB)
    assert [i["itemType"] for i in items] == ["book", "journalArticle"]
    assert items[0]["fields"]["title"] == "A Test Book"
    assert [c["lastName"] for c in items[0]["creators"]] == ["Doe", "Roe"]
    with pytest.raises(ops.Invalid, match="no BibTeX entries"):
        ops.parse_bibtex("not bibtex at all")


def test_import_items_adds_them_all_or_none(zotero_db_path):
    keys = ops.import_items(zotero_db_path, ops.parse_bibtex(BIB))
    assert len(keys) == 2 and all(len(k) == 8 for k in keys)
    with connect_database(zotero_db_path) as conn:
        assert all(conn.execute("SELECT 1 FROM items WHERE key = ?", (k,)).fetchone() for k in keys)
        before = conn.execute("SELECT count(*) FROM items").fetchone()[0]
    with pytest.raises(ops.Invalid, match="Unknown item type"):
        ops.import_items(zotero_db_path, [ops.parse_bibtex(BIB)[0], {"itemType": "spaceship", "fields": {"title": "x"}}])
    with connect_database(zotero_db_path) as conn:
        assert conn.execute("SELECT count(*) FROM items").fetchone()[0] == before, "rolled back"
    with pytest.raises(ops.NotFound):
        ops.import_items(zotero_db_path.parent / "missing.sqlite", [])
