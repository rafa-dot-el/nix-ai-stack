-- Official Calibre metadata.db schema from kovidgoyal/calibre
-- Source: https://github.com/kovidgoyal/calibre/blob/master/resources/metadata_sqlite.sql
-- Note: FTS virtual tables and views using custom aggregate functions (sortconcat, concat,
-- books_list_filter, title_sort) are omitted — SQLite cannot create those without
-- Calibre's C extensions. The core tables, triggers, and indexes are preserved.

CREATE TABLE authors ( id INTEGER PRIMARY KEY,
                       name TEXT NOT NULL COLLATE NOCASE,
                       sort TEXT COLLATE NOCASE,
                       link TEXT NOT NULL DEFAULT '',
                       UNIQUE(name));

CREATE TABLE books ( id INTEGER PRIMARY KEY AUTOINCREMENT,
                     title TEXT NOT NULL DEFAULT 'Unknown' COLLATE NOCASE,
                     sort TEXT COLLATE NOCASE,
                     timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                     pubdate TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                     series_index REAL NOT NULL DEFAULT 1.0,
                     author_sort TEXT COLLATE NOCASE,
                     path TEXT NOT NULL DEFAULT '',
                     uuid TEXT,
                     has_cover BOOL DEFAULT 0,
                     last_modified TIMESTAMP NOT NULL DEFAULT '2000-01-01 00:00:00+00:00');

CREATE TABLE books_authors_link ( id INTEGER PRIMARY KEY,
                                  book INTEGER NOT NULL,
                                  author INTEGER NOT NULL,
                                  UNIQUE(book, author));

CREATE TABLE books_languages_link ( id INTEGER PRIMARY KEY,
                                    book INTEGER NOT NULL,
                                    lang_code INTEGER NOT NULL,
                                    item_order INTEGER NOT NULL DEFAULT 0,
                                    UNIQUE(book, lang_code));

CREATE TABLE books_plugin_data(id INTEGER PRIMARY KEY,
                               book INTEGER NOT NULL,
                               name TEXT NOT NULL,
                               val TEXT NOT NULL,
                               UNIQUE(book,name));

CREATE TABLE books_publishers_link ( id INTEGER PRIMARY KEY,
                                     book INTEGER NOT NULL,
                                     publisher INTEGER NOT NULL,
                                     UNIQUE(book));

CREATE TABLE books_ratings_link ( id INTEGER PRIMARY KEY,
                                  book INTEGER NOT NULL,
                                  rating INTEGER NOT NULL,
                                  UNIQUE(book, rating));

CREATE TABLE books_series_link ( id INTEGER PRIMARY KEY,
                                 book INTEGER NOT NULL,
                                 series INTEGER NOT NULL,
                                 UNIQUE(book));

CREATE TABLE books_tags_link ( id INTEGER PRIMARY KEY,
                               book INTEGER NOT NULL,
                               tag INTEGER NOT NULL,
                               UNIQUE(book, tag));

CREATE TABLE comments ( id INTEGER PRIMARY KEY,
                        book INTEGER NOT NULL,
                        text TEXT NOT NULL COLLATE NOCASE,
                        UNIQUE(book));

CREATE TABLE conversion_options ( id INTEGER PRIMARY KEY,
                                  format TEXT NOT NULL COLLATE NOCASE,
                                  book INTEGER,
                                  data BLOB NOT NULL,
                                  UNIQUE(format,book));

CREATE TABLE custom_columns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    label TEXT NOT NULL,
    name TEXT NOT NULL,
    datatype TEXT NOT NULL,
    mark_for_delete BOOL DEFAULT 0 NOT NULL,
    editable BOOL DEFAULT 1 NOT NULL,
    display TEXT DEFAULT '{}' NOT NULL,
    is_multiple BOOL DEFAULT 0 NOT NULL,
    normalized BOOL NOT NULL,
    UNIQUE(label));

CREATE TABLE data ( id INTEGER PRIMARY KEY,
                    book INTEGER NOT NULL,
                    format TEXT NOT NULL COLLATE NOCASE,
                    uncompressed_size INTEGER NOT NULL,
                    name TEXT NOT NULL,
                    UNIQUE(book, format));

CREATE TABLE feeds ( id INTEGER PRIMARY KEY,
                     title TEXT NOT NULL,
                     script TEXT NOT NULL,
                     UNIQUE(title));

CREATE TABLE identifiers ( id INTEGER PRIMARY KEY,
                           book INTEGER NOT NULL,
                           type TEXT NOT NULL DEFAULT 'isbn' COLLATE NOCASE,
                           val TEXT NOT NULL COLLATE NOCASE,
                           UNIQUE(book, type));

CREATE TABLE languages ( id INTEGER PRIMARY KEY,
                         lang_code TEXT NOT NULL COLLATE NOCASE,
                         link TEXT NOT NULL DEFAULT '',
                         UNIQUE(lang_code));

CREATE TABLE library_id ( id INTEGER PRIMARY KEY,
                          uuid TEXT NOT NULL,
                          UNIQUE(uuid));

CREATE TABLE metadata_dirtied(id INTEGER PRIMARY KEY,
                              book INTEGER NOT NULL,
                              UNIQUE(book));

CREATE TABLE annotations_dirtied(id INTEGER PRIMARY KEY,
                                 book INTEGER NOT NULL,
                                 UNIQUE(book));

CREATE TABLE preferences(id INTEGER PRIMARY KEY,
                         key TEXT NOT NULL,
                         val TEXT NOT NULL,
                         UNIQUE(key));

CREATE TABLE publishers ( id INTEGER PRIMARY KEY,
                          name TEXT NOT NULL COLLATE NOCASE,
                          sort TEXT COLLATE NOCASE,
                          link TEXT NOT NULL DEFAULT '',
                          UNIQUE(name));

CREATE TABLE ratings ( id INTEGER PRIMARY KEY,
                       rating INTEGER CHECK(rating > -1 AND rating < 11),
                       link TEXT NOT NULL DEFAULT '',
                       UNIQUE (rating));

CREATE TABLE series ( id INTEGER PRIMARY KEY,
                      name TEXT NOT NULL COLLATE NOCASE,
                      sort TEXT COLLATE NOCASE,
                      link TEXT NOT NULL DEFAULT '',
                      UNIQUE (name));

CREATE TABLE tags ( id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL COLLATE NOCASE,
                    link TEXT NOT NULL DEFAULT '',
                    UNIQUE (name));

CREATE TABLE last_read_positions ( id INTEGER PRIMARY KEY,
                                   book INTEGER NOT NULL,
                                   format TEXT NOT NULL COLLATE NOCASE,
                                   user TEXT NOT NULL,
                                   device TEXT NOT NULL,
                                   cfi TEXT NOT NULL,
                                   epoch REAL NOT NULL,
                                   pos_frac REAL NOT NULL DEFAULT 0,
                                   UNIQUE(user, device, book, format));

CREATE TABLE annotations ( id INTEGER PRIMARY KEY,
                           book INTEGER NOT NULL,
                           format TEXT NOT NULL COLLATE NOCASE,
                           user_type TEXT NOT NULL,
                           user TEXT NOT NULL,
                           timestamp REAL NOT NULL,
                           annot_id TEXT NOT NULL,
                           annot_type TEXT NOT NULL,
                           annot_data TEXT NOT NULL,
                           searchable_text TEXT NOT NULL DEFAULT '',
                           UNIQUE(book, user_type, user, format, annot_type, annot_id));

-- Indexes
CREATE INDEX authors_idx ON books (author_sort COLLATE NOCASE);
CREATE INDEX books_authors_link_aidx ON books_authors_link (author);
CREATE INDEX books_authors_link_bidx ON books_authors_link (book);
CREATE INDEX books_idx ON books (sort COLLATE NOCASE);
CREATE INDEX books_languages_link_aidx ON books_languages_link (lang_code);
CREATE INDEX books_languages_link_bidx ON books_languages_link (book);
CREATE INDEX books_publishers_link_aidx ON books_publishers_link (publisher);
CREATE INDEX books_publishers_link_bidx ON books_publishers_link (book);
CREATE INDEX books_ratings_link_aidx ON books_ratings_link (rating);
CREATE INDEX books_ratings_link_bidx ON books_ratings_link (book);
CREATE INDEX books_series_link_aidx ON books_series_link (series);
CREATE INDEX books_series_link_bidx ON books_series_link (book);
CREATE INDEX books_tags_link_aidx ON books_tags_link (tag);
CREATE INDEX books_tags_link_bidx ON books_tags_link (book);
CREATE INDEX comments_idx ON comments (book);
CREATE INDEX conversion_options_idx_a ON conversion_options (format COLLATE NOCASE);
CREATE INDEX conversion_options_idx_b ON conversion_options (book);
CREATE INDEX custom_columns_idx ON custom_columns (label);
CREATE INDEX data_idx ON data (book);
CREATE INDEX lrp_idx ON last_read_positions (book);
CREATE INDEX annot_idx ON annotations (book);
CREATE INDEX formats_idx ON data (format);
CREATE INDEX languages_idx ON languages (lang_code COLLATE NOCASE);
CREATE INDEX publishers_idx ON publishers (name COLLATE NOCASE);
CREATE INDEX series_idx ON series (name COLLATE NOCASE);
CREATE INDEX tags_idx ON tags (name COLLATE NOCASE);

-- Triggers: cascade delete on books
CREATE TRIGGER books_delete_trg
    AFTER DELETE ON books
    BEGIN
        DELETE FROM books_authors_link WHERE book=OLD.id;
        DELETE FROM books_publishers_link WHERE book=OLD.id;
        DELETE FROM books_ratings_link WHERE book=OLD.id;
        DELETE FROM books_series_link WHERE book=OLD.id;
        DELETE FROM books_tags_link WHERE book=OLD.id;
        DELETE FROM books_languages_link WHERE book=OLD.id;
        DELETE FROM data WHERE book=OLD.id;
        DELETE FROM last_read_positions WHERE book=OLD.id;
        DELETE FROM annotations WHERE book=OLD.id;
        DELETE FROM comments WHERE book=OLD.id;
        DELETE FROM conversion_options WHERE book=OLD.id;
        DELETE FROM books_plugin_data WHERE book=OLD.id;
        DELETE FROM identifiers WHERE book=OLD.id;
    END;

-- Triggers: FK enforcement via RAISE
CREATE TRIGGER fkc_comments_insert BEFORE INSERT ON comments
    BEGIN SELECT CASE WHEN (SELECT id from books WHERE id=NEW.book) IS NULL
        THEN RAISE(ABORT, 'Foreign key violation: book not in books') END; END;
CREATE TRIGGER fkc_data_insert BEFORE INSERT ON data
    BEGIN SELECT CASE WHEN (SELECT id from books WHERE id=NEW.book) IS NULL
        THEN RAISE(ABORT, 'Foreign key violation: book not in books') END; END;
CREATE TRIGGER fkc_insert_books_authors_link BEFORE INSERT ON books_authors_link
    BEGIN SELECT CASE
        WHEN (SELECT id from books WHERE id=NEW.book) IS NULL
            THEN RAISE(ABORT, 'Foreign key violation: book not in books')
        WHEN (SELECT id from authors WHERE id=NEW.author) IS NULL
            THEN RAISE(ABORT, 'Foreign key violation: author not in authors')
    END; END;
CREATE TRIGGER fkc_insert_books_publishers_link BEFORE INSERT ON books_publishers_link
    BEGIN SELECT CASE
        WHEN (SELECT id from books WHERE id=NEW.book) IS NULL
            THEN RAISE(ABORT, 'Foreign key violation: book not in books')
        WHEN (SELECT id from publishers WHERE id=NEW.publisher) IS NULL
            THEN RAISE(ABORT, 'Foreign key violation: publisher not in publishers')
    END; END;
CREATE TRIGGER fkc_insert_books_ratings_link BEFORE INSERT ON books_ratings_link
    BEGIN SELECT CASE
        WHEN (SELECT id from books WHERE id=NEW.book) IS NULL
            THEN RAISE(ABORT, 'Foreign key violation: book not in books')
        WHEN (SELECT id from ratings WHERE id=NEW.rating) IS NULL
            THEN RAISE(ABORT, 'Foreign key violation: rating not in ratings')
    END; END;
CREATE TRIGGER fkc_insert_books_series_link BEFORE INSERT ON books_series_link
    BEGIN SELECT CASE
        WHEN (SELECT id from books WHERE id=NEW.book) IS NULL
            THEN RAISE(ABORT, 'Foreign key violation: book not in books')
        WHEN (SELECT id from series WHERE id=NEW.series) IS NULL
            THEN RAISE(ABORT, 'Foreign key violation: series not in series')
    END; END;
CREATE TRIGGER fkc_insert_books_tags_link BEFORE INSERT ON books_tags_link
    BEGIN SELECT CASE
        WHEN (SELECT id from books WHERE id=NEW.book) IS NULL
            THEN RAISE(ABORT, 'Foreign key violation: book not in books')
        WHEN (SELECT id from tags WHERE id=NEW.tag) IS NULL
            THEN RAISE(ABORT, 'Foreign key violation: tag not in tags')
    END; END;

-- Application marker
PRAGMA application_id = 0x63616c69;
PRAGMA user_version=27;
