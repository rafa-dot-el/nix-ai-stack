"""
ISBN extraction from PDF/EPUB files and text.
"""

import re
import sys
from pathlib import Path
from typing import Any

ISBN_PATTERNS = [
    r"ISBN(?:-13)?[:\s]+978[-\s]?\d{1,5}[-\s]?\d{1,7}[-\s]?\d{1,7}[-\s]?\d",
    r"ISBN(?:-10)?[:\s]+\d{1,5}[-\s]?\d{1,7}[-\s]?\d{1,7}[-\s]?\d",
    r"978[-\s]?\d{10}",
    r"ISBN[:\s]+\d{10,13}",
]


def normalize_isbn(isbn_str: str) -> str:
    """Normalize ISBN string by removing hyphens, spaces, and labels."""
    isbn_str = re.sub(r"ISBN[:\s-]*(?:13|10)?[:\s-]*", "", isbn_str, flags=re.IGNORECASE)
    isbn_str = re.sub(r"[^\dXx]", "", isbn_str)
    return isbn_str


def validate_isbn(isbn: str) -> bool:
    """Validate ISBN-10 or ISBN-13 checksum."""
    isbn = isbn.strip().replace("-", "").replace(" ", "")

    if len(isbn) == 10:
        if not (isbn[:-1].isdigit() and (isbn[-1].isdigit() or isbn[-1].upper() == "X")):
            return False
        total = sum((10 - i) * (10 if c.upper() == "X" else int(c)) for i, c in enumerate(isbn))
        return total % 11 == 0

    elif len(isbn) == 13:
        if not isbn.isdigit():
            return False
        total = sum(int(c) * (1 if i % 2 == 0 else 3) for i, c in enumerate(isbn))
        return total % 10 == 0

    return False


def find_isbn_in_text(text: str) -> list[str]:
    """Search text for ISBN patterns and return all valid ISBNs found."""
    found_isbns = []
    for pattern in ISBN_PATTERNS:
        for match in re.finditer(pattern, text, re.IGNORECASE):
            isbn_str = match.group(0)
            normalized = normalize_isbn(isbn_str)
            if normalized and validate_isbn(normalized) and normalized not in found_isbns:
                found_isbns.append(normalized)
    found_isbns.sort(key=len, reverse=True)
    return found_isbns


def extract_isbn_from_epub_metadata(filepath: Path) -> str | None:
    """
    Extract ISBN from EPUB metadata (fast path).

    Uses zipfile + xml.etree — no external dependencies required.
    """
    try:
        import xml.etree.ElementTree as ET
        import zipfile

        with zipfile.ZipFile(str(filepath), "r") as epub:
            container = epub.read("META-INF/container.xml")
            container_root = ET.fromstring(container)
            ns = {"container": "urn:oasis:names:tc:opendocument:xmlns:container"}
            rootfile_el = container_root.find(".//container:rootfile", ns)
            if rootfile_el is None:
                return None
            opf_path = rootfile_el.get("full-path")
            if opf_path is None:
                return None
            opf_content = epub.read(opf_path)
            opf_root = ET.fromstring(opf_content)
            ns_dc = {"dc": "http://purl.org/dc/elements/1.1/"}
            for identifier in opf_root.findall(".//dc:identifier", ns_dc):
                id_value = identifier.text or ""
                if "isbn" in id_value.lower() or re.match(r"97[89]\d{10}", id_value):
                    return normalize_isbn(id_value)
        return None
    except Exception:
        return None


def extract_isbn_from_pdf(filepath: Path, max_chars: int = 50000) -> str | None:
    """
    Extract ISBN from PDF file using pypdf text extraction.

    Args:
        filepath: Path to PDF file
        max_chars: Maximum characters to extract

    Returns:
        ISBN string if found, None otherwise
    """
    try:
        from pypdf import PdfReader

        reader = PdfReader(str(filepath))
        text_parts = []
        chars_extracted = 0

        for page in reader.pages:
            if chars_extracted >= max_chars:
                break
            page_text = page.extract_text()
            text_parts.append(page_text)
            chars_extracted += len(page_text)

        text = "\n".join(text_parts)
        if len(text) > max_chars:
            text = text[:max_chars]

        isbns = find_isbn_in_text(text)
        return isbns[0] if isbns else None
    except Exception as exc:
        print(
            f"[DEBUG] Exception in extract_isbn_from_pdf: {type(exc).__name__}: {exc}",
            file=sys.stderr,
        )
        return None


def extract_isbn_from_epub(filepath: Path, max_chars: int = 50000) -> str | None:
    """
    Extract ISBN from EPUB file.

    Tries metadata first, then full text extraction via ebooklib.

    Args:
        filepath: Path to EPUB file
        max_chars: Maximum characters to extract from text

    Returns:
        ISBN string if found, None otherwise
    """
    # Fast path: metadata
    isbn = extract_isbn_from_epub_metadata(filepath)
    if isbn:
        return isbn

    # Slow path: text extraction
    try:
        from html.parser import HTMLParser

        from ebooklib import epub

        class _SimpleHTMLParser(HTMLParser):
            def __init__(self):
                super().__init__()
                self.text_parts: list[str] = []

            def handle_data(self, data):
                self.text_parts.append(data)

            def get_text(self) -> str:
                return "".join(self.text_parts)

        book = epub.read_epub(str(filepath))
        text_parts = []
        chars_extracted = 0

        for item_id, _ in book.spine:
            if chars_extracted >= max_chars:
                break
            item = book.get_item_with_id(item_id)
            if not item:
                continue
            content = item.get_content().decode("utf-8", errors="ignore")
            parser = _SimpleHTMLParser()
            parser.feed(content)
            chapter_text = parser.get_text()
            text_parts.append(chapter_text.strip())
            chars_extracted += len(chapter_text)

        text = "\n\n".join(text_parts)
        if len(text) > max_chars:
            text = text[:max_chars]

        isbns = find_isbn_in_text(text)
        return isbns[0] if isbns else None
    except Exception as exc:
        print(
            f"[DEBUG] Exception in extract_isbn_from_epub: {type(exc).__name__}: {exc}",
            file=sys.stderr,
        )
        return None


def find_isbn(filepath: Path, max_chars: int = 50000) -> str | None:
    """
    Dispatch ISBN extraction based on file extension.

    Args:
        filepath: Path to PDF or EPUB file
        max_chars: Maximum characters to extract

    Returns:
        ISBN string if found, None otherwise
    """
    suffix = filepath.suffix.lower()
    if suffix == ".pdf":
        return extract_isbn_from_pdf(filepath, max_chars)
    elif suffix == ".epub":
        return extract_isbn_from_epub(filepath, max_chars)
    return None


def find_isbn_for_book(
    book: dict[str, Any], _storage_path: Path, max_chars: int = 50000
) -> str | None:
    """
    Try multiple methods to find ISBN for a book from its files.

    Args:
        book: Book dictionary from database (must have 'files' with 'path' keys)
        storage_path: Storage root path (unused here, paths already resolved)
        max_chars: Maximum characters to extract

    Returns:
        ISBN string if found, None otherwise
    """
    files = book.get("files", [])
    if not files:
        return None

    for file_info in files:
        file_path = Path(file_info["path"])
        if not file_path.exists():
            continue
        isbn = find_isbn(file_path, max_chars)
        if isbn:
            return isbn

    return None
