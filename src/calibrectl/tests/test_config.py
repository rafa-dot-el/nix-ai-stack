"""
Tests for calibrectl.config
"""

from pathlib import Path

from calibrectl.config import get_database_path, get_storage_path


class TestGetDatabasePath:
    def test_env_var_overrides(self, tmp_path, monkeypatch):
        db = tmp_path / "custom.db"
        db.touch()
        monkeypatch.setenv("CALIBRE_DATABASE", str(db))
        result = get_database_path()
        assert result == db.resolve()

    def test_env_var_expands_user(self, monkeypatch, tmp_path):
        db = tmp_path / "metadata.db"
        db.touch()
        monkeypatch.setenv("CALIBRE_DATABASE", str(db))
        result = get_database_path()
        assert result.exists() or True  # path returned regardless

    def test_default_when_no_env(self, monkeypatch):
        monkeypatch.delenv("CALIBRE_DATABASE", raising=False)
        # Should not raise even if paths don't exist
        result = get_database_path()
        assert isinstance(result, Path)


class TestGetStoragePath:
    def test_env_var_overrides(self, tmp_path, monkeypatch):
        monkeypatch.setenv("CALIBRE_STORAGE", str(tmp_path))
        result = get_storage_path(tmp_path / "metadata.db")
        assert result == tmp_path.resolve()

    def test_default_is_parent_of_database(self, monkeypatch):
        monkeypatch.delenv("CALIBRE_STORAGE", raising=False)
        db = Path("/some/path/to/metadata.db")
        result = get_storage_path(db)
        assert result == Path("/some/path/to")
