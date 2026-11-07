"""
Path sanitization and library path layout helpers.
"""

from pathlib import Path
from typing import Any

SUPPORTED_FORMATS = [
    "PDF",
    "EPUB",
    "MOBI",
    "AZW3",
    "TXT",
    "HTML",
    "RTF",
    "DOC",
    "DOCX",
    "CBZ",
    "CBR",
]


def sanitize_path_component(text: str) -> str:
    """
    Sanitize text for use in file paths.

    Removes or replaces invalid filesystem characters.

    Args:
        text: Text to sanitize

    Returns:
        str: Sanitized text safe for filenames

    Example:
        >>> sanitize_path_component("Title: With/Bad*Chars?")
        "Title_ With_Bad_Chars_"
    """
    invalid_chars = ["/", "\\", ":", "*", "?", '"', "<", ">", "|"]

    result = text
    for char in invalid_chars:
        result = result.replace(char, "_")

    result = result.strip(". ")

    if not result:
        result = "Unknown"

    return result


def generate_storage_path(author_name: str, title: str, book_id: int) -> str:
    """
    Generate Calibre-style storage path for a book.

    Pattern: {Author Name}/{Title} ({book_id})

    Args:
        author_name: First author name (or "Unknown")
        title: Book title
        book_id: Book database ID

    Returns:
        str: Relative path from storage root

    Example:
        >>> generate_storage_path("Isaac Asimov", "Foundation", 42)
        "Isaac Asimov/Foundation (42)"
    """
    safe_author = sanitize_path_component(author_name) if author_name else "Unknown"
    safe_title = sanitize_path_component(title)
    return f"{safe_author}/{safe_title} ({book_id})"


def detect_file_format(filepath: str) -> str | None:
    """
    Detect file format from file extension.

    Args:
        filepath: Path to file

    Returns:
        str: Uppercase format string (e.g., "PDF", "EPUB"), or None if unknown
    """
    format_map = {
        ".pdf": "PDF",
        ".epub": "EPUB",
        ".mobi": "MOBI",
        ".azw": "AZW",
        ".azw3": "AZW3",
        ".txt": "TXT",
        ".html": "HTML",
        ".htm": "HTML",
        ".rtf": "RTF",
        ".doc": "DOC",
        ".docx": "DOCX",
        ".odt": "ODT",
        ".cbz": "CBZ",
        ".cbr": "CBR",
        ".zip": "ZIP",
        ".fb2": "FB2",
        ".djvu": "DJVU",
        ".lit": "LIT",
        ".prc": "PRC",
        ".pdb": "PDB",
    }
    ext = Path(filepath).suffix.lower()
    return format_map.get(ext)


def resolve_file_paths(
    storage_root: Path, book_path: str, files: list[dict[str, Any]]
) -> list[dict[str, Any]]:
    """
    Resolve file paths from database format to absolute filesystem paths.

    Args:
        storage_root: Root storage directory
        book_path: Book's relative path from storage root
        files: List of file dictionaries with 'format', 'name', 'size'

    Returns:
        List of file dictionaries with added 'path' field
    """
    result = []
    for file_info in files:
        file_with_path = dict(file_info)
        file_format = file_info["format"].lower()
        file_name = file_info["name"]
        file_path = storage_root / book_path / f"{file_name}.{file_format}"
        file_with_path["path"] = str(file_path)
        result.append(file_with_path)
    return result


def resolve_cover_path(storage_root: Path, book_path: str, has_cover: bool) -> str | None:
    """
    Resolve cover image path.

    Calibre stores cover images as cover.jpg in the book's directory.

    Args:
        storage_root: Root storage directory
        book_path: Book's relative path from storage root
        has_cover: Whether book has a cover image

    Returns:
        Absolute path to cover.jpg if has_cover is True, None otherwise
    """
    if not has_cover:
        return None
    cover_path = storage_root / book_path / "cover.jpg"
    return str(cover_path)


def resolve_format_paths(
    storage_root: Path, book_path: str, formats_dict: dict[str, list[str]]
) -> dict[str, list[str]]:
    """
    Resolve format file paths from file names to absolute paths.

    Args:
        storage_root: Root storage directory
        book_path: Book's relative path from storage root
        formats_dict: Dictionary mapping format to list of file names

    Returns:
        Dictionary mapping format to list of absolute file paths
    """
    result = {}
    for file_format, file_names in formats_dict.items():
        paths = []
        for file_name in file_names:
            file_path = storage_root / book_path / f"{file_name}.{file_format.lower()}"
            paths.append(str(file_path))
        result[file_format] = paths
    return result
