"""
File attachment management for zoteroctl.
"""

import mimetypes
import shutil
import sqlite3
from contextlib import suppress
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from zoteroctl.formatters import echo_info


def _is_managed_storage_attachment(path_value: str, link_mode: int | None) -> bool:
    """Return whether an attachment is stored under Zotero's storage directory."""
    return path_value.startswith("storage:") or link_mode == 0


def _resolve_attachment_path(
    attachment_key: str,
    path_value: str,
    storage_base: Path,
) -> Path | None:
    """Resolve an attachment row path to an on-disk path."""
    if not path_value:
        return None

    if path_value.startswith("storage:"):
        filename = path_value.removeprefix("storage:")
        if not filename:
            return None
        return storage_base / attachment_key / filename

    return Path(path_value)


def _generate_zotero_key(conn: sqlite3.Connection, library_id: int = 1) -> str:
    """Generate unique 8-character Zotero key."""
    import random
    import string

    chars = string.ascii_uppercase + string.digits
    while True:
        key = "".join(random.choices(chars, k=8))
        cursor = conn.execute(
            "SELECT 1 FROM items WHERE libraryID = ? AND key = ?",
            (library_id, key),
        )
        if cursor.fetchone() is None:
            return key


def add_file_attachment(
    conn: sqlite3.Connection,
    parent_key: str,
    file_path: Path,
    storage_base: Path,
    title: str | None = None,
    library_id: int = 1,
) -> str:
    """
    Add file attachment to parent item.

    Args:
        conn: Database connection
        parent_key: Parent item key
        file_path: Path to file to attach
        storage_base: Base storage directory
        title: Optional attachment title (defaults to filename)
        library_id: Library ID (default: 1)

    Returns:
        Attachment item key

    Raises:
        ValueError: If parent item not found or file doesn't exist
        OSError: If storage directory cannot be created or file cannot be copied
    """
    if not file_path.exists():
        raise ValueError(f"File not found: {file_path}")

    cursor = conn.execute("SELECT itemID FROM items WHERE key = ?", (parent_key,))
    row = cursor.fetchone()
    if row is None:
        raise ValueError(f"Parent item not found: {parent_key}")
    parent_item_id = row["itemID"]

    attachment_key = _generate_zotero_key(conn, library_id)

    mime_type, _ = mimetypes.guess_type(file_path)
    if mime_type is None:
        mime_type = "application/octet-stream"

    if title is None:
        title = file_path.name

    cursor = conn.execute("SELECT itemTypeID FROM itemTypes WHERE typeName = 'attachment'")
    row = cursor.fetchone()
    if row is None:
        raise ValueError("Attachment item type not found in database")
    attachment_type_id = row["itemTypeID"]

    now = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S")  # Zotero stores UTC

    cursor = conn.execute(
        """
        INSERT INTO items (itemTypeID, libraryID, key, dateAdded, dateModified, clientDateModified)
        VALUES (?, ?, ?, ?, ?, ?)
        """,
        (attachment_type_id, library_id, attachment_key, now, now, now),
    )
    attachment_id = cursor.lastrowid

    if title:
        cursor = conn.execute("SELECT fieldID FROM fieldsCombined WHERE fieldName = 'title'")
        row = cursor.fetchone()
        if row:
            field_id = row["fieldID"]
            cursor = conn.execute("SELECT valueID FROM itemDataValues WHERE value = ?", (title,))
            row = cursor.fetchone()
            if row is None:
                cursor = conn.execute("INSERT INTO itemDataValues (value) VALUES (?)", (title,))
                value_id = cursor.lastrowid
            else:
                value_id = row["valueID"]
            conn.execute(
                "INSERT INTO itemData (itemID, fieldID, valueID) VALUES (?, ?, ?)",
                (attachment_id, field_id, value_id),
            )

    storage_dir = storage_base / attachment_key
    try:
        storage_dir.mkdir(parents=True, exist_ok=True)
    except OSError as e:
        raise OSError(f"Failed to create storage directory {storage_dir}: {e}") from e

    dest_path = storage_dir / file_path.name
    try:
        shutil.copy2(file_path, dest_path)
    except OSError as e:
        with suppress(OSError):
            shutil.rmtree(storage_dir)
        raise OSError(f"Failed to copy file to storage: {e}") from e

    path_value = f"storage:{file_path.name}"
    conn.execute(
        """
        INSERT INTO itemAttachments (itemID, parentItemID, linkMode, contentType, path)
        VALUES (?, ?, 0, ?, ?)
        """,
        (attachment_id, parent_item_id, mime_type, path_value),
    )

    return attachment_key


def list_file_attachments(
    conn: sqlite3.Connection,
    item_key: str,
    storage: Path,
) -> list[dict[str, Any]]:
    """
    List file attachments for an item.

    Args:
        conn: Database connection
        item_key: Parent item key
        storage: Storage base directory

    Returns:
        List of attachment dicts with key, title, path, contentType, linkMode, exists

    Raises:
        ValueError: If item not found
    """
    cursor = conn.execute("SELECT itemID FROM items WHERE key = ?", (item_key,))
    row = cursor.fetchone()
    if row is None:
        raise ValueError(f"Item not found: {item_key}")
    item_id = row["itemID"]

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

    attachments = []
    for row in cursor:
        attachment_key = row["attachment_key"]

        title_cursor = conn.execute(
            """
            SELECT idv.value
            FROM itemData id
            JOIN fieldsCombined f ON id.fieldID = f.fieldID
            JOIN itemDataValues idv ON id.valueID = idv.valueID
            JOIN items ai ON id.itemID = ai.itemID
            WHERE ai.key = ? AND f.fieldName = 'title'
            """,
            (attachment_key,),
        )
        title_row = title_cursor.fetchone()
        title = title_row["value"] if title_row else ""

        path_value = row["path"] or ""
        file_path = _resolve_attachment_path(attachment_key, path_value, storage)
        exists = file_path.exists() if file_path else False

        attachments.append(
            {
                "key": attachment_key,
                "title": title,
                "path": str(file_path) if file_path else path_value,
                "contentType": row["contentType"] or "",
                "linkMode": row["linkMode"],
                "exists": exists,
            }
        )

    return attachments


def delete_attachment(
    conn: sqlite3.Connection,
    attachment_key: str,
    storage_base: Path,
    keep_file: bool = False,
) -> bool:
    """
    Delete attachment item and optionally its file.

    Args:
        conn: Database connection
        attachment_key: Attachment item key
        storage_base: Base storage directory
        keep_file: If True, keep file on disk

    Returns:
        True if attachment was deleted, False if not found

    Raises:
        ValueError: If item is not an attachment
    """
    cursor = conn.execute(
        """
        SELECT i.itemID, i.itemTypeID, ia.path, ia.linkMode
        FROM items i
        LEFT JOIN itemAttachments ia ON i.itemID = ia.itemID
        WHERE i.key = ?
        """,
        (attachment_key,),
    )
    row = cursor.fetchone()

    if row is None:
        return False

    item_id = row["itemID"]

    cursor = conn.execute("SELECT itemTypeID FROM itemTypes WHERE typeName = 'attachment'")
    type_row = cursor.fetchone()
    if type_row is None:
        raise ValueError("Attachment item type not found in database")
    attachment_type_id = type_row["itemTypeID"]

    if row["itemTypeID"] != attachment_type_id:
        raise ValueError(f"Item {attachment_key} is not an attachment")

    path_value = row["path"] or ""

    storage_dir = None
    if not keep_file and _is_managed_storage_attachment(path_value, row["linkMode"]):
        storage_dir = storage_base / attachment_key

    conn.execute("DELETE FROM items WHERE itemID = ?", (item_id,))

    if storage_dir and storage_dir.exists():
        try:
            shutil.rmtree(storage_dir)
            echo_info(f"Deleted storage directory: {storage_dir.name}")
        except OSError as e:
            echo_info(f"Warning: Could not delete storage directory: {e}")

    return True
