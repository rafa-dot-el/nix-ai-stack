"""Runtime configuration loading for zoteroctl."""

from __future__ import annotations

import os
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any

_DEFAULT_TRANSLATION_SERVER = "http://localhost:1969"


def _expand_path(path_str: str) -> Path:
    """Expand environment variables and user home in a filesystem path."""
    return Path(os.path.expandvars(path_str)).expanduser()


def _xdg_config_home() -> Path:
    """Return the XDG config directory, defaulting to ``~/.config``."""
    return _expand_path(os.environ.get("XDG_CONFIG_HOME", "~/.config"))


def _default_database_path() -> Path:
    """Return the fallback Zotero database path."""
    return Path.home() / "Zotero" / "zotero.sqlite"


def _default_storage_path() -> Path:
    """Return the fallback Zotero storage path."""
    return Path.home() / "Zotero" / "storage"


def _config_path_from_env() -> Path | None:
    """Return an explicit config file from env vars, if configured."""
    for name in ("ZOTEROCTL_CONFIG", "ZOTERO_CONFIG"):
        value = os.environ.get(name)
        if value:
            return _expand_path(value)
    return None


def default_config_path() -> Path:
    """Canonical config path under ``$XDG_CONFIG_HOME/rafa/zoteroctl``."""
    return _xdg_config_home() / "rafa" / "zoteroctl" / "config.toml"


def legacy_config_path() -> Path:
    """Legacy config path kept for compatibility."""
    return _xdg_config_home() / "zoteroctl" / "config.toml"


def config_candidates(config_path: Path | None = None) -> list[Path]:
    """Return config search candidates in precedence order."""
    explicit_path = config_path or _config_path_from_env()
    if explicit_path is not None:
        return [explicit_path.expanduser()]
    return [default_config_path(), legacy_config_path()]


def _read_config_file(config_path: Path | None = None) -> tuple[dict[str, Any], Path | None]:
    """Load the first existing TOML config file."""
    candidates = config_candidates(config_path)

    for candidate in candidates:
        if candidate.exists():
            try:
                with candidate.open("rb") as handle:
                    data = tomllib.load(handle)
            except tomllib.TOMLDecodeError as exc:
                raise ValueError(f"Invalid TOML in config file {candidate}: {exc}") from exc
            if not isinstance(data, dict):
                raise ValueError(f"Config file {candidate} must contain a TOML table")
            return data, candidate

    explicit_path = config_path or _config_path_from_env()
    if explicit_path is not None:
        raise FileNotFoundError(f"Config file not found: {explicit_path}")

    return {}, None


def _get_first_env(*names: str) -> str | None:
    """Return the first non-empty environment variable value."""
    for name in names:
        value = os.environ.get(name)
        if value:
            return value
    return None


@dataclass(frozen=True)
class RuntimeConfig:
    """Fully resolved runtime configuration."""

    database: Path
    storage: Path
    translation_server: str
    config_file: Path | None = None


def resolve_runtime_config(
    *,
    database: Path | None = None,
    storage: Path | None = None,
    translation_server: str | None = None,
    config_path: Path | None = None,
) -> RuntimeConfig:
    """Resolve runtime configuration from CLI, env, config file, and defaults."""
    file_config, file_path = _read_config_file(config_path)

    database_value = database or _expand_path(
        _get_first_env("ZOTERO_DATABASE", "ZOTEROCTL_DATABASE")
        or str(file_config.get("database", _default_database_path()))
    )
    storage_value = storage or _expand_path(
        _get_first_env("ZOTERO_STORAGE", "ZOTEROCTL_STORAGE")
        or str(file_config.get("storage", _default_storage_path()))
    )
    translation_value = (
        translation_server
        or _get_first_env("ZOTERO_TRANSLATION_SERVER", "ZOTEROCTL_TRANSLATION_SERVER")
        or str(file_config.get("translation_server", _DEFAULT_TRANSLATION_SERVER))
    )

    return RuntimeConfig(
        database=database_value.expanduser(),
        storage=storage_value.expanduser(),
        translation_server=translation_value.strip(),
        config_file=file_path,
    )


def get_database_path(config_path: Path | None = None) -> Path:
    """Get the resolved database path."""
    return resolve_runtime_config(config_path=config_path).database


def get_storage_path(config_path: Path | None = None) -> Path:
    """Get the resolved storage path."""
    return resolve_runtime_config(config_path=config_path).storage


def get_translation_server_url(config_path: Path | None = None) -> str:
    """Get the resolved translation server URL."""
    return resolve_runtime_config(config_path=config_path).translation_server
