"""
Database connection utilities and schema validation for zoteroctl.
"""

import sqlite3
from contextlib import contextmanager
from pathlib import Path
from typing import Any

from zoteroctl.config import get_database_path

# Tables required for zoteroctl to operate correctly
REQUIRED_ZOTERO_TABLES = [
    "items",
    "itemTypes",
    "itemData",
    "itemDataValues",
    "fieldsCombined",
    "itemCreators",
    "creators",
    "creatorTypes",
    "itemTypeCreatorTypes",
    "itemTypeFields",
    "collections",
    "collectionItems",
    "tags",
    "itemTags",
    "itemAttachments",
    "itemNotes",
]

# Cache for schema data (loaded once per execution)
_schema_cache: dict[str, Any] | None = None


@contextmanager
def connect_database(database_path: Path, read_only: bool = True):
    """
    Context manager for database connections.

    Args:
        database_path: Path to SQLite database
        read_only: If True, open in read-only mode

    Yields:
        sqlite3.Connection object

    Raises:
        sqlite3.OperationalError: If database is locked or cannot be opened
    """
    if read_only:
        uri = f"file:{database_path}?mode=ro&immutable=1"
        conn = sqlite3.connect(uri, uri=True)
    else:
        conn = sqlite3.connect(database_path)
        conn.execute("PRAGMA foreign_keys = ON")

    conn.row_factory = sqlite3.Row

    try:
        yield conn
        if not read_only:
            conn.commit()
    except Exception:
        if not read_only:
            conn.rollback()
        raise
    finally:
        conn.close()


def check_required_tables(existing_tables: list[str], required_tables: list[str]) -> list[str]:
    """
    Check which required tables are missing from the database.

    Pure function - no I/O, only set comparison.

    Args:
        existing_tables: List of table names found in the database
        required_tables: List of table names that must exist

    Returns:
        List of missing table names (empty if all present)

    Examples:
        >>> check_required_tables(['items', 'tags'], ['items', 'tags', 'creators'])
        ['creators']
        >>> check_required_tables(['items', 'tags', 'creators'], ['items', 'tags'])
        []
    """
    existing_set = set(existing_tables)
    return sorted([t for t in required_tables if t not in existing_set])


def load_schema(db_path: Path | None = None) -> dict[str, Any]:
    """
    Load Zotero database schema for validation.

    Queries itemTypes, fields, creatorTypes, itemTypeFields, and
    itemTypeCreatorTypes tables to build complete validation schema.

    Args:
        db_path: Path to database (defaults to ZOTERO_DATABASE)

    Returns:
        Schema dictionary with keys:
            - itemTypes: {type_name: type_id}
            - fields: {field_name: field_id}
            - creatorTypes: {creator_type: creator_type_id}
            - itemTypeFields: {item_type_id: [field_id1, field_id2, ...]}
            - itemTypeCreatorTypes: {item_type_id: [creator_type_id1, ...]}
    """
    global _schema_cache

    if _schema_cache is not None:
        return _schema_cache

    if db_path is None:
        db_path = get_database_path()

    conn = sqlite3.connect(str(db_path))
    try:
        schema: dict[str, Any] = {
            "itemTypes": {},
            "fields": {},
            "creatorTypes": {},
            "itemTypeFields": {},
            "itemTypeCreatorTypes": {},
        }

        cursor = conn.execute("SELECT itemTypeID, typeName FROM itemTypes")
        for row in cursor.fetchall():
            schema["itemTypes"][row[1]] = row[0]

        cursor = conn.execute("SELECT fieldID, fieldName FROM fields")
        for row in cursor.fetchall():
            schema["fields"][row[1]] = row[0]

        cursor = conn.execute("SELECT creatorTypeID, creatorType FROM creatorTypes")
        for row in cursor.fetchall():
            schema["creatorTypes"][row[1]] = row[0]

        cursor = conn.execute("SELECT itemTypeID, fieldID FROM itemTypeFields")
        for row in cursor.fetchall():
            item_type_id = row[0]
            field_id = row[1]
            if item_type_id not in schema["itemTypeFields"]:
                schema["itemTypeFields"][item_type_id] = []
            schema["itemTypeFields"][item_type_id].append(field_id)

        cursor = conn.execute("SELECT itemTypeID, creatorTypeID FROM itemTypeCreatorTypes")
        for row in cursor.fetchall():
            item_type_id = row[0]
            creator_type_id = row[1]
            if item_type_id not in schema["itemTypeCreatorTypes"]:
                schema["itemTypeCreatorTypes"][item_type_id] = []
            schema["itemTypeCreatorTypes"][item_type_id].append(creator_type_id)

        _schema_cache = schema
        return schema

    finally:
        conn.close()
