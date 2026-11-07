"""
Tests for calibrectl.validation
"""

import pytest

from calibrectl.validation import classify_integrity_result, validate_environment_paths


@pytest.mark.parametrize(
    "result, expected",
    [
        ("ok", "ok"),
        ("OK", "ok"),
        ("  ok  ", "ok"),
        ("Ok", "ok"),
        ("*** in database main ***", "error"),
        ("Page 4: btree imbalance", "error"),
        ("", "error"),
        ("not ok", "error"),
        ("ok extra", "error"),
    ],
)
def test_classify_integrity_result(result, expected):
    assert classify_integrity_result(result) == expected


class TestValidateEnvironmentPaths:
    def test_both_exist(self, tmp_path):
        db = tmp_path / "metadata.db"
        db.write_bytes(b"")
        storage = tmp_path / "storage"
        storage.mkdir()

        checks = validate_environment_paths(db, storage)
        statuses = {c["check"]: c["status"] for c in checks}
        assert statuses["database_file"] == "ok"
        assert statuses["storage_directory"] == "ok"

    def test_missing_database(self, tmp_path):
        db = tmp_path / "nonexistent.db"
        storage = tmp_path
        checks = validate_environment_paths(db, storage)
        statuses = {c["check"]: c["status"] for c in checks}
        assert statuses["database_file"] == "error"

    def test_missing_storage(self, tmp_path):
        db = tmp_path / "metadata.db"
        db.write_bytes(b"")
        storage = tmp_path / "missing_storage"
        checks = validate_environment_paths(db, storage)
        statuses = {c["check"]: c["status"] for c in checks}
        assert statuses["storage_directory"] == "warning"

    def test_database_is_directory(self, tmp_path):
        db = tmp_path / "a_directory"
        db.mkdir()
        storage = tmp_path
        checks = validate_environment_paths(db, storage)
        statuses = {c["check"]: c["status"] for c in checks}
        assert statuses["database_file"] == "error"

    def test_storage_is_file(self, tmp_path):
        db = tmp_path / "metadata.db"
        db.write_bytes(b"")
        storage = tmp_path / "a_file"
        storage.write_bytes(b"")
        checks = validate_environment_paths(db, storage)
        statuses = {c["check"]: c["status"] for c in checks}
        assert statuses["storage_directory"] == "error"

    def test_check_keys_present(self, tmp_path):
        db = tmp_path / "metadata.db"
        db.write_bytes(b"")
        storage = tmp_path
        checks = validate_environment_paths(db, storage)
        for check in checks:
            assert "check" in check
            assert "status" in check
            assert "message" in check

    def test_database_size_in_message(self, tmp_path):
        db = tmp_path / "metadata.db"
        db.write_bytes(b"x" * 100)
        storage = tmp_path
        checks = validate_environment_paths(db, storage)
        db_check = next(c for c in checks if c["check"] == "database_file")
        assert "100 bytes" in db_check["message"]
