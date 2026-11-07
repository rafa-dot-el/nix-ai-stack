"""
Pure validation logic for zoteroctl item data and environment paths.
"""

import re
from pathlib import Path
from typing import Any


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
        database_path: Path to the Zotero SQLite database
        storage_path: Path to the Zotero storage directory

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
                "message": (
                    f"Database found: {database_path} ({database_path.stat().st_size} bytes)"
                ),
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


def validate_item_type(data: dict[str, Any], schema: dict[str, Any]) -> list[str]:
    """
    Validate itemType field exists and is valid.

    Pure function - no I/O, only data transformation.

    Args:
        data: Item data to validate
        schema: Schema dictionary from load_schema()

    Returns:
        List of error messages (empty if valid)
    """
    errors: list[str] = []

    if "itemType" not in data:
        errors.append("Missing required field: itemType")
    elif data["itemType"] not in schema["itemTypes"]:
        errors.append(f"Unknown itemType: '{data['itemType']}'")

    return errors


def validate_field_names(data: dict[str, Any], schema: dict[str, Any]) -> list[str]:
    """
    Validate all field names are valid for the given itemType.

    Pure function - no I/O, only data transformation.

    Args:
        data: Item data to validate
        schema: Schema dictionary from load_schema()

    Returns:
        List of error messages (empty if valid)
    """
    errors: list[str] = []

    reserved_fields = {
        "itemKey",
        "key",
        "itemType",
        "dateAdded",
        "dateModified",
        "fields",
        "creators",
        "tags",
        "attachments",
        "notes",
        "collections",
    }

    item_type_name = data.get("itemType")
    if not item_type_name or item_type_name not in schema["itemTypes"]:
        return errors

    item_type_id = schema["itemTypes"][item_type_name]
    valid_field_ids = schema["itemTypeFields"].get(item_type_id, [])

    for field_name in data:
        if field_name in reserved_fields:
            continue

        if field_name not in schema["fields"]:
            errors.append(f"Unknown field: '{field_name}' for itemType '{item_type_name}'")
            continue

        field_id = schema["fields"][field_name]
        if field_id not in valid_field_ids:
            errors.append(f"Field '{field_name}' not valid for itemType '{item_type_name}'")

    return errors


def validate_data_types(data: dict[str, Any]) -> list[str]:
    """
    Validate data types of fields.

    Pure function - no I/O, only data transformation.

    Args:
        data: Item data to validate

    Returns:
        List of error messages (empty if valid)
    """
    errors = []

    if "creators" in data:
        if not isinstance(data["creators"], list):
            errors.append(f"Field 'creators' must be list, got {type(data['creators']).__name__}")
        else:
            for idx, creator in enumerate(data["creators"]):
                if not isinstance(creator, dict):
                    errors.append(
                        f"Creator at index {idx} must be dict, got {type(creator).__name__}"
                    )
                    continue

                if "creatorType" not in creator:
                    errors.append(f"Creator at index {idx} missing required field 'creatorType'")

                has_personal_name = "firstName" in creator or "lastName" in creator
                has_institutional_name = "name" in creator

                if has_personal_name and has_institutional_name:
                    errors.append(
                        f"Creator at index {idx} cannot have both 'name' and 'firstName'/'lastName'"
                    )
                elif has_personal_name and "lastName" not in creator:
                    errors.append(f"Creator at index {idx} missing required field 'lastName'")
                elif not has_personal_name and not has_institutional_name:
                    errors.append(f"Creator at index {idx} must have either 'lastName' or 'name'")

    if "tags" in data and not isinstance(data["tags"], list):
        errors.append(f"Field 'tags' must be list, got {type(data['tags']).__name__}")

    if "attachments" in data:
        if not isinstance(data["attachments"], list):
            errors.append(
                f"Field 'attachments' must be list, got {type(data['attachments']).__name__}"
            )
        else:
            for idx, attachment in enumerate(data["attachments"]):
                if not isinstance(attachment, dict):
                    errors.append(
                        f"Attachment at index {idx} must be dict, got {type(attachment).__name__}"
                    )

    if "notes" in data:
        if not isinstance(data["notes"], list):
            errors.append(f"Field 'notes' must be list, got {type(data['notes']).__name__}")
        else:
            for idx, note in enumerate(data["notes"]):
                if not isinstance(note, dict):
                    errors.append(f"Note at index {idx} must be dict, got {type(note).__name__}")

    return errors


def validate_item(data: dict[str, Any], schema: dict[str, Any]) -> list[str]:
    """
    Validate complete item data against schema.

    Orchestrates all validation functions and accumulates errors.
    Pure function - no I/O, only data transformation.

    Args:
        data: Item data to validate
        schema: Schema dictionary from load_schema()

    Returns:
        List of all validation errors (empty if valid)
    """
    errors = []
    errors.extend(validate_item_type(data, schema))
    errors.extend(validate_field_names(data, schema))
    errors.extend(validate_data_types(data))
    return errors


def extract_item_key(data: dict[str, Any]) -> str:
    """
    Extract and validate itemKey from data.

    Pure function - no I/O, only data transformation.

    Args:
        data: Dictionary containing itemKey field

    Returns:
        Item key string

    Raises:
        ValueError: If itemKey missing, empty, or invalid format
    """
    item_key = data.get("itemKey")

    if item_key is None:
        raise ValueError("Missing required field: itemKey")

    if not isinstance(item_key, str):
        raise ValueError(f"itemKey must be string, got {type(item_key).__name__}")

    if not item_key.strip():
        raise ValueError("itemKey cannot be empty")

    if not re.match(r"^[A-Z0-9]{8}$", item_key):
        raise ValueError(f"itemKey must be 8 uppercase alphanumeric characters, got: {item_key}")

    return item_key
