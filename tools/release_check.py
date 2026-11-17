#!/usr/bin/env python3
"""Resolve a release tag and require successful CI for that exact source commit."""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib


REQUIRED = {
    "audit", "msrv", "tours", "histories", "proofs", "miri", "assurance-gate",
    "Analyze (actions)", "Analyze (python)", "Analyze (rust)",
}


def validate_checks(checks):
    # GitHub returns newest runs first. A failed rerun must supersede an old pass.
    latest = {}
    for check in sorted(checks, key=lambda item: item["id"], reverse=True):
        if check.get("app", {}).get("slug") == "github-actions":
            latest.setdefault(check["name"], check)
    failed = sorted(name for name in REQUIRED if name not in latest
                    or latest[name].get("status") != "completed"
                    or latest[name].get("conclusion") != "success")
    if failed:
        raise ValueError("Release CI is missing or unsuccessful: " + ", ".join(failed))


def resolve(tag):
    if not re.fullmatch(r"v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)(?:-beta\.[1-9]\d*)?", tag):
        raise ValueError("Expected vMAJOR.MINOR.PATCH or vMAJOR.MINOR.PATCH-beta.N")

    def git(*args):
        return subprocess.check_output(["git", *args], text=True).strip()

    commit = git("rev-parse", "--verify", f"refs/tags/{tag}^{{commit}}")
    subprocess.run(["git", "merge-base", "--is-ancestor", commit, "origin/main"], check=True)

    def manifest(path):
        return tomllib.loads(git("show", f"{commit}:{path}"))

    version = manifest("Cargo.toml")["workspace"]["package"]["version"]
    if tag != f"v{version}":
        raise ValueError("Release tag does not match workspace version")
    for crate in ("fairing-widgets", "fairing"):
        package = manifest(f"crates/{crate}/Cargo.toml")["package"]
        if package["name"] != crate or package["version"] != {"workspace": True}:
            raise ValueError(f"Unexpected release package/version: {crate}")
        if package.get("publish") is False:
            raise ValueError(f"Package is not publishable: {crate}")
    sibling = manifest("crates/fairing/Cargo.toml")["dependencies"]["fairing-widgets"]
    if sibling["version"] != version:
        raise ValueError("Sibling dependency does not match release version")
    return commit


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tag")
    parser.add_argument("--expected-commit", help="Reject a changed tag after preflight")
    parser.add_argument("--checkout", action="store_true", help="Detach HEAD only after all release checks pass")
    args = parser.parse_args()
    commit = resolve(args.tag)
    if args.expected_commit and args.expected_commit != commit:
        raise ValueError("Release tag changed after preflight")
    rows = subprocess.check_output([
        "gh", "api", "--paginate", "--jq", ".check_runs[] | @json",
        f"repos/shim9610/fairing/commits/{commit}/check-runs?per_page=100",
    ], text=True)
    validate_checks([json.loads(row) for row in rows.splitlines() if row.strip()])
    if args.checkout:
        subprocess.run(["git", "checkout", "--detach", commit], check=True)
    report = {"tag": args.tag, "commit": commit, "required_checks": sorted(REQUIRED)}
    Path("release-source.json").write_text(json.dumps(report, indent=2) + "\n")
    if output := os.environ.get("GITHUB_OUTPUT"):
        with open(output, "a") as stream:
            stream.write(f"commit={commit}\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
