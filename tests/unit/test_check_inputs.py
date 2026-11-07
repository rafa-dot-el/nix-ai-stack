"""Unit tests for scripts/check-inputs.jq: one nixpkgs node per channel (ADR-0001)."""

import json
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[2]
FILTER = ROOT / "scripts" / "check-inputs.jq"


def nixpkgs(ref):
    return {"locked": {"owner": "nixos", "repo": "nixpkgs", "rev": ref, "type": "github"},
            "original": {"owner": "nixos", "ref": ref, "repo": "nixpkgs", "type": "github"}}


def run(lock: dict) -> list[str]:
    out = subprocess.run(["jq", "-r", "-f", str(FILTER)], input=json.dumps(lock),
                         capture_output=True, text=True, check=True).stdout
    return [line for line in out.splitlines() if line]


def test_followed_lock_has_no_violations():
    lock = {"root": "root", "nodes": {
        "root": {"inputs": {"nixpkgs": "nixpkgs", "nixpkgs-unstable": "nixpkgs-unstable",
                            "uv2nix": "uv2nix"}},
        "nixpkgs": nixpkgs("nixos-26.05"),
        "nixpkgs-unstable": nixpkgs("nixos-unstable"),
        "uv2nix": {"inputs": {"nixpkgs": ["nixpkgs"]}, "locked": {"repo": "uv2nix"}},
    }}
    assert run(lock) == []


def test_unfollowed_nixpkgs_is_reported_with_its_parent():
    lock = {"root": "root", "nodes": {
        "root": {"inputs": {"nixpkgs": "nixpkgs_2", "hermes-agent-src": "hermes-agent-src"}},
        "nixpkgs_2": nixpkgs("nixos-25.11"),
        "nixpkgs": nixpkgs("nixos-unstable"),
        "hermes-agent-src": {"inputs": {"nixpkgs": "nixpkgs"}, "locked": {"repo": "hermes-agent"}},
    }}
    assert run(lock) == ["nixpkgs (nixos-unstable) <- hermes-agent-src"]


def test_case_insensitive_owner_and_nixpkgs_lib_style_repos_are_covered():
    node = nixpkgs("nixos-unstable")
    node["locked"]["owner"] = "NixOS"
    lock = {"root": "root", "nodes": {
        "root": {"inputs": {"nixpkgs": "nixpkgs", "flake-parts": "flake-parts"}},
        "nixpkgs": nixpkgs("nixos-26.05"),
        "flake-parts": {"inputs": {"nixpkgs-lib": "nixpkgs-lib"}},
        "nixpkgs-lib": node,
    }}
    assert run(lock) == ["nixpkgs-lib (nixos-unstable) <- flake-parts"]
