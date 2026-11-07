"""Unit tests for checks/extras.py: every dependency of the selected extras must be installed."""

import importlib.util
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
_spec = importlib.util.spec_from_file_location("check_extras", ROOT / "checks" / "extras.py")
assert _spec is not None and _spec.loader is not None
extras = importlib.util.module_from_spec(_spec)
sys.modules["check_extras"] = extras
_spec.loader.exec_module(extras)

PYPROJECT = {
    "project": {
        "name": "hermes-agent",
        "optional-dependencies": {
            "anthropic": ["anthropic==0.87.0"],
            "messaging": ["slack-sdk==3.44.1", "aiohttp==3.14.3; sys_platform == 'linux'",
                          "pywin32==311; sys_platform == 'win32'"],
            "matrix": ["mautrix[encryption]==0.21.0"],
            "all": ["hermes-agent[anthropic,messaging]", "tomli>=2; python_version < '3.11'"],
        },
    }
}
LINUX_PY314 = {"sys_platform": "linux", "python_version": "3.14", "platform_system": "Linux"}


def names(reqs):
    return sorted(str(r) for r in reqs)


def test_resolves_one_group_to_its_pinned_requirements():
    assert names(extras.required(PYPROJECT, ["anthropic"], LINUX_PY314)) == ["anthropic==0.87.0"]


def test_markers_that_do_not_apply_are_dropped():
    got = names(extras.required(PYPROJECT, ["messaging"], LINUX_PY314))
    assert got == ['aiohttp==3.14.3; sys_platform == "linux"', "slack-sdk==3.44.1"]


def test_self_references_expand_recursively_and_dedupe():
    got = names(extras.required(PYPROJECT, ["all", "anthropic"], LINUX_PY314))
    assert got == ['aiohttp==3.14.3; sys_platform == "linux"', "anthropic==0.87.0", "slack-sdk==3.44.1"]


def test_unknown_group_is_an_error():
    try:
        extras.required(PYPROJECT, ["honcho"], LINUX_PY314)
    except KeyError as e:
        assert "honcho" in str(e)
    else:
        raise AssertionError("an extra that upstream dropped must fail the check, not be skipped")


def test_problems_reports_missing_and_mismatched_distributions():
    reqs = extras.required(PYPROJECT, ["anthropic", "messaging", "matrix"], LINUX_PY314)
    installed = {"anthropic": "0.87.0", "slack-sdk": "3.40.0", "mautrix": "0.21.0"}
    assert extras.problems(reqs, installed.get) == [
        "aiohttp: not installed (needs ==3.14.3)",
        "slack-sdk: 3.40.0 installed, needs ==3.44.1",
    ]


def test_problems_normalises_distribution_names():
    reqs = extras.required({"project": {"name": "x", "optional-dependencies": {"g": ["Foo_Bar.baz==1"]}}},
                           ["g"], LINUX_PY314)
    assert extras.problems(reqs, {"foo-bar-baz": "1"}.get) == []
