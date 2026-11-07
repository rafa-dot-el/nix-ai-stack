"""
Tests for zoteroctl CLI using click.testing.CliRunner.
"""

import json
import sqlite3

from click.testing import CliRunner

from zoteroctl.cli import cli
from zoteroctl.collections import create_collection_record


def _extract_json(output: str) -> object:
    """Extract first valid JSON object/array from output that may contain progress lines."""
    import re

    decoder = json.JSONDecoder()
    for m in re.finditer(r"[\[{]", output):
        try:
            obj, _ = decoder.raw_decode(output, m.start())
            return obj
        except json.JSONDecodeError:
            continue
    raise ValueError(f"No JSON found in output: {output!r}")


def _db_path_env(db_path, storage_path=None):
    """Build env dict for CLI invocation."""
    env = {"ZOTERO_DATABASE": str(db_path)}
    if storage_path:
        env["ZOTERO_STORAGE"] = str(storage_path)
    return env


class TestHelpOutput:
    def test_root_help(self):
        runner = CliRunner()
        result = runner.invoke(cli, ["--help"])
        assert result.exit_code == 0
        assert "Zotero" in result.output

    def test_get_help(self):
        runner = CliRunner()
        result = runner.invoke(cli, ["get", "--help"])
        assert result.exit_code == 0
        assert "KEY" in result.output

    def test_doctor_help(self):
        runner = CliRunner()
        result = runner.invoke(cli, ["doctor", "--help"])
        assert result.exit_code == 0

    def test_list_collections_help(self):
        runner = CliRunner()
        result = runner.invoke(cli, ["list-collections", "--help"])
        assert result.exit_code == 0

    def test_add_file_help(self):
        runner = CliRunner()
        result = runner.invoke(cli, ["add-file", "--help"])
        assert result.exit_code == 0

    def test_delete_file_help(self):
        runner = CliRunner()
        result = runner.invoke(cli, ["delete-file", "--help"])
        assert result.exit_code == 0

    def test_export_help(self):
        runner = CliRunner()
        result = runner.invoke(cli, ["export", "--help"])
        assert result.exit_code == 0

    def test_create_collection_help(self):
        runner = CliRunner()
        result = runner.invoke(cli, ["create-collection", "--help"])
        assert result.exit_code == 0

    def test_bibtex_to_yaml_help(self):
        runner = CliRunner()
        result = runner.invoke(cli, ["bibtex-to-yaml", "--help"])
        assert result.exit_code == 0


class TestMissingDatabase:
    def test_get_missing_db_gives_clean_error(self, tmp_path):
        runner = CliRunner()
        env = {"ZOTERO_DATABASE": str(tmp_path / "nonexistent.sqlite")}
        result = runner.invoke(cli, ["get", "ABCD1234"], env=env)
        assert result.exit_code != 0
        assert "Database not found" in result.output or "not found" in result.output.lower()
        # No traceback
        assert "Traceback" not in result.output

    def test_doctor_missing_db_gives_clean_error(self, tmp_path):
        runner = CliRunner()
        env = {"ZOTERO_DATABASE": str(tmp_path / "missing.sqlite")}
        result = runner.invoke(cli, ["doctor"], env=env)
        assert result.exit_code != 0
        assert "Traceback" not in result.output

    def test_list_collections_missing_db_gives_clean_error(self, tmp_path):
        runner = CliRunner()
        env = {"ZOTERO_DATABASE": str(tmp_path / "missing.sqlite")}
        result = runner.invoke(cli, ["list-collections"], env=env)
        assert result.exit_code != 0
        assert "Traceback" not in result.output

    def test_create_collection_missing_db_gives_clean_error(self, tmp_path):
        runner = CliRunner()
        env = {"ZOTERO_DATABASE": str(tmp_path / "missing.sqlite")}
        result = runner.invoke(cli, ["create-collection", "Test"], env=env)
        assert result.exit_code != 0
        assert "Traceback" not in result.output


class TestDoctorCommand:
    def test_doctor_json_output(self, zotero_db_path, storage_dir):
        runner = CliRunner()
        env = _db_path_env(zotero_db_path, storage_dir)
        result = runner.invoke(cli, ["--json-output", "doctor"], env=env)
        parsed = _extract_json(result.output)
        assert isinstance(parsed, dict)
        assert "environment" in parsed
        assert "integrity" in parsed
        assert "schema" in parsed

    def test_doctor_yaml_output(self, zotero_db_path, storage_dir):
        runner = CliRunner()
        env = _db_path_env(zotero_db_path, storage_dir)
        result = runner.invoke(cli, ["doctor"], env=env)
        assert "environment" in result.output

    def test_doctor_exits_1_when_issues(self, zotero_db_path, storage_dir):
        """Doctor exits 1 if issues found; may be 0 if healthy."""
        runner = CliRunner()
        env = _db_path_env(zotero_db_path, storage_dir)
        result = runner.invoke(cli, ["doctor"], env=env)
        assert result.exit_code in (0, 1)  # depends on DB state


class TestListCollectionsCommand:
    def test_empty_collections_json(self, zotero_db_path, storage_dir):
        runner = CliRunner()
        env = _db_path_env(zotero_db_path, storage_dir)
        result = runner.invoke(cli, ["list-collections", "--json"], env=env)
        # Empty DB exits 0 (no collections found exits 0 with message)
        assert result.exit_code == 0

    def test_with_collections_json(self, zotero_db_path, storage_dir):
        # Add a collection directly
        conn = sqlite3.connect(str(zotero_db_path))
        conn.row_factory = sqlite3.Row
        create_collection_record(conn, "Test Collection")
        conn.commit()
        conn.close()

        runner = CliRunner()
        env = _db_path_env(zotero_db_path, storage_dir)
        result = runner.invoke(cli, ["list-collections", "--json"], env=env)
        assert result.exit_code == 0
        collections = json.loads(result.output)
        assert isinstance(collections, list)
        assert any(c["name"] == "Test Collection" for c in collections)

    def test_with_collections_yaml(self, zotero_db_path, storage_dir):
        conn = sqlite3.connect(str(zotero_db_path))
        conn.row_factory = sqlite3.Row
        create_collection_record(conn, "YAMLColl")
        conn.commit()
        conn.close()

        runner = CliRunner()
        env = _db_path_env(zotero_db_path, storage_dir)
        result = runner.invoke(cli, ["list-collections"], env=env)
        assert result.exit_code == 0
        assert "YAMLColl" in result.output

    def test_parent_flag_renders_children(self, zotero_db_path, storage_dir):
        conn = sqlite3.connect(str(zotero_db_path))
        conn.row_factory = sqlite3.Row
        parent_key = create_collection_record(conn, "Parent")
        create_collection_record(conn, "Child One", parent_key)
        create_collection_record(conn, "Child Two", parent_key)
        conn.commit()
        conn.close()

        runner = CliRunner()
        env = _db_path_env(zotero_db_path, storage_dir)
        result = runner.invoke(cli, ["list-collections", "--parent", parent_key], env=env)
        assert result.exit_code == 0
        assert "Child One" in result.output
        assert "Child Two" in result.output

    def test_parent_flag_json_returns_children_array(self, zotero_db_path, storage_dir):
        conn = sqlite3.connect(str(zotero_db_path))
        conn.row_factory = sqlite3.Row
        parent_key = create_collection_record(conn, "Root")
        create_collection_record(conn, "Sub A", parent_key)
        conn.commit()
        conn.close()

        runner = CliRunner()
        env = _db_path_env(zotero_db_path, storage_dir)
        result = runner.invoke(
            cli, ["--json-output", "list-collections", "--parent", parent_key], env=env
        )
        assert result.exit_code == 0
        data = _extract_json(result.output)
        assert isinstance(data, list)
        assert all(c["parentKey"] == parent_key for c in data)


class TestGetCommand:
    def test_get_nonexistent_key(self, zotero_db_path, storage_dir):
        runner = CliRunner()
        env = _db_path_env(zotero_db_path, storage_dir)
        result = runner.invoke(cli, ["get", "ABCD1234"], env=env)
        assert result.exit_code != 0

    def test_get_existing_item_yaml(self, zotero_db_path, storage_dir):
        import yaml

        from zoteroctl.cli import add_or_update_item

        conn = sqlite3.connect(str(zotero_db_path))
        conn.row_factory = sqlite3.Row
        key = add_or_update_item(
            conn,
            {
                "itemType": "book",
                "fields": {"title": "Hello World"},
                "creators": [],
                "tags": [],
            },
        )
        conn.commit()
        conn.close()

        runner = CliRunner()
        env = _db_path_env(zotero_db_path, storage_dir)
        result = runner.invoke(cli, ["get", key], env=env)
        assert result.exit_code == 0
        data = yaml.safe_load(result.output)
        assert data["fields"]["title"] == "Hello World"

    def test_get_existing_item_json(self, zotero_db_path, storage_dir):
        from zoteroctl.cli import add_or_update_item

        conn = sqlite3.connect(str(zotero_db_path))
        conn.row_factory = sqlite3.Row
        key = add_or_update_item(
            conn,
            {
                "itemType": "book",
                "fields": {"title": "JSON Book"},
                "creators": [],
                "tags": [],
            },
        )
        conn.commit()
        conn.close()

        runner = CliRunner()
        env = _db_path_env(zotero_db_path, storage_dir)
        result = runner.invoke(cli, ["--json-output", "get", key], env=env)
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["fields"]["title"] == "JSON Book"


class TestCreateCollectionCommand:
    def test_create_top_level(self, zotero_db_path, storage_dir):
        runner = CliRunner()
        env = _db_path_env(zotero_db_path, storage_dir)
        result = runner.invoke(cli, ["create-collection", "My Research"], env=env)
        assert result.exit_code == 0
        assert "Collection created" in result.output

    def test_create_with_invalid_parent(self, zotero_db_path, storage_dir):
        runner = CliRunner()
        env = _db_path_env(zotero_db_path, storage_dir)
        result = runner.invoke(cli, ["create-collection", "Child", "--parent", "NOTFOUND"], env=env)
        assert result.exit_code != 0


class TestVersionFlag:
    def test_version(self):
        runner = CliRunner()
        result = runner.invoke(cli, ["--version"])
        assert result.exit_code == 0
        assert "0.1.0" in result.output
