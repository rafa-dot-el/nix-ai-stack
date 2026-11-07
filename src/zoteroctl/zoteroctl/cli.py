"""
Click CLI root group and all subcommands for zoteroctl.
"""

import json
import sqlite3
import sys
from pathlib import Path
from typing import Any

import click
import requests
import yaml

from zoteroctl import __version__, ops
from zoteroctl.attachments import (
    add_file_attachment,
    delete_attachment,
    list_file_attachments,
)
from zoteroctl.bibtex import convert_bibtex_to_yaml, item_to_bibtex
from zoteroctl.collections import (
    add_item_to_collection,
    create_collection_record,
    get_collections_tree,
    remove_item_from_collection,
    resolve_collection_path,
    resolve_or_create_collection_path,
)
from zoteroctl.config import resolve_runtime_config
from zoteroctl.db import (
    REQUIRED_ZOTERO_TABLES,
    check_required_tables,
    connect_database,
    load_schema,
)
from zoteroctl.doctor import run_database_diagnostics
from zoteroctl.items import (
    load_item_data,
    read_yaml_input,
    merge_partial_updates,
    add_or_update_item,
    get_item_by_key,
    delete_item,
    list_items_query,
    query_child_items,
    filter_bibliographic_items,
    nest_child_items,
)
from zoteroctl.formatters import (
    dump_item_data,
    echo_info,
    get_output_formatter,
    set_quiet_mode,
)
from zoteroctl.translation import import_from_url
from zoteroctl.validation import (
    classify_integrity_result,
    extract_item_key,
    validate_environment_paths,
    validate_item,
)

# ---------------------------------------------------------------------------
# CLI group
# ---------------------------------------------------------------------------


@click.group()
@click.version_option(version=__version__, prog_name="zoteroctl")
@click.option(
    "--json-output",
    is_flag=True,
    default=False,
    help="Output in JSON format instead of YAML (default: YAML)",
)
@click.option(
    "--quiet",
    "-q",
    is_flag=True,
    default=False,
    help="Suppress informational messages (keep errors and success)",
)
@click.option(
    "--config",
    "config_path",
    type=click.Path(path_type=Path, dir_okay=False),
    help="Read defaults from a config file",
)
@click.option(
    "--database",
    type=click.Path(path_type=Path, dir_okay=False),
    help="Path to zotero.sqlite",
)
@click.option(
    "--storage",
    "storage_path",
    type=click.Path(path_type=Path, file_okay=False),
    help="Path to the Zotero storage directory",
)
@click.option(
    "--translation-server",
    type=str,
    help="Zotero translation server base URL",
)
@click.pass_context
def cli(
    ctx,
    json_output: bool,
    quiet: bool,
    config_path: Path | None,
    database: Path | None,
    storage_path: Path | None,
    translation_server: str | None,
):
    """
    Zotero CLI - Direct database manipulation for Zotero libraries.

    Manage Zotero items, files, and collections through direct database access.
    Supports JSON/YAML data exchange and URL imports via translation server.

    Configuration precedence:

      CLI options > environment variables > config file > built-in defaults

    Supported environment variables:

      ZOTERO_DATABASE     Path to zotero.sqlite
      ZOTERO_STORAGE      Path to storage directory
      ZOTERO_TRANSLATION_SERVER  Translation server URL

    Config file:

      ~/.config/rafa/zoteroctl/config.toml

    Examples:

      # Add item from JSON
      zoteroctl add book.json

      # Get item as YAML (default)
      zoteroctl get ABCD1234

      # Get item as JSON
      zoteroctl get ABCD1234 --json-output

      # Query items
      zoteroctl query "itemType='book'"

      # Export to BibTeX
      zoteroctl export ABCD1234 --output refs.bib
    """
    set_quiet_mode(quiet)
    ctx.ensure_object(dict)
    try:
        runtime_config = resolve_runtime_config(
            database=database,
            storage=storage_path,
            translation_server=translation_server,
            config_path=config_path,
        )
    except (FileNotFoundError, ValueError) as exc:
        click.echo(f"[-] Configuration error: {exc}", err=True)
        raise SystemExit(1) from exc

    ctx.obj["database"] = runtime_config.database
    ctx.obj["storage"] = runtime_config.storage
    ctx.obj["translation_server"] = runtime_config.translation_server
    ctx.obj["config_file"] = runtime_config.config_file
    ctx.obj["json_output"] = json_output
    ctx.obj["quiet"] = quiet


# ---------------------------------------------------------------------------
# add
# ---------------------------------------------------------------------------


@cli.command()
@click.argument("source", type=str)
@click.option("--from-url", is_flag=True, help="Import from URL using translation server")
@click.pass_context
def add(ctx, source: str, from_url: bool):
    """
    Add or update item from JSON/YAML file or URL.

    If item has existing 'key' field, updates that item.
    Otherwise creates new item with generated key.

    SOURCE: Path to JSON/YAML file, or URL if --from-url specified
    """
    database = ctx.obj["database"]
    translation_server = ctx.obj["translation_server"]

    if not database.exists():
        click.echo(f"[-] Error: Database not found: {database}", err=True)
        sys.exit(1)

    try:
        if from_url:
            echo_info("Importing from URL via translation server...")
            item_data = import_from_url(source, translation_server)
            click.echo(f"[+] Imported: {item_data.get('title', 'Unknown')}", err=True)
        else:
            source_path = Path(source)
            if not source_path.exists():
                click.echo(f"[-] Error: File not found: {source}", err=True)
                sys.exit(1)
            echo_info("Loading item data...")
            item_data = load_item_data(source_path)

        action = "Updating" if "key" in item_data else "Adding"
        echo_info(f"{action} item...")

        with connect_database(database, read_only=False) as conn:
            item_key = add_or_update_item(conn, item_data)
            click.echo(f"[+] Item {action.lower()}: {item_key}", err=True)

    except requests.RequestException as e:
        click.echo(f"[-] Translation server error: {e}", err=True)
        sys.exit(1)
    except ValueError as e:
        click.echo(f"[-] Validation error: {e}", err=True)
        sys.exit(1)
    except sqlite3.Error as e:
        click.echo(f"[-] Database error: {e}", err=True)
        sys.exit(1)
    except Exception as e:
        click.echo(f"[-] Error: {e}", err=True)
        sys.exit(1)


# ---------------------------------------------------------------------------
# update
# ---------------------------------------------------------------------------


@cli.command()
@click.argument("source", type=click.Path(path_type=Path), required=False)
@click.pass_context
def update(ctx, source: Path | None):
    """
    Update existing item from YAML file or stdin.

    Performs partial field updates - only fields present in YAML are updated.
    Requires itemKey field to identify the item to update.

    SOURCE: Path to YAML file, or omit to read from stdin
    """
    database = ctx.obj["database"]
    json_output = ctx.obj.get("json_output", False)

    if not database.exists():
        click.echo(f"[-] Error: Database not found: {database}", err=True)
        sys.exit(1)

    try:
        echo_info("Reading update data...")
        updates = read_yaml_input(source)

        echo_info("Validating item key...")
        item_key = extract_item_key(updates)

        with connect_database(database, read_only=False) as conn:
            echo_info(f"Fetching existing item: {item_key}")
            existing = get_item_by_key(conn, item_key)

            if existing is None:
                click.echo(f"[-] Error: Item not found: {item_key}", err=True)
                sys.exit(1)

            echo_info("Merging updates...")
            merged = merge_partial_updates(existing, updates)

            echo_info("Validating merged data...")
            schema = load_schema(database)
            errors = validate_item(merged, schema)

            if errors:
                click.echo("[-] Validation errors:", err=True)
                for error in errors:
                    click.echo(f"  - {error}", err=True)
                sys.exit(1)

            echo_info("Updating database...")
            add_or_update_item(conn, merged)
            click.echo(f"[+] Item updated: {item_key}", err=True)

            updated = get_item_by_key(conn, item_key)
            if updated is None:
                raise ValueError(f"Updated item not found after write: {item_key}")
            formatter = get_output_formatter(json_output)
            click.echo(formatter(updated))

    except FileNotFoundError as e:
        click.echo(f"[-] File error: {e}", err=True)
        sys.exit(1)
    except ValueError as e:
        click.echo(f"[-] Validation error: {e}", err=True)
        sys.exit(1)
    except yaml.YAMLError as e:
        click.echo(f"[-] YAML syntax error: {e}", err=True)
        sys.exit(1)
    except sqlite3.Error as e:
        click.echo(f"[-] Database error: {e}", err=True)
        sys.exit(1)
    except Exception as e:
        click.echo(f"[-] Error: {e}", err=True)
        sys.exit(1)


# ---------------------------------------------------------------------------
# create
# ---------------------------------------------------------------------------


@cli.command()
@click.argument("source", type=click.Path(path_type=Path), required=False)
@click.pass_context
def create(ctx, source: Path | None):
    """
    Create new item from YAML file or stdin.

    Creates a new item with auto-generated itemKey. Validates all fields
    before writing to database. Never updates existing items.

    SOURCE: Path to YAML file, or omit to read from stdin
    """
    database = ctx.obj["database"]
    json_output = ctx.obj.get("json_output", False)

    if not database.exists():
        click.echo(f"[-] Error: Database not found: {database}", err=True)
        sys.exit(1)

    try:
        echo_info("Reading item data...")
        item_data = read_yaml_input(source)

        if "key" in item_data:
            click.echo(
                "[-] Error: 'key' field found - use 'update' command for existing items",
                err=True,
            )
            sys.exit(1)
        if "itemKey" in item_data:
            click.echo(
                "[-] Error: 'itemKey' field found - use 'update' command for existing items",
                err=True,
            )
            sys.exit(1)

        collection_path = item_data.pop("collection_path", None)

        echo_info("Validating item data...")
        schema = load_schema(database)
        errors = validate_item(item_data, schema)

        if errors:
            click.echo("[-] Validation errors:", err=True)
            for error in errors:
                click.echo(f"  - {error}", err=True)
            sys.exit(1)

        echo_info("Creating item in database...")
        with connect_database(database, read_only=False) as conn:
            item_key = add_or_update_item(conn, item_data)
            click.echo(f"[+] Item created: {item_key}", err=True)

            if collection_path:
                echo_info(f"Resolving collection path: {collection_path}")
                collection_key = resolve_or_create_collection_path(conn, collection_path)
                add_item_to_collection(conn, item_key, collection_key)
                click.echo(f"[+] Item added to collection: {collection_path}", err=True)

            created = get_item_by_key(conn, item_key)
            if created is None:
                raise ValueError(f"Created item not found after write: {item_key}")
            formatter = get_output_formatter(json_output)
            click.echo(formatter(created))

    except FileNotFoundError as e:
        click.echo(f"[-] File error: {e}", err=True)
        sys.exit(1)
    except ValueError as e:
        click.echo(f"[-] Validation error: {e}", err=True)
        sys.exit(1)
    except yaml.YAMLError as e:
        click.echo(f"[-] YAML syntax error: {e}", err=True)
        sys.exit(1)
    except sqlite3.Error as e:
        click.echo(f"[-] Database error: {e}", err=True)
        sys.exit(1)
    except Exception as e:
        click.echo(f"[-] Error: {e}", err=True)
        sys.exit(1)


# ---------------------------------------------------------------------------
# bibtex-to-yaml
# ---------------------------------------------------------------------------


@cli.command(name="bibtex-to-yaml")
@click.argument("bibtex_file", type=click.Path(exists=True, path_type=Path))
@click.option("--output", type=click.Path(path_type=Path), help="Write to file instead of stdout")
@click.pass_context
def bibtex_to_yaml(ctx, bibtex_file: Path, output: Path | None):
    """
    Convert BibTeX file to YAML format.

    Converts BibTeX entries to Zotero YAML format compatible with the
    create command. Does NOT import to database - conversion only.

    BIBTEX_FILE: Path to BibTeX file to convert
    """
    json_output = ctx.obj.get("json_output", False)

    try:
        import bibtexparser
        from bibtexparser.bparser import BibTexParser

        echo_info(f"Parsing BibTeX file: {bibtex_file}")

        with open(bibtex_file) as f:
            parser = BibTexParser(common_strings=True)
            bib_database = bibtexparser.load(f, parser=parser)

        if not bib_database.entries:
            click.echo("[-] No entries found in BibTeX file", err=True)
            sys.exit(1)

        echo_info(f"Found {len(bib_database.entries)} entries")

        converted_items = []
        for entry in bib_database.entries:
            try:
                item = convert_bibtex_to_yaml(entry)
                converted_items.append(item)
            except Exception as e:
                cite_key = entry.get("ID", "unknown")
                echo_info(f"Warning: Failed to convert entry {cite_key}: {e}")

        if not converted_items:
            click.echo("[-] No entries successfully converted", err=True)
            sys.exit(1)

        click.echo(f"[+] Successfully converted {len(converted_items)} entries", err=True)

        formatter = get_output_formatter(json_output)

        if len(converted_items) == 1:
            output_str = formatter(converted_items[0])
        else:
            if json_output:
                output_str = formatter(converted_items)
            else:
                output_str = "\n---\n".join(formatter(item) for item in converted_items)

        if output:
            output.write_text(output_str)
            click.echo(f"[+] Converted entries written to: {output}", err=True)
        else:
            click.echo(output_str)

    except ImportError:
        click.echo("[-] Error: bibtexparser library not available", err=True)
        sys.exit(1)
    except FileNotFoundError as e:
        click.echo(f"[-] File error: {e}", err=True)
        sys.exit(1)
    except Exception as e:
        click.echo(f"[-] Error: {e}", err=True)
        sys.exit(1)


# ---------------------------------------------------------------------------
# get
# ---------------------------------------------------------------------------


@cli.command()
@click.argument("key", type=str)
@click.option(
    "--output",
    type=click.Path(path_type=Path),
    help="Write to file instead of stdout",
)
@click.pass_context
def get(ctx, key: str, output: Path | None):
    """
    Get item details in YAML format (default) or JSON with --json-output flag.

    KEY: 8-character item key (e.g., ABCD1234)
    """
    database = ctx.obj["database"]
    json_output = ctx.obj.get("json_output", False)

    if not database.exists():
        click.echo(f"[-] Error: Database not found: {database}", err=True)
        sys.exit(1)

    try:
        with connect_database(database, read_only=True) as conn:
            item_data = get_item_by_key(conn, key)

            if item_data is None:
                click.echo(f"[-] Error: Item not found: {key}", err=True)
                sys.exit(1)

            format_name = "json" if json_output else "yaml"
            output_str = dump_item_data(item_data, format_name)

            if output:
                output.write_text(output_str)
                click.echo(f"[+] Item exported to: {output}", err=True)
            else:
                click.echo(output_str)

    except sqlite3.Error as e:
        click.echo(f"[-] Database error: {e}", err=True)
        sys.exit(1)
    except Exception as e:
        click.echo(f"[-] Error: {e}", err=True)
        sys.exit(1)


# ---------------------------------------------------------------------------
# delete
# ---------------------------------------------------------------------------


@cli.command()
@click.argument("key", type=str)
@click.option(
    "--keep-files",
    is_flag=True,
    help="Keep attachment files (delete database records only)",
)
@click.pass_context
def delete(ctx, key: str, keep_files: bool):
    """
    Delete item from database.

    By default, deletes both database records and attachment files.
    Use --keep-files to preserve attachment files on disk.

    KEY: 8-character item key
    """
    database = ctx.obj["database"]
    storage = ctx.obj["storage"]

    if not database.exists():
        click.echo(f"[-] Error: Database not found: {database}", err=True)
        sys.exit(1)

    try:
        with connect_database(database, read_only=False) as conn:
            if delete_item(conn, key, storage, keep_files):
                action = "deleted (files kept)" if keep_files else "deleted"
                click.echo(f"[+] Item {action}: {key}", err=True)
            else:
                click.echo(f"[-] Error: Item not found: {key}", err=True)
                sys.exit(1)

    except sqlite3.Error as e:
        click.echo(f"[-] Database error: {e}", err=True)
        sys.exit(1)
    except Exception as e:
        click.echo(f"[-] Error: {e}", err=True)
        sys.exit(1)


# ---------------------------------------------------------------------------
# add-file
# ---------------------------------------------------------------------------


@cli.command(name="add-file")
@click.argument("item_key", type=str)
@click.argument("file_path", type=click.Path(exists=True, path_type=Path))
@click.option("--title", type=str, help="Attachment title (default: filename)")
@click.pass_context
def add_file(ctx, item_key: str, file_path: Path, title: str | None):
    """
    Add file attachment to item.

    ITEM_KEY: Parent item key
    FILE_PATH: Path to file to attach
    """
    database = ctx.obj["database"]
    storage = ctx.obj["storage"]

    if not database.exists():
        click.echo(f"[-] Error: Database not found: {database}", err=True)
        sys.exit(1)

    try:
        echo_info(f"Adding file to item {item_key}: {file_path}")

        with connect_database(database, read_only=False) as conn:
            attachment_key = add_file_attachment(conn, item_key, file_path, storage, title)
            click.echo(f"[+] File attached: {attachment_key}", err=True)

    except ValueError as e:
        click.echo(f"[-] Validation error: {e}", err=True)
        sys.exit(1)
    except OSError as e:
        click.echo(f"[-] File system error: {e}", err=True)
        sys.exit(1)
    except sqlite3.Error as e:
        click.echo(f"[-] Database error: {e}", err=True)
        sys.exit(1)
    except Exception as e:
        click.echo(f"[-] Error: {e}", err=True)
        sys.exit(1)


# ---------------------------------------------------------------------------
# list-files
# ---------------------------------------------------------------------------


@cli.command(name="list-files")
@click.argument("item_key", type=str)
@click.option("--json", "output_json", is_flag=True, help="Output as JSON")
@click.pass_context
def list_files(ctx, item_key: str, output_json: bool):
    """
    List file attachments for item.

    ITEM_KEY: Item key
    """
    database = ctx.obj["database"]
    storage = ctx.obj["storage"]

    if not database.exists():
        click.echo(f"[-] Error: Database not found: {database}", err=True)
        sys.exit(1)

    try:
        with connect_database(database, read_only=True) as conn:
            try:
                attachments = list_file_attachments(conn, item_key, storage)
            except ValueError as e:
                click.echo(f"[-] Error: {e}", err=True)
                sys.exit(1)

            if not attachments:
                echo_info(f"No attachments found for item: {item_key}")
                sys.exit(0)

            if output_json:
                click.echo(json.dumps(attachments, indent=2, ensure_ascii=False))
            else:
                echo_info(f"Found {len(attachments)} attachment(s) for {item_key}:\n")
                for att in attachments:
                    click.echo(f"[{att['key']}] {att['title']}")
                    click.echo(f"  Path: {att['path']}")
                    click.echo(f"  Type: {att['contentType']}")
                    click.echo(f"  Exists: {'Yes' if att['exists'] else 'No'}")
                    click.echo()

    except sqlite3.Error as e:
        click.echo(f"[-] Database error: {e}", err=True)
        sys.exit(1)
    except Exception as e:
        click.echo(f"[-] Error: {e}", err=True)
        sys.exit(1)


# ---------------------------------------------------------------------------
# delete-file
# ---------------------------------------------------------------------------


@cli.command(name="delete-file")
@click.argument("attachment_key", type=str)
@click.option(
    "--keep-file",
    is_flag=True,
    help="Keep file on disk (delete database record only)",
)
@click.pass_context
def delete_file(ctx, attachment_key: str, keep_file: bool):
    """
    Delete file attachment.

    ATTACHMENT_KEY: Attachment item key
    """
    database = ctx.obj["database"]
    storage = ctx.obj["storage"]

    if not database.exists():
        click.echo(f"[-] Error: Database not found: {database}", err=True)
        sys.exit(1)

    try:
        echo_info(f"Deleting attachment: {attachment_key}")

        with connect_database(database, read_only=False) as conn:
            if delete_attachment(conn, attachment_key, storage, keep_file):
                action = "deleted (file kept)" if keep_file else "deleted"
                click.echo(f"[+] Attachment {action}: {attachment_key}", err=True)
            else:
                click.echo(f"[-] Error: Attachment not found: {attachment_key}", err=True)
                sys.exit(1)

    except ValueError as e:
        click.echo(f"[-] Validation error: {e}", err=True)
        sys.exit(1)
    except sqlite3.Error as e:
        click.echo(f"[-] Database error: {e}", err=True)
        sys.exit(1)
    except Exception as e:
        click.echo(f"[-] Error: {e}", err=True)
        sys.exit(1)


# ---------------------------------------------------------------------------
# query
# ---------------------------------------------------------------------------


@cli.command()
@click.argument("where_clause", type=str)
@click.option("--limit", type=int, default=None, help="Maximum results (default: no limit)")
@click.pass_context
def query(ctx, where_clause: str, limit: int | None):
    """
    Query items with SQL WHERE clause.

    WHERE_CLAUSE: SQL WHERE condition
    """
    database = ctx.obj["database"]
    json_output = ctx.obj.get("json_output", False)

    if not database.exists():
        click.echo(f"[-] Error: Database not found: {database}", err=True)
        sys.exit(1)

    try:
        echo_info(f"Querying items: {where_clause}")

        query_sql = f"""
            SELECT DISTINCT
                i.itemID,
                i.key,
                it.typeName as itemType,
                i.dateAdded,
                i.dateModified
            FROM items i
            JOIN itemTypes it ON i.itemTypeID = it.itemTypeID
            LEFT JOIN itemData id ON i.itemID = id.itemID
            LEFT JOIN fieldsCombined f ON id.fieldID = f.fieldID
            LEFT JOIN itemDataValues idv ON id.valueID = idv.valueID
            LEFT JOIN itemCreators ic ON i.itemID = ic.itemID
            LEFT JOIN creators c ON ic.creatorID = c.creatorID
            LEFT JOIN itemTags itag ON i.itemID = itag.itemID
            LEFT JOIN tags t ON itag.tagID = t.tagID
            WHERE {where_clause}
            ORDER BY i.dateAdded DESC
        """
        if limit is not None:
            query_sql += " LIMIT ?"

        with connect_database(database, read_only=True) as conn:
            cursor = conn.execute(query_sql, (limit,) if limit is not None else ())
            items = []
            for row in cursor:
                item_id = row["itemID"]
                title_cursor = conn.execute(
                    """
                    SELECT idv.value FROM itemData id
                    JOIN fieldsCombined f ON id.fieldID = f.fieldID
                    JOIN itemDataValues idv ON id.valueID = idv.valueID
                    WHERE id.itemID = ? AND f.fieldName = 'title'
                    """,
                    (item_id,),
                )
                title_row = title_cursor.fetchone()
                date_cursor = conn.execute(
                    """
                    SELECT idv.value FROM itemData id
                    JOIN fieldsCombined f ON id.fieldID = f.fieldID
                    JOIN itemDataValues idv ON id.valueID = idv.valueID
                    WHERE id.itemID = ? AND f.fieldName = 'date'
                    """,
                    (item_id,),
                )
                date_row = date_cursor.fetchone()
                creators_cursor = conn.execute(
                    """
                    SELECT c.firstName, c.lastName, c.fieldMode
                    FROM itemCreators ic
                    JOIN creators c ON ic.creatorID = c.creatorID
                    WHERE ic.itemID = ?
                    ORDER BY ic.orderIndex
                    """,
                    (item_id,),
                )
                creator_names = []
                for cr in creators_cursor:
                    if cr["fieldMode"] == 0:
                        name = f"{cr['firstName']} {cr['lastName']}"
                    else:
                        name = cr["lastName"]
                    creator_names.append(name.strip())
                items.append(
                    {
                        "key": row["key"],
                        "itemType": row["itemType"],
                        "title": title_row["value"] if title_row else "",
                        "creators": "; ".join(creator_names),
                        "date": date_row["value"] if date_row else "",
                        "dateAdded": row["dateAdded"],
                    }
                )

            if not items:
                echo_info("No items found")
                sys.exit(0)

            formatter = get_output_formatter(json_output)
            click.echo(formatter({"items": items}))

    except sqlite3.OperationalError as e:
        click.echo(f"[-] SQL error: {e}", err=True)
        echo_info("Check your WHERE clause syntax")
        sys.exit(1)
    except sqlite3.Error as e:
        click.echo(f"[-] Database error: {e}", err=True)
        sys.exit(1)
    except Exception as e:
        click.echo(f"[-] Error: {e}", err=True)
        sys.exit(1)


# ---------------------------------------------------------------------------
# list-items
# ---------------------------------------------------------------------------


@cli.command(name="list-items")
@click.option("--collection", type=str, help="Filter by collection key")
@click.option("--tag", type=str, help="Filter by tag name")
@click.option("--parent", type=str, help="Filter by parent item key")
@click.option("--title", type=str, help="Filter by title (partial match, case-insensitive)")
@click.option("--no-attachments", is_flag=True, help="Show only items without file attachments")
@click.option("--limit", type=int, default=None, help="Maximum results (default: no limit)")
@click.pass_context
def list_items(
    ctx,
    collection: str | None,
    tag: str | None,
    parent: str | None,
    title: str | None,
    no_attachments: bool,
    limit: int | None,
):
    """
    List items with optional filters.

    Returns items in YAML format (default) or JSON with --json-output flag.
    """
    database = ctx.obj["database"]
    json_output = ctx.obj.get("json_output", False)

    if not database.exists():
        click.echo(f"[-] Error: Database not found: {database}", err=True)
        sys.exit(1)

    try:
        with connect_database(database, read_only=True) as conn:
            items = list_items_query(conn, collection, tag, parent, title, limit)

            if no_attachments:
                items = [
                    item for item in items
                    if not conn.execute(
                        """SELECT 1 FROM itemAttachments ia
                           JOIN items ai ON ia.itemID = ai.itemID
                           JOIN items pi ON ia.parentItemID = pi.itemID
                           WHERE pi.key = ?""",
                        (item.get("key"),),
                    ).fetchone()
                ]

            if not parent:
                child_items = query_child_items(conn)
                items = [
                    nest_child_items(item, child_items["attachments"], child_items["notes"])
                    for item in filter_bibliographic_items(items)
                ]

            if not items:
                echo_info("No items found")
                sys.exit(0)

            formatter = get_output_formatter(json_output)
            click.echo(formatter({"items": items}))

    except sqlite3.Error as e:
        click.echo(f"[-] Database error: {e}", err=True)
        sys.exit(1)
    except Exception as e:
        click.echo(f"[-] Error: {e}", err=True)
        sys.exit(1)


# ---------------------------------------------------------------------------
# export
# ---------------------------------------------------------------------------


@cli.command()
@click.argument("key", type=str, required=False)
@click.option("--collection", type=str, help="Export entire collection")
@click.option("--all", "export_all", is_flag=True, help="Export entire library")
@click.option("--output", type=click.Path(path_type=Path), help="Output file (default: stdout)")
@click.pass_context
def export(ctx, key: str | None, collection: str | None, export_all: bool, output: Path | None):
    """
    Export items to BibTeX format.

    Can export single item, collection, or entire library.

    KEY: Item key to export (optional if using --collection or --all)
    """
    if not key and not collection and not export_all:
        click.echo("[-] Error: Specify KEY, --collection, or --all", err=True)
        sys.exit(1)

    database = ctx.obj["database"]

    if not database.exists():
        click.echo(f"[-] Error: Database not found: {database}", err=True)
        sys.exit(1)

    try:
        with connect_database(database, read_only=True) as conn:
            try:
                bibtex_entries = ops.export_entries(conn, key=key, collection=collection, export_all=export_all)
            except ops.NotFound as exc:
                click.echo(f"[-] Error: {exc}", err=True)
                sys.exit(1)

            if not bibtex_entries:
                echo_info("No items to export")
                sys.exit(0)

            output_str = "\n\n".join(bibtex_entries)

            if output:
                output.write_text(output_str)
                click.echo(f"[+] Exported {len(bibtex_entries)} items to: {output}", err=True)
            else:
                click.echo(output_str)

    except sqlite3.Error as e:
        click.echo(f"[-] Database error: {e}", err=True)
        sys.exit(1)
    except Exception as e:
        click.echo(f"[-] Error: {e}", err=True)
        sys.exit(1)


# ---------------------------------------------------------------------------
# add-to-collection
# ---------------------------------------------------------------------------


@cli.command(name="add-to-collection")
@click.argument("item_key", type=str)
@click.argument("collection_key", type=str)
@click.pass_context
def add_to_collection(ctx, item_key: str, collection_key: str):
    """
    Add item to collection.

    ITEM_KEY: Item key
    COLLECTION_KEY: Collection key
    """
    database = ctx.obj["database"]

    if not database.exists():
        click.echo(f"[-] Error: Database not found: {database}", err=True)
        sys.exit(1)

    try:
        echo_info(f"Adding item {item_key} to collection {collection_key}")

        with connect_database(database, read_only=False) as conn:
            if add_item_to_collection(conn, item_key, collection_key):
                click.echo("[+] Item added to collection", err=True)
            else:
                echo_info("Item already in collection")

    except ValueError as e:
        click.echo(f"[-] Validation error: {e}", err=True)
        sys.exit(1)
    except sqlite3.Error as e:
        click.echo(f"[-] Database error: {e}", err=True)
        sys.exit(1)
    except Exception as e:
        click.echo(f"[-] Error: {e}", err=True)
        sys.exit(1)


# ---------------------------------------------------------------------------
# remove-from-collection
# ---------------------------------------------------------------------------


@cli.command(name="remove-from-collection")
@click.argument("item_key", type=str)
@click.argument("collection_key", type=str)
@click.pass_context
def remove_from_collection(ctx, item_key: str, collection_key: str):
    """
    Remove item from collection.

    ITEM_KEY: Item key
    COLLECTION_KEY: Collection key
    """
    database = ctx.obj["database"]

    if not database.exists():
        click.echo(f"[-] Error: Database not found: {database}", err=True)
        sys.exit(1)

    try:
        echo_info(f"Removing item {item_key} from collection {collection_key}")

        with connect_database(database, read_only=False) as conn:
            if remove_item_from_collection(conn, item_key, collection_key):
                click.echo("[+] Item removed from collection", err=True)
            else:
                echo_info("Item was not in collection")

    except ValueError as e:
        click.echo(f"[-] Validation error: {e}", err=True)
        sys.exit(1)
    except sqlite3.Error as e:
        click.echo(f"[-] Database error: {e}", err=True)
        sys.exit(1)
    except Exception as e:
        click.echo(f"[-] Error: {e}", err=True)
        sys.exit(1)


# ---------------------------------------------------------------------------
# create-collection
# ---------------------------------------------------------------------------


@cli.command(name="create-collection")
@click.argument("name", type=str)
@click.option("--parent", type=str, help="Parent collection key or path")
@click.pass_context
def create_collection(ctx, name: str, parent: str | None):
    """
    Create a new collection.

    NAME: Collection name
    """
    database = ctx.obj["database"]

    if not database.exists():
        click.echo(f"[-] Error: Database not found: {database}", err=True)
        sys.exit(1)

    try:
        with connect_database(database, read_only=False) as conn:
            parent_key = None
            if parent:
                if len(parent) == 8 and parent.isalnum() and parent.isupper():
                    cursor = conn.execute("SELECT key FROM collections WHERE key = ?", (parent,))
                    if cursor.fetchone() is None:
                        click.echo(f"[-] Error: Parent collection not found: {parent}", err=True)
                        sys.exit(1)
                    parent_key = parent
                else:
                    parent_key = resolve_collection_path(conn, parent)
                    if parent_key is None:
                        click.echo(
                            f"[-] Error: Parent collection path not found: {parent}", err=True
                        )
                        sys.exit(1)

            echo_info(f"Creating collection: {name}")
            if "/" in name and not parent:
                collection_key = resolve_or_create_collection_path(conn, name)
            else:
                collection_key = create_collection_record(conn, name, parent_key)
            click.echo(f"[+] Collection created: {collection_key}", err=True)

    except ValueError as e:
        click.echo(f"[-] Validation error: {e}", err=True)
        sys.exit(1)
    except sqlite3.Error as e:
        click.echo(f"[-] Database error: {e}", err=True)
        sys.exit(1)
    except Exception as e:
        click.echo(f"[-] Error: {e}", err=True)
        sys.exit(1)


# ---------------------------------------------------------------------------
# list-collections
# ---------------------------------------------------------------------------


@cli.command(name="list-collections")
@click.option("--parent", type=str, help="Filter by parent collection key")
@click.option("--json", "output_json", is_flag=True, help="Output as JSON")
@click.pass_context
def list_collections(ctx, parent: str | None, output_json: bool):
    """
    List all collections in library.

    Shows collection keys, names, and hierarchy.
    """
    database = ctx.obj["database"]

    if not database.exists():
        click.echo(f"[-] Error: Database not found: {database}", err=True)
        sys.exit(1)

    try:
        with connect_database(database, read_only=True) as conn:
            collections = get_collections_tree(conn, parent)

            if not collections:
                echo_info("No collections found")
                sys.exit(0)

            if output_json or ctx.obj.get("json_output", False):
                click.echo(json.dumps(collections, indent=2, ensure_ascii=False))
            elif parent:
                # Flat list: all results are children of the requested parent
                echo_info(f"Found {len(collections)} collections:\n")
                for coll in collections:
                    click.echo(f"[{coll['key']}] {coll['name']}")
                    click.echo(f"  Items: {coll['itemCount']}")
                click.echo()
            else:
                echo_info(f"Found {len(collections)} collections:\n")
                top_level = [c for c in collections if c["parentKey"] is None]
                nested = [c for c in collections if c["parentKey"] is not None]

                for coll in top_level:
                    click.echo(f"[{coll['key']}] {coll['name']}")
                    click.echo(f"  Items: {coll['itemCount']}")

                    children = [c for c in nested if c["parentKey"] == coll["key"]]
                    for child in children:
                        click.echo(f"  └─ [{child['key']}] {child['name']}")
                        click.echo(f"     Items: {child['itemCount']}")

                    click.echo()

    except sqlite3.Error as e:
        click.echo(f"[-] Database error: {e}", err=True)
        sys.exit(1)
    except Exception as e:
        click.echo(f"[-] Error: {e}", err=True)
        sys.exit(1)


# ---------------------------------------------------------------------------
# doctor
# ---------------------------------------------------------------------------


@cli.command()
@click.option(
    "--storage",
    type=click.Path(path_type=Path),
    help="Custom storage directory (default: from ZOTERO_STORAGE)",
)
@click.pass_context
def doctor(ctx, storage: Path | None):
    """
    Run database health check and diagnostics.

    Checks for:
    - Environment paths (database file, storage directory)
    - SQLite integrity (PRAGMA integrity_check)
    - Required database schema (all expected Zotero tables)
    - Orphaned items (items not in any collection)
    - Missing attachment files
    - Broken foreign key references
    - Duplicate detection (hash, identifier, title)
    """
    database = ctx.obj["database"]
    storage_path = storage if storage else ctx.obj["storage"]
    json_output = ctx.obj.get("json_output", False)

    report: dict[str, Any] = {
        "environment": [],
        "integrity": {},
        "schema": {},
        "diagnostics": {},
    }
    has_errors = False

    echo_info("Checking environment paths...")
    env_checks = validate_environment_paths(database, storage_path)
    report["environment"] = env_checks

    for check in env_checks:
        if check["status"] == "error":
            has_errors = True
            click.echo(f"[-] {check['message']}", err=True)
        elif check["status"] == "warning":
            click.echo(f"[*] {check['message']}", err=True)

    if not database.exists():
        formatter = get_output_formatter(json_output)
        click.echo(formatter(report))
        click.echo("\n[-] Cannot continue: database file not found", err=True)
        sys.exit(1)

    try:
        with connect_database(database, read_only=True) as conn:
            echo_info("Running SQLite integrity check...")
            cursor = conn.execute("PRAGMA integrity_check")
            integrity_result = cursor.fetchone()[0]
            integrity_status = classify_integrity_result(integrity_result)

            report["integrity"] = {
                "status": integrity_status,
                "detail": integrity_result if integrity_status != "ok" else "ok",
            }

            if integrity_status != "ok":
                has_errors = True
                click.echo(f"[-] SQLite integrity check failed: {integrity_result}", err=True)

            echo_info("Validating database schema...")
            cursor = conn.execute(
                "SELECT name FROM sqlite_master WHERE type='table' OR type='view'"
            )
            existing_tables = [row[0] for row in cursor.fetchall()]
            missing_tables = check_required_tables(existing_tables, REQUIRED_ZOTERO_TABLES)

            report["schema"] = {
                "status": "ok" if not missing_tables else "error",
                "tables_found": len(existing_tables),
                "tables_required": len(REQUIRED_ZOTERO_TABLES),
                "missing_tables": missing_tables,
            }

            if missing_tables:
                has_errors = True
                click.echo(f"[-] Missing required tables: {', '.join(missing_tables)}", err=True)

            echo_info("Running content diagnostics...")
            diagnostics = run_database_diagnostics(conn, storage_path)
            report["diagnostics"] = diagnostics

            if diagnostics["summary"]["status"] == "issues_found":
                has_errors = True

        formatter = get_output_formatter(json_output)
        click.echo(formatter(report))

        if has_errors:
            click.echo("\n[-] Issues detected in database", err=True)
            sys.exit(1)
        else:
            click.echo("\n[+] Database is healthy", err=True)

    except sqlite3.Error as e:
        click.echo(f"[-] Database error: {e}", err=True)
        sys.exit(1)
    except Exception as e:
        click.echo(f"[-] Error: {e}", err=True)
        sys.exit(1)


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------


def main():
    cli(auto_envvar_prefix="ZOTEROCTL")


@cli.command()
@click.pass_context
def serve(ctx):
    """MCP server on stdio (search_items, get_item, export_bibtex, import_from_url, import_bibtex)."""
    from zoteroctl import mcp_server
    from zoteroctl.config import RuntimeConfig

    config = RuntimeConfig(database=ctx.obj["database"], storage=ctx.obj["storage"], translation_server=ctx.obj["translation_server"])
    mcp_server.build(config).run("stdio")
