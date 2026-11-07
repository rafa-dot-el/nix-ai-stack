"""
Item helpers (no click): key generation, item I/O, adding, updating, reading, listing and
deleting items. Moved out of cli.py so ops and the MCP server can use them without the CLI.
"""

import json
import random
import sqlite3
import string
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import yaml

from zoteroctl.formatters import echo_info


# ---------------------------------------------------------------------------
# Key generation helpers
# ---------------------------------------------------------------------------


def generate_zotero_key(conn: sqlite3.Connection, library_id: int = 1) -> str:
    """
    Generate unique 8-character Zotero key.

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
            "SELECT 1 FROM items WHERE libraryID = ? AND key = ?",
            (library_id, key),
        )
        if cursor.fetchone() is None:
            return key


# ---------------------------------------------------------------------------
# JSON/YAML I/O helpers
# ---------------------------------------------------------------------------


def load_item_data(file_path: Path) -> dict[str, Any]:
    """Load item data from JSON or YAML file."""
    suffix = file_path.suffix.lower()
    with open(file_path) as f:
        if suffix == ".json":
            return json.load(f)
        elif suffix in {".yaml", ".yml"}:
            return yaml.safe_load(f)
        else:
            raise ValueError(f"Unsupported file format: {suffix}")


def read_yaml_input(file_path: Path | None) -> dict[str, Any]:
    """Read YAML content from file or stdin."""
    if file_path is not None:
        if not file_path.exists():
            raise FileNotFoundError(f"File not found: {file_path}")
        with open(file_path) as f:
            content = f.read()
    else:
        content = sys.stdin.read()

    data = yaml.safe_load(content)

    if data is None:
        raise ValueError("Empty YAML content")

    if not isinstance(data, dict):
        raise ValueError(f"YAML content must be a dictionary, got {type(data).__name__}")

    return data


def merge_partial_updates(existing: dict[str, Any], updates: dict[str, Any]) -> dict[str, Any]:
    """Merge partial updates into existing item data."""
    result = dict(existing)
    for field, value in updates.items():
        if field == "itemKey":
            continue
        elif field == "fields" and isinstance(value, dict) and "fields" in result:
            result["fields"] = dict(result["fields"])
            result["fields"].update(value)
        else:
            result[field] = value
    return result


# ---------------------------------------------------------------------------
# Database mutation helpers
# ---------------------------------------------------------------------------


def add_or_update_item(
    conn: sqlite3.Connection, item_data: dict[str, Any], library_id: int = 1
) -> str:
    """Add new item or update existing item in database."""
    item_key = item_data.get("key")
    is_update = item_key is not None

    if is_update:
        cursor = conn.execute("SELECT itemID FROM items WHERE key = ?", (item_key,))
        row = cursor.fetchone()
        if row is None:
            raise ValueError(f"Item not found for update: {item_key}")
        item_id = row["itemID"]
    else:
        item_key = generate_zotero_key(conn, library_id)

    item_type = item_data.get("itemType", "journalArticle")
    cursor = conn.execute("SELECT itemTypeID FROM itemTypes WHERE typeName = ?", (item_type,))
    row = cursor.fetchone()
    if row is None:
        raise ValueError(f"Unknown item type: {item_type}")
    item_type_id = row["itemTypeID"]

    now = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S")  # Zotero stores UTC

    if is_update:
        conn.execute(
            """
            UPDATE items
            SET dateModified = ?,
                clientDateModified = ?,
                version = version + 1,
                synced = 0
            WHERE itemID = ?
            """,
            (now, now, item_id),
        )
        conn.execute("DELETE FROM itemData WHERE itemID = ?", (item_id,))
        conn.execute("DELETE FROM itemCreators WHERE itemID = ?", (item_id,))
        conn.execute("DELETE FROM itemTags WHERE itemID = ?", (item_id,))
        conn.execute("DELETE FROM collectionItems WHERE itemID = ?", (item_id,))
    else:
        cursor = conn.execute(
            """
            INSERT INTO items (
                itemTypeID, libraryID, key, dateAdded, dateModified, clientDateModified
            )
            VALUES (?, ?, ?, ?, ?, ?)
            """,
            (item_type_id, library_id, item_key, now, now, now),
        )
        item_id = cursor.lastrowid

    reserved_keys = {
        "itemType", "key", "itemKey", "creators", "tags", "collections",
        "attachments", "notes", "dateAdded", "dateModified", "fields",
        "collection_path",
    }
    fields = dict(item_data.get("fields", {}))
    for k, v in item_data.items():
        if k not in reserved_keys and k not in fields:
            fields[k] = v

    for field_name, field_value in fields.items():
        cursor = conn.execute(
            "SELECT fieldID FROM fieldsCombined WHERE fieldName = ?", (field_name,)
        )
        row = cursor.fetchone()
        if row is None:
            echo_info(f"Warning: Unknown field '{field_name}', skipping")
            continue
        field_id = row["fieldID"]

        field_value_str = str(field_value) if field_value is not None else ""

        cursor = conn.execute(
            "SELECT valueID FROM itemDataValues WHERE value = ?", (field_value_str,)
        )
        row = cursor.fetchone()
        if row is None:
            cursor = conn.execute(
                "INSERT INTO itemDataValues (value) VALUES (?)", (field_value_str,)
            )
            value_id = cursor.lastrowid
        else:
            value_id = row["valueID"]

        conn.execute(
            "INSERT INTO itemData (itemID, fieldID, valueID) VALUES (?, ?, ?)",
            (item_id, field_id, value_id),
        )

    creators = item_data.get("creators", [])
    for order_index, creator in enumerate(creators):
        creator_type = creator.get("creatorType", "author")
        field_mode = creator.get("fieldMode", 0)

        cursor = conn.execute(
            "SELECT creatorTypeID FROM creatorTypes WHERE creatorType = ?", (creator_type,)
        )
        row = cursor.fetchone()
        if row is None:
            echo_info(f"Warning: Unknown creator type '{creator_type}', using 'author'")
            creator_type_id = 1
        else:
            creator_type_id = row["creatorTypeID"]

        if field_mode == 0:
            first_name = creator.get("firstName", "")
            last_name = creator.get("lastName", "")
        else:
            first_name = None
            last_name = creator.get("name", "")

        cursor = conn.execute(
            """
            SELECT creatorID
            FROM creators
            WHERE lastName = ? AND firstName IS ? AND fieldMode = ?
            """,
            (last_name, first_name, field_mode),
        )
        row = cursor.fetchone()
        if row is None:
            cursor = conn.execute(
                "INSERT INTO creators (firstName, lastName, fieldMode) VALUES (?, ?, ?)",
                (first_name, last_name, field_mode),
            )
            creator_id = cursor.lastrowid
        else:
            creator_id = row["creatorID"]

        conn.execute(
            """
            INSERT INTO itemCreators (itemID, creatorID, creatorTypeID, orderIndex)
            VALUES (?, ?, ?, ?)
            """,
            (item_id, creator_id, creator_type_id, order_index),
        )

    tags = item_data.get("tags", [])
    for tag_name in tags:
        cursor = conn.execute("SELECT tagID FROM tags WHERE name = ?", (tag_name,))
        row = cursor.fetchone()
        if row is None:
            cursor = conn.execute("INSERT INTO tags (name) VALUES (?)", (tag_name,))
            tag_id = cursor.lastrowid
        else:
            tag_id = row["tagID"]
        conn.execute(
            "INSERT INTO itemTags (itemID, tagID, type) VALUES (?, ?, 0)", (item_id, tag_id)
        )

    collections = item_data.get("collections", [])
    for collection_key in collections:
        cursor = conn.execute(
            "SELECT collectionID FROM collections WHERE key = ?", (collection_key,)
        )
        row = cursor.fetchone()
        if row is None:
            echo_info(f"Warning: Collection not found '{collection_key}', skipping")
            continue
        collection_id = row["collectionID"]
        conn.execute(
            "INSERT INTO collectionItems (collectionID, itemID, orderIndex) VALUES (?, ?, 0)",
            (collection_id, item_id),
        )

    assert item_key is not None
    return item_key


# ---------------------------------------------------------------------------
# Database query helpers
# ---------------------------------------------------------------------------


def get_item_by_key(conn: sqlite3.Connection, key: str) -> dict[str, Any] | None:
    """Retrieve complete item data by key."""
    cursor = conn.execute(
        """
        SELECT
            i.itemID,
            i.key,
            it.typeName as itemType,
            i.dateAdded,
            i.dateModified
        FROM items i
        JOIN itemTypes it ON i.itemTypeID = it.itemTypeID
        WHERE i.key = ?
        """,
        (key,),
    )

    row = cursor.fetchone()
    if row is None:
        return None

    item_id = row["itemID"]
    item_data: dict[str, Any] = {
        "key": row["key"],
        "itemType": row["itemType"],
        "dateAdded": row["dateAdded"],
        "dateModified": row["dateModified"],
        "fields": {},
        "creators": [],
        "tags": [],
        "collections": [],
        "attachments": [],
        "notes": [],
    }

    cursor = conn.execute(
        """
        SELECT f.fieldName, idv.value
        FROM itemData id
        JOIN fieldsCombined f ON id.fieldID = f.fieldID
        JOIN itemDataValues idv ON id.valueID = idv.valueID
        WHERE id.itemID = ?
        """,
        (item_id,),
    )
    for row in cursor:
        item_data["fields"][row["fieldName"]] = row["value"]

    cursor = conn.execute(
        """
        SELECT
            c.firstName,
            c.lastName,
            c.fieldMode,
            ct.creatorType,
            ic.orderIndex
        FROM itemCreators ic
        JOIN creators c ON ic.creatorID = c.creatorID
        JOIN creatorTypes ct ON ic.creatorTypeID = ct.creatorTypeID
        WHERE ic.itemID = ?
        ORDER BY ic.orderIndex
        """,
        (item_id,),
    )
    for row in cursor:
        creator: dict[str, Any] = {
            "creatorType": row["creatorType"],
            "fieldMode": row["fieldMode"],
        }
        if row["fieldMode"] == 0:
            creator["firstName"] = row["firstName"] or ""
            creator["lastName"] = row["lastName"]
        else:
            creator["name"] = row["lastName"]
        item_data["creators"].append(creator)

    cursor = conn.execute(
        """
        SELECT t.name
        FROM itemTags it
        JOIN tags t ON it.tagID = t.tagID
        WHERE it.itemID = ?
        ORDER BY t.name
        """,
        (item_id,),
    )
    item_data["tags"] = [row["name"] for row in cursor]

    cursor = conn.execute(
        """
        SELECT c.key
        FROM collectionItems ci
        JOIN collections c ON ci.collectionID = c.collectionID
        WHERE ci.itemID = ?
        """,
        (item_id,),
    )
    item_data["collections"] = [row["key"] for row in cursor]

    cursor = conn.execute(
        """
        SELECT
            ai.key as attachment_key,
            ia.linkMode,
            ia.path,
            ia.contentType
        FROM itemAttachments ia
        JOIN items ai ON ia.itemID = ai.itemID
        WHERE ia.parentItemID = ?
        """,
        (item_id,),
    )
    for row in cursor:
        attachment: dict[str, Any] = {
            "key": row["attachment_key"],
            "linkMode": row["linkMode"],
            "contentType": row["contentType"] or "",
        }
        if row["linkMode"] == 0:
            path_value = row["path"] or ""
            attachment["path"] = (
                path_value if path_value.startswith("storage:") else f"storage:{path_value}"
            )
        elif row["linkMode"] == 1:
            attachment["path"] = row["path"]
        else:
            attachment["path"] = row["path"] or ""
        item_data["attachments"].append(attachment)

    cursor = conn.execute(
        """
        SELECT ni.key as note_key, inn.note
        FROM itemNotes inn
        JOIN items ni ON inn.itemID = ni.itemID
        WHERE inn.parentItemID = ?
        """,
        (item_id,),
    )
    for row in cursor:
        item_data["notes"].append({"key": row["note_key"], "note": row["note"]})

    return item_data


def delete_item(
    conn: sqlite3.Connection,
    key: str,
    storage_base: Path,
    keep_files: bool = False,
) -> bool:
    """Delete item from database and optionally delete attachment files."""
    cursor = conn.execute("SELECT itemID FROM items WHERE key = ?", (key,))
    row = cursor.fetchone()
    if row is None:
        return False

    item_id = row["itemID"]
    attachment_paths = []

    if not keep_files:
        cursor = conn.execute(
            """
            SELECT ai.key as attachment_key, ia.linkMode, ia.path
            FROM itemAttachments ia
            JOIN items ai ON ia.itemID = ai.itemID
            WHERE ia.parentItemID = ?
            """,
            (item_id,),
        )
        for row in cursor:
            if row["linkMode"] == 0:
                attachment_key = row["attachment_key"]
                filename = row["path"]
                if filename and filename.startswith("storage:"):
                    filename = filename[8:]
                if filename:
                    file_path = storage_base / attachment_key / filename
                    if file_path.exists():
                        attachment_paths.append(storage_base / attachment_key)

        cursor = conn.execute(
            "SELECT linkMode, path, parentItemID FROM itemAttachments WHERE itemID = ?",
            (item_id,),
        )
        row = cursor.fetchone()
        if row and row["linkMode"] == 0:
            filename = row["path"]
            if filename and filename.startswith("storage:"):
                filename = filename[8:]
            if filename:
                file_path = storage_base / key / filename
                if file_path.exists():
                    attachment_paths.append(storage_base / key)

    conn.execute("DELETE FROM items WHERE itemID = ?", (item_id,))

    if not keep_files and attachment_paths:
        import shutil

        for dir_path in attachment_paths:
            try:
                if dir_path.exists():
                    shutil.rmtree(dir_path)
                    echo_info(f"Deleted storage directory: {dir_path.name}")
            except OSError as e:
                echo_info(f"Warning: Could not delete {dir_path}: {e}")

    return True


def list_items_query(
    conn: sqlite3.Connection,
    collection_key: str | None = None,
    tag_name: str | None = None,
    parent_key: str | None = None,
    title_name: str | None = None,
    limit: int | None = None,
) -> list[dict[str, Any]]:
    """Query items with optional filters."""
    # A parent's children are attachments and notes; otherwise list bibliographic items.
    types = "IN ('attachment', 'note')" if parent_key else "NOT IN ('attachment', 'note')"
    query = f"""
        SELECT DISTINCT
            i.itemID,
            i.key,
            it.typeName as itemType,
            i.dateAdded,
            i.dateModified
        FROM items i
        JOIN itemTypes it ON i.itemTypeID = it.itemTypeID
        WHERE it.typeName {types}
          AND i.itemID NOT IN (SELECT itemID FROM deletedItems)
    """
    params: list = []

    if title_name:
        query += """
            AND i.itemID IN (
                SELECT id.itemID
                FROM itemData id
                JOIN fieldsCombined f ON id.fieldID = f.fieldID
                JOIN itemDataValues idv ON id.valueID = idv.valueID
                WHERE f.fieldName = 'title' AND idv.value LIKE ? COLLATE NOCASE
            )
        """
        params.append(f"%{title_name}%")

    if parent_key:
        query += """
            AND (
                i.itemID IN (
                    SELECT ia.itemID
                    FROM itemAttachments ia
                    JOIN items parent ON ia.parentItemID = parent.itemID
                    WHERE parent.key = ?
                )
                OR i.itemID IN (
                    SELECT in_table.itemID
                    FROM itemNotes in_table
                    JOIN items parent ON in_table.parentItemID = parent.itemID
                    WHERE parent.key = ?
                )
            )
        """
        params.append(parent_key)
        params.append(parent_key)

    if collection_key:
        query += """
            AND i.itemID IN (
                SELECT ci.itemID
                FROM collectionItems ci
                JOIN collections c ON ci.collectionID = c.collectionID
                WHERE c.key = ?
            )
        """
        params.append(collection_key)

    if tag_name:
        query += """
            AND i.itemID IN (
                SELECT it.itemID
                FROM itemTags it
                JOIN tags t ON it.tagID = t.tagID
                WHERE t.name = ?
            )
        """
        params.append(tag_name)

    query += " ORDER BY i.dateAdded DESC"
    if limit is not None:
        query += " LIMIT ?"
        params.append(limit)

    cursor = conn.execute(query, params)
    items = []

    for row in cursor:
        item_id = row["itemID"]

        title_cursor = conn.execute(
            """
            SELECT idv.value
            FROM itemData id
            JOIN fieldsCombined f ON id.fieldID = f.fieldID
            JOIN itemDataValues idv ON id.valueID = idv.valueID
            WHERE id.itemID = ? AND f.fieldName = 'title'
            """,
            (item_id,),
        )
        title_row = title_cursor.fetchone()
        title = title_row["value"] if title_row else ""

        date_cursor = conn.execute(
            """
            SELECT idv.value
            FROM itemData id
            JOIN fieldsCombined f ON id.fieldID = f.fieldID
            JOIN itemDataValues idv ON id.valueID = idv.valueID
            WHERE id.itemID = ? AND f.fieldName = 'date'
            """,
            (item_id,),
        )
        date_row = date_cursor.fetchone()
        date = date_row["value"] if date_row else ""

        creators_cursor = conn.execute(
            """
            SELECT c.firstName, c.lastName, c.fieldMode, ct.creatorType
            FROM itemCreators ic
            JOIN creators c ON ic.creatorID = c.creatorID
            JOIN creatorTypes ct ON ic.creatorTypeID = ct.creatorTypeID
            WHERE ic.itemID = ?
            ORDER BY ic.orderIndex
            """,
            (item_id,),
        )
        creator_names = []
        for creator_row in creators_cursor:
            if creator_row["fieldMode"] == 0:
                name = f"{creator_row['firstName']} {creator_row['lastName']}"
            else:
                name = creator_row["lastName"]
            creator_names.append(name.strip())
        creators = "; ".join(creator_names) if creator_names else ""

        items.append(
            {
                "key": row["key"],
                "itemType": row["itemType"],
                "title": title,
                "creators": creators,
                "date": date,
                "dateAdded": row["dateAdded"],
            }
        )

    return items


def query_child_items(conn: sqlite3.Connection) -> dict[str, list[dict[str, Any]]]:
    """Query all attachments and notes with their parent keys."""
    cursor = conn.execute(
        """
        SELECT
            ai.key as key,
            parent.key as parentKey,
            ia.contentType,
            ia.linkMode,
            ia.path
        FROM items ai
        JOIN itemTypes ait ON ai.itemTypeID = ait.itemTypeID
        JOIN itemAttachments ia ON ai.itemID = ia.itemID
        LEFT JOIN items parent ON ia.parentItemID = parent.itemID
        WHERE ait.typeName = 'attachment'
        """
    )

    attachments = []
    for row in cursor:
        title_cursor = conn.execute(
            """
            SELECT idv.value
            FROM itemData id
            JOIN fieldsCombined f ON id.fieldID = f.fieldID
            JOIN itemDataValues idv ON id.valueID = idv.valueID
            JOIN items ai ON id.itemID = ai.itemID
            WHERE ai.key = ? AND f.fieldName = 'title'
            """,
            (row["key"],),
        )
        title_row = title_cursor.fetchone()
        title = title_row["value"] if title_row else ""

        attachments.append(
            {
                "key": row["key"],
                "parentKey": row["parentKey"],
                "title": title,
                "contentType": row["contentType"] or "",
                "linkMode": row["linkMode"],
                "path": row["path"] or "",
            }
        )

    cursor = conn.execute(
        """
        SELECT
            ni.key as key,
            parent.key as parentKey,
            inn.note
        FROM items ni
        JOIN itemTypes nit ON ni.itemTypeID = nit.itemTypeID
        JOIN itemNotes inn ON ni.itemID = inn.itemID
        LEFT JOIN items parent ON inn.parentItemID = parent.itemID
        WHERE nit.typeName = 'note'
        """
    )

    notes = []
    for row in cursor:
        notes.append({"key": row["key"], "parentKey": row["parentKey"], "note": row["note"] or ""})

    return {"attachments": attachments, "notes": notes}


def filter_bibliographic_items(items: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Filter bibliographic items from mixed item list."""
    excluded_types = {"attachment", "note"}
    return [item for item in items if item.get("itemType") not in excluded_types]


def nest_child_items(
    item: dict[str, Any],
    all_attachments: list[dict[str, Any]],
    all_notes: list[dict[str, Any]],
) -> dict[str, Any]:
    """Nest child attachments and notes under parent item."""
    item_key = item.get("key")
    item_attachments = [att for att in all_attachments if att.get("parentKey") == item_key]
    item_notes = [note for note in all_notes if note.get("parentKey") == item_key]
    result = dict(item)
    result["attachments"] = item_attachments
    result["notes"] = item_notes
    return result


