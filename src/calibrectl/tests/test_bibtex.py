"""
Tests for calibrectl.bibtex
"""

import pytest

from calibrectl.bibtex import book_to_bibtex, escape_bibtex, export_bibtex_books, generate_cite_key


class TestEscapeBibtex:
    @pytest.mark.parametrize(
        "char, escaped",
        [
            ("&", "\\&"),
            ("%", "\\%"),
            ("$", "\\$"),
            ("#", "\\#"),
            ("_", "\\_"),
            ("~", "\\textasciitilde{}"),
            ("^", "\\textasciicircum{}"),
            ("{", "\\{"),
            ("}", "\\}"),
            ("\\", "\\textbackslash{}"),
        ],
    )
    def test_special_char_escaped(self, char, escaped):
        result = escape_bibtex(f"prefix{char}suffix")
        assert escaped in result

    def test_empty_string(self):
        assert escape_bibtex("") == ""

    def test_none(self):
        assert escape_bibtex(None) == ""

    def test_plain_text_unchanged(self):
        assert escape_bibtex("Hello World") == "Hello World"

    def test_multiple_chars(self):
        result = escape_bibtex("R&D at 100%")
        assert "\\&" in result
        assert "\\%" in result


class TestGenerateCiteKey:
    def test_basic_author_year(self):
        book = {"id": 1, "publication_date": "2024-01-01", "authors": ["John Doe"]}
        used: set = set()
        key = generate_cite_key(book, used)
        assert key == "Doe2024"
        assert "Doe2024" in used

    def test_adds_to_used_keys(self):
        book = {"id": 1, "publication_date": "2024", "authors": ["Smith"]}
        used: set = set()
        generate_cite_key(book, used)
        assert len(used) == 1

    def test_dedup_with_suffix_a(self):
        book = {"id": 1, "publication_date": "2024", "authors": ["John Doe"]}
        used = {"Doe2024"}
        key = generate_cite_key(book, used)
        assert key == "Doe2024a"

    def test_dedup_with_suffix_b(self):
        book = {"id": 1, "publication_date": "2024", "authors": ["John Doe"]}
        used = {"Doe2024", "Doe2024a"}
        key = generate_cite_key(book, used)
        assert key == "Doe2024b"

    def test_no_author_uses_book_id(self):
        book = {"id": 42, "publication_date": "2020", "authors": []}
        used: set = set()
        key = generate_cite_key(book, used)
        assert "Book42" in key
        assert "2020" in key

    def test_no_date_uses_nodate(self):
        book = {"id": 1, "publication_date": None, "authors": ["Doe"]}
        used: set = set()
        key = generate_cite_key(book, used)
        assert "NODATE" in key

    def test_empty_date_uses_nodate(self):
        book = {"id": 1, "publication_date": "", "authors": ["Doe"]}
        used: set = set()
        key = generate_cite_key(book, used)
        assert "NODATE" in key

    def test_multiple_authors_uses_first(self):
        book = {
            "id": 1,
            "publication_date": "2021",
            "authors": ["Alice Walker", "Bob Smith"],
        }
        used: set = set()
        key = generate_cite_key(book, used)
        assert "Walker" in key

    def test_overflow_27th_collision_uses_numeric_suffix(self):
        book = {"id": 1, "publication_date": "2024", "authors": ["Security"]}
        used = {"Security2024"} | {f"Security2024{chr(ord('a') + i)}" for i in range(26)}
        key = generate_cite_key(book, used)
        assert key == "Security202427"
        assert key in used

    def test_overflow_140th_collision_unique(self):
        book = {"id": 1, "publication_date": "2024", "authors": ["Security"]}
        used = {"Security2024"}
        used |= {f"Security2024{chr(ord('a') + i)}" for i in range(26)}
        used |= {f"Security2024{n}" for n in range(27, 140)}
        key = generate_cite_key(book, used)
        assert key == "Security2024140"

    def test_no_duplicate_in_large_batch(self):
        book_template = {"id": 1, "publication_date": "2024", "authors": ["Nist"]}
        used: set = set()
        keys = [generate_cite_key(book_template, used) for _ in range(200)]
        assert len(keys) == len(set(keys)), "Duplicate cite keys generated"


class TestBookToBibtex:
    def _make_book(self, **kwargs):
        defaults = {
            "id": 1,
            "title": "Test Book",
            "authors": ["John Doe"],
            "publication_date": "2024-01-01",
            "publisher": None,
            "isbn": None,
            "series": None,
            "abstract": None,
        }
        defaults.update(kwargs)
        return defaults

    def test_basic_entry(self):
        book = self._make_book()
        entry = book_to_bibtex(book, set())
        assert "@book{" in entry
        assert "title = {Test Book}" in entry
        assert "author = {John Doe}" in entry
        assert "year = {2024}" in entry

    def test_multiple_authors_joined(self):
        book = self._make_book(authors=["Alice Walker", "Bob Smith"])
        entry = book_to_bibtex(book, set())
        assert "Alice Walker and Bob Smith" in entry

    def test_empty_authors_no_author_field(self):
        book = self._make_book(authors=[])
        entry = book_to_bibtex(book, set())
        assert "author" not in entry

    def test_isbn_present(self):
        book = self._make_book(isbn="9780596005900")
        entry = book_to_bibtex(book, set())
        assert "isbn = {9780596005900}" in entry

    def test_no_isbn(self):
        book = self._make_book(isbn=None)
        entry = book_to_bibtex(book, set())
        assert "isbn" not in entry

    def test_no_date(self):
        book = self._make_book(publication_date=None)
        entry = book_to_bibtex(book, set())
        assert "year" not in entry

    def test_series_present(self):
        book = self._make_book(series="Foundation Series")
        entry = book_to_bibtex(book, set())
        assert "series = {Foundation Series}" in entry

    def test_abstract_escaped(self):
        book = self._make_book(abstract="50% off & $10 deal")
        entry = book_to_bibtex(book, set())
        assert "\\%" in entry
        assert "\\&" in entry
        assert "\\$" in entry

    def test_no_trailing_comma_on_last_field(self):
        book = self._make_book()
        entry = book_to_bibtex(book, set())
        lines = entry.strip().splitlines()
        # Last line should be "}"
        assert lines[-1] == "}"
        # Second to last should not end with comma
        assert not lines[-2].endswith(",")

    def test_unique_keys_across_books(self):
        book1 = self._make_book(id=1, authors=["Doe"])
        book2 = self._make_book(id=2, authors=["Doe"])
        used: set = set()
        entry1 = book_to_bibtex(book1, used)
        entry2 = book_to_bibtex(book2, used)
        # Extract cite keys
        key1 = entry1.split("{")[1].split(",")[0]
        key2 = entry2.split("{")[1].split(",")[0]
        assert key1 != key2


class TestExportBibtexBooks:
    def test_export_all_returns_empty_for_no_args(self, calibre_db_file):
        entries = export_bibtex_books(calibre_db_file)
        # No book_id or tag — should return all books (one for each in fixture)
        assert isinstance(entries, list)

    def test_export_by_book_id(self, calibre_db_file):
        entries = export_bibtex_books(calibre_db_file, book_id=1)
        assert len(entries) == 1
        assert "Test Book" in entries[0]

    def test_export_nonexistent_book_id(self, calibre_db_file):
        entries = export_bibtex_books(calibre_db_file, book_id=9999)
        assert entries == []

    def test_export_by_tag(self, calibre_db_file):
        entries = export_bibtex_books(calibre_db_file, tag="fiction")
        assert len(entries) == 1
        assert "Test Book" in entries[0]

    def test_export_by_nonexistent_tag(self, calibre_db_file):
        entries = export_bibtex_books(calibre_db_file, tag="nonexistent")
        assert entries == []
