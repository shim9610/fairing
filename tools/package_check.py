#!/usr/bin/env python3
"""Build release archives, test their extracted contents, and run an external app.

Requires Python 3.12+, rustup and the repository's normal all-feature build inputs.
No package is published. Only the unpublished sibling crate is patched, from its
extracted archive, in temporary manifests outside the repository.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parents[1]


def registry_entries(lockfile):
    """Pinned third-party identities, excluding the sibling source substitution."""
    return {
        (p["name"], p["version"], p["source"], p.get("checksum"))
        for p in tomllib.loads(lockfile.read_text())["package"]
        if p.get("source") and p["name"] not in {"fairing", "fairing-widgets"}
    }


def check(allow_dirty=False):
    cargo = os.environ.get("CARGO", "cargo")
    env = os.environ.copy()
    # Temporary projects have no rust-toolchain.toml. Keep the root's selected
    # toolchain without installing a default or changing rustup configuration.
    toolchain = subprocess.check_output(
        ["rustup", "show", "active-toolchain"], cwd=ROOT, text=True, env=env
    ).split()[0]
    env["RUSTUP_TOOLCHAIN"] = toolchain
    host = next(line.removeprefix("host: ") for line in subprocess.check_output(
        ["rustc", "-vV"], cwd=ROOT, text=True, env=env,
    ).splitlines() if line.startswith("host: "))
    metadata = json.loads(subprocess.check_output(
        [cargo, "metadata", "--locked", "--no-deps", "--format-version", "1"],
        cwd=ROOT, text=True, env=env,
    ))
    target = Path(metadata["target_directory"])
    out = target / "package-check"
    out.mkdir(parents=True, exist_ok=True)
    env["CARGO_TARGET_DIR"] = str(target)
    # A caller's repository-guide flag must not enable excluded source files in
    # the archives. Rustdoc still denies warnings on their public API docs.
    env.pop("CARGO_ENCODED_RUSTDOCFLAGS", None)
    env["RUSTDOCFLAGS"] = "-D warnings"
    source_lock_hash = hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest()
    report = {"toolchain": toolchain, "host": host, "allow_dirty": allow_dirty,
              "source_lock_sha256": source_lock_hash,
              "archives": [], "checks": [], "complete": False}

    def run(name, args, cwd):
        command = [cargo, *map(str, args)]
        log = out / (name + ".log")
        print(f"{name}: running; log: {log}", flush=True)
        with log.open("w") as output:
            result = subprocess.run(command, cwd=cwd, env=env,
                                    stdout=output, stderr=subprocess.STDOUT)
        report["checks"].append({"name": name, "command": command,
                                 "exit_code": result.returncode, "log": str(log)})
        if result.returncode:
            raise RuntimeError(f"{name} failed ({result.returncode}); see {log}")
        print(f"{name}: passed", flush=True)

    try:
        args = ["package", "--locked", "-p", "fairing-widgets", "-p", "fairing",
                "--all-features"]
        if allow_dirty:
            args.append("--allow-dirty")
        run("package", args, ROOT)
        version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
        with tempfile.TemporaryDirectory(prefix="fairing-package-check-") as temporary:
            scratch = Path(temporary)
            for name in ["fairing-widgets", "fairing"]:
                archive = target / "package" / f"{name}-{version}.crate"
                report["archives"].append({"name": name, "version": version,
                    "sha256": hashlib.sha256(archive.read_bytes()).hexdigest()})
                with tarfile.open(archive) as packed:
                    packed.extractall(scratch, filter="data")
            widgets = scratch / f"fairing-widgets-{version}"
            shell = scratch / f"fairing-{version}"
            pinned = registry_entries(shell / "Cargo.lock")
            patch = scratch / "sibling.toml"
            patch.write_text("[patch.crates-io]\nfairing-widgets = { path = "
                             + json.dumps(str(widgets)) + " }\n")
            # Retain the archive's pinned dependency versions while resolving
            # the unpublished sibling from its extracted archive.
            run("shell-lockfile", ["metadata", "--offline", "--all-features",
                                   "--filter-platform", host, "--format-version", "1",
                                   "--manifest-path", shell / "Cargo.toml", "--config", patch], scratch)
            if not registry_entries(shell / "Cargo.lock") <= pinned:
                raise RuntimeError("sibling substitution changed pinned third-party dependencies")
            run("widgets-tests", ["test", "--offline", "--locked", "--manifest-path",
                                  widgets / "Cargo.toml"], scratch)
            run("shell-tests", ["test", "--offline", "--locked", "--all-features",
                                "--manifest-path", shell / "Cargo.toml", "--config", patch], scratch)

            consumer = scratch / "consumer"
            (consumer / "src").mkdir(parents=True)
            (consumer / "src/main.rs").write_bytes((ROOT / "tools/package_consumer.rs").read_bytes())
            features = tomllib.loads((shell / "Cargo.toml").read_text())["features"]
            forwarded = ["fairing/" + name for name in sorted(features)]
            (consumer / "Cargo.toml").write_text(
                '[package]\nname = "fairing-package-consumer"\nversion = "0.0.0"\nedition = "2021"\n'
                '\n[dependencies]\nfairing = { path = ' + json.dumps(str(shell))
                + ', default-features = false }\n'
                '\n[features]\ndefault = []\nstandard = ["fairing/default"]\nfull = '
                + json.dumps(forwarded) + '\n\n' + patch.read_text()
            )
            (consumer / "Cargo.lock").write_bytes((shell / "Cargo.lock").read_bytes())
            run("consumer-lockfile", ["metadata", "--offline", "--all-features",
                                      "--filter-platform", host, "--format-version", "1",
                                      "--manifest-path", consumer / "Cargo.toml"], scratch)
            if not registry_entries(consumer / "Cargo.lock") <= pinned:
                raise RuntimeError("consumer resolution changed pinned third-party dependencies")
            run("consumer-format", ["fmt", "--manifest-path", consumer / "Cargo.toml",
                                    "--", "--check"], scratch)
            # Assert that neither library resolves back into the source checkout.
            resolved = json.loads(subprocess.check_output(
                [cargo, "metadata", "--offline", "--locked", "--all-features",
                 "--format-version", "1", "--filter-platform", host, "--manifest-path",
                 str(consumer / "Cargo.toml")], cwd=scratch, env=env, text=True,
            ))
            if Path(resolved["workspace_root"]).resolve() != consumer.resolve():
                raise RuntimeError("consumer unexpectedly belongs to another workspace")
            local_manifests = {"fairing": shell / "Cargo.toml",
                               "fairing-widgets": widgets / "Cargo.toml",
                               "fairing-package-consumer": consumer / "Cargo.toml"}
            for package in resolved["packages"]:
                if package["source"] is None and Path(package["manifest_path"]).resolve() != local_manifests.get(package["name"]):
                    raise RuntimeError(f"unexpected local consumer dependency: {package['name']}")
            for name, expected in [("fairing", shell), ("fairing-widgets", widgets)]:
                matches = [p for p in resolved["packages"] if p["name"] == name]
                if len(matches) != 1 or Path(matches[0]["manifest_path"]).resolve() != expected / "Cargo.toml":
                    raise RuntimeError(f"consumer did not use the extracted {name} archive")
            report["consumer_uses_extracted_archives"] = True
            for profile, flags in [("minimal", ["--no-default-features"]),
                                   ("default", ["--features", "standard"]),
                                   ("all", ["--all-features"])]:
                run("consumer-" + profile, ["run", "--offline", "--locked", "--manifest-path",
                                           consumer / "Cargo.toml", *flags], scratch)
        if hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest() != source_lock_hash:
            raise RuntimeError("release check modified the repository lockfile")
        report["registry_versions_preserved"] = True
        report["complete"] = True
    finally:
        (out / "summary.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"Release package checks passed; summary: {out / 'summary.json'}", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--allow-dirty", action="store_true",
                        help="Review uncommitted local changes; CI requires a clean tree")
    args = parser.parse_args()
    try:
        check(args.allow_dirty)
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        raise SystemExit(str(error)) from error
