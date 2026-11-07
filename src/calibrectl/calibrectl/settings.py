"""
~/.config/rafa/calibrectl.yaml: database, storage and attach_roots (the directories an MCP
client may attach files from). Precedence: an explicit flag, then CALIBRE_DATABASE and
CALIBRE_STORAGE, then this file, then Calibre's defaults.
"""

import re
from dataclasses import dataclass, field
from pathlib import Path

import yaml


@dataclass(frozen=True)
class Settings:
    database: Path | None = None
    storage: Path | None = None
    attach_roots: tuple[Path, ...] = field(default_factory=tuple)


_VAR = re.compile(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}")


def _expand(value: str, env: dict[str, str], origin: str) -> str:
    def sub(m: re.Match) -> str:
        if m.group(1) not in env:
            raise ValueError(f"{origin}: ${{{m.group(1)}}} is not set")
        return env[m.group(1)]

    return _VAR.sub(sub, value)


def parse(text: str, origin: str, env: dict[str, str]) -> Settings:
    """The file's settings, ${VAR} expanded from env; unknown keys are errors (pure)."""
    try:
        data = yaml.safe_load(text) or {}
    except yaml.YAMLError as exc:
        raise ValueError(f"{origin}: {exc}") from exc
    if not isinstance(data, dict):
        raise ValueError(f"{origin}: expected a mapping")
    unknown = sorted(set(data) - {"version", "database", "storage", "attach_roots"})
    if unknown:
        raise ValueError(f"{origin}: unknown keys {unknown} (version, database, storage, attach_roots)")
    path = lambda k: Path(_expand(str(data[k]), env, origin)) if data.get(k) else None  # noqa: E731
    roots = data.get("attach_roots") or []
    if not isinstance(roots, list):
        raise ValueError(f"{origin}: attach_roots: expected a list of directories")
    return Settings(database=path("database"), storage=path("storage"), attach_roots=tuple(Path(_expand(str(r), env, origin)) for r in roots))


def load(env: dict[str, str]) -> Settings:
    """Read the file named by XDG_CONFIG_HOME (or ~/.config); missing means defaults."""
    base = env.get("XDG_CONFIG_HOME") or f"{env.get('HOME', '')}/.config"
    path = Path(base) / "rafa" / "calibrectl.yaml"
    try:
        text = path.read_text()
    except FileNotFoundError:
        return Settings()
    return parse(text, str(path), env)
