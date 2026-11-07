"""
Tests for zoteroctl.collections module.
"""

import pytest

from zoteroctl.cli import add_or_update_item
from zoteroctl.collections import (
    add_item_to_collection,
    create_collection_record,
    get_collections_tree,
    remove_item_from_collection,
    resolve_collection_path,
    resolve_or_create_collection_path,
)


def _insert_book(conn, title="Test Book"):
    return add_or_update_item(
        conn,
        {
            "itemType": "book",
            "fields": {"title": title},
            "creators": [],
            "tags": [],
        },
    )


def _insert_collection(conn, name, parent_key=None):
    return create_collection_record(conn, name, parent_key)


class TestCreateCollectionRecord:
    def test_creates_top_level(self, zotero_db):
        key = create_collection_record(zotero_db, "My Collection")
        assert len(key) == 8
        row = zotero_db.execute("SELECT * FROM collections WHERE key = ?", (key,)).fetchone()
        assert row is not None
        assert row["collectionName"] == "My Collection"
        assert row["parentCollectionID"] is None

    def test_creates_nested(self, zotero_db):
        parent_key = create_collection_record(zotero_db, "Parent")
        child_key = create_collection_record(zotero_db, "Child", parent_key)

        child_row = zotero_db.execute(
            "SELECT parentCollectionID FROM collections WHERE key = ?", (child_key,)
        ).fetchone()
        parent_row = zotero_db.execute(
            "SELECT collectionID FROM collections WHERE key = ?", (parent_key,)
        ).fetchone()
        assert child_row["parentCollectionID"] == parent_row["collectionID"]

    def test_duplicate_name_allowed(self, zotero_db):
        k1 = create_collection_record(zotero_db, "Same Name")
        k2 = create_collection_record(zotero_db, "Same Name")
        assert k1 != k2

    def test_invalid_parent_raises(self, zotero_db):
        with pytest.raises(ValueError, match="Parent collection not found"):
            create_collection_record(zotero_db, "Child", "NOTFOUND")


class TestAddItemToCollection:
    def test_add_success(self, zotero_db):
        item_key = _insert_book(zotero_db)
        coll_key = _insert_collection(zotero_db, "Research")

        result = add_item_to_collection(zotero_db, item_key, coll_key)
        assert result is True

        row = zotero_db.execute(
            """
            SELECT 1 FROM collectionItems ci
            JOIN items i ON ci.itemID = i.itemID
            JOIN collections c ON ci.collectionID = c.collectionID
            WHERE i.key = ? AND c.key = ?
            """,
            (item_key, coll_key),
        ).fetchone()
        assert row is not None

    def test_already_in_collection_returns_false(self, zotero_db):
        item_key = _insert_book(zotero_db)
        coll_key = _insert_collection(zotero_db, "Research")
        add_item_to_collection(zotero_db, item_key, coll_key)

        result = add_item_to_collection(zotero_db, item_key, coll_key)
        assert result is False

    def test_item_not_found_raises(self, zotero_db):
        coll_key = _insert_collection(zotero_db, "Research")
        with pytest.raises(ValueError, match="Item not found"):
            add_item_to_collection(zotero_db, "NOTFOUND", coll_key)

    def test_collection_not_found_raises(self, zotero_db):
        item_key = _insert_book(zotero_db)
        with pytest.raises(ValueError, match="Collection not found"):
            add_item_to_collection(zotero_db, item_key, "NOTFOUND")


class TestRemoveItemFromCollection:
    def test_remove_success(self, zotero_db):
        item_key = _insert_book(zotero_db)
        coll_key = _insert_collection(zotero_db, "Research")
        add_item_to_collection(zotero_db, item_key, coll_key)

        result = remove_item_from_collection(zotero_db, item_key, coll_key)
        assert result is True

        row = zotero_db.execute(
            """
            SELECT 1 FROM collectionItems ci
            JOIN items i ON ci.itemID = i.itemID
            JOIN collections c ON ci.collectionID = c.collectionID
            WHERE i.key = ? AND c.key = ?
            """,
            (item_key, coll_key),
        ).fetchone()
        assert row is None

    def test_not_in_collection_returns_false(self, zotero_db):
        item_key = _insert_book(zotero_db)
        coll_key = _insert_collection(zotero_db, "Research")

        result = remove_item_from_collection(zotero_db, item_key, coll_key)
        assert result is False

    def test_item_not_found_raises(self, zotero_db):
        coll_key = _insert_collection(zotero_db, "Research")
        with pytest.raises(ValueError, match="Item not found"):
            remove_item_from_collection(zotero_db, "NOTFOUND", coll_key)

    def test_collection_not_found_raises(self, zotero_db):
        item_key = _insert_book(zotero_db)
        with pytest.raises(ValueError, match="Collection not found"):
            remove_item_from_collection(zotero_db, item_key, "NOTFOUND")


class TestGetCollectionsTree:
    def test_empty_library(self, zotero_db):
        result = get_collections_tree(zotero_db)
        assert result == []

    def test_single_collection(self, zotero_db):
        _insert_collection(zotero_db, "Root")
        result = get_collections_tree(zotero_db)
        assert len(result) == 1
        assert result[0]["name"] == "Root"
        assert result[0]["parentKey"] is None

    def test_nested_hierarchy(self, zotero_db):
        parent_key = _insert_collection(zotero_db, "Parent")
        _insert_collection(zotero_db, "Child", parent_key)

        result = get_collections_tree(zotero_db)
        names = [c["name"] for c in result]
        assert "Parent" in names
        assert "Child" in names

        child = next(c for c in result if c["name"] == "Child")
        assert child["parentKey"] == parent_key

    def test_filter_by_parent(self, zotero_db):
        parent_key = _insert_collection(zotero_db, "TopLevel")
        _insert_collection(zotero_db, "ChildA", parent_key)
        _insert_collection(zotero_db, "ChildB", parent_key)
        _insert_collection(zotero_db, "Sibling")  # different top-level

        result = get_collections_tree(zotero_db, parent_key)
        names = [c["name"] for c in result]
        assert "ChildA" in names
        assert "ChildB" in names
        assert "Sibling" not in names

    def test_item_count(self, zotero_db):
        coll_key = _insert_collection(zotero_db, "HasItems")
        item_key = _insert_book(zotero_db)
        add_item_to_collection(zotero_db, item_key, coll_key)

        result = get_collections_tree(zotero_db)
        coll = next(c for c in result if c["name"] == "HasItems")
        assert coll["itemCount"] == 1


class TestResolveCollectionPath:
    def test_top_level(self, zotero_db):
        key = _insert_collection(zotero_db, "Research")
        result = resolve_collection_path(zotero_db, "Research")
        assert result == key

    def test_nested_path(self, zotero_db):
        parent_key = _insert_collection(zotero_db, "Science")
        child_key = _insert_collection(zotero_db, "Physics", parent_key)

        result = resolve_collection_path(zotero_db, "Science/Physics")
        assert result == child_key

    def test_not_found_returns_none(self, zotero_db):
        assert resolve_collection_path(zotero_db, "Nonexistent") is None

    def test_partial_path_returns_none(self, zotero_db):
        _insert_collection(zotero_db, "A")
        assert resolve_collection_path(zotero_db, "A/B") is None

    def test_empty_path_returns_none(self, zotero_db):
        assert resolve_collection_path(zotero_db, "") is None


class TestResolveOrCreateCollectionPath:
    def test_existing_path(self, zotero_db):
        key = _insert_collection(zotero_db, "Existing")
        result = resolve_or_create_collection_path(zotero_db, "Existing")
        assert result == key

    def test_creates_missing(self, zotero_db):
        key = resolve_or_create_collection_path(zotero_db, "NewCollection")
        row = zotero_db.execute(
            "SELECT collectionName FROM collections WHERE key = ?", (key,)
        ).fetchone()
        assert row["collectionName"] == "NewCollection"

    def test_creates_nested(self, zotero_db):
        key = resolve_or_create_collection_path(zotero_db, "Parent/Child")
        child_row = zotero_db.execute(
            "SELECT c.collectionName, c.parentCollectionID FROM collections c WHERE c.key = ?",
            (key,),
        ).fetchone()
        assert child_row["collectionName"] == "Child"
        assert child_row["parentCollectionID"] is not None

    def test_empty_path_raises(self, zotero_db):
        with pytest.raises(ValueError, match="Empty collection path"):
            resolve_or_create_collection_path(zotero_db, "")
