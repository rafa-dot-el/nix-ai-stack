"""
Zotero translation server HTTP client for zoteroctl.
"""

from typing import Any

import requests


def import_from_url(url: str, translation_server: str) -> dict[str, Any]:
    """
    Import item metadata from URL using Zotero translation server.

    Args:
        url: URL to import from
        translation_server: Translation server base URL

    Returns:
        Item metadata dictionary

    Raises:
        requests.RequestException: If translation server request fails
        ValueError: If no items are returned
    """
    endpoint = f"{translation_server}/web"

    response = requests.post(
        endpoint,
        json={"url": url},
        headers={"Content-Type": "application/json"},
        timeout=30,
    )

    response.raise_for_status()
    items = response.json()

    if not items or len(items) == 0:
        raise ValueError(f"No items found at URL: {url}")

    return items[0]
