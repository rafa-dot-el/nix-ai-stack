"""
fetch-metadata command: wraps fetch-ebook-metadata subprocess.
"""

import subprocess
import xml.etree.ElementTree as ET
from typing import Any


def fetch_metadata_from_calibre(
    isbn: str | None = None,
    title: str | None = None,
    authors: list[str] | None = None,
    timeout: int = 30,
) -> dict[str, Any] | None:
    """
    Fetch book metadata using Calibre's fetch-ebook-metadata tool.

    Uses Calibre's built-in metadata sources (Google Books, Open Library, Amazon, etc.).

    Args:
        isbn: Book ISBN (most reliable identifier)
        title: Book title
        authors: List of author names
        timeout: Timeout in seconds (default: 30)

    Returns:
        Dictionary with metadata fields, or None if fetch fails
    """
    import re

    cmd = ["fetch-ebook-metadata", "--opf", "--timeout", str(timeout)]

    if isbn:
        cmd.extend(["--isbn", isbn])
    elif title:
        cmd.extend(["--title", title])
        if authors:
            cmd.extend(["--authors", ", ".join(authors)])
    else:
        return None

    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout + 5)

        if result.returncode != 0:
            return None

        root = ET.fromstring(result.stdout)

        ns = {
            "opf": "http://www.idpf.org/2007/opf",
            "dc": "http://purl.org/dc/elements/1.1/",
        }

        metadata: dict[str, Any] = {}

        title_elem = root.find(".//dc:title", ns)
        if title_elem is not None and title_elem.text:
            metadata["title"] = title_elem.text

        authors_elems = root.findall('.//dc:creator[@opf:role="aut"]', ns)
        if authors_elems:
            metadata["authors"] = [elem.text for elem in authors_elems if elem.text]

        publisher_elem = root.find(".//dc:publisher", ns)
        if publisher_elem is not None and publisher_elem.text:
            metadata["publisher"] = publisher_elem.text

        date_elem = root.find(".//dc:date", ns)
        if date_elem is not None and date_elem.text:
            date_str = date_elem.text
            if "T" in date_str:
                date_str = date_str.split("T")[0]
            metadata["pubdate"] = date_str

        desc_elem = root.find(".//dc:description", ns)
        if desc_elem is not None and desc_elem.text:
            desc_text = desc_elem.text
            desc_text = re.sub(r"<[^>]+>", "", desc_text)
            desc_text = re.sub(r"&lt;", "<", desc_text)
            desc_text = re.sub(r"&gt;", ">", desc_text)
            desc_text = re.sub(r"&amp;", "&", desc_text)
            metadata["abstract"] = desc_text.strip()

        isbn_elem = root.find('.//dc:identifier[@opf:scheme="ISBN"]', ns)
        if isbn_elem is not None and isbn_elem.text:
            metadata["isbn"] = isbn_elem.text

        subject_elems = root.findall(".//dc:subject", ns)
        if subject_elems:
            metadata["tags"] = [elem.text for elem in subject_elems if elem.text]

        lang_elem = root.find(".//dc:language", ns)
        if lang_elem is not None and lang_elem.text:
            metadata["language"] = lang_elem.text

        return metadata if metadata else None

    except subprocess.TimeoutExpired:
        return None
    except FileNotFoundError:
        return None
    except Exception:
        return None
