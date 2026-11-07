#!@python@
"""hermes-mcp-install-likec4 -- register the LikeC4 MCP server for the current
project in a Hermes profile.

  hermes-mcp-install-likec4 [--profile NAME] [--name NAME] [WORKSPACE]

Hermes (verified v0.21.0) has no project-scoped MCP config: servers live only
in a profile's config.yaml (`mcp_servers`). This writes
  mcp_servers.<name> = { command: likec4-mcp, args: [<workspace>] }
into $HERMES_ROOT/profiles/<profile>/config.yaml (no --profile: the root
config.yaml). HERMES_ROOT defaults to the resolved ~/.hermes, so the tracked
harness file is edited, not a copy.

The file is tracked, so the edit must be additions only. The YAML style
(line width, how nulls are written) is detected by finding the style that
reproduces the file byte for byte; the edit is refused when none does, or
when the result would remove any line outside the entry being replaced.
`hermes mcp add` and yq both reformat the file. A workspace under $HOME is
written as ${HOME}/..., which Hermes expands, so the file deploys everywhere.

Exit codes: 64 usage, 65 cannot edit without reformatting, 66 no LikeC4
project or no such profile.
"""
import argparse
import difflib
import io
import os
import sys
from pathlib import Path

from ruamel.yaml import YAML
from ruamel.yaml.comments import CommentedMap, CommentedSeq
from ruamel.yaml.representer import RoundTripRepresenter

CONFIG_NAMES = ("likec4.config.json", "likec4.config.ts", "likec4.config.js",
                "likec4.config.mjs", "likec4.config.mts", ".likec4rc")


class TildeNullRepresenter(RoundTripRepresenter):
    """Writes None as `~` (the default round-trip representer writes nothing)."""


TildeNullRepresenter.add_representer(
    type(None),
    lambda r, _d: r.represent_scalar("tag:yaml.org,2002:null", "~"))


def fail(code, msg):
    print(f"[-] hermes-mcp-install-likec4: {msg}", file=sys.stderr)
    sys.exit(code)


def has_likec4_project(root: Path, depth: int = 3) -> bool:
    base = len(root.parts)
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d != "node_modules"]
        if len(Path(dirpath).parts) - base >= depth:
            dirnames[:] = []
        if any(f in CONFIG_NAMES for f in filenames):
            return True
    return False


def make_yaml(width: int, tilde: bool) -> YAML:
    y = YAML()
    if tilde:
        y.Representer = TildeNullRepresenter
    y.preserve_quotes = True
    y.width = width
    y.indent(mapping=2, sequence=4, offset=2)
    return y


def dump(y: YAML, data) -> str:
    out = io.StringIO()
    y.dump(data, out)
    return out.getvalue()


def faithful_yaml(src: str) -> YAML:
    """The YAML style that reproduces `src` exactly, or None."""
    for width in (4096, 80):
        for tilde in (False, True):
            y = make_yaml(width, tilde)
            if dump(y, y.load(src)) == src:
                return y
    return None


def entry_line_range(data, servers, name):
    """0-based [start, end) source lines of an existing mcp_servers entry."""
    keys = list(servers.keys())
    start = servers.lc.key(name)[0]
    i = keys.index(name)
    if i + 1 < len(keys):
        return start, servers.lc.key(keys[i + 1])[0]
    top = list(data.keys())
    j = top.index("mcp_servers")
    end = data.lc.key(top[j + 1])[0] if j + 1 < len(top) else sys.maxsize
    return start, end


def removed_lines(old: str, new: str):
    a, b = old.splitlines(), new.splitlines()
    for tag, i1, i2, _j1, _j2 in difflib.SequenceMatcher(a=a, b=b, autojunk=False).get_opcodes():
        if tag in ("delete", "replace"):
            yield from ((n, a[n]) for n in range(i1, i2))


def main() -> None:
    ap = argparse.ArgumentParser(prog="hermes-mcp-install-likec4",
                                 description=__doc__.split("\n\n")[0])
    ap.add_argument("--profile", help="Hermes profile (default: root config.yaml)")
    ap.add_argument("--name", default="likec4", help="server key (default: likec4)")
    ap.add_argument("workspace", nargs="?", default=".")
    try:
        args = ap.parse_args()
    except SystemExit as e:
        sys.exit(0 if e.code == 0 else 64)

    ws = Path(args.workspace)
    if not ws.is_dir():
        fail(66, f"not a directory: {ws}")
    ws = ws.resolve()
    if not has_likec4_project(ws):
        fail(66, f"no LikeC4 project (likec4.config.*) under {ws}")

    hermes_root = Path(os.environ.get("HERMES_ROOT") or Path.home() / ".hermes").resolve()
    cfg = hermes_root / "profiles" / args.profile / "config.yaml" if args.profile \
        else hermes_root / "config.yaml"
    if not cfg.is_file():
        fail(66, f"no Hermes config at {cfg}")

    home = Path.home().resolve()
    arg = f"${{HOME}}/{ws.relative_to(home)}" if ws.is_relative_to(home) and ws != home else str(ws)

    src = cfg.read_text()
    y = faithful_yaml(src)
    if y is None:
        fail(65, f"{cfg} does not round-trip in any known style (mixed line wrapping?); "
                 f"add by hand: mcp_servers.{args.name} = {{command: likec4-mcp, args: [{arg}]}}")
    data = y.load(src)
    if data is None:
        data = CommentedMap()

    servers = data.get("mcp_servers")
    allowed = (0, 0)
    if servers is None:
        servers = data["mcp_servers"] = CommentedMap()
    elif args.name in servers:
        cur = {k: list(v) if isinstance(v, list) else v for k, v in servers[args.name].items()}
        if cur == {"command": "likec4-mcp", "args": [arg]}:
            print(f"[*] {cfg}: '{args.name}' already registered")
            return
        allowed = entry_line_range(data, servers, args.name)
        del servers[args.name]

    entry = CommentedMap()
    entry["command"] = "likec4-mcp"
    entry["args"] = CommentedSeq([arg])
    # First position: a comment block after the last entry stays with it.
    servers.insert(0, args.name, entry)

    new = dump(y, data)
    bad = [(n, line) for n, line in removed_lines(src, new)
           if not (allowed[0] <= n < allowed[1]) or not line.strip() or line.lstrip().startswith("#")]
    if bad:
        fail(65, f"editing {cfg} would change line {bad[0][0] + 1} ({bad[0][1].strip()!r}); "
                 f"add by hand: mcp_servers.{args.name} = {{command: likec4-mcp, args: [{arg}]}}")

    tmp = cfg.with_name(cfg.name + ".likec4-tmp")
    tmp.write_text(new)
    os.chmod(tmp, cfg.stat().st_mode & 0o7777)
    tmp.replace(cfg)
    print(f"[+] {cfg}: mcp_servers.{args.name} -> {arg}")
    profile = f"-p {args.profile} " if args.profile else ""
    print(f"[*] next: task check:harness (repo root); verify: hermes {profile}mcp test {args.name}")


if __name__ == "__main__":
    main()
