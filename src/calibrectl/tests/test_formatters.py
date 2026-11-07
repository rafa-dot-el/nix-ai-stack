"""
Tests for calibrectl.formatters
"""

import json

import pytest

from calibrectl.formatters import (
    convert_date_to_iso,
    format_date_simple,
    format_error,
    format_info,
    format_json,
    format_success,
    format_yaml,
    set_quiet,
)


class TestFormatPrefixes:
    def test_format_error(self):
        assert format_error("bad") == "[-] bad"

    def test_format_success(self):
        assert format_success("ok") == "[+] ok"

    def test_format_info(self):
        assert format_info("note") == "[*] note"

    def test_format_error_empty(self):
        assert format_error("") == "[-] "

    def test_format_success_empty(self):
        assert format_success("") == "[+] "


class TestFormatYAML:
    def test_format_yaml_roundtrip(self):
        data = {"key": "value", "number": 42}
        result = format_yaml(data)
        assert "key: value" in result
        assert "number: 42" in result

    def test_format_yaml_unicode(self):
        assert "Quantenmechanik" in format_yaml({"t": "Quantenmechanik"})

    def test_format_yaml_nested(self):
        data = {"outer": {"inner": "value"}}
        result = format_yaml(data)
        assert "outer:" in result
        assert "inner: value" in result

    def test_format_yaml_list(self):
        data = {"items": [1, 2, 3]}
        result = format_yaml(data)
        assert "items:" in result


class TestFormatJSON:
    def test_format_json_roundtrip(self):
        data = {"key": "value", "number": 42}
        assert json.loads(format_json(data)) == data

    def test_format_json_indented(self):
        result = format_json({"a": 1, "b": 2})
        assert "\n" in result
        assert "  " in result

    def test_format_json_unicode(self):
        data = {"title": "Ärger mit Technik"}
        result = format_json(data)
        parsed = json.loads(result)
        assert parsed["title"] == "Ärger mit Technik"

    def test_format_json_list(self):
        data = [1, 2, 3]
        result = format_json(data)
        assert json.loads(result) == [1, 2, 3]

    def test_format_json_empty(self):
        assert json.loads(format_json({})) == {}


@pytest.mark.parametrize(
    "date_str, expected",
    [
        ("2024-08-12 16:26:50.191078+00:00", "2024-08-12"),
        ("2024-01-15", "2024-01-15"),
        ("2000-12-31T23:59:59", "2000-12-31"),
        (None, None),
        ("", None),
        ("2024", "2024"),  # shorter than 10 chars returned as-is
    ],
)
def test_format_date_simple(date_str, expected):
    assert format_date_simple(date_str) == expected


@pytest.mark.parametrize(
    "date_str, expected",
    [
        ("2024-01-15", "2024-01-15 00:00:00+00:00"),
        (None, None),
        ("", None),
        ("not-a-date", None),
        ("2000-12-31", "2000-12-31 00:00:00+00:00"),
    ],
)
def test_convert_date_to_iso(date_str, expected):
    assert convert_date_to_iso(date_str) == expected


class TestEchoFunctions:
    def test_echo_info_quiet_suppresses(self, capsys):
        set_quiet(True)
        from calibrectl.formatters import echo_info

        echo_info("should not appear")
        # echo_info writes to stderr; capsys captures it
        captured = capsys.readouterr()
        assert "should not appear" not in captured.err
        set_quiet(False)

    def test_echo_info_not_quiet(self, capsys):
        set_quiet(False)
        from calibrectl.formatters import echo_info

        echo_info("visible message")
        captured = capsys.readouterr()
        assert "visible message" in captured.err

    def test_echo_success_always_shown(self, capsys):
        set_quiet(True)
        from calibrectl.formatters import echo_success

        echo_success("always visible")
        captured = capsys.readouterr()
        assert "always visible" in captured.err
        set_quiet(False)

    def test_echo_error_always_shown(self, capsys):
        set_quiet(True)
        from calibrectl.formatters import echo_error

        echo_error("error visible")
        captured = capsys.readouterr()
        assert "error visible" in captured.err
        set_quiet(False)
