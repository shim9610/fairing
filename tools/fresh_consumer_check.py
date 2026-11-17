#!/usr/bin/env python3
"""Check fresh registry resolution on the declared MSRV using extracted archives.

Install that toolchain and the pinned cargo-deny first. No package is published;
only the unpublished sibling is patched into the temporary consumer.
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


def check(allow_dirty=False):
    package = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]
    version, msrv = package["version"], package["rust-version"]
    env = dict(os.environ, RUSTUP_TOOLCHAIN=msrv, CARGO_INCREMENTAL="0")
    env.pop("CARGO_ENCODED_RUSTDOCFLAGS", None)
    env["RUSTDOCFLAGS"] = "-D warnings"
    cargo = os.environ.get("CARGO", "cargo")
    metadata = json.loads(subprocess.check_output(
        [cargo, "metadata", "--locked", "--no-deps", "--format-version", "1"],
        cwd=ROOT, env=env, text=True))
    target = Path(metadata["target_directory"])
    out = target / "fresh-consumer-check"
    out.mkdir(parents=True, exist_ok=True)
    env["CARGO_TARGET_DIR"] = str(target)
    original_lock = (ROOT / "Cargo.lock").read_bytes()
    report = {"msrv": msrv, "allow_dirty": allow_dirty, "complete": False,
              "source_lock_sha256": hashlib.sha256(original_lock).hexdigest(), "checks": []}

    def run(name, args, cwd):
        command = [cargo, *map(str, args)]
        log = out / (name + ".log")
        print(f"{name}: running; log: {log}", flush=True)
        with log.open("w") as stream:
            result = subprocess.run(command, cwd=cwd, env=env, stdout=stream, stderr=subprocess.STDOUT)
        report["checks"].append({"name": name, "command": command, "exit_code": result.returncode})
        if result.returncode:
            raise RuntimeError(f"{name} failed ({result.returncode}); see {log}")
        print(f"{name}: passed", flush=True)

    try:
        report["rustc"] = subprocess.check_output(["rustc", "-vV"], cwd=ROOT, env=env, text=True)
        args = ["package", "--locked", "--no-verify", "--all-features", "-p", "fairing-widgets", "-p", "fairing"]
        if allow_dirty:
            args.append("--allow-dirty")
        run("archives", args, ROOT)
        with tempfile.TemporaryDirectory(prefix="fairing-fresh-consumer-") as tmp:
            scratch = Path(tmp)
            manifests = {}
            report["archives"] = {}
            for name in ["fairing-widgets", "fairing"]:
                archive = target / "package" / f"{name}-{version}.crate"
                report["archives"][name] = hashlib.sha256(archive.read_bytes()).hexdigest()
                with tarfile.open(archive) as packed:
                    packed.extractall(scratch, filter="data")
                manifests[name] = scratch / f"{name}-{version}" / "Cargo.toml"
            features = tomllib.loads(manifests["fairing"].read_text())["features"]
            consumer = scratch / "consumer"
            (consumer / "src").mkdir(parents=True)
            (consumer / "src/main.rs").write_bytes((ROOT / "tools/package_consumer.rs").read_bytes())
            (consumer / "Cargo.toml").write_text(
                '[package]\nname = "fairing-fresh-consumer"\nversion = "0.0.0"\nedition = "2021"\n'
                f'rust-version = "{msrv}"\nlicense = "MIT"\n\n[dependencies]\n'
                f'fairing = {{ version = "{version}", path = ' + json.dumps(str(manifests["fairing"].parent))
                + ', default-features = false }\n\n[features]\ndefault = []\nstandard = ["fairing/default"]\nfull = '
                + json.dumps(["fairing/" + key for key in sorted(features)])
                + '\n\n[patch.crates-io]\nfairing-widgets = { path = '
                + json.dumps(str(manifests["fairing-widgets"].parent)) + ' }\n')
            # A consumer starts without a lockfile; registry dependencies resolve online.
            if (consumer / "Cargo.lock").exists():
                raise RuntimeError("fresh consumer unexpectedly has a lockfile")
            run("fresh-resolution", ["generate-lockfile", "--manifest-path", consumer / "Cargo.toml"], scratch)
            (out / "Cargo.lock").write_bytes((consumer / "Cargo.lock").read_bytes())
            resolved = json.loads(subprocess.check_output(
                [cargo, "metadata", "--locked", "--all-features", "--format-version", "1",
                 "--manifest-path", str(consumer / "Cargo.toml")], cwd=scratch, env=env, text=True))
            if Path(resolved["workspace_root"]).resolve() != consumer.resolve():
                raise RuntimeError("fresh consumer unexpectedly belongs to another workspace")
            expected = {**manifests, "fairing-fresh-consumer": consumer / "Cargo.toml"}
            for package in resolved["packages"]:
                if package["source"] is None and Path(package["manifest_path"]).resolve() != expected.get(package["name"]):
                    raise RuntimeError(f"unexpected local dependency: {package['name']}")
            for name, manifest in manifests.items():
                matches = [p for p in resolved["packages"] if p["name"] == name]
                if len(matches) != 1 or Path(matches[0]["manifest_path"]).resolve() != manifest:
                    raise RuntimeError(f"consumer did not use the extracted {name} archive")
            (out / "metadata.json").write_text(json.dumps(resolved, indent=2))
            for profile, flags in [("minimal", ["--no-default-features"]),
                                   ("default", ["--features", "standard"]),
                                   ("all", ["--all-features"])]:
                run("msrv-" + profile, ["run", "--locked", "--manifest-path", consumer / "Cargo.toml", *flags], scratch)
            run("dependency-policy", ["deny", "--manifest-path", consumer / "Cargo.toml",
                "--config", ROOT / "deny.toml", "--all-features", "--locked", "check"], scratch)
            report["consumer_uses_extracted_archives"] = True
        if (ROOT / "Cargo.lock").read_bytes() != original_lock:
            raise RuntimeError("fresh resolution modified the repository lockfile")
        report["complete"] = True
    finally:
        (out / "summary.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"Fresh consumer checks passed; summary: {out / 'summary.json'}", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--allow-dirty", action="store_true")
    args = parser.parse_args()
    try:
        check(args.allow_dirty)
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        raise SystemExit(str(error)) from error
