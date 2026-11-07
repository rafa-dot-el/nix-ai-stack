"""
Operations shared by the CLI and the MCP server: choosing what to export, parsing BibTeX,
importing items. They raise NotFound or Invalid and never print or exit.
"""

import sqlite3
from pathlib import Path
from typing import Any

import bibtexparser
from bibtexparser.bparser import BibTexParser

from zoteroctl.bibtex import convert_bibtex_to_yaml, item_to_bibtex
from zoteroctl.db import connect_database
from zoteroctl.formatters import echo_info
from zoteroctl.items import add_or_update_item, get_item_by_key, list_items_query


class NotFound(LookupError):
    """An item or database that does not exist."""


class Invalid(ValueError):
    """A request that cannot be carried out as given."""


def export_entries(conn: sqlite3.Connection, key: str | None = None, collection: str | None = None, export_all: bool = False) -> list[str]:
    """BibTeX entries for one item, a collection or the whole library (no size limit)."""
    if key:
        item = get_item_by_key(conn, key)
        if item is None:
            raise NotFound(f"Item not found: {key}")
        items = [item]
    elif collection or export_all:
        listed = list_items_query(conn, collection_key=collection, limit=None)
        items = [i for i in (get_item_by_key(conn, s["key"]) for s in listed) if i]
    else:
        raise Invalid("Specify KEY, --collection, or --all")
    used: set = set()
    entries = []
    for item in items:
        try:
            entries.append(item_to_bibtex(item, used))
        except Exception as exc:  # one malformed item does not stop the export
            echo_info(f"Warning: Failed to export item {item['key']}: {exc}")
    return entries


def parse_bibtex(text: str) -> list[dict[str, Any]]:
    """BibTeX text -> item data dictionaries for import (pure)."""
    parser = BibTexParser(common_strings=True)
    entries = bibtexparser.loads(text, parser=parser).entries
    if not entries:
        raise Invalid("no BibTeX entries found")
    return [convert_bibtex_to_yaml(e) for e in entries]


def import_items(database: Path, items: list[dict[str, Any]]) -> list[str]:
    """Add the items in one transaction; their new keys."""
    if not database.exists():
        raise NotFound(f"no Zotero database at {database}")
    with connect_database(database, read_only=False) as conn:
        try:
            return [add_or_update_item(conn, {k: v for k, v in item.items() if k != "key"}) for item in items]
        except ValueError as exc:
            raise Invalid(str(exc)) from exc
