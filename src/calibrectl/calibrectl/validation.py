"""
Pure validation functions: integrity classification, path validation, environment checks.
"""

from pathlib import Path


def classify_integrity_result(result: str) -> str:
    """
    Classify an integrity_check result as 'ok' or 'error'.

    Pure function - no I/O, only string comparison.

    Args:
        result: Raw output from PRAGMA integrity_check

    Returns:
        'ok' if database is healthy, 'error' otherwise

    Examples:
        >>> classify_integrity_result('ok')
        'ok'
        >>> classify_integrity_result('*** in database main ***\\nPage 4: btree...')
        'error'
    """
    return "ok" if result.strip().lower() == "ok" else "error"


def validate_environment_paths(
    database_path: Path,
    storage_path: Path,
) -> list[dict[str, str]]:
    """
    Validate that database and storage paths exist and are accessible.

    Args:
        database_path: Path to the Calibre metadata.db
        storage_path: Path to the Calibre storage directory

    Returns:
        List of issue dicts with 'check', 'status', and 'message' keys
    """
    checks = []

    if not database_path.exists():
        checks.append(
            {
                "check": "database_file",
                "status": "error",
                "message": f"Database file not found: {database_path}",
            }
        )
    elif not database_path.is_file():
        checks.append(
            {
                "check": "database_file",
                "status": "error",
                "message": f"Database path is not a file: {database_path}",
            }
        )
    else:
        checks.append(
            {
                "check": "database_file",
                "status": "ok",
                "message": f"Database found: {database_path} ({database_path.stat().st_size} bytes)",
            }
        )

    if not storage_path.exists():
        checks.append(
            {
                "check": "storage_directory",
                "status": "warning",
                "message": f"Storage directory not found: {storage_path}",
            }
        )
    elif not storage_path.is_dir():
        checks.append(
            {
                "check": "storage_directory",
                "status": "error",
                "message": f"Storage path is not a directory: {storage_path}",
            }
        )
    else:
        checks.append(
            {
                "check": "storage_directory",
                "status": "ok",
                "message": f"Storage directory found: {storage_path}",
            }
        )

    return checks
