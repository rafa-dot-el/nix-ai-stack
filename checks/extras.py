"""Every distribution the selected Hermes extras require is installed at a matching version.

Run with Hermes's own interpreter so ``importlib.metadata`` sees its sealed venv:

  python3 extras.py PYPROJECT GROUP...

An extra that upstream renamed or dropped is an error (it used to surface only as a
failed build); a requirement whose marker does not apply to this interpreter is skipped.
"""

from __future__ import annotations

import importlib.metadata
import platform
import sys
import tomllib
from typing import Callable, Iterable, Optional

from packaging.requirements import Requirement
from packaging.utils import canonicalize_name


def _applies(req: Requirement, env: dict) -> bool:
    return req.marker is None or req.marker.evaluate(env)


def required(pyproject: dict, groups: Iterable[str], env: dict) -> list[Requirement]:
    """Requirements of ``groups``, self-references (``<project>[a,b]``) expanded, markers applied."""
    project = pyproject["project"]
    own_name = canonicalize_name(project["name"])
    optional = project.get("optional-dependencies", {})
    seen_groups: set[str] = set()
    found: dict[str, Requirement] = {}

    def visit(group: str) -> None:
        if group in seen_groups:
            return
        if group not in optional:
            raise KeyError(f"extra {group!r} does not exist in {project['name']}'s pyproject")
        seen_groups.add(group)
        for spec in optional[group]:
            req = Requirement(spec)
            if not _applies(req, env):
                continue
            if canonicalize_name(req.name) == own_name:
                for extra in sorted(req.extras):
                    visit(extra)
                continue
            found.setdefault(str(req), req)

    for group in groups:
        visit(group)
    return list(found.values())


def problems(reqs: Iterable[Requirement], installed_version: Callable[[str], Optional[str]]) -> list[str]:
    """One line per requirement that is missing or installed at a version outside its specifier."""
    out = []
    for req in sorted(reqs, key=lambda r: canonicalize_name(r.name)):
        name = canonicalize_name(req.name)
        version = installed_version(name)
        if version is None:
            out.append(f"{name}: not installed (needs {req.specifier})")
        elif req.specifier and not req.specifier.contains(version, prereleases=True):
            out.append(f"{name}: {version} installed, needs {req.specifier}")
    return out


def _installed() -> Callable[[str], Optional[str]]:
    versions: dict[str, str] = {canonicalize_name(d.metadata["Name"]): d.version
                                for d in importlib.metadata.distributions()}
    return lambda name: versions.get(name)


def _marker_env() -> dict:
    return {
        "sys_platform": sys.platform,
        "platform_system": platform.system(),
        "platform_machine": platform.machine(),
        "python_version": ".".join(platform.python_version_tuple()[:2]),
        "python_full_version": platform.python_version(),
        "implementation_name": sys.implementation.name,
        "platform_python_implementation": platform.python_implementation(),
        "os_name": "posix",
        "extra": "",
    }


def main(argv: list[str]) -> int:
    pyproject_path, groups = argv[1], argv[2:]
    with open(pyproject_path, "rb") as f:
        pyproject = tomllib.load(f)
    try:
        reqs = required(pyproject, groups, _marker_env())
    except KeyError as e:
        print(f"[-] {e.args[0]}", file=sys.stderr)
        return 1
    found = problems(reqs, _installed())
    for line in found:
        print(f"[-] {line}", file=sys.stderr)
    if found:
        return 1
    print(f"[+] {len(reqs)} requirements of {len(groups)} extras installed: {' '.join(groups)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
