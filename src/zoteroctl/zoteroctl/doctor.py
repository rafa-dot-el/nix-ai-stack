"""
Doctor diagnostic command logic for zoteroctl.
"""

import sqlite3
from collections import defaultdict
from pathlib import Path
from typing import Any

from zoteroctl.formatters import echo_info


def detect_orphaned_items(items: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """
    Detect items without parent collections.

    Pure function - no I/O, only data transformation.

    Args:
        items: List of items with collections field

    Returns:
        List of orphaned items with reason
    """
    orphans = []
    for item in items:
        collections = item.get("collections", [])
        if not collections:
            orphans.append(
                {
                    "itemKey": item.get("itemKey", "unknown"),
                    "title": item.get("title", ""),
                    "itemType": item.get("itemType", ""),
                    "reason": "No parent collection",
                }
            )
    return orphans


def detect_missing_files(
    attachments: list[dict[str, Any]], storage_path: Path
) -> list[dict[str, Any]]:
    """
    Detect attachment files that don't exist in storage.

    Args:
        attachments: List of attachments with path field
        storage_path: Base storage directory path

    Returns:
        List of missing files with expected paths
    """
    missing = []

    for attachment in attachments:
        path_str = attachment.get("path", "")
        item_key = attachment.get("itemKey", "unknown")

        if path_str.startswith("storage:"):
            filename = path_str[8:]
            expected_path = storage_path / item_key / filename
        else:
            expected_path = Path(path_str)

        if not expected_path.exists():
            missing.append(
                {
                    "itemKey": item_key,
                    "attachmentTitle": attachment.get("title", ""),
                    "expectedPath": str(expected_path),
                    "reason": "File not found in storage",
                }
            )

    return missing


def detect_broken_references(
    items: list[dict[str, Any]], valid_creator_ids: list[int]
) -> list[dict[str, Any]]:
    """
    Detect items with invalid creator references.

    Pure function - no I/O, only data transformation.

    Args:
        items: List of items with creators
        valid_creator_ids: List of valid creator IDs from database

    Returns:
        List of broken references
    """
    broken = []
    valid_set = set(valid_creator_ids)

    for item in items:
        creators = item.get("creators", [])
        for creator in creators:
            creator_id = creator.get("creatorID")
            if creator_id and creator_id not in valid_set:
                broken.append(
                    {
                        "itemKey": item.get("itemKey", "unknown"),
                        "field": "creator",
                        "creatorID": creator_id,
                        "reason": "References deleted creator",
                    }
                )

    return broken


def detect_duplicate_attachment_hashes(
    attachments: list[dict[str, Any]],
    storage_path: Path,
    algorithm: str = "sha256",
) -> dict[str, list[dict[str, Any]]]:
    """
    Detect duplicate attachments by computing file hashes.

    Args:
        attachments: List of attachment items with paths
        storage_path: Storage base directory
        algorithm: Hash algorithm (default: sha256)

    Returns:
        Dictionary mapping hash -> list of duplicate attachments
    """
    import hashlib

    hash_to_attachments: dict[str, list] = defaultdict(list)

    for attachment in attachments:
        path_str = attachment.get("path", "")
        item_key = attachment.get("itemKey", "unknown")

        if path_str.startswith("storage:"):
            filename = path_str[8:]
            file_path = storage_path / item_key / filename
        elif path_str:
            file_path = Path(path_str)
        else:
            continue

        if not file_path.exists() or not file_path.is_file():
            continue

        try:
            hasher = hashlib.sha256() if algorithm == "sha256" else hashlib.md5()
            chunk_size = 65536

            with open(file_path, "rb") as f:
                while True:
                    chunk = f.read(chunk_size)
                    if not chunk:
                        break
                    hasher.update(chunk)

            file_hash = hasher.hexdigest()

            hash_to_attachments[file_hash].append(
                {
                    "itemKey": item_key,
                    "parentKey": attachment.get("parentKey"),
                    "title": attachment.get("title", ""),
                    "path": str(file_path),
                    "contentType": attachment.get("contentType", ""),
                    "size": file_path.stat().st_size,
                }
            )
        except Exception:
            continue

    return {h: atts for h, atts in hash_to_attachments.items() if len(atts) > 1}


def detect_identifier_duplicates(
    conn: sqlite3.Connection,
) -> dict[str, list[dict[str, Any]]]:
    """
    Detect items with duplicate unique identifiers (DOI, ISBN, URL).

    Args:
        conn: Database connection

    Returns:
        Dictionary with keys 'doi_duplicates', 'isbn_duplicates', 'url_duplicates'
    """
    result: dict[str, list] = {
        "doi_duplicates": [],
        "isbn_duplicates": [],
        "url_duplicates": [],
    }

    def fetch_field_duplicates(field_name: str, limit: int | None = None) -> list[dict]:
        query = """
            SELECT
                idv.value as identifier,
                GROUP_CONCAT(i.key) as item_keys,
                COUNT(*) as count
            FROM itemData id
            JOIN fieldsCombined f ON id.fieldID = f.fieldID
            JOIN itemDataValues idv ON id.valueID = idv.valueID
            JOIN items i ON id.itemID = i.itemID
            JOIN itemTypes it ON i.itemTypeID = it.itemTypeID
            WHERE f.fieldName = ?
              AND idv.value != ''
              AND it.typeName NOT IN ('attachment', 'note')
            GROUP BY identifier
            HAVING count > 1
            ORDER BY count DESC
        """
        if limit:
            query += f" LIMIT {limit}"

        cursor = conn.execute(query, (field_name,))

        duplicates = []
        for row in cursor:
            item_keys = row["item_keys"].split(",")
            items = []
            for key in item_keys:
                title_cursor = conn.execute(
                    """
                    SELECT idv.value
                    FROM itemData id
                    JOIN fieldsCombined f ON id.fieldID = f.fieldID
                    JOIN itemDataValues idv ON id.valueID = idv.valueID
                    JOIN items i ON id.itemID = i.itemID
                    WHERE i.key = ? AND f.fieldName = 'title'
                    """,
                    (key,),
                )
                title_row = title_cursor.fetchone()

                type_cursor = conn.execute(
                    """
                    SELECT it.typeName
                    FROM items i
                    JOIN itemTypes it ON i.itemTypeID = it.itemTypeID
                    WHERE i.key = ?
                    """,
                    (key,),
                )
                type_row = type_cursor.fetchone()

                items.append(
                    {
                        "itemKey": key,
                        "title": title_row["value"] if title_row else "",
                        "itemType": type_row["typeName"] if type_row else "unknown",
                    }
                )

            duplicates.append(
                {
                    "identifier": row["identifier"],
                    "count": row["count"],
                    "items": items,
                }
            )

        return duplicates

    result["doi_duplicates"] = fetch_field_duplicates("DOI")
    result["isbn_duplicates"] = fetch_field_duplicates("ISBN")
    url_dups = fetch_field_duplicates("url", limit=20)
    for dup in url_dups:
        if len(dup["identifier"]) > 100:
            dup["identifier"] = dup["identifier"][:100] + "..."
    result["url_duplicates"] = url_dups

    return result


def detect_title_duplicates(conn: sqlite3.Connection, limit: int = 50) -> list[dict[str, Any]]:
    """
    Detect items with matching normalized titles.

    Args:
        conn: Database connection
        limit: Maximum number of duplicate groups to return

    Returns:
        List of duplicate groups with normalized titles
    """
    cursor = conn.execute(
        """
        SELECT
            LOWER(TRIM(idv.value)) as norm_title,
            GROUP_CONCAT(i.key) as item_keys,
            COUNT(*) as count,
            MAX(idv.value) as original_title
        FROM itemData id
        JOIN fieldsCombined f ON id.fieldID = f.fieldID
        JOIN itemDataValues idv ON id.valueID = idv.valueID
        JOIN items i ON id.itemID = i.itemID
        JOIN itemTypes it ON i.itemTypeID = it.itemTypeID
        WHERE f.fieldName = 'title'
          AND it.typeName NOT IN ('attachment', 'note')
        GROUP BY norm_title
        HAVING count > 1
        ORDER BY count DESC
        LIMIT ?
        """,
        (limit,),
    )

    duplicates = []
    for row in cursor:
        item_keys = row["item_keys"].split(",")
        items = []
        for key in item_keys:
            type_cursor = conn.execute(
                """
                SELECT it.typeName
                FROM items i
                JOIN itemTypes it ON i.itemTypeID = it.itemTypeID
                WHERE i.key = ?
                """,
                (key,),
            )
            type_row = type_cursor.fetchone()
            items.append(
                {
                    "itemKey": key,
                    "itemType": type_row["typeName"] if type_row else "unknown",
                }
            )

        duplicates.append(
            {
                "title": row["original_title"],
                "normalized_title": row["norm_title"],
                "count": row["count"],
                "items": items,
            }
        )

    return duplicates


def generate_diagnostic_report(
    issues: dict[str, Any],
    total_items: int,
    total_attachments: int,
) -> dict[str, Any]:
    """
    Generate diagnostic report from collected issues.

    Pure function - no I/O, only data transformation.

    Args:
        issues: Dictionary with orphanedItems, missingFiles, brokenReferences,
                hashDuplicates, identifierDuplicates, titleDuplicates
        total_items: Total item count
        total_attachments: Total attachment count

    Returns:
        Complete diagnostic report with summary and issues
    """
    orphan_count = len(issues.get("orphanedItems", []))
    missing_count = len(issues.get("missingFiles", []))
    broken_count = len(issues.get("brokenReferences", []))

    hash_duplicate_count = len(issues.get("hashDuplicates", {}))
    doi_duplicate_count = len(issues.get("identifierDuplicates", {}).get("doi_duplicates", []))
    isbn_duplicate_count = len(issues.get("identifierDuplicates", {}).get("isbn_duplicates", []))
    url_duplicate_count = len(issues.get("identifierDuplicates", {}).get("url_duplicates", []))
    title_duplicate_count = len(issues.get("titleDuplicates", []))

    has_issues = orphan_count > 0 or missing_count > 0 or broken_count > 0
    has_duplicates = (
        hash_duplicate_count > 0
        or doi_duplicate_count > 0
        or isbn_duplicate_count > 0
        or url_duplicate_count > 0
        or title_duplicate_count > 0
    )

    if has_issues:
        status = "issues_found"
    elif has_duplicates:
        status = "duplicates_found"
    else:
        status = "healthy"

    return {
        "summary": {
            "totalItems": total_items,
            "totalAttachments": total_attachments,
            "orphanedItems": orphan_count,
            "missingFiles": missing_count,
            "brokenReferences": broken_count,
            "hashDuplicates": hash_duplicate_count,
            "doiDuplicates": doi_duplicate_count,
            "isbnDuplicates": isbn_duplicate_count,
            "urlDuplicates": url_duplicate_count,
            "titleDuplicates": title_duplicate_count,
            "status": status,
        },
        "issues": issues,
    }


def run_database_diagnostics(conn: sqlite3.Connection, storage_path: Path) -> dict[str, Any]:
    """
    Run complete database diagnostics.

    Args:
        conn: Database connection
        storage_path: Base storage directory

    Returns:
        Complete diagnostic report with summary and issues
    """
    cursor = conn.execute(
        """
        SELECT COUNT(*) as count
        FROM items i
        JOIN itemTypes it ON i.itemTypeID = it.itemTypeID
        WHERE it.typeName NOT IN ('attachment', 'note')
        """
    )
    total_items = cursor.fetchone()["count"]

    cursor = conn.execute(
        """
        SELECT COUNT(*) as count
        FROM items i
        JOIN itemTypes it ON i.itemTypeID = it.itemTypeID
        WHERE it.typeName = 'attachment'
        """
    )
    total_attachments = cursor.fetchone()["count"]

    cursor = conn.execute(
        """
        SELECT DISTINCT
            i.itemID,
            i.key as itemKey,
            it.typeName as itemType
        FROM items i
        JOIN itemTypes it ON i.itemTypeID = it.itemTypeID
        WHERE it.typeName NOT IN ('attachment', 'note')
        """
    )

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

        coll_cursor = conn.execute(
            """
            SELECT c.key
            FROM collectionItems ci
            JOIN collections c ON ci.collectionID = c.collectionID
            WHERE ci.itemID = ?
            """,
            (item_id,),
        )
        collections = [r["key"] for r in coll_cursor]

        items.append(
            {
                "itemKey": row["itemKey"],
                "itemType": row["itemType"],
                "title": title,
                "collections": collections,
            }
        )

    cursor = conn.execute(
        """
        SELECT
            i.key as itemKey,
            ia.path,
            ia.linkMode
        FROM items i
        JOIN itemTypes it ON i.itemTypeID = it.itemTypeID
        JOIN itemAttachments ia ON i.itemID = ia.itemID
        WHERE it.typeName = 'attachment'
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
            (row["itemKey"],),
        )
        title_row = title_cursor.fetchone()
        title = title_row["value"] if title_row else ""

        attachments.append(
            {
                "itemKey": row["itemKey"],
                "title": title,
                "path": row["path"] or "",
                "linkMode": row["linkMode"],
            }
        )

    cursor = conn.execute("SELECT creatorID FROM creators")
    valid_creator_ids = [row["creatorID"] for row in cursor]

    orphaned_items = detect_orphaned_items(items)
    missing_files = detect_missing_files(attachments, storage_path)
    broken_references = detect_broken_references(items, valid_creator_ids)

    echo_info("Computing file hashes for duplicate detection...")
    hash_duplicates = detect_duplicate_attachment_hashes(attachments, storage_path)

    echo_info("Checking identifier duplicates...")
    identifier_duplicates = detect_identifier_duplicates(conn)

    echo_info("Checking title duplicates...")
    title_duplicates = detect_title_duplicates(conn)

    issues = {
        "orphanedItems": orphaned_items,
        "missingFiles": missing_files,
        "brokenReferences": broken_references,
        "hashDuplicates": hash_duplicates,
        "identifierDuplicates": identifier_duplicates,
        "titleDuplicates": title_duplicates,
    }

    return generate_diagnostic_report(issues, total_items, total_attachments)
