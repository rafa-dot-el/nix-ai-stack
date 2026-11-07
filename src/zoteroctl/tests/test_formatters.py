"""
Tests for zoteroctl.formatters module.
"""

import json

import pytest
import yaml

from zoteroctl.formatters import (
    dump_item_data,
    echo_error,
    echo_info,
    echo_success,
    echo_warning,
    format_as_json,
    format_as_yaml,
    get_output_formatter,
    set_quiet_mode,
)


class TestFormatAsYaml:
    def test_simple_dict(self):
        result = format_as_yaml({"key": "value"})
        assert "key: value" in result

    def test_nested_dict(self):
        result = format_as_yaml({"parent": {"child": "value"}})
        assert "parent:" in result
        assert "child: value" in result

    def test_unicode(self):
        result = format_as_yaml({"title": "Über die Quantenmechanik"})
        assert "Über" in result

    def test_list_value(self):
        result = format_as_yaml({"items": [1, 2, 3]})
        assert "items:" in result

    def test_empty_dict(self):
        result = format_as_yaml({})
        assert result == "{}\n"

    def test_returns_valid_yaml(self):
        data = {"key": "value", "number": 42, "nested": {"a": 1}}
        parsed = yaml.safe_load(format_as_yaml(data))
        assert parsed == data

    def test_unicode_not_escaped(self):
        result = format_as_yaml({"name": "日本語"})
        assert "日本語" in result

    @pytest.mark.parametrize(
        "key,value",
        [
            ("empty_str", ""),
            ("none_val", None),
            ("zero", 0),
            ("false_val", False),
        ],
    )
    def test_edge_values(self, key, value):
        result = format_as_yaml({key: value})
        assert isinstance(result, str)

    def test_block_style(self):
        result = format_as_yaml({"a": {"b": "c"}})
        assert "{" not in result  # block style, not flow style


class TestFormatAsJson:
    def test_simple_dict(self):
        data = {"key": "value", "number": 42}
        result = format_as_json(data)
        assert json.loads(result) == data

    def test_indented(self):
        result = format_as_json({"a": 1})
        assert "\n" in result and "  " in result

    def test_unicode(self):
        result = format_as_json({"name": "Niels Bohr"})
        assert "Niels Bohr" in result

    def test_unicode_not_escaped(self):
        result = format_as_json({"name": "日本語"})
        assert "日本語" in result

    def test_empty_dict(self):
        result = format_as_json({})
        assert json.loads(result) == {}

    def test_nested(self):
        data = {"outer": {"inner": [1, 2, 3]}}
        assert json.loads(format_as_json(data)) == data

    @pytest.mark.parametrize(
        "value",
        [None, "", 0, False, [], {}],
    )
    def test_edge_values(self, value):
        result = format_as_json({"v": value})
        assert json.loads(result)["v"] == value


class TestGetOutputFormatter:
    def test_yaml_formatter(self):
        assert get_output_formatter(json_output=False) is format_as_yaml

    def test_json_formatter(self):
        assert get_output_formatter(json_output=True) is format_as_json

    def test_default_is_yaml(self):
        assert get_output_formatter() is format_as_yaml


class TestDumpItemData:
    def test_json_format(self):
        data = {"k": "v"}
        result = dump_item_data(data, format="json")
        assert json.loads(result) == data

    def test_yaml_format(self):
        data = {"k": "v"}
        result = dump_item_data(data, format="yaml")
        assert "k: v" in result

    def test_unsupported_format_raises(self):
        with pytest.raises(ValueError, match="Unsupported format"):
            dump_item_data({"k": "v"}, format="xml")

    def test_json_roundtrip(self):
        data = {"key": "value", "nested": {"a": [1, 2, 3]}}
        assert json.loads(dump_item_data(data, format="json")) == data

    def test_yaml_roundtrip(self):
        data = {"key": "value", "number": 42}
        parsed = yaml.safe_load(dump_item_data(data, format="yaml"))
        assert parsed == data


class TestEchoFunctions:
    """Tests for side-effecting echo functions (capture stderr via capsys)."""

    def test_echo_info_shown_when_not_quiet(self, capsys):
        set_quiet_mode(False)
        echo_info("test message")
        captured = capsys.readouterr()
        assert "[*] test message" in captured.err

    def test_echo_info_suppressed_when_quiet(self, capsys):
        set_quiet_mode(True)
        echo_info("should not appear")
        captured = capsys.readouterr()
        assert captured.err == ""
        set_quiet_mode(False)  # reset

    def test_echo_success(self, capsys):
        echo_success("all good")
        captured = capsys.readouterr()
        assert "[+] all good" in captured.err

    def test_echo_error(self, capsys):
        echo_error("something failed")
        captured = capsys.readouterr()
        assert "[-] something failed" in captured.err

    def test_echo_warning(self, capsys):
        echo_warning("watch out")
        captured = capsys.readouterr()
        assert "[!] watch out" in captured.err
