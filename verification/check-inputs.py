#!/usr/bin/env python3
"""Check tool-only lock additions against the owner's dated dependency allowlist."""
import json
from pathlib import Path
import re
import subprocess
import tomllib

root = Path(__file__).resolve().parents[1]
primary = {(p["name"], p["version"]) for p in tomllib.loads((root / "Cargo.lock").read_text())["package"]}
for forbidden in [".cargo", "vendor"]:
    if (root / "verification" / forbidden).exists():
        raise SystemExit(f"Unapproved build-input override: verification/{forbidden}")
for package in tomllib.loads((root / "verification/Cargo.lock").read_text())["package"]:
    if package.get("source") not in {None, "registry+https://github.com/rust-lang/crates.io-index"}:
        raise SystemExit(f"Unapproved lock source: {package['name']}")
    if package.get("source") and not re.fullmatch(r"[a-f0-9]{64}", package.get("checksum", "")):
        raise SystemExit(f"Missing registry checksum: {package['name']}")
approved = {}
for line in (root / "deps.allow").read_text().splitlines():
    if not line.strip() or line.lstrip().startswith("#"):
        continue
    name, version, license, reason = re.split(r"\s{2,}", line.strip(), maxsplit=3)
    if version.startswith("=") and "TODO" not in reason and re.search(r"\([^,]+, \d{4}-\d{2}-\d{2}\)", reason):
        approved[(name, version[1:])] = " ".join(license.split())
metadata = json.loads(subprocess.check_output([
    "cargo", "metadata", "--locked", "--all-features", "--format-version", "1",
    "--manifest-path", str(root / "verification/Cargo.toml"),
], text=True, cwd=root))
count = 0
for package in metadata["packages"]:
    key = package["name"], package["version"]
    source = package["source"]
    if source is None:
        expected = {"fairing": root / "crates/fairing/Cargo.toml", "fairing-widgets": root / "crates/fairing-widgets/Cargo.toml", "fairing-assurance": root / "verification/Cargo.toml"}
        if Path(package["manifest_path"]).resolve() != expected.get(package["name"]):
            raise SystemExit(f"Unapproved local dependency: {key}")
    elif source != "registry+https://github.com/rust-lang/crates.io-index":
        raise SystemExit(f"Unapproved source: {key}: {source}")
    elif key not in primary:
        if approved.get(key) != " ".join((package["license"] or "").split()):
            raise SystemExit(f"Unapproved tool-only package or licence: {key}")
        count += 1
print(f"Approved {count} tool-only packages; existing versions reuse the primary lockfile")
