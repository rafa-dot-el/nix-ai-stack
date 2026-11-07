"""
Maintenance commands: retag and dedupe.
"""

import shutil
from collections import defaultdict
from datetime import datetime
from pathlib import Path
from typing import Any

from calibrectl.db import (
    compute_file_hash,
    connect_database,
)


def apply_retag(
    database_path: Path,
    mappings: dict[str, str | None],
    dry_run: bool = False,
    backup: bool = True,
) -> dict[str, Any]:
    """
    Refactor library tags based on mapping dictionary.

    Args:
        database_path: Path to Calibre metadata.db
        mappings: {old_tag: new_tag_or_None}; None means delete
        dry_run: Preview changes without writing
        backup: Create database backup before writing

    Returns:
        Statistics dictionary
    """
    delete_tags = {old: new for old, new in mappings.items() if new is None}
    consolidate_tags = {old: new for old, new in mappings.items() if new is not None and old != new}

    with connect_database(database_path, read_only=True) as conn:
        cursor = conn.execute("""
            SELECT t.id, t.name, COUNT(btl.book) as book_count
            FROM tags t
            LEFT JOIN books_tags_link btl ON t.id = btl.tag
            GROUP BY t.id ORDER BY t.name
        """)
        current_tags = {row[1]: {"id": row[0], "count": row[2]} for row in cursor.fetchall()}
        total_books = conn.execute("SELECT COUNT(*) FROM books").fetchone()[0]

    stats: dict[str, Any] = {
        "current_tags": len(current_tags),
        "total_books": total_books,
        "tags_deleted": 0,
        "tags_consolidated": 0,
        "tag_associations_deleted": 0,
        "tag_associations_moved": 0,
        "dry_run": dry_run,
    }

    if dry_run:
        stats["would_delete"] = [t for t in delete_tags if t in current_tags]
        stats["would_consolidate"] = {
            old: new for old, new in consolidate_tags.items() if old in current_tags
        }
        return stats

    if backup:
        backup_path = f"{database_path}.backup-{datetime.now().strftime('%Y%m%d_%H%M%S')}"
        shutil.copy2(database_path, backup_path)

    with connect_database(database_path, read_only=False) as conn:
        conn.execute("BEGIN TRANSACTION")
        try:
            # Phase 1: Consolidate
            for old_tag, new_tag in consolidate_tags.items():
                if old_tag not in current_tags:
                    continue
                old_tag_id = current_tags[old_tag]["id"]
                old_tag_count = current_tags[old_tag]["count"]

                cursor = conn.execute("SELECT id FROM tags WHERE name = ?", (new_tag,))
                row = cursor.fetchone()
                if row:
                    new_tag_id = row[0]
                else:
                    cursor = conn.execute("INSERT INTO tags (name) VALUES (?)", (new_tag,))
                    new_tag_id = cursor.lastrowid

                books_rows = conn.execute(
                    "SELECT book FROM books_tags_link WHERE tag = ?", (old_tag_id,)
                ).fetchall()
                books_with_old_tag = [r[0] for r in books_rows]

                moved = 0
                for bid in books_with_old_tag:
                    exists = conn.execute(
                        "SELECT 1 FROM books_tags_link WHERE book = ? AND tag = ?",
                        (bid, new_tag_id),
                    ).fetchone()
                    if exists:
                        conn.execute(
                            "DELETE FROM books_tags_link WHERE book = ? AND tag = ?",
                            (bid, old_tag_id),
                        )
                    else:
                        conn.execute(
                            "UPDATE books_tags_link SET tag = ? WHERE book = ? AND tag = ?",
                            (new_tag_id, bid, old_tag_id),
                        )
                        moved += 1

                conn.execute("DELETE FROM tags WHERE id = ?", (old_tag_id,))
                stats["tags_consolidated"] += 1
                stats["tag_associations_moved"] += moved

            # Phase 2: Delete
            for old_tag in delete_tags:
                if old_tag not in current_tags:
                    continue
                old_tag_id = current_tags[old_tag]["id"]
                old_tag_count = current_tags[old_tag]["count"]
                conn.execute("DELETE FROM books_tags_link WHERE tag = ?", (old_tag_id,))
                conn.execute("DELETE FROM tags WHERE id = ?", (old_tag_id,))
                stats["tags_deleted"] += 1
                stats["tag_associations_deleted"] += old_tag_count

            final_tag_count = conn.execute("SELECT COUNT(*) FROM tags").fetchone()[0]
            stats["final_tags"] = final_tag_count
            conn.commit()

        except Exception:
            conn.rollback()
            raise

    return stats


def find_duplicates_by_hash(
    database_path: Path,
    storage_path: Path,
    algorithm: str = "sha256",
    check_formats: list[str] | None = None,
) -> tuple[dict[str, list[dict[str, Any]]], int]:
    """
    Find duplicate book files by hash.

    Args:
        database_path: Path to Calibre metadata.db
        storage_path: Storage root directory
        algorithm: Hash algorithm
        check_formats: List of formats to check, or None for all

    Returns:
        (duplicate_groups, total_files_processed)
    """
    with connect_database(database_path, read_only=True) as conn:
        cursor = conn.execute("""
            SELECT
                d.book,
                d.format,
                d.name,
                d.uncompressed_size,
                b.path,
                b.title,
                b.timestamp
            FROM data d
            JOIN books b ON d.book = b.id
            ORDER BY d.book, d.format
        """)
        all_files = cursor.fetchall()

    if check_formats:
        all_files = [f for f in all_files if f["format"] in check_formats]

    hash_to_files: dict[str, list[dict[str, Any]]] = defaultdict(list)

    for file_row in all_files:
        file_path = (
            storage_path / file_row["path"] / f"{file_row['name']}.{file_row['format'].lower()}"
        )
        file_hash = compute_file_hash(file_path, algorithm)
        if file_hash:
            hash_to_files[file_hash].append(
                {
                    "book_id": file_row["book"],
                    "format": file_row["format"],
                    "path": str(file_path),
                    "size": file_row["uncompressed_size"],
                    "title": file_row["title"],
                    "date_added": file_row["timestamp"],
                }
            )

    duplicate_groups = {h: files for h, files in hash_to_files.items() if len(files) > 1}
    return duplicate_groups, len(all_files)
