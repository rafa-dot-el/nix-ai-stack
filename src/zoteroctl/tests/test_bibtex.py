"""
Tests for zoteroctl.bibtex module.
"""

import pytest

from zoteroctl.bibtex import (
    convert_bibtex_to_yaml,
    escape_bibtex,
    generate_cite_key,
    item_to_bibtex,
    map_bibtex_type,
    parse_bibtex_authors,
)

# ---------------------------------------------------------------------------
# escape_bibtex
# ---------------------------------------------------------------------------


class TestEscapeBibtex:
    @pytest.mark.parametrize(
        "char,expected_fragment",
        [
            ("{", "\\{"),
            ("}", "\\}"),
            ("\\", "\\textbackslash{}"),
            ("$", "\\$"),
            ("%", "\\%"),
            ("&", "\\&"),
            ("#", "\\#"),
            ("_", "\\_"),
            ("^", "\\textasciicircum{}"),
            ("~", "\\textasciitilde{}"),
        ],
    )
    def test_special_chars(self, char, expected_fragment):
        result = escape_bibtex(f"a{char}b")
        assert expected_fragment in result

    def test_empty_string(self):
        assert escape_bibtex("") == ""

    def test_none_returns_empty(self):
        assert escape_bibtex(None) == ""

    def test_plain_text_unchanged(self):
        assert escape_bibtex("Hello World") == "Hello World"

    def test_ampersand_in_title(self):
        assert "\\&" in escape_bibtex("Science & Technology")

    def test_percent_in_text(self):
        assert "\\%" in escape_bibtex("100%")

    def test_multiple_specials(self):
        result = escape_bibtex("$100 & 50%")
        assert "\\$" in result
        assert "\\&" in result
        assert "\\%" in result

    def test_unicode_preserved(self):
        result = escape_bibtex("Über")
        assert "Über" in result

    def test_backslash_replaced_before_braces(self):
        # Backslash must be replaced first to avoid double-escaping
        result = escape_bibtex("\\{")
        assert "\\textbackslash{}" in result


# ---------------------------------------------------------------------------
# generate_cite_key
# ---------------------------------------------------------------------------


class TestGenerateCiteKey:
    def _make_item(self, last_name="Einstein", date="2024", key="ABC12345", field_mode=0):
        creator = {"fieldMode": field_mode, "creatorType": "author"}
        if field_mode == 0:
            creator["lastName"] = last_name
            creator["firstName"] = "Albert"
        else:
            creator["name"] = last_name
        return {"key": key, "fields": {"date": date}, "creators": [creator]}

    def test_basic_key(self):
        item = self._make_item()
        used = set()
        key = generate_cite_key(item, used)
        assert key == "Einstein2024"
        assert "Einstein2024" in used

    def test_duplicate_gets_suffix(self):
        item = self._make_item()
        used = {"Einstein2024"}
        key = generate_cite_key(item, used)
        assert key == "Einstein2024a"

    def test_multiple_duplicates(self):
        item = self._make_item()
        used = {"Einstein2024", "Einstein2024a", "Einstein2024b"}
        key = generate_cite_key(item, used)
        assert key == "Einstein2024c"

    def test_no_author_uses_item_key(self):
        item = {"key": "XYZ99999", "fields": {"date": "2020"}, "creators": []}
        key = generate_cite_key(item, set())
        assert "XYZ99999" in key
        assert "2020" in key

    def test_no_date_uses_nodate(self):
        item = {
            "key": "ABC12345",
            "fields": {},
            "creators": [{"fieldMode": 0, "lastName": "Doe", "firstName": "J"}],
        }
        key = generate_cite_key(item, set())
        assert "NODATE" in key

    def test_non_ascii_lastname_cleaned(self):
        item = self._make_item(last_name="Müller")
        key = generate_cite_key(item, set())
        assert "Mller" in key or "ller" in key  # non-alpha stripped

    def test_single_field_mode_author(self):
        item = {
            "key": "ABC12345",
            "fields": {"date": "2021"},
            "creators": [{"fieldMode": 1, "name": "CERN Collaboration"}],
        }
        key = generate_cite_key(item, set())
        assert "Collaboration" in key or "2021" in key

    def test_used_keys_updated(self):
        item = self._make_item()
        used: set = set()
        generate_cite_key(item, used)
        assert len(used) == 1

    def test_overflow_27th_collision_uses_numeric_suffix(self):
        item = self._make_item(last_name="Security", date="2024")
        used = {"Security2024"} | {f"Security2024{chr(ord('a') + i)}" for i in range(26)}
        key = generate_cite_key(item, used)
        assert key == "Security202427"
        assert key in used

    def test_overflow_140th_collision_unique(self):
        item = self._make_item(last_name="Security", date="2024")
        used = {"Security2024"}
        used |= {f"Security2024{chr(ord('a') + i)}" for i in range(26)}
        used |= {f"Security2024{n}" for n in range(27, 140)}
        key = generate_cite_key(item, used)
        assert key == "Security2024140"
        assert key not in used - {"Security2024140"}

    def test_no_duplicate_in_large_batch(self):
        item_template = {
            "key": "DUMMY000",
            "fields": {"date": "2024"},
            "creators": [{"fieldMode": 0, "lastName": "Nist", "firstName": "X"}],
        }
        used: set = set()
        keys = [generate_cite_key(item_template, used) for _ in range(200)]
        assert len(keys) == len(set(keys)), "Duplicate cite keys generated"


# ---------------------------------------------------------------------------
# map_bibtex_type
# ---------------------------------------------------------------------------


class TestMapBibtexType:
    @pytest.mark.parametrize(
        "entry_type,expected",
        [
            ("article", "journalArticle"),
            ("book", "book"),
            ("inproceedings", "conferencePaper"),
            ("incollection", "bookSection"),
            ("phdthesis", "thesis"),
            ("mastersthesis", "thesis"),
            ("techreport", "report"),
            ("misc", "document"),
            ("unpublished", "manuscript"),
            ("proceedings", "book"),
            ("inbook", "bookSection"),
        ],
    )
    def test_known_types(self, entry_type, expected):
        assert map_bibtex_type(entry_type) == expected

    def test_unknown_type_returns_document(self):
        assert map_bibtex_type("weirdtype") == "document"

    def test_case_insensitive(self):
        assert map_bibtex_type("ARTICLE") == "journalArticle"
        assert map_bibtex_type("Book") == "book"


# ---------------------------------------------------------------------------
# parse_bibtex_authors
# ---------------------------------------------------------------------------


class TestParseBibtexAuthors:
    def test_last_comma_first(self):
        result = parse_bibtex_authors("Doe, John")
        assert len(result) == 1
        assert result[0]["lastName"] == "Doe"
        assert result[0]["firstName"] == "John"

    def test_multiple_authors(self):
        result = parse_bibtex_authors("Doe, John and Smith, Jane")
        assert len(result) == 2
        assert result[1]["lastName"] == "Smith"

    def test_first_last_format(self):
        result = parse_bibtex_authors("Albert Einstein")
        assert result[0]["firstName"] == "Albert"
        assert result[0]["lastName"] == "Einstein"

    def test_single_name(self):
        result = parse_bibtex_authors("Aristotle")
        assert result[0]["lastName"] == "Aristotle"
        assert result[0]["firstName"] == ""

    def test_creator_type_is_author(self):
        result = parse_bibtex_authors("Doe, John")
        assert result[0]["creatorType"] == "author"

    def test_unicode_name(self):
        result = parse_bibtex_authors("Müller, Hans")
        assert result[0]["lastName"] == "Müller"

    def test_three_authors(self):
        result = parse_bibtex_authors("A, B and C, D and E, F")
        assert len(result) == 3


# ---------------------------------------------------------------------------
# convert_bibtex_to_yaml
# ---------------------------------------------------------------------------


class TestConvertBibtexToYaml:
    def test_article_type_mapping(self):
        entry = {"ENTRYTYPE": "article", "ID": "doe2020", "title": "Test"}
        result = convert_bibtex_to_yaml(entry)
        assert result["itemType"] == "journalArticle"

    def test_book_type_mapping(self):
        entry = {"ENTRYTYPE": "book", "ID": "b1", "title": "A Book"}
        result = convert_bibtex_to_yaml(entry)
        assert result["itemType"] == "book"

    def test_field_mapping(self):
        entry = {
            "ENTRYTYPE": "article",
            "ID": "d2020",
            "title": "Test Paper",
            "year": "2020",
            "journal": "Nature",
            "doi": "10.1/x",
        }
        result = convert_bibtex_to_yaml(entry)
        assert result["fields"]["title"] == "Test Paper"
        assert result["fields"]["date"] == "2020"
        assert result["fields"]["publicationTitle"] == "Nature"
        assert result["fields"]["DOI"] == "10.1/x"

    def test_author_parsed(self):
        entry = {
            "ENTRYTYPE": "article",
            "ID": "d2020",
            "author": "Doe, John and Smith, Jane",
        }
        result = convert_bibtex_to_yaml(entry)
        assert len(result["creators"]) == 2
        assert result["creators"][0]["lastName"] == "Doe"

    def test_editor_creator_type(self):
        entry = {"ENTRYTYPE": "book", "ID": "b1", "editor": "Jones, Bob"}
        result = convert_bibtex_to_yaml(entry)
        editors = [c for c in result["creators"] if c["creatorType"] == "editor"]
        assert len(editors) == 1
        assert editors[0]["lastName"] == "Jones"

    def test_keywords_to_tags(self):
        entry = {"ENTRYTYPE": "article", "ID": "x", "keywords": "AI, ML; NLP"}
        result = convert_bibtex_to_yaml(entry)
        assert "AI" in result["tags"]
        assert "ML" in result["tags"]
        assert "NLP" in result["tags"]

    def test_empty_entry(self):
        entry = {"ENTRYTYPE": "misc", "ID": "x"}
        result = convert_bibtex_to_yaml(entry)
        assert result["itemType"] == "document"
        assert result["creators"] == []
        assert result["tags"] == []


# ---------------------------------------------------------------------------
# Round-trip: convert_bibtex_to_yaml -> item_to_bibtex
# ---------------------------------------------------------------------------


class TestBibtexRoundTrip:
    def test_round_trip_preserves_title(self):
        """YAML item -> BibTeX -> should contain the title."""
        item = {
            "key": "ABC12345",
            "itemType": "journalArticle",
            "fields": {"title": "A Great Paper", "date": "2023"},
            "creators": [
                {"fieldMode": 0, "firstName": "John", "lastName": "Doe", "creatorType": "author"}
            ],
            "tags": [],
            "collections": [],
        }
        used: set = set()
        bibtex_str = item_to_bibtex(item, used)
        assert "A Great Paper" in bibtex_str

    def test_round_trip_preserves_author(self):
        item = {
            "key": "ABC12345",
            "itemType": "journalArticle",
            "fields": {"title": "Paper", "date": "2023"},
            "creators": [
                {
                    "fieldMode": 0,
                    "firstName": "Albert",
                    "lastName": "Einstein",
                    "creatorType": "author",
                }
            ],
            "tags": [],
        }
        used: set = set()
        bibtex_str = item_to_bibtex(item, used)
        assert "Einstein" in bibtex_str

    def test_bibtex_type_in_output(self):
        item = {
            "key": "ABC12345",
            "itemType": "book",
            "fields": {"title": "Book", "date": "2020"},
            "creators": [],
            "tags": [],
        }
        bibtex_str = item_to_bibtex(item, set())
        assert "@book{" in bibtex_str

    def test_year_extracted_from_date(self):
        item = {
            "key": "ABC12345",
            "itemType": "journalArticle",
            "fields": {"title": "X", "date": "2019-05-01"},
            "creators": [],
            "tags": [],
        }
        bibtex_str = item_to_bibtex(item, set())
        assert "year = {2019}" in bibtex_str
