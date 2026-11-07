"""
Tests for zoteroctl.validation module.
"""

import pytest

from zoteroctl.validation import (
    classify_integrity_result,
    extract_item_key,
    validate_data_types,
    validate_environment_paths,
    validate_field_names,
    validate_item,
    validate_item_type,
)

# ---------------------------------------------------------------------------
# classify_integrity_result
# ---------------------------------------------------------------------------


class TestClassifyIntegrityResult:
    @pytest.mark.parametrize(
        "result",
        ["ok", "OK", "Ok", "  ok  ", "  OK  ", "ok\n"],
    )
    def test_ok_variants(self, result):
        assert classify_integrity_result(result) == "ok"

    @pytest.mark.parametrize(
        "result",
        [
            "*** in database main ***",
            "error on page 4",
            "Page 4: btree page mismatch",
            "error on page 4\nmore errors",
            "ok2",
            "",
            "  ",
        ],
    )
    def test_error_variants(self, result):
        assert classify_integrity_result(result) == "error"


# ---------------------------------------------------------------------------
# validate_environment_paths
# ---------------------------------------------------------------------------


class TestValidateEnvironmentPaths:
    def test_both_exist(self, tmp_path):
        db = tmp_path / "zotero.sqlite"
        db.write_bytes(b"SQLite")
        storage = tmp_path / "storage"
        storage.mkdir()

        checks = validate_environment_paths(db, storage)
        statuses = {c["check"]: c["status"] for c in checks}
        assert statuses["database_file"] == "ok"
        assert statuses["storage_directory"] == "ok"

    def test_database_missing(self, tmp_path):
        db = tmp_path / "missing.sqlite"
        storage = tmp_path / "storage"
        storage.mkdir()

        checks = validate_environment_paths(db, storage)
        db_check = next(c for c in checks if c["check"] == "database_file")
        assert db_check["status"] == "error"
        assert "not found" in db_check["message"]

    def test_database_is_directory(self, tmp_path):
        db = tmp_path / "notafile"
        db.mkdir()
        storage = tmp_path / "storage"
        storage.mkdir()

        checks = validate_environment_paths(db, storage)
        db_check = next(c for c in checks if c["check"] == "database_file")
        assert db_check["status"] == "error"
        assert "not a file" in db_check["message"]

    def test_storage_missing_is_warning(self, tmp_path):
        db = tmp_path / "zotero.sqlite"
        db.write_bytes(b"SQLite")
        storage = tmp_path / "nostorage"

        checks = validate_environment_paths(db, storage)
        st_check = next(c for c in checks if c["check"] == "storage_directory")
        assert st_check["status"] == "warning"

    def test_storage_is_file_is_error(self, tmp_path):
        db = tmp_path / "zotero.sqlite"
        db.write_bytes(b"SQLite")
        storage = tmp_path / "notadir"
        storage.write_bytes(b"x")

        checks = validate_environment_paths(db, storage)
        st_check = next(c for c in checks if c["check"] == "storage_directory")
        assert st_check["status"] == "error"

    def test_message_contains_path(self, tmp_path):
        db = tmp_path / "zotero.sqlite"
        storage = tmp_path / "storage"
        checks = validate_environment_paths(db, storage)
        for check in checks:
            assert str(tmp_path) in check["message"]

    def test_database_ok_message_includes_size(self, tmp_path):
        db = tmp_path / "zotero.sqlite"
        db.write_bytes(b"SQLite database content here")
        storage = tmp_path / "storage"
        storage.mkdir()
        checks = validate_environment_paths(db, storage)
        db_check = next(c for c in checks if c["check"] == "database_file")
        assert "bytes" in db_check["message"]


# ---------------------------------------------------------------------------
# validate_item_type
# ---------------------------------------------------------------------------


class TestValidateItemType:
    def test_valid_type(self, sample_schema):
        assert validate_item_type({"itemType": "book"}, sample_schema) == []

    def test_missing_type(self, sample_schema):
        errors = validate_item_type({}, sample_schema)
        assert any("Missing required field: itemType" in e for e in errors)

    def test_unknown_type(self, sample_schema):
        errors = validate_item_type({"itemType": "spaceship"}, sample_schema)
        assert any("Unknown itemType" in e for e in errors)

    @pytest.mark.parametrize("item_type", ["book", "journalArticle"])
    def test_all_valid_types(self, sample_schema, item_type):
        assert validate_item_type({"itemType": item_type}, sample_schema) == []


# ---------------------------------------------------------------------------
# validate_field_names
# ---------------------------------------------------------------------------


class TestValidateFieldNames:
    def test_valid_fields(self, sample_schema):
        data = {"itemType": "book", "title": "T", "date": "2024"}
        assert validate_field_names(data, sample_schema) == []

    def test_unknown_field(self, sample_schema):
        data = {"itemType": "book", "nonexistent": "x"}
        errors = validate_field_names(data, sample_schema)
        assert len(errors) > 0
        assert any("Unknown field" in e for e in errors)

    def test_reserved_fields_skipped(self, sample_schema):
        data = {
            "itemType": "book",
            "key": "ABC",
            "itemKey": "ABC",
            "creators": [],
            "tags": [],
        }
        assert validate_field_names(data, sample_schema) == []

    def test_unknown_item_type_returns_empty(self, sample_schema):
        data = {"itemType": "unknown", "someField": "x"}
        assert validate_field_names(data, sample_schema) == []

    def test_field_not_valid_for_type(self, sample_schema):
        # DOI is only in journalArticle (id 2), not book (id 1)
        data = {"itemType": "book", "DOI": "10.1234/x"}
        errors = validate_field_names(data, sample_schema)
        assert any("not valid for itemType" in e for e in errors)


# ---------------------------------------------------------------------------
# validate_data_types
# ---------------------------------------------------------------------------


class TestValidateDataTypes:
    def test_valid_creators(self):
        data = {"creators": [{"creatorType": "author", "firstName": "J", "lastName": "Doe"}]}
        assert validate_data_types(data) == []

    def test_creators_not_list(self):
        errors = validate_data_types({"creators": "not a list"})
        assert any("must be list" in e for e in errors)

    def test_creator_not_dict(self):
        errors = validate_data_types({"creators": ["notadict"]})
        assert any("must be dict" in e for e in errors)

    def test_creator_missing_creator_type(self):
        errors = validate_data_types({"creators": [{"firstName": "J", "lastName": "D"}]})
        assert any("creatorType" in e for e in errors)

    def test_creator_missing_last_name(self):
        errors = validate_data_types({"creators": [{"creatorType": "author", "firstName": "J"}]})
        assert any("lastName" in e for e in errors)

    def test_creator_both_name_styles(self):
        errors = validate_data_types(
            {
                "creators": [
                    {"creatorType": "author", "firstName": "J", "lastName": "D", "name": "X"}
                ]
            }
        )
        assert any("cannot have both" in e for e in errors)

    def test_creator_no_name_at_all(self):
        errors = validate_data_types({"creators": [{"creatorType": "author"}]})
        assert any("must have either" in e for e in errors)

    def test_creator_institutional_name_ok(self):
        errors = validate_data_types({"creators": [{"creatorType": "author", "name": "CERN"}]})
        assert errors == []

    def test_tags_not_list(self):
        errors = validate_data_types({"tags": "not a list"})
        assert any("tags" in e and "must be list" in e for e in errors)

    def test_tags_list_ok(self):
        assert validate_data_types({"tags": ["a", "b"]}) == []

    def test_attachments_not_list(self):
        errors = validate_data_types({"attachments": "bad"})
        assert any("attachments" in e and "must be list" in e for e in errors)

    def test_attachments_item_not_dict(self):
        errors = validate_data_types({"attachments": ["notadict"]})
        assert any("must be dict" in e for e in errors)

    def test_notes_not_list(self):
        errors = validate_data_types({"notes": 42})
        assert any("notes" in e and "must be list" in e for e in errors)

    def test_notes_item_not_dict(self):
        errors = validate_data_types({"notes": ["notadict"]})
        assert any("must be dict" in e for e in errors)

    def test_empty_data(self):
        assert validate_data_types({}) == []


# ---------------------------------------------------------------------------
# validate_item
# ---------------------------------------------------------------------------


class TestValidateItem:
    def test_valid_item(self, sample_schema):
        data = {
            "itemType": "book",
            "title": "T",
            "creators": [{"creatorType": "author", "lastName": "Doe"}],
        }
        assert validate_item(data, sample_schema) == []

    def test_accumulates_errors(self, sample_schema):
        # Missing itemType AND bad creator
        data = {"creators": "not a list"}
        errors = validate_item(data, sample_schema)
        assert len(errors) >= 2

    def test_missing_item_type(self, sample_schema):
        errors = validate_item({}, sample_schema)
        assert any("itemType" in e for e in errors)


# ---------------------------------------------------------------------------
# extract_item_key
# ---------------------------------------------------------------------------


class TestExtractItemKey:
    def test_valid_key(self):
        assert extract_item_key({"itemKey": "ABC12345"}) == "ABC12345"

    def test_all_digits(self):
        assert extract_item_key({"itemKey": "12345678"}) == "12345678"

    def test_all_uppercase_letters(self):
        assert extract_item_key({"itemKey": "ABCDEFGH"}) == "ABCDEFGH"

    def test_missing_key_raises(self):
        with pytest.raises(ValueError, match="Missing required field"):
            extract_item_key({})

    def test_empty_string_raises(self):
        with pytest.raises(ValueError, match="cannot be empty"):
            extract_item_key({"itemKey": ""})

    def test_whitespace_only_raises(self):
        with pytest.raises(ValueError, match="cannot be empty"):
            extract_item_key({"itemKey": "   "})

    def test_invalid_format_raises(self):
        with pytest.raises(ValueError, match="8 uppercase alphanumeric"):
            extract_item_key({"itemKey": "abc"})

    def test_lowercase_raises(self):
        with pytest.raises(ValueError, match="8 uppercase alphanumeric"):
            extract_item_key({"itemKey": "abcdefgh"})

    def test_too_short_raises(self):
        with pytest.raises(ValueError):
            extract_item_key({"itemKey": "ABCD"})

    def test_too_long_raises(self):
        with pytest.raises(ValueError):
            extract_item_key({"itemKey": "ABCDEFGHI"})

    def test_non_string_raises(self):
        with pytest.raises(ValueError, match="must be string"):
            extract_item_key({"itemKey": 12345678})

    def test_special_chars_raises(self):
        with pytest.raises(ValueError):
            extract_item_key({"itemKey": "ABC-1234"})
