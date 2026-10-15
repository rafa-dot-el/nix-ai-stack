"""Find processes by exact argv entry, via /proc (stdlib only).

Used to prove Chromium is really running for a test and to clean up what a
test started. Matching is on whole argv entries, never on substrings, so an
unrelated process cannot be caught by accident.
"""

from pathlib import Path


def argv_of(pid: int) -> list:
    try:
        raw = (Path("/proc") / str(pid) / "cmdline").read_bytes()
    except OSError:
        return []
    return [a.decode(errors="replace") for a in raw.split(b"\0") if a]


def pids_with_arg(arg: str) -> list:
    found = []
    for entry in Path("/proc").iterdir():
        if entry.name.isdigit() and arg in argv_of(int(entry.name)):
            found.append(int(entry.name))
    return sorted(found)


def environ_of(pid: int) -> dict:
    try:
        raw = (Path("/proc") / str(pid) / "environ").read_bytes()
    except OSError:
        return {}
    pairs = (p.decode(errors="replace").partition("=") for p in raw.split(b"\0") if p)
    return {k: v for k, _, v in pairs}


def ppid_of(pid: int) -> int:
    try:
        stat = (Path("/proc") / str(pid) / "stat").read_text()
    except OSError:
        return 0
    # Field 2 (comm) can contain spaces and parentheses; the fields after the last ')' are fixed.
    return int(stat.rsplit(")", 1)[1].split()[1])


def _descends_from_home(pid: int, home: str) -> bool:
    current = pid
    for _ in range(64):
        if current <= 1:
            return False
        if environ_of(current).get("HOME") == home:
            return True
        current = ppid_of(current)
    return False


def pids_for_session(home: str) -> list:
    """Processes started under one test session: those with its HOME, or descended from one that has it.

    Chromium is spawned with a stripped environment, so its own HOME is missing; its agent-browser daemon
    parent carries the session HOME. Matching on argv would miss it: its profile lives in /tmp, not the session.
    """
    found = []
    for entry in Path("/proc").iterdir():
        if entry.name.isdigit() and _descends_from_home(int(entry.name), home):
            found.append(int(entry.name))
    return sorted(found)
