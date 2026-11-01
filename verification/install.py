#!/usr/bin/env python3
"""Install version-pinned assurance tools without running remote installer scripts."""
import argparse
import hashlib
import json
import os
import platform
from pathlib import Path
import shutil
import subprocess
import tarfile
import urllib.request
import zipfile

HERE = Path(__file__).resolve().parent
PINS = json.loads((HERE / "tools.json").read_text())
BASE = Path(os.environ.get("FAIRING_ASSURANCE_TOOLS", Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache")) / "fairing-assurance"))
LAYOUT = {
    "verus": ("verus", "verus-x86-linux/verus"),
    "kani": ("kani", "kani-0.68.0/bin/kani-driver"),
    "cargo_fuzz": ("cargo_fuzz", "cargo-fuzz"),
    "cargo_cyclonedx": ("cyclonedx", "cargo-cyclonedx-x86_64-unknown-linux-gnu/cargo-cyclonedx"),
}


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def tool(name):
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        raise ValueError("The pinned assurance tool archives support Linux x86_64")
    pin = PINS[name]
    directory, executable = LAYOUT[name]
    dest = BASE / directory
    binary = dest / executable
    receipt = dest / ".fairing-tool.json"
    if receipt.exists() and binary.is_file():
        saved = json.loads(receipt.read_text())
        if saved.get("archive_sha256") == pin["sha256"] and saved.get("binary_sha256") == digest(binary):
            return binary
    archives = BASE / "archives"
    archives.mkdir(parents=True, exist_ok=True)
    archive = archives / pin["url"].rsplit("/", 1)[1]
    if not archive.exists() or digest(archive) != pin["sha256"]:
        temporary = archive.with_suffix(archive.suffix + ".download")
        with urllib.request.urlopen(pin["url"], timeout=60) as source, temporary.open("wb") as output:
            shutil.copyfileobj(source, output)
        if digest(temporary) != pin["sha256"]:
            raise ValueError(f"{name} archive checksum mismatch")
        temporary.replace(archive)
    dest.mkdir(parents=True, exist_ok=True)
    if archive.suffix == ".zip":
        with zipfile.ZipFile(archive) as compressed:
            for member in compressed.infolist():
                if member.filename.startswith("/") or ".." in Path(member.filename).parts:
                    raise ValueError("Unsafe archive path")
            compressed.extractall(dest)
            for member in compressed.infolist():
                mode = (member.external_attr >> 16) & 0o777
                if mode:
                    (dest / member.filename).chmod(mode)
    else:
        with tarfile.open(archive) as compressed:
            compressed.extractall(dest, filter="data")
    if not binary.is_file():
        raise ValueError(f"Missing executable in {name} archive")
    receipt.write_text(json.dumps({"archive_sha256": pin["sha256"], "binary_sha256": digest(binary)}) + "\n")
    print(f"Verified {name}: {pin['version']} ({pin['sha256']})", flush=True)
    return binary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("profile", choices=["proofs", "miri", "fuzz", "sbom", "all"])
    args = parser.parse_args()
    if args.profile in {"proofs", "all"}:
        subprocess.run(["rustup", "toolchain", "install", PINS["verus_toolchain"], "--profile", "minimal"], check=True)
        subprocess.run(["rustup", "toolchain", "install", PINS["kani_toolchain"], "--profile", "minimal", "--component", "rustc-dev"], check=True)
        tool("verus")
        tool("kani")
    if args.profile in {"miri", "fuzz", "all"}:
        subprocess.run(["rustup", "toolchain", "install", PINS["nightly"], "--profile", "minimal", "--component", "miri,rust-src,llvm-tools-preview"], check=True)
    if args.profile in {"fuzz", "all"}:
        tool("cargo_fuzz")
    if args.profile in {"sbom", "all"}:
        tool("cargo_cyclonedx")


if __name__ == "__main__":
    main()
