"""Tests for zoteroctl.config module."""

from pathlib import Path

import pytest

from zoteroctl.config import (
    config_candidates,
    default_config_path,
    legacy_config_path,
    resolve_runtime_config,
)


def _write_config(path: Path, body: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(body)


class TestConfigPaths:
    def test_default_config_path_uses_rafa_namespace(self, tmp_path, monkeypatch):
        monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path))

        assert default_config_path() == tmp_path / "rafa" / "zoteroctl" / "config.toml"

    def test_legacy_config_path_kept_for_compatibility(self, tmp_path, monkeypatch):
        monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path))

        assert legacy_config_path() == tmp_path / "zoteroctl" / "config.toml"

    def test_config_candidates_prefer_canonical_then_legacy(self, tmp_path, monkeypatch):
        monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path))

        assert config_candidates() == [
            tmp_path / "rafa" / "zoteroctl" / "config.toml",
            tmp_path / "zoteroctl" / "config.toml",
        ]


class TestResolveRuntimeConfig:
    def test_defaults_use_home_zotero_paths(self, tmp_path, monkeypatch):
        monkeypatch.setenv("HOME", str(tmp_path))
        monkeypatch.delenv("XDG_CONFIG_HOME", raising=False)
        monkeypatch.delenv("ZOTERO_DATABASE", raising=False)
        monkeypatch.delenv("ZOTERO_STORAGE", raising=False)
        monkeypatch.delenv("ZOTERO_TRANSLATION_SERVER", raising=False)
        monkeypatch.delenv("ZOTEROCTL_DATABASE", raising=False)
        monkeypatch.delenv("ZOTEROCTL_STORAGE", raising=False)
        monkeypatch.delenv("ZOTEROCTL_TRANSLATION_SERVER", raising=False)

        config = resolve_runtime_config()

        assert config.database == tmp_path / "Zotero" / "zotero.sqlite"
        assert config.storage == tmp_path / "Zotero" / "storage"
        assert config.translation_server == "http://localhost:1969"
        assert config.config_file is None

    def test_canonical_config_file_is_loaded(self, tmp_path, monkeypatch):
        monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path))
        config_file = tmp_path / "rafa" / "zoteroctl" / "config.toml"
        _write_config(
            config_file,
            'database = "/tmp/library.sqlite"\n'
            'storage = "/tmp/storage"\n'
            'translation_server = "http://example.test:1969"\n',
        )

        config = resolve_runtime_config()

        assert config.database == Path("/tmp/library.sqlite")
        assert config.storage == Path("/tmp/storage")
        assert config.translation_server == "http://example.test:1969"
        assert config.config_file == config_file

    def test_legacy_config_is_used_when_canonical_missing(self, tmp_path, monkeypatch):
        monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path))
        config_file = tmp_path / "zoteroctl" / "config.toml"
        _write_config(config_file, 'database = "/tmp/legacy.sqlite"\n')

        config = resolve_runtime_config()

        assert config.database == Path("/tmp/legacy.sqlite")
        assert config.config_file == config_file

    def test_env_overrides_config_file(self, tmp_path, monkeypatch):
        monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path))
        _write_config(
            tmp_path / "rafa" / "zoteroctl" / "config.toml",
            'database = "/tmp/from-config.sqlite"\n'
            'storage = "/tmp/from-config-storage"\n'
            'translation_server = "http://config.test:1969"\n',
        )
        monkeypatch.setenv("ZOTERO_DATABASE", "/tmp/from-env.sqlite")
        monkeypatch.setenv("ZOTERO_STORAGE", "/tmp/from-env-storage")
        monkeypatch.setenv("ZOTERO_TRANSLATION_SERVER", "http://env.test:1969")

        config = resolve_runtime_config()

        assert config.database == Path("/tmp/from-env.sqlite")
        assert config.storage == Path("/tmp/from-env-storage")
        assert config.translation_server == "http://env.test:1969"

    def test_cli_arguments_override_env_and_config(self, tmp_path, monkeypatch):
        monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path))
        _write_config(
            tmp_path / "rafa" / "zoteroctl" / "config.toml",
            'database = "/tmp/from-config.sqlite"\n'
            'storage = "/tmp/from-config-storage"\n'
            'translation_server = "http://config.test:1969"\n',
        )
        monkeypatch.setenv("ZOTERO_DATABASE", "/tmp/from-env.sqlite")
        monkeypatch.setenv("ZOTERO_STORAGE", "/tmp/from-env-storage")
        monkeypatch.setenv("ZOTERO_TRANSLATION_SERVER", "http://env.test:1969")

        config = resolve_runtime_config(
            database=Path("/tmp/from-cli.sqlite"),
            storage=Path("/tmp/from-cli-storage"),
            translation_server="http://cli.test:1969",
        )

        assert config.database == Path("/tmp/from-cli.sqlite")
        assert config.storage == Path("/tmp/from-cli-storage")
        assert config.translation_server == "http://cli.test:1969"

    def test_explicit_missing_config_path_raises(self, tmp_path):
        with pytest.raises(FileNotFoundError, match="Config file not found"):
            resolve_runtime_config(config_path=tmp_path / "missing.toml")

    def test_invalid_toml_raises_clear_error(self, tmp_path):
        config_file = tmp_path / "broken.toml"
        _write_config(config_file, "database = [\n")

        with pytest.raises(ValueError, match="Invalid TOML"):
            resolve_runtime_config(config_path=config_file)
