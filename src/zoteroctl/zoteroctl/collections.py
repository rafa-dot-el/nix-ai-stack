"""
Collection CRUD helpers for zoteroctl.
"""

import random
import sqlite3
import string
from typing import Any

import click


def generate_collection_key(conn: sqlite3.Connection, library_id: int = 1) -> str:
    """
    Generate unique 8-character key for collections.

    Args:
        conn: Database connection
        library_id: Library ID to check uniqueness against

    Returns:
        Unique 8-character uppercase alphanumeric key
    """
    chars = string.ascii_uppercase + string.digits
    while True:
        key = "".join(random.choices(chars, k=8))
        cursor = conn.execute(
            "SELECT 1 FROM collections WHERE libraryID = ? AND key = ?",
            (library_id, key),
        )
        if cursor.fetchone() is None:
            return key


def create_collection_record(
    conn: sqlite3.Connection,
    name: str,
    parent_key: str | None = None,
    library_id: int = 1,
) -> str:
    """
    Create a new collection in the database.

    Args:
        conn: Database connection
        name: Collection name
        parent_key: Parent collection key (None for top-level)
        library_id: Library ID (default: 1)

    Returns:
        New collection key

    Raises:
        ValueError: If parent collection not found
    """
    parent_id = None
    if parent_key:
        cursor = conn.execute(
            "SELECT collectionID FROM collections WHERE key = ?",
            (parent_key,),
        )
        row = cursor.fetchone()
        if row is None:
            raise ValueError(f"Parent collection not found: {parent_key}")
        parent_id = row["collectionID"]

    collection_key = generate_collection_key(conn, library_id)

    conn.execute(
        """
        INSERT INTO collections (
            collectionName, parentCollectionID, libraryID, key, version, synced
        )
        VALUES (?, ?, ?, ?, 0, 0)
        """,
        (name, parent_id, library_id, collection_key),
    )

    return collection_key


def add_item_to_collection(conn: sqlite3.Connection, item_key: str, collection_key: str) -> bool:
    """
    Add item to collection.

    Args:
        conn: Database connection
        item_key: Item key to add
        collection_key: Collection key

    Returns:
        True if item was added, False if already in collection

    Raises:
        ValueError: If item or collection not found
    """
    cursor = conn.execute("SELECT itemID FROM items WHERE key = ?", (item_key,))
    row = cursor.fetchone()
    if row is None:
        raise ValueError(f"Item not found: {item_key}")
    item_id = row["itemID"]

    cursor = conn.execute("SELECT collectionID FROM collections WHERE key = ?", (collection_key,))
    row = cursor.fetchone()
    if row is None:
        raise ValueError(f"Collection not found: {collection_key}")
    collection_id = row["collectionID"]

    cursor = conn.execute(
        "SELECT 1 FROM collectionItems WHERE collectionID = ? AND itemID = ?",
        (collection_id, item_id),
    )
    if cursor.fetchone() is not None:
        return False

    cursor = conn.execute(
        """
        SELECT COALESCE(MAX(orderIndex), -1) as maxOrder
        FROM collectionItems
        WHERE collectionID = ?
        """,
        (collection_id,),
    )
    max_order = cursor.fetchone()["maxOrder"]

    conn.execute(
        "INSERT INTO collectionItems (collectionID, itemID, orderIndex) VALUES (?, ?, ?)",
        (collection_id, item_id, max_order + 1),
    )

    return True


def remove_item_from_collection(
    conn: sqlite3.Connection, item_key: str, collection_key: str
) -> bool:
    """
    Remove item from collection.

    Args:
        conn: Database connection
        item_key: Item key to remove
        collection_key: Collection key

    Returns:
        True if item was removed, False if not in collection

    Raises:
        ValueError: If item or collection not found
    """
    cursor = conn.execute("SELECT itemID FROM items WHERE key = ?", (item_key,))
    row = cursor.fetchone()
    if row is None:
        raise ValueError(f"Item not found: {item_key}")
    item_id = row["itemID"]

    cursor = conn.execute("SELECT collectionID FROM collections WHERE key = ?", (collection_key,))
    row = cursor.fetchone()
    if row is None:
        raise ValueError(f"Collection not found: {collection_key}")
    collection_id = row["collectionID"]

    cursor = conn.execute(
        "DELETE FROM collectionItems WHERE collectionID = ? AND itemID = ?",
        (collection_id, item_id),
    )

    return cursor.rowcount > 0


def get_collections_tree(
    conn: sqlite3.Connection, parent_key: str | None = None
) -> list[dict[str, Any]]:
    """
    Get all collections with hierarchy information.

    Args:
        conn: Database connection
        parent_key: Optional parent collection key to filter by

    Returns:
        List of collection dictionaries with key, name, parentKey, itemCount
    """
    if parent_key:
        cursor = conn.execute(
            """
            SELECT
                c.collectionID,
                c.key,
                c.collectionName,
                c.parentCollectionID,
                (
                    SELECT COUNT(*)
                    FROM collectionItems ci
                    WHERE ci.collectionID = c.collectionID
                ) as itemCount
            FROM collections c
            JOIN collections parent ON c.parentCollectionID = parent.collectionID
            WHERE parent.key = ?
            ORDER BY c.collectionName
            """,
            (parent_key,),
        )
    else:
        cursor = conn.execute(
            """
            SELECT
                c.collectionID,
                c.key,
                c.collectionName,
                c.parentCollectionID,
                (
                    SELECT COUNT(*)
                    FROM collectionItems ci
                    WHERE ci.collectionID = c.collectionID
                ) as itemCount
            FROM collections c
            ORDER BY c.collectionName
            """
        )

    collections = []

    for row in cursor:
        resolved_parent_key = None
        if row["parentCollectionID"]:
            parent_cursor = conn.execute(
                "SELECT key FROM collections WHERE collectionID = ?",
                (row["parentCollectionID"],),
            )
            parent_row = parent_cursor.fetchone()
            if parent_row:
                resolved_parent_key = parent_row["key"]

        collections.append(
            {
                "key": row["key"],
                "name": row["collectionName"],
                "parentKey": resolved_parent_key,
                "itemCount": row["itemCount"],
            }
        )

    return collections


def resolve_collection_path(conn: sqlite3.Connection, path: str) -> str | None:
    """
    Resolve collection path to key.

    Path format: "Parent/Child/Grandchild"
    Uses exact matching, case-sensitive.

    Args:
        conn: Database connection
        path: Collection path with "/" separator

    Returns:
        Collection key if found, None otherwise
    """
    parts = [p.strip() for p in path.split("/") if p.strip()]

    if not parts:
        return None

    current_parent_id = None
    current_key = None

    for part in parts:
        if current_parent_id is None:
            cursor = conn.execute(
                """
                SELECT collectionID, key FROM collections
                WHERE collectionName = ? AND parentCollectionID IS NULL
                """,
                (part,),
            )
        else:
            cursor = conn.execute(
                """
                SELECT collectionID, key FROM collections
                WHERE collectionName = ? AND parentCollectionID = ?
                """,
                (part, current_parent_id),
            )

        row = cursor.fetchone()
        if row is None:
            return None

        current_parent_id = row["collectionID"]
        current_key = row["key"]

    return current_key


def resolve_or_create_collection_path(
    conn: sqlite3.Connection, path: str, library_id: int = 1
) -> str:
    """
    Resolve collection path to key, creating missing collections as needed.

    Args:
        conn: Database connection
        path: Collection path with "/" separator
        library_id: Library ID for new collections

    Returns:
        Collection key (existing or newly created)
    """
    parts = [p.strip() for p in path.split("/") if p.strip()]

    if not parts:
        raise ValueError("Empty collection path")

    current_parent_id = None
    current_parent_key = None
    current_key: str | None = None

    for part in parts:
        if current_parent_id is None:
            cursor = conn.execute(
                """
                SELECT collectionID, key FROM collections
                WHERE collectionName = ? AND parentCollectionID IS NULL
                """,
                (part,),
            )
        else:
            cursor = conn.execute(
                """
                SELECT collectionID, key FROM collections
                WHERE collectionName = ? AND parentCollectionID = ?
                """,
                (part, current_parent_id),
            )

        row = cursor.fetchone()

        if row is None:
            new_key = create_collection_record(conn, part, current_parent_key, library_id)
            click.echo(f"[+] Collection created: {part}", err=True)

            cursor = conn.execute(
                "SELECT collectionID FROM collections WHERE key = ?",
                (new_key,),
            )
            row = cursor.fetchone()
            current_parent_id = row["collectionID"]
            current_parent_key = new_key
            current_key = new_key
        else:
            current_parent_id = row["collectionID"]
            current_parent_key = row["key"]
            current_key = row["key"]

    assert current_key is not None
    return current_key
