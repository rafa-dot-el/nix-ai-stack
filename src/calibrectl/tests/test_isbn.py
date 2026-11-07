"""
Tests for calibrectl.isbn
"""

from pathlib import Path

import pytest

from calibrectl.isbn import (
    extract_isbn_from_epub,
    extract_isbn_from_pdf,
    find_isbn,
    find_isbn_in_text,
    normalize_isbn,
    validate_isbn,
)


class TestValidateISBN:
    @pytest.mark.parametrize(
        "isbn, expected",
        [
            ("9780596005900", True),
            ("9780596005901", False),  # wrong checksum
            ("0596005903", True),
            ("080442957X", True),
            ("0804429570", False),  # wrong checksum for ISBN-10
            ("123", False),
            ("", False),
            ("abcdefghijklm", False),
        ],
    )
    def test_validate_isbn(self, isbn, expected):
        assert validate_isbn(isbn) == expected


class TestNormalizeISBN:
    def test_hyphens_removed(self):
        assert normalize_isbn("978-0-596-00590-0") == "9780596005900"

    def test_spaces_removed(self):
        assert normalize_isbn("978 0 596 00590 0") == "9780596005900"

    def test_isbn_label_removed(self):
        assert normalize_isbn("ISBN: 9780596005900") == "9780596005900"

    def test_isbn13_label_removed(self):
        assert normalize_isbn("ISBN-13: 978-0-596-00590-0") == "9780596005900"

    def test_isbn10_label_removed(self):
        result = normalize_isbn("ISBN-10: 0596005903")
        assert result == "0596005903"

    def test_already_normalized(self):
        assert normalize_isbn("9780596005900") == "9780596005900"


class TestFindIsbnInText:
    def test_isbn13_found(self):
        text = "The ISBN: 9780596005900 for this book."
        isbns = find_isbn_in_text(text)
        assert "9780596005900" in isbns

    def test_isbn_not_found(self):
        assert find_isbn_in_text("No ISBN here.") == []

    def test_longer_isbn_preferred(self):
        text = "ISBN 9780596005900 and ISBN 0596005903"
        isbns = find_isbn_in_text(text)
        # ISBN-13 should come first (longer)
        if len(isbns) >= 2:
            assert len(isbns[0]) >= len(isbns[1])

    def test_no_duplicates(self):
        text = "ISBN 9780596005900 repeated: 9780596005900"
        isbns = find_isbn_in_text(text)
        assert len(isbns) == len(set(isbns))

    def test_invalid_isbn_not_included(self):
        # 9780000000000 has bad checksum
        isbns = find_isbn_in_text("ISBN 9780000000000")
        assert "9780000000000" not in isbns


class TestExtractIsbnFromPdf:
    def test_success(self, tmp_path, mocker):
        mock_reader = mocker.MagicMock()
        mock_page = mocker.MagicMock()
        mock_page.extract_text.return_value = "ISBN 9780596005900 some text"
        mock_reader.pages = [mock_page]

        # Patch at the source module level so lazy import picks it up
        mocker.patch("pypdf.PdfReader", return_value=mock_reader)

        pdf_file = tmp_path / "book.pdf"
        pdf_file.write_bytes(b"%PDF-1.4")

        result = extract_isbn_from_pdf(pdf_file)
        assert result == "9780596005900"

    def test_not_found(self, tmp_path, mocker):
        mock_reader = mocker.MagicMock()
        mock_page = mocker.MagicMock()
        mock_page.extract_text.return_value = "No ISBN here at all."
        mock_reader.pages = [mock_page]

        mocker.patch("pypdf.PdfReader", return_value=mock_reader)

        pdf_file = tmp_path / "book.pdf"
        pdf_file.write_bytes(b"%PDF-1.4")

        result = extract_isbn_from_pdf(pdf_file)
        assert result is None

    def test_corrupted_file_returns_none(self, tmp_path, mocker):
        mocker.patch("pypdf.PdfReader", side_effect=Exception("Corrupt"))

        pdf_file = tmp_path / "book.pdf"
        pdf_file.write_bytes(b"not a pdf")

        result = extract_isbn_from_pdf(pdf_file)
        assert result is None


class TestExtractIsbnFromEpub:
    def test_metadata_fast_path(self, tmp_path, mocker):
        mocker.patch(
            "calibrectl.isbn.extract_isbn_from_epub_metadata",
            return_value="9780596005900",
        )

        epub_file = tmp_path / "book.epub"
        epub_file.write_bytes(b"PK")

        result = extract_isbn_from_epub(epub_file)
        assert result == "9780596005900"

    def test_text_extraction_fallback(self, tmp_path, mocker):
        mocker.patch(
            "calibrectl.isbn.extract_isbn_from_epub_metadata",
            return_value=None,
        )

        mock_item = mocker.MagicMock()
        mock_item.get_content.return_value = b"<html><body>ISBN 9780596005900</body></html>"

        mock_book = mocker.MagicMock()
        mock_book.spine = [("item1", None)]
        mock_book.get_item_with_id.return_value = mock_item

        mocker.patch("ebooklib.epub.read_epub", return_value=mock_book)

        epub_file = tmp_path / "book.epub"
        epub_file.write_bytes(b"PK")

        result = extract_isbn_from_epub(epub_file)
        assert result == "9780596005900"

    def test_not_found_returns_none(self, tmp_path, mocker):
        mocker.patch(
            "calibrectl.isbn.extract_isbn_from_epub_metadata",
            return_value=None,
        )

        mock_item = mocker.MagicMock()
        mock_item.get_content.return_value = b"<html><body>No ISBN here</body></html>"

        mock_book = mocker.MagicMock()
        mock_book.spine = [("item1", None)]
        mock_book.get_item_with_id.return_value = mock_item

        mocker.patch("ebooklib.epub.read_epub", return_value=mock_book)

        epub_file = tmp_path / "book.epub"
        epub_file.write_bytes(b"PK")

        result = extract_isbn_from_epub(epub_file)
        assert result is None

    def test_corrupted_returns_none(self, tmp_path, mocker):
        mocker.patch(
            "calibrectl.isbn.extract_isbn_from_epub_metadata",
            return_value=None,
        )
        mocker.patch(
            "ebooklib.epub.read_epub",
            side_effect=Exception("corrupt"),
        )

        epub_file = tmp_path / "book.epub"
        epub_file.write_bytes(b"PK")

        result = extract_isbn_from_epub(epub_file)
        assert result is None


class TestFindIsbn:
    def test_dispatches_pdf(self, tmp_path, mocker):
        # patch the function in the module where it is defined
        mocker.patch("calibrectl.isbn.extract_isbn_from_pdf", return_value="9780596005900")
        pdf = tmp_path / "book.pdf"
        pdf.write_bytes(b"")
        result = find_isbn(pdf)
        assert result == "9780596005900"

    def test_dispatches_epub(self, tmp_path, mocker):
        mocker.patch("calibrectl.isbn.extract_isbn_from_epub", return_value="9780596005900")
        epub_file = tmp_path / "book.epub"
        epub_file.write_bytes(b"")
        result = find_isbn(epub_file)
        assert result == "9780596005900"

    def test_unknown_extension_returns_none(self, tmp_path):
        unknown = tmp_path / "book.docx"
        unknown.write_bytes(b"")
        assert find_isbn(unknown) is None


class TestExtractIsbnFromEpubMetadata:
    def _make_epub(self, tmp_path: Path, isbn: str) -> Path:
        """Create a minimal EPUB zip with OPF metadata containing an ISBN."""
        import zipfile

        opf = f"""<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Book</dc:title>
    <dc:identifier id="bookid">isbn:{isbn}</dc:identifier>
  </metadata>
</package>"""
        container = """<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf"
              media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"""
        epub_path = tmp_path / "test.epub"
        with zipfile.ZipFile(str(epub_path), "w") as zf:
            zf.writestr("META-INF/container.xml", container)
            zf.writestr("OEBPS/content.opf", opf)
        return epub_path

    def test_extracts_isbn_from_epub_metadata(self, tmp_path):
        from calibrectl.isbn import extract_isbn_from_epub_metadata

        epub = self._make_epub(tmp_path, "9780596005900")
        result = extract_isbn_from_epub_metadata(epub)
        assert result == "9780596005900"

    def test_returns_none_for_corrupt_epub(self, tmp_path):
        from calibrectl.isbn import extract_isbn_from_epub_metadata

        corrupt = tmp_path / "bad.epub"
        corrupt.write_bytes(b"not a zip file")
        assert extract_isbn_from_epub_metadata(corrupt) is None

    def test_returns_none_for_epub_without_isbn(self, tmp_path):
        import zipfile

        from calibrectl.isbn import extract_isbn_from_epub_metadata

        opf = """<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>No ISBN Book</dc:title>
  </metadata>
</package>"""
        container = """<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf"
              media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"""
        epub_path = tmp_path / "no_isbn.epub"
        with zipfile.ZipFile(str(epub_path), "w") as zf:
            zf.writestr("META-INF/container.xml", container)
            zf.writestr("OEBPS/content.opf", opf)
        assert extract_isbn_from_epub_metadata(epub_path) is None


class TestFindIsbnForBook:
    def test_no_files_returns_none(self, tmp_path):
        from calibrectl.isbn import find_isbn_for_book

        book = {"files": []}
        assert find_isbn_for_book(book, tmp_path) is None

    def test_missing_file_skipped(self, tmp_path):
        from calibrectl.isbn import find_isbn_for_book

        book = {"files": [{"path": str(tmp_path / "nonexistent.pdf")}]}
        assert find_isbn_for_book(book, tmp_path) is None

    def test_returns_isbn_from_file(self, tmp_path, mocker):
        from calibrectl.isbn import find_isbn_for_book

        epub = tmp_path / "book.epub"
        epub.write_bytes(b"")
        mocker.patch("calibrectl.isbn.find_isbn", return_value="9780596005900")
        book = {"files": [{"path": str(epub)}]}
        result = find_isbn_for_book(book, tmp_path)
        assert result == "9780596005900"

    def test_empty_book_no_files_key(self, tmp_path):
        from calibrectl.isbn import find_isbn_for_book

        assert find_isbn_for_book({}, tmp_path) is None
