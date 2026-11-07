"""
Output formatting utilities for zoteroctl.

All formatting functions are pure (no I/O), except echo_* which write to stderr.
"""

import json
from collections.abc import Callable
from typing import Any

import click
import yaml

# Global quiet flag - set by CLI --quiet option
_QUIET_MODE = False


def set_quiet_mode(quiet: bool) -> None:
    """Set the global quiet mode flag."""
    global _QUIET_MODE
    _QUIET_MODE = quiet


def echo_info(message: str) -> None:
    """
    Output info message respecting --quiet flag.

    Args:
        message: Info message

    Note:
        If --quiet is set, info messages are suppressed.
        Errors ([-]) and success ([+]) messages are never suppressed.
    """
    if not _QUIET_MODE:
        click.echo(f"[*] {message}", err=True)


def echo_success(message: str) -> None:
    """Output a success message (never suppressed)."""
    click.echo(f"[+] {message}", err=True)


def echo_error(message: str) -> None:
    """Output an error message (never suppressed)."""
    click.echo(f"[-] {message}", err=True)


def echo_warning(message: str) -> None:
    """Output a warning message (never suppressed)."""
    click.echo(f"[!] {message}", err=True)


def format_as_yaml(data: Any) -> str:
    """
    Convert dictionary to YAML string.

    Pure function - no I/O, only data transformation.

    Args:
        data: Dictionary to convert

    Returns:
        YAML-formatted string with block style, Unicode support

    Examples:
        >>> format_as_yaml({"key": "value", "number": 42})
        'key: value\\nnumber: 42\\n'
    """
    return yaml.safe_dump(
        data,
        default_flow_style=False,
        allow_unicode=True,
        sort_keys=False,
        indent=2,
    )


def format_as_json(data: Any) -> str:
    """
    Convert dictionary to JSON string.

    Pure function - no I/O, only data transformation.

    Args:
        data: Dictionary to convert

    Returns:
        JSON-formatted string with indentation, Unicode support

    Examples:
        >>> format_as_json({"key": "value", "number": 42})
        '{\\n  "key": "value",\\n  "number": 42\\n}'
    """
    return json.dumps(data, indent=2, ensure_ascii=False, sort_keys=False)


def get_output_formatter(json_output: bool = False) -> Callable[[Any], str]:
    """
    Get appropriate output formatter based on output format flag.

    Args:
        json_output: If True, return JSON formatter; if False, return YAML formatter

    Returns:
        Formatter function (format_as_json or format_as_yaml)
    """
    if json_output:
        return format_as_json
    else:
        return format_as_yaml


def dump_item_data(data: Any, format: str = "json") -> str:
    """
    Serialize item data to JSON or YAML string.

    Args:
        data: Item data dictionary
        format: Output format ('json' or 'yaml')

    Returns:
        Serialized string
    """
    if format == "json":
        return format_as_json(data)
    elif format == "yaml":
        return format_as_yaml(data)
    else:
        raise ValueError(f"Unsupported format: {format}")
