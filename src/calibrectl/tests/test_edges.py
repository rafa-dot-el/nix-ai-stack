"""Edge cases the module tests did not reach: precedence, malformed inputs, failure paths."""

import sqlite3
from contextlib import closing
import subprocess
import zipfile
from pathlib import Path

import click
import pytest

from calibrectl import bibtex, config, db, formatters, isbn, maintenance, metadata
from tests.test_integration import _build_library


@pytest.fixture
def library(tmp_path):
    return _build_library(tmp_path)


class TestConfigPrecedence:
    def test_settings_file_sits_between_env_and_defaults(self, tmp_path, monkeypatch):
        monkeypatch.delenv("CALIBRE_DATABASE", raising=False)
        monkeypatch.delenv("CALIBRE_STORAGE", raising=False)
        monkeypatch.setenv("HOME", str(tmp_path))
        monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path / "cfg"))
        assert config.get_database_path() == tmp_path / "Calibre Library" / "metadata.db", "nothing exists: the standard path"
        (tmp_path / "books").mkdir()
        (tmp_path / "books/metadata.db").write_text("")
        assert config.get_database_path() == tmp_path / "books/metadata.db"
        (tmp_path / "Calibre Library").mkdir()
        (tmp_path / "Calibre Library/metadata.db").write_text("")
        assert config.get_database_path() == tmp_path / "Calibre Library/metadata.db"
        (tmp_path / "cfg/rafa").mkdir(parents=True)
        (tmp_path / "cfg/rafa/calibrectl.yaml").write_text("database: /lib/metadata.db\nstorage: /lib/files\n")
        assert config.get_database_path() == Path("/lib/metadata.db")
        assert config.get_storage_path(Path("/x/metadata.db")) == Path("/lib/files")
        monkeypatch.setenv("CALIBRE_DATABASE", str(tmp_path / "env.db"))
        assert config.get_database_path() == (tmp_path / "env.db").resolve(), "the environment wins"


def test_echo_warning_respects_quiet(capsys):
    formatters.set_quiet(False)
    formatters.echo_warning("careful")
    assert "[!] careful" in capsys.readouterr().err
    formatters.set_quiet(True)
    formatters.echo_warning("hidden")
    assert capsys.readouterr().err == ""
    formatters.set_quiet(False)


class TestBibtexEdges:
    def test_keys_and_fields_from_odd_records(self):
        used: set = set()
        assert bibtex.generate_cite_key({"id": 7, "authors": ["123 456"], "publication_date": "n.d."}, used).startswith("Book7")
        assert bibtex.generate_cite_key({"id": 8, "authors": "Ursula Le Guin", "publication_date": "1969"}, used) == "Guin1969"
        entry = bibtex.book_to_bibtex({"id": 9, "authors": "Solo Author", "publication_date": "undated"}, set())
        assert entry.startswith("@book{") and "title" not in entry and "year" not in entry and "Solo Author" in entry

    def test_tag_and_all_exports_and_a_failing_record(self, library, monkeypatch):
        db_path, _ = library
        assert bibtex.export_bibtex_books(db_path, book_id=999) == []
        everything = bibtex.export_bibtex_books(db_path, export_all=True)
        assert len(everything) == 6
        with closing(sqlite3.connect(db_path)) as c:
            tag = c.execute("SELECT name FROM tags LIMIT 1").fetchone()[0]
        assert bibtex.export_bibtex_books(db_path, tag=tag)
        assert bibtex.export_bibtex_books(db_path) == [], "nothing asked, nothing exported"
        monkeypatch.setattr(bibtex, "book_to_bibtex", lambda *_: (_ for _ in ()).throw(RuntimeError("boom")))
        assert bibtex.export_bibtex_books(db_path, export_all=True) == [], "a failing record is skipped"


class TestMetadataEdges:
    def _opf(self, mocker, stdout):
        run = mocker.patch("calibrectl.metadata.subprocess.run")
        run.return_value = subprocess.CompletedProcess([], 0, stdout=stdout, stderr="")
        return run

    def test_a_minimal_opf_and_dates_with_times(self, mocker):
        self._opf(mocker, '<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata><dc:date>2001-02-03T00:00:00</dc:date></metadata></package>')
        assert metadata.fetch_metadata_from_calibre(isbn="1") == {"pubdate": "2001-02-03"}
        self._opf(mocker, '<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata><dc:title></dc:title></metadata></package>')
        assert metadata.fetch_metadata_from_calibre(title="t") is None, "empty elements are no metadata"

    def test_nothing_to_search_and_garbage(self, mocker):
        assert metadata.fetch_metadata_from_calibre() is None
        self._opf(mocker, "not xml")
        assert metadata.fetch_metadata_from_calibre(title="t", authors=["a"]) is None


class TestIsbnEdges:
    def test_isbn10_with_bad_characters(self):
        assert not isbn.validate_isbn("12345X789X")
        assert not isbn.validate_isbn("ABCDEFGHIJ")

    def _epub(self, path, container, opf=None):
        with zipfile.ZipFile(path, "w") as z:
            z.writestr("META-INF/container.xml", container)
            if opf:
                z.writestr("content.opf", opf)
        return path

    def test_epub_metadata_without_a_rootfile_or_path_or_isbn(self, tmp_path):
        ns = 'xmlns="urn:oasis:names:tc:opendocument:xmlns:container"'
        assert isbn.extract_isbn_from_epub_metadata(self._epub(tmp_path / "a.epub", f"<container {ns}><rootfiles/></container>")) is None
        assert isbn.extract_isbn_from_epub_metadata(self._epub(tmp_path / "b.epub", f"<container {ns}><rootfiles><rootfile/></rootfiles></container>")) is None
        opf = '<package xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata><dc:identifier>uuid:1</dc:identifier><dc:identifier>9780306406157</dc:identifier></metadata></package>'
        c = self._epub(tmp_path / "c.epub", f'<container {ns}><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>', opf)
        assert isbn.extract_isbn_from_epub_metadata(c) == "9780306406157"

    def test_pdf_text_is_bounded(self, mocker, tmp_path):
        page = mocker.Mock()
        page.extract_text.return_value = "x" * 30 + " ISBN 978-0-306-40615-7 "
        reader = mocker.patch("pypdf.PdfReader")
        reader.return_value.pages = [page, page, page]
        assert isbn.extract_isbn_from_pdf(tmp_path / "f.pdf", max_chars=40) is None, "the ISBN lies past the limit"
        assert isbn.extract_isbn_from_pdf(tmp_path / "f.pdf", max_chars=1000) == "9780306406157"

    def test_epub_text_is_bounded_and_missing_items_are_skipped(self, mocker, tmp_path):
        item = mocker.Mock()
        item.get_content.return_value = b"<p>" + b"y" * 30 + b" ISBN 978-0-306-40615-7</p>"
        book = mocker.Mock()
        book.spine = [("gone", True), ("one", True), ("two", True)]
        book.get_item_with_id.side_effect = lambda i: None if i == "gone" else item
        mocker.patch("ebooklib.epub.read_epub", return_value=book)
        assert isbn.extract_isbn_from_epub(tmp_path / "f.epub", max_chars=20) is None
        assert isbn.extract_isbn_from_epub(tmp_path / "f.epub", max_chars=1000) == "9780306406157"

    def test_a_books_files_are_tried_in_order(self, tmp_path, mocker):
        real = tmp_path / "b.pdf"
        real.write_bytes(b"")
        mocker.patch("calibrectl.isbn.find_isbn", side_effect=[None, "9780306406157"])
        book = {"files": [{"path": str(tmp_path / "missing.pdf")}, {"path": str(real)}, {"path": str(real)}]}
        assert isbn.find_isbn_for_book(book, tmp_path) == "9780306406157"
        assert isbn.find_isbn_for_book({"files": [{"path": str(tmp_path / "missing.pdf")}]}, tmp_path) is None


class TestDbEdges:
    def test_write_connections_provide_calibres_functions(self, library):
        db_path, _ = library
        with db.connect_database(db_path, read_only=False) as conn:
            assert tuple(conn.execute("SELECT title_sort('The Dispossessed'), title_sort('')").fetchone()) == ("dispossessed", "")
            assert conn.execute("SELECT title_sort('Plain')").fetchone()[0] == "plain"
            assert len(conn.execute("SELECT uuid4()").fetchone()[0]) == 36

    def test_a_locked_database_is_reported(self, library):
        db_path, _ = library
        holder = sqlite3.connect(db_path, isolation_level=None)
        holder.execute("BEGIN EXCLUSIVE")
        try:
            with pytest.raises(click.ClickException, match="locked"):
                with db.connect_database(db_path, read_only=False):
                    pass
            assert db.detect_database_lock(db_path)
        finally:
            holder.execute("ROLLBACK")
            holder.close()
        assert not db.detect_database_lock(db_path)
        assert db.detect_database_lock(db_path.parent / "missing.db")

    def test_a_corrupt_file_is_a_database_error(self, tmp_path):
        bad = tmp_path / "metadata.db"
        bad.write_bytes(b"not a database" * 100)
        with pytest.raises(click.ClickException):
            with db.connect_database(bad, read_only=False):
                pass


class TestRetagEdges:
    def test_consolidate_into_an_existing_tag_delete_and_backup(self, library):
        db_path, _ = library
        with closing(sqlite3.connect(db_path)) as c:
            tags = [r[0] for r in c.execute("SELECT name FROM tags ORDER BY id")]
        assert len(tags) >= 2
        before = set(db_path.parent.iterdir())
        stats = maintenance.apply_retag(db_path, {tags[0]: tags[1], tags[-1]: None, "no-such-tag": None}, backup=True)
        assert stats["tags_consolidated"] == 1 and stats["tags_deleted"] == 1
        assert len(set(db_path.parent.iterdir()) - before) == 1, "one backup file"

    def test_a_failure_rolls_back(self, library, mocker):
        db_path, _ = library
        with closing(sqlite3.connect(db_path)) as c:
            tags = [r[0] for r in c.execute("SELECT name FROM tags ORDER BY id")]
            count = c.execute("SELECT count(*) FROM tags").fetchone()[0]
        real = db.connect_database

        class Boom:
            def __init__(self, conn):
                self.conn = conn

            def execute(self, sql, *a):
                if sql.startswith("DELETE FROM tags"):
                    raise sqlite3.OperationalError("boom")
                return self.conn.execute(sql, *a)

            def __getattr__(self, n):
                return getattr(self.conn, n)

        from contextlib import contextmanager

        @contextmanager
        def wrapped(path, read_only=True):
            with real(path, read_only=read_only) as conn:
                yield conn if read_only else Boom(conn)

        mocker.patch("calibrectl.maintenance.connect_database", wrapped)
        with pytest.raises(sqlite3.OperationalError):
            maintenance.apply_retag(db_path, {tags[0]: None}, backup=False)
        with closing(sqlite3.connect(db_path)) as c:
            assert c.execute("SELECT count(*) FROM tags").fetchone()[0] == count
