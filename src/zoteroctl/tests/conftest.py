"""
Shared pytest fixtures for zoteroctl tests.
"""

import sqlite3

import pytest


def _create_test_zotero_db() -> sqlite3.Connection:
    """Create an in-memory Zotero-like database for testing."""
    conn = sqlite3.connect(":memory:")
    conn.row_factory = sqlite3.Row
    conn.executescript("""
        CREATE TABLE items (
            itemID INTEGER PRIMARY KEY,
            itemTypeID INTEGER,
            libraryID INTEGER DEFAULT 1,
            key TEXT NOT NULL,
            dateAdded TEXT DEFAULT '2024-01-01 00:00:00',
            dateModified TEXT DEFAULT '2024-01-01 00:00:00',
            clientDateModified TEXT DEFAULT '2024-01-01 00:00:00',
            version INTEGER DEFAULT 0,
            synced INTEGER DEFAULT 0
        );
        CREATE TABLE itemTypes (itemTypeID INTEGER PRIMARY KEY, typeName TEXT NOT NULL);
        CREATE TABLE itemData (itemID INTEGER, fieldID INTEGER, valueID INTEGER);
        CREATE TABLE itemDataValues (valueID INTEGER PRIMARY KEY, value TEXT);
        CREATE TABLE fieldsCombined (fieldID INTEGER PRIMARY KEY, fieldName TEXT NOT NULL);
        CREATE TABLE itemCreators (
            itemID INTEGER,
            creatorID INTEGER,
            creatorTypeID INTEGER,
            orderIndex INTEGER
        );
        CREATE TABLE creators (
            creatorID INTEGER PRIMARY KEY,
            firstName TEXT,
            lastName TEXT,
            fieldMode INTEGER DEFAULT 0
        );
        CREATE TABLE creatorTypes (creatorTypeID INTEGER PRIMARY KEY, creatorType TEXT);
        CREATE TABLE itemTypeCreatorTypes (itemTypeID INTEGER, creatorTypeID INTEGER);
        CREATE TABLE itemTypeFields (itemTypeID INTEGER, fieldID INTEGER, orderIndex INTEGER);
        CREATE TABLE collections (
            collectionID INTEGER PRIMARY KEY,
            collectionName TEXT,
            parentCollectionID INTEGER,
            libraryID INTEGER DEFAULT 1,
            key TEXT NOT NULL,
            version INTEGER DEFAULT 0,
            synced INTEGER DEFAULT 0
        );
        CREATE TABLE collectionItems (
            collectionID INTEGER,
            itemID INTEGER,
            orderIndex INTEGER
        );
        CREATE TABLE tags (tagID INTEGER PRIMARY KEY, name TEXT NOT NULL);
        CREATE TABLE itemTags (itemID INTEGER, tagID INTEGER, type INTEGER DEFAULT 0);
        CREATE TABLE itemAttachments (
            itemID INTEGER PRIMARY KEY,
            parentItemID INTEGER,
            linkMode INTEGER,
            contentType TEXT,
            path TEXT
        );
        CREATE TABLE itemNotes (
            itemID INTEGER PRIMARY KEY,
            parentItemID INTEGER,
            note TEXT
        );
        CREATE TABLE deletedItems (itemID INTEGER PRIMARY KEY);
        CREATE TABLE libraries (
            libraryID INTEGER PRIMARY KEY,
            type TEXT,
            editable INT DEFAULT 1
        );
        CREATE VIEW fields AS SELECT fieldID, fieldName FROM fieldsCombined;

        INSERT INTO libraries (libraryID, type) VALUES (1, 'user');

        INSERT INTO itemTypes VALUES (1, 'book');
        INSERT INTO itemTypes VALUES (2, 'journalArticle');
        INSERT INTO itemTypes VALUES (3, 'attachment');
        INSERT INTO itemTypes VALUES (4, 'note');
        INSERT INTO itemTypes VALUES (5, 'conferencePaper');

        INSERT INTO creatorTypes VALUES (1, 'author');
        INSERT INTO creatorTypes VALUES (2, 'editor');

        INSERT INTO fieldsCombined VALUES (1, 'title');
        INSERT INTO fieldsCombined VALUES (2, 'date');
        INSERT INTO fieldsCombined VALUES (3, 'abstractNote');
        INSERT INTO fieldsCombined VALUES (4, 'url');
        INSERT INTO fieldsCombined VALUES (5, 'DOI');
        INSERT INTO fieldsCombined VALUES (6, 'ISBN');
        INSERT INTO fieldsCombined VALUES (7, 'publisher');

        INSERT INTO itemTypeFields VALUES (1, 1, 1);
        INSERT INTO itemTypeFields VALUES (1, 2, 2);
        INSERT INTO itemTypeFields VALUES (1, 7, 3);
        INSERT INTO itemTypeFields VALUES (2, 1, 1);
        INSERT INTO itemTypeFields VALUES (2, 2, 2);
        INSERT INTO itemTypeFields VALUES (2, 5, 3);
        INSERT INTO itemTypeFields VALUES (5, 1, 1);
        INSERT INTO itemTypeFields VALUES (5, 2, 2);

        INSERT INTO itemTypeCreatorTypes VALUES (1, 1);
        INSERT INTO itemTypeCreatorTypes VALUES (1, 2);
        INSERT INTO itemTypeCreatorTypes VALUES (2, 1);
        INSERT INTO itemTypeCreatorTypes VALUES (5, 1);
    """)
    return conn


@pytest.fixture
def zotero_db():
    """Pytest fixture: in-memory Zotero database."""
    conn = _create_test_zotero_db()
    yield conn
    conn.close()


@pytest.fixture
def zotero_db_path(tmp_path):
    """Pytest fixture: on-disk Zotero database (for functions requiring a Path)."""
    db_path = tmp_path / "zotero.sqlite"
    conn = _create_test_zotero_db()
    disk = sqlite3.connect(str(db_path))
    conn.backup(disk)
    disk.close()
    conn.close()
    return db_path


@pytest.fixture
def storage_dir(tmp_path):
    """Pytest fixture: temporary storage directory."""
    storage = tmp_path / "storage"
    storage.mkdir()
    return storage


@pytest.fixture
def sample_schema():
    """Pytest fixture: minimal Zotero schema dict for validation tests."""
    return {
        "itemTypes": {"book": 1, "journalArticle": 2},
        "fields": {"title": 1, "date": 2, "abstractNote": 3, "DOI": 5},
        "itemTypeFields": {1: [1, 2], 2: [1, 2, 5]},
        "itemTypeCreatorTypes": {1: [1, 2], 2: [1]},
        "creatorTypes": {"author": 1, "editor": 2},
    }


@pytest.fixture
def sample_item_data():
    """Pytest fixture: a valid minimal item data dict for a book."""
    return {
        "itemType": "book",
        "fields": {
            "title": "The Test Book",
            "date": "2024",
        },
        "creators": [
            {"creatorType": "author", "firstName": "Jane", "lastName": "Doe"},
        ],
        "tags": ["testing"],
        "collections": [],
    }
