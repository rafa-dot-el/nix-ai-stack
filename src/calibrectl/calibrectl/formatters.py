"""
Output formatters: echo helpers, JSON/YAML renderers, date formatting.
"""

import json
from typing import Any

import click
import yaml

# Global quiet flag - set by CLI --quiet option
_QUIET_MODE = False


def set_quiet(value: bool) -> None:
    """Set the global quiet mode flag."""
    global _QUIET_MODE
    _QUIET_MODE = value


def echo_info(message: str) -> None:
    """
    Output info message respecting --quiet flag.

    Args:
        message: Info message

    Note:
        If --quiet is set, info messages are suppressed.
    """
    global _QUIET_MODE
    if not _QUIET_MODE:
        click.echo(f"[*] {message}", err=True)


def echo_success(message: str) -> None:
    """Output success message (never suppressed)."""
    click.echo(f"[+] {message}", err=True)


def echo_error(message: str) -> None:
    """Output error message (never suppressed)."""
    click.echo(f"[-] {message}", err=True)


def echo_warning(message: str) -> None:
    """Output warning message respecting --quiet flag."""
    global _QUIET_MODE
    if not _QUIET_MODE:
        click.echo(f"[!] {message}", err=True)


def format_error(message: str) -> str:
    """Format error message with [-] prefix."""
    return f"[-] {message}"


def format_success(message: str) -> str:
    """Format success message with [+] prefix."""
    return f"[+] {message}"


def format_info(message: str) -> str:
    """Format info message with [*] prefix."""
    return f"[*] {message}"


def format_yaml(data: dict[str, Any]) -> str:
    """
    Format data as YAML string.

    Args:
        data: Dictionary to format

    Returns:
        str: YAML formatted string
    """
    return yaml.dump(data, default_flow_style=False, allow_unicode=True, sort_keys=False)


def format_json(data: Any) -> str:
    """
    Format data as JSON string.

    Args:
        data: Data to format

    Returns:
        str: JSON formatted string with indentation
    """
    return json.dumps(data, indent=2, ensure_ascii=False)


def format_date_simple(date_str: str | None) -> str | None:
    """
    Format ISO datetime string to simple YYYY-MM-DD format.

    Args:
        date_str: ISO format datetime string (e.g., "2024-08-12 16:26:50.191078+00:00")

    Returns:
        Date in YYYY-MM-DD format, or None if input is None/empty

    Example:
        >>> format_date_simple("2024-08-12 16:26:50.191078+00:00")
        "2024-08-12"
        >>> format_date_simple(None)
        None
    """
    if not date_str:
        return None
    return date_str[:10] if len(date_str) >= 10 else date_str


def convert_date_to_iso(date_str: str | None) -> str | None:
    """
    Convert simple YYYY-MM-DD date to Calibre's ISO timestamp format.

    Args:
        date_str: Date in YYYY-MM-DD format

    Returns:
        ISO timestamp string (YYYY-MM-DD HH:MM:SS+00:00), or None if input is None/empty

    Example:
        >>> convert_date_to_iso("2024-01-15")
        "2024-01-15 00:00:00+00:00"
        >>> convert_date_to_iso(None)
        None
    """
    if not date_str:
        return None

    from datetime import datetime

    try:
        date_obj = datetime.strptime(date_str, "%Y-%m-%d")
        return date_obj.strftime("%Y-%m-%d %H:%M:%S") + "+00:00"
    except ValueError:
        return None
