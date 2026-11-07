"""
Tests for calibrectl.paths
"""

import pytest

from calibrectl.paths import (
    detect_file_format,
    generate_storage_path,
    resolve_cover_path,
    resolve_file_paths,
    resolve_format_paths,
    sanitize_path_component,
)


class TestSanitizePathComponent:
    def test_plain_text_unchanged(self):
        assert sanitize_path_component("Hello World") == "Hello World"

    def test_slash_replaced(self):
        assert "/" not in sanitize_path_component("path/component")

    def test_backslash_replaced(self):
        assert "\\" not in sanitize_path_component("path\\component")

    def test_colon_replaced(self):
        assert ":" not in sanitize_path_component("Title: Subtitle")

    def test_asterisk_replaced(self):
        assert "*" not in sanitize_path_component("file*name")

    def test_question_mark_replaced(self):
        assert "?" not in sanitize_path_component("what?")

    def test_quotes_replaced(self):
        assert '"' not in sanitize_path_component('"quoted"')

    def test_angle_brackets_replaced(self):
        result = sanitize_path_component("<tag>")
        assert "<" not in result
        assert ">" not in result

    def test_pipe_replaced(self):
        assert "|" not in sanitize_path_component("a|b")

    def test_empty_returns_unknown(self):
        assert sanitize_path_component("") == "Unknown"

    def test_only_dots_returns_unknown(self):
        assert sanitize_path_component("...") == "Unknown"

    def test_leading_trailing_dots_stripped(self):
        result = sanitize_path_component(".hidden.")
        assert not result.startswith(".")
        assert not result.endswith(".")

    @pytest.mark.parametrize(
        "text",
        [
            "café résumé",
            "日本語タイトル",
            "Ärger mit Technik",
        ],
    )
    def test_unicode_preserved(self, text):
        result = sanitize_path_component(text)
        assert len(result) > 0

    @pytest.mark.parametrize(
        "special",
        ["#", "%", "&", "@", "!"],
    )
    def test_special_chars_allowed(self, special):
        # These chars are not in the invalid_chars list — they pass through
        result = sanitize_path_component(f"name{special}here")
        assert special in result

    def test_path_traversal_dot_dot_no_slash(self):
        # slash is removed, dots are stripped from edges
        result = sanitize_path_component("../../etc/passwd")
        assert "/" not in result

    def test_very_long_path_component(self):
        long_name = "a" * 500
        result = sanitize_path_component(long_name)
        assert len(result) == 500


class TestGenerateStoragePath:
    def test_basic(self):
        result = generate_storage_path("Isaac Asimov", "Foundation", 42)
        assert result == "Isaac Asimov/Foundation (42)"

    def test_no_author_fallback(self):
        result = generate_storage_path("", "Title", 1)
        assert "Unknown" in result

    def test_none_author_fallback(self):
        result = generate_storage_path(None, "Title", 1)
        assert "Unknown" in result

    def test_special_chars_in_title(self):
        result = generate_storage_path("Author", "Title: Subtitle", 5)
        assert ":" not in result

    def test_book_id_in_path(self):
        result = generate_storage_path("Author", "Book", 99)
        assert "(99)" in result


class TestDetectFileFormat:
    @pytest.mark.parametrize(
        "filepath, expected",
        [
            ("/path/to/book.pdf", "PDF"),
            ("/path/to/book.epub", "EPUB"),
            ("/path/to/book.mobi", "MOBI"),
            ("/path/to/book.azw3", "AZW3"),
            ("/path/to/book.txt", "TXT"),
            ("/path/to/book.cbz", "CBZ"),
            ("/path/to/file.xyz", None),
            ("/path/to/book.PDF", "PDF"),
            ("/path/to/book.EPUB", "EPUB"),
        ],
    )
    def test_format_detection(self, filepath, expected):
        assert detect_file_format(filepath) == expected


class TestResolveFilePaths:
    def test_resolves_pdf(self, tmp_path):
        files = [{"format": "PDF", "name": "Book - Author", "size": 1024}]
        result = resolve_file_paths(tmp_path, "Author/Book (1)", files)
        assert len(result) == 1
        assert result[0]["path"].endswith("Book - Author.pdf")

    def test_resolves_epub(self, tmp_path):
        files = [{"format": "EPUB", "name": "Book - Author", "size": 512}]
        result = resolve_file_paths(tmp_path, "Author/Book (1)", files)
        assert result[0]["path"].endswith("Book - Author.epub")

    def test_original_fields_preserved(self, tmp_path):
        files = [{"format": "PDF", "name": "mybook", "size": 9999}]
        result = resolve_file_paths(tmp_path, "A/B", files)
        assert result[0]["format"] == "PDF"
        assert result[0]["name"] == "mybook"
        assert result[0]["size"] == 9999


class TestResolveCoverPath:
    def test_has_cover_true(self, tmp_path):
        result = resolve_cover_path(tmp_path, "Author/Book (1)", True)
        assert result is not None
        assert result.endswith("cover.jpg")

    def test_has_cover_false(self, tmp_path):
        result = resolve_cover_path(tmp_path, "Author/Book (1)", False)
        assert result is None


class TestResolveFormatPaths:
    def test_basic(self, tmp_path):
        formats = {"EPUB": ["Book - Author"], "PDF": ["Book - Author"]}
        result = resolve_format_paths(tmp_path, "Author/Book (1)", formats)
        assert "EPUB" in result
        assert "PDF" in result
        assert result["EPUB"][0].endswith("Book - Author.epub")
        assert result["PDF"][0].endswith("Book - Author.pdf")

    def test_multiple_files_per_format(self, tmp_path):
        formats = {"PDF": ["file1", "file2"]}
        result = resolve_format_paths(tmp_path, "A/B", formats)
        assert len(result["PDF"]) == 2
