"""
BibTeX conversion utilities for zoteroctl.

All functions are pure (no I/O, no database access).
"""

import re
from typing import Any


def escape_bibtex(text: str) -> str:
    """
    Escape special BibTeX characters.

    Args:
        text: Text to escape

    Returns:
        Escaped text safe for BibTeX
    """
    if not text:
        return ""

    # Replace backslash with placeholder first to prevent double-escaping
    # when {} in \textbackslash{} would otherwise get escaped by the {} rules.
    _BS = "\x00BACKSLASH\x00"
    result = text.replace("\\", _BS)
    for char, replacement in [
        ("{", "\\{"),
        ("}", "\\}"),
        ("%", "\\%"),
        ("&", "\\&"),
        ("$", "\\$"),
        ("#", "\\#"),
        ("_", "\\_"),
        ("~", "\\textasciitilde{}"),
        ("^", "\\textasciicircum{}"),
    ]:
        result = result.replace(char, replacement)
    return result.replace(_BS, "\\textbackslash{}")


def generate_cite_key(item_data: dict[str, Any], used_keys: set) -> str:
    """
    Generate unique BibTeX cite key.

    Args:
        item_data: Item data dictionary
        used_keys: Set of already used cite keys

    Returns:
        Unique cite key in format LastnameYEAR or KeyYEAR

    Pattern: AuthorLastnameYYYY, with suffix (a, b, c) if duplicate
    """
    date_str = item_data["fields"].get("date", "")
    year_match = re.search(r"(\d{4})", date_str)
    year = year_match.group(1) if year_match else "NODATE"

    base_key = None
    creators = item_data.get("creators", [])

    if creators:
        first_author = creators[0]
        if first_author.get("fieldMode") == 0:
            last_name = first_author.get("lastName", "").strip()
        else:
            # Corporate/institutional author: prefer container title for key
            container = (
                item_data["fields"].get("encyclopediaTitle")
                or item_data["fields"].get("publicationTitle")
                or item_data["fields"].get("conferenceName")
            )
            if container:
                last_name = container.split()[0]
            else:
                name = first_author.get("name", "").strip()
                last_name = name.split()[0] if name else ""

        last_name = re.sub(r"[^a-zA-Z]", "", last_name)
        if last_name:
            base_key = f"{last_name}{year}"

    if not base_key:
        base_key = f"{item_data['key']}{year}"

    cite_key = base_key
    n = 1
    while cite_key in used_keys:
        suffix = chr(ord("a") + n - 1) if n <= 26 else str(n)
        cite_key = f"{base_key}{suffix}"
        n += 1

    used_keys.add(cite_key)
    return cite_key


def item_to_bibtex(item_data: dict[str, Any], used_keys: set) -> str:
    """
    Convert Zotero item to BibTeX entry.

    Args:
        item_data: Complete item data from get_item_by_key()
        used_keys: Set of already used cite keys

    Returns:
        BibTeX entry as string
    """
    type_map = {
        "book": "book",
        "journalArticle": "article",
        "conferencePaper": "inproceedings",
        "thesis": "phdthesis",
        "webpage": "online",
        "report": "techreport",
        "bookSection": "incollection",
        "magazineArticle": "article",
        "newspaperArticle": "article",
        "patent": "misc",
        "document": "misc",
        "email": "misc",
        "encyclopediaArticle": "misc",
        "dictionaryEntry": "misc",
        "forumPost": "online",
        "blogPost": "online",
        "presentation": "misc",
        "computerProgram": "software",
        "manuscript": "unpublished",
        "note": "misc",
    }

    bibtex_type = type_map.get(item_data["itemType"], "misc")
    cite_key = generate_cite_key(item_data, used_keys)

    lines = [f"@{bibtex_type}{{{cite_key},"]

    field_map = {
        "title": "title",
        "publisher": "publisher",
        "publicationTitle": "journal",
        "volume": "volume",
        "issue": "number",
        "pages": "pages",
        "DOI": "doi",
        "ISBN": "isbn",
        "ISSN": "issn",
        "url": "url",
        "place": "address",
        "edition": "edition",
        "series": "series",
        "abstractNote": "abstract",
        "accessDate": "urldate",
        "conferenceName": "booktitle",
        "encyclopediaTitle": "booktitle",
        "language": "langid",
    }

    date_str = item_data["fields"].get("date", "")
    if date_str:
        year_match = re.search(r"(\d{4})", date_str)
        if year_match:
            lines.append(f"  year = {{{year_match.group(1)}}},")

    for zotero_field, bibtex_field in field_map.items():
        if zotero_field in item_data["fields"]:
            value = item_data["fields"][zotero_field]
            escaped_value = escape_bibtex(str(value))
            lines.append(f"  {bibtex_field} = {{{escaped_value}}},")

    creators = item_data.get("creators", [])
    if creators:
        authors = []
        editors = []

        for creator in creators:
            field_mode = creator.get("fieldMode")

            if field_mode == 0:
                first = creator.get("firstName", "").strip()
                last = creator.get("lastName", "").strip()
                name = f"{last}, {first}" if first else last
            else:
                name = creator.get("name", "").strip()

            creator_type = creator.get("creatorType", "author")
            if creator_type in ("author", "artist", "contributor"):
                authors.append(name)
            elif creator_type == "editor":
                editors.append(name)

        if authors:
            author_str = " and ".join(authors)
            lines.append(f"  author = {{{author_str}}},")

        if editors:
            editor_str = " and ".join(editors)
            lines.append(f"  editor = {{{editor_str}}},")

    if lines[-1].endswith(","):
        lines[-1] = lines[-1][:-1]

    lines.append("}")

    return "\n".join(lines)


def map_bibtex_type(entry_type: str) -> str:
    """
    Map BibTeX entry type to Zotero itemType.

    Pure function - no I/O, only data transformation.

    Args:
        entry_type: BibTeX entry type (article, book, inproceedings, etc.)

    Returns:
        Zotero itemType string

    Examples:
        >>> map_bibtex_type("article")
        'journalArticle'
        >>> map_bibtex_type("book")
        'book'
    """
    type_map = {
        "article": "journalArticle",
        "book": "book",
        "inproceedings": "conferencePaper",
        "incollection": "bookSection",
        "phdthesis": "thesis",
        "mastersthesis": "thesis",
        "techreport": "report",
        "misc": "document",
        "unpublished": "manuscript",
        "proceedings": "book",
        "inbook": "bookSection",
    }
    return type_map.get(entry_type.lower(), "document")


def parse_bibtex_authors(author_string: str) -> list[dict[str, str]]:
    """
    Parse BibTeX author string to Zotero creator format.

    Pure function - no I/O, only data transformation.

    Args:
        author_string: BibTeX author string (e.g., "Doe, John and Smith, Jane")

    Returns:
        List of creator dictionaries with creatorType, firstName, lastName
    """
    creators = []
    authors = author_string.split(" and ")

    for author in authors:
        author = author.strip()
        if not author:
            continue

        if "," in author:
            parts = author.split(",", 1)
            last_name = parts[0].strip()
            first_name = parts[1].strip() if len(parts) > 1 else ""
        else:
            parts = author.rsplit(" ", 1)
            if len(parts) == 2:
                first_name = parts[0].strip()
                last_name = parts[1].strip()
            else:
                first_name = ""
                last_name = parts[0].strip()

        creators.append({"creatorType": "author", "firstName": first_name, "lastName": last_name})

    return creators


def convert_bibtex_to_yaml(bibtex_entry: dict[str, Any]) -> dict[str, Any]:
    """
    Convert BibTeX entry to Zotero YAML format.

    Pure function - no I/O, only data transformation.

    Args:
        bibtex_entry: Parsed BibTeX entry dictionary

    Returns:
        Zotero item dictionary in YAML-compatible format
    """
    entry_type = bibtex_entry.get("ENTRYTYPE", "misc")
    item_type = map_bibtex_type(entry_type)

    item: dict[str, Any] = {
        "itemType": item_type,
        "fields": {},
        "creators": [],
        "tags": [],
    }

    field_map = {
        "title": "title",
        "year": "date",
        "journal": "publicationTitle",
        "booktitle": "publicationTitle",
        "volume": "volume",
        "number": "issue",
        "pages": "pages",
        "publisher": "publisher",
        "address": "place",
        "doi": "DOI",
        "isbn": "ISBN",
        "issn": "ISSN",
        "url": "url",
        "abstract": "abstractNote",
        "edition": "edition",
        "series": "series",
    }

    for bibtex_field, zotero_field in field_map.items():
        if bibtex_field in bibtex_entry:
            item["fields"][zotero_field] = str(bibtex_entry[bibtex_field])

    if "author" in bibtex_entry:
        item["creators"].extend(parse_bibtex_authors(bibtex_entry["author"]))

    if "editor" in bibtex_entry:
        editors = parse_bibtex_authors(bibtex_entry["editor"])
        for editor in editors:
            editor["creatorType"] = "editor"
        item["creators"].extend(editors)

    if "keywords" in bibtex_entry:
        keywords = bibtex_entry["keywords"]
        if isinstance(keywords, str):
            import re as _re

            tags = [k.strip() for k in _re.split(r"[,;]", keywords) if k.strip()]
            item["tags"] = tags

    return item
