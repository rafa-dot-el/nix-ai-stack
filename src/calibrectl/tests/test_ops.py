"""ops: update_metadata and attach_file, against the official-schema fixture library."""

import os
import sqlite3
from contextlib import closing
import time
from datetime import datetime, timezone

import pytest

from calibrectl import ops
from tests.test_integration import _build_library


@pytest.fixture
def library(tmp_path):
    return _build_library(tmp_path)


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


def _row(db, sql, *args):
    with closing(sqlite3.connect(db)) as c:
        return c.execute(sql, args).fetchone()


def _stamp_age_seconds(stamp: str) -> float:
    return abs((datetime.now(timezone.utc) - datetime.fromisoformat(stamp)).total_seconds())


def test_utc_now_is_utc_whatever_the_local_zone(warsaw):
    # bug 41ab56d8: local wall-clock time was labelled +00:00
    s = ops.utc_now()
    assert s.endswith("+00:00")
    assert _stamp_age_seconds(s) < 60


def test_the_cli_stamps_edits_in_utc(library, warsaw):
    from click.testing import CliRunner

    from calibrectl.cli import cli

    db, storage = library
    r = CliRunner().invoke(cli, ["update", "1", "--title", "Renamed", "--database", str(db), "--no-backup"])
    assert r.exit_code == 0, r.output
    assert _stamp_age_seconds(_row(db, "SELECT last_modified FROM books WHERE id = 1")[0]) < 60


def test_update_applies_every_kind_of_field(library):
    db, _ = library
    out = ops.update_metadata(db, 1, {"title": "New", "authors": ["A One", "B Two"], "tags": "x, y", "add_tag": "z", "series": "S", "series_index": 2.0, "rating": 8, "isbn": "9780000000002", "publisher": "P", "pubdate": "2020-05-01", "abstract": "Abs"}, backup=False)
    assert out["book_id"] == 1 and set(out["updated_fields"]) >= {"title", "authors", "tags", "add_tag", "rating", "abstract"}
    assert _row(db, "SELECT title FROM books WHERE id = 1")[0] == "New"
    with closing(sqlite3.connect(db)) as c:
        authors = [r[0] for r in c.execute("SELECT a.name FROM authors a JOIN books_authors_link l ON l.author = a.id WHERE l.book = 1 ORDER BY a.name")]
        tags = sorted(r[0] for r in c.execute("SELECT t.name FROM tags t JOIN books_tags_link l ON l.tag = t.id WHERE l.book = 1"))
    assert authors == ["A One", "B Two"] and tags == ["x", "y", "z"]
    ops.update_metadata(db, 1, {"remove_tag": "x", "add_author": "C", "remove_author": "A One"}, backup=False)
    with closing(sqlite3.connect(db)) as c:
        assert "x" not in [r[0] for r in c.execute("SELECT t.name FROM tags t JOIN books_tags_link l ON l.tag = t.id WHERE l.book = 1")]


def test_update_refusals(library):
    db, _ = library
    with pytest.raises(ops.Invalid, match="no fields"):
        ops.update_metadata(db, 1, {}, backup=False)
    with pytest.raises(ops.Invalid, match="colour"):
        ops.update_metadata(db, 1, {"colour": "red"}, backup=False)
    with pytest.raises(ops.Invalid, match="rating"):
        ops.update_metadata(db, 1, {"rating": 11}, backup=False)
    with pytest.raises(ops.NotFound, match="999"):
        ops.update_metadata(db, 999, {"title": "x"}, backup=False)
    with pytest.raises(ops.NotFound):
        ops.update_metadata(db.parent / "missing.db", 1, {"title": "x"}, backup=False)


def test_update_makes_a_backup_unless_told_not_to(library):
    db, _ = library
    before = set(db.parent.iterdir())
    ops.update_metadata(db, 1, {"title": "B"})
    assert len(set(db.parent.iterdir()) - before) == 1


def test_attach_copies_registers_and_refuses_a_second_copy(library, tmp_path):
    db, storage = library
    src = tmp_path / "notes.txt"
    src.write_text("hello")
    out = ops.attach_file(db, storage, 1, src, backup=False)
    assert out["format"] == "TXT" and out["size_bytes"] == 5
    dest = storage / _row(db, "SELECT path FROM books WHERE id = 1")[0]
    assert any(p.read_bytes() == b"hello" for p in dest.iterdir() if p.suffix == ".txt")
    assert _row(db, "SELECT count(*) FROM data WHERE book = 1 AND format = 'TXT'")[0] == 1
    with pytest.raises(ops.Invalid, match="already has a TXT"):
        ops.attach_file(db, storage, 1, src, backup=False)
    with pytest.raises(ops.NotFound):
        ops.attach_file(db, storage, 999, src, backup=False)
    with pytest.raises(ops.NotFound):
        ops.attach_file(db, storage, 1, tmp_path / "nope.pdf", backup=False)
    odd = tmp_path / "blob"
    odd.write_bytes(b"\x00")
    with pytest.raises(ops.Invalid, match="format"):
        ops.attach_file(db, storage, 1, odd, backup=False)


def test_a_format_cannot_steer_the_copy_out_of_the_book_directory(library, tmp_path):
    # automated security review of 0c5ade5: file_format went into the file name unchecked
    db, storage = library
    src = tmp_path / "notes.txt"
    src.write_text("x")
    for fmt in ("/../../../../escaped", "txt/../../x", "a b", "TOOLONGFORMAT1"):
        with pytest.raises(ops.Invalid, match="format"):
            ops.attach_file(db, storage, 1, src, fmt, backup=False)
    assert not any(tmp_path.rglob("escaped*")), "nothing was written outside"
    with closing(sqlite3.connect(db)) as c:
        c.execute("UPDATE books SET path = '../../outside' WHERE id = 2")
        c.commit()
    with pytest.raises(ops.Invalid, match="book directory"):
        ops.attach_file(db, storage, 2, src, "TXT", backup=False)
