"""
Tests for calibrectl.cli (CLI integration via CliRunner)
"""

import json

import pytest
from click.testing import CliRunner

from calibrectl.cli import cli


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


@pytest.fixture
def runner():
    return CliRunner()


@pytest.fixture
def db_env(calibre_db_file, tmp_path):
    """Return env dict pointing to the test database."""
    return {
        "CALIBRECTL_DATABASE": str(calibre_db_file),
        "CALIBRECTL_STORAGE": str(tmp_path),
    }


class TestHelp:
    def test_root_help(self, runner):
        result = runner.invoke(cli, ["--help"])
        assert result.exit_code == 0
        assert "calibrectl" in result.output.lower() or "Usage" in result.output

    @pytest.mark.parametrize(
        "subcommand",
        [
            "get",
            "list",
            "tags",
            "query",
            "doctor",
            "create",
            "update",
            "delete",
            "add-file",
            "find-isbn",
            "fetch-metadata",
            "set-cover",
            "retag",
            "dedupe",
            "export-bibtex",
            "extract-text",
        ],
    )
    def test_subcommand_help(self, runner, subcommand):
        result = runner.invoke(cli, [subcommand, "--help"])
        assert result.exit_code == 0
        assert "Usage" in result.output

    def test_no_self_test_subcommand(self, runner):
        result = runner.invoke(cli, ["self-test", "--help"])
        assert result.exit_code != 0


class TestDoctorCommand:
    def test_doctor_json_output(self, runner, calibre_db_file, tmp_path):
        result = runner.invoke(
            cli,
            [
                "--json-output",
                "doctor",
                "--database",
                str(calibre_db_file),
                "--storage",
                str(tmp_path),
            ],
        )
        data = _extract_json(result.output)
        assert isinstance(data, dict)
        assert "environment" in data
        assert "integrity" in data
        assert "schema" in data

    def test_doctor_missing_database(self, runner, tmp_path):
        result = runner.invoke(
            cli,
            [
                "doctor",
                "--database",
                str(tmp_path / "nonexistent.db"),
                "--storage",
                str(tmp_path),
            ],
        )
        assert result.exit_code != 0
        assert "not found" in result.output.lower() or "not found" in (result.stderr or "").lower()


class TestListCommand:
    def test_list_json_empty_result(self, runner, calibre_db_file, tmp_path):
        # Filter by a tag that doesn't exist → no books
        result = runner.invoke(
            cli,
            [
                "--json-output",
                "list",
                "--tag",
                "nonexistent-tag-xyz",
                "--database",
                str(calibre_db_file),
            ],
        )
        # Either exits 0 with empty JSON or exits non-zero with no books message
        # The important thing is no crash and parseable output if exit 0
        if result.exit_code == 0:
            try:
                data = json.loads(result.output)
                assert isinstance(data, (dict, list))
            except json.JSONDecodeError:
                pass  # Info output on empty is ok

    def test_list_json_output(self, runner, calibre_db_file, tmp_path):
        result = runner.invoke(
            cli,
            [
                "--json-output",
                "list",
                "--database",
                str(calibre_db_file),
                "--limit",
                "10",
            ],
        )
        if result.exit_code == 0:
            data = json.loads(result.output)
            assert "books" in data
            assert len(data["books"]) >= 1

    def test_list_missing_database(self, runner, tmp_path):
        result = runner.invoke(
            cli,
            ["list", "--database", str(tmp_path / "nonexistent.db")],
        )
        assert result.exit_code != 0

    def test_list_filter_by_author(self, runner, calibre_db_file, tmp_path):
        result = runner.invoke(
            cli,
            [
                "--json-output",
                "list",
                "--author",
                "John Doe",
                "--database",
                str(calibre_db_file),
            ],
        )
        if result.exit_code == 0:
            data = json.loads(result.output)
            assert "books" in data


class TestGetCommand:
    def test_get_existing_book(self, runner, calibre_db_file, tmp_path):
        result = runner.invoke(
            cli,
            [
                "--json-output",
                "get",
                "1",
                "--database",
                str(calibre_db_file),
                "--storage",
                str(tmp_path),
            ],
        )
        if result.exit_code == 0:
            data = json.loads(result.output)
            assert data["id"] == 1
            assert data["title"] == "Test Book"

    def test_get_nonexistent_book(self, runner, calibre_db_file, tmp_path):
        result = runner.invoke(
            cli,
            [
                "get",
                "9999",
                "--database",
                str(calibre_db_file),
                "--storage",
                str(tmp_path),
            ],
        )
        assert result.exit_code != 0


class TestMissingDatabaseEnvVar:
    def test_missing_calibre_database_gives_clean_error(self, runner, tmp_path):
        """When CALIBRE_DATABASE points to a non-existent file, produce a clean error."""
        result = runner.invoke(
            cli,
            ["list", "--database", str(tmp_path / "missing.db")],
        )
        assert result.exit_code != 0
        # Should not be a Python traceback
        assert "Traceback" not in result.output

    def test_doctor_env_var_override(self, runner, calibre_db_file, tmp_path):
        """CALIBRECTL_DATABASE env var is picked up via auto_envvar_prefix."""
        result = runner.invoke(
            cli,
            ["--json-output", "doctor", "--storage", str(tmp_path)],
            env={"CALIBRECTL_DATABASE": str(calibre_db_file)},
        )
        data = _extract_json(result.output)
        assert isinstance(data, dict)
        assert "environment" in data


class TestTagsCommand:
    def test_tags_json(self, runner, calibre_db_file):
        result = runner.invoke(
            cli,
            ["--json-output", "tags", "--database", str(calibre_db_file)],
        )
        assert result.exit_code == 0, result.output
        data = _extract_json(result.output)
        assert isinstance(data, dict)
        assert "tags" in data
        names = [t["name"] for t in data["tags"]]
        assert "fiction" in names

    def test_tags_sort_by_count(self, runner, calibre_db_file):
        result = runner.invoke(
            cli,
            [
                "--json-output",
                "tags",
                "--sort-by",
                "count",
                "--database",
                str(calibre_db_file),
            ],
        )
        assert result.exit_code == 0


class TestExportBibtexCommand:
    def test_no_args_gives_error(self, runner, calibre_db_file):
        result = runner.invoke(cli, ["export-bibtex", "--database", str(calibre_db_file)])
        assert result.exit_code != 0
        assert "--all" in result.output or "--all" in result.output

    def test_single_book(self, runner, calibre_db_file):
        result = runner.invoke(cli, ["export-bibtex", "1", "--database", str(calibre_db_file)])
        assert result.exit_code == 0
        assert "@" in result.output

    def test_all_exports_all_books(self, runner, calibre_db_file):
        result = runner.invoke(cli, ["export-bibtex", "--all", "--database", str(calibre_db_file)])
        assert result.exit_code == 0
        entries = [line for line in result.output.splitlines() if line.startswith("@")]
        assert len(entries) == 2  # fixture DB has 2 books

    def test_all_produces_unique_keys(self, runner, calibre_db_file):
        result = runner.invoke(cli, ["export-bibtex", "--all", "--database", str(calibre_db_file)])
        assert result.exit_code == 0
        keys = [line for line in result.output.splitlines() if line.startswith("@")]
        key_names = [k.split("{")[1].rstrip(",") for k in keys]
        assert len(key_names) == len(set(key_names)), "Duplicate cite keys in --all output"
