"""
Environment variable loading and path construction helpers.
"""

import os
from pathlib import Path


def get_database_path() -> Path:
    """
    Get database path from environment variable or default.

    Checks locations in order:
    1. CALIBRE_DATABASE environment variable (if set)
    2. ~/Calibre Library/metadata.db (standard Calibre location)
    3. ~/books/metadata.db (common alternative location)

    Returns:
        Path: Database file path (first found, or standard location if none exist)
    """
    if "CALIBRE_DATABASE" in os.environ:
        return Path(os.environ["CALIBRE_DATABASE"]).expanduser().resolve()

    from calibrectl.settings import load

    configured = load(dict(os.environ)).database
    if configured:
        return configured

    standard_path = Path.home() / "Calibre Library" / "metadata.db"
    if standard_path.exists():
        return standard_path

    books_path = Path.home() / "books" / "metadata.db"
    if books_path.exists():
        return books_path

    return standard_path


def get_storage_path(database_path: Path) -> Path:
    """
    Get storage directory path from environment variable or default to database directory.

    Args:
        database_path: Path to database file

    Returns:
        Path: Storage directory path
    """
    storage = os.environ.get("CALIBRE_STORAGE")
    if storage:
        return Path(storage).expanduser().resolve()

    from calibrectl.settings import load

    configured = load(dict(os.environ)).storage
    if configured:
        return configured
    return database_path.parent
