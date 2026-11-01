#!/usr/bin/env python3
"""Build a pinned public Ferrocene checkout and check fairing with it."""

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
PIN = json.loads((HERE / "source.json").read_text())


def run(args, *, cwd=ROOT, env=None, capture=False):
    print("+", " ".join(map(str, args)), flush=True)
    return subprocess.run(
        list(map(str, args)), cwd=cwd, env=env, check=True,
        text=True, stdout=subprocess.PIPE if capture else None,
    ).stdout


def require_checkout(source):
    actual = run(["git", "rev-parse", "HEAD"], cwd=source, capture=True).strip()
    if actual != PIN["revision"]:
        raise ValueError(f"Unexpected Ferrocene revision: {actual}")
    run(["git", "diff", "--exit-code", "--ignore-submodules=none"], cwd=source)
    run(["git", "diff", "--cached", "--exit-code"], cwd=source)
    for relative, expected in PIN["submodules"].items():
        directory = source / relative
        actual = run(["git", "rev-parse", "HEAD"], cwd=directory, capture=True).strip()
        if actual != expected:
            raise ValueError(f"Unexpected {relative} revision: {actual}")
        run(["git", "diff", "--exit-code"], cwd=directory)
        run(["git", "diff", "--cached", "--exit-code"], cwd=directory)
    if (source / "bootstrap.toml").read_bytes() != (HERE / "bootstrap.toml").read_bytes():
        raise ValueError("bootstrap.toml differs from the checked-in build configuration")


def build(source, jobs):
    for command in ["git", "cmake", "ninja", "g++", "make", "rustup"]:
        if shutil.which(command) is None:
            raise ValueError(f"Install {command} before building Ferrocene")
    if not source.exists():
        source.mkdir(parents=True)
        run(["git", "init", "--quiet"], cwd=source)
        run(["git", "remote", "add", "origin", PIN["repository"]], cwd=source)
        run(["git", "fetch", "--depth", "1", "origin", PIN["revision"]], cwd=source)
        run(["git", "checkout", "--detach", "FETCH_HEAD"], cwd=source)
    actual = run(["git", "rev-parse", "HEAD"], cwd=source, capture=True).strip()
    if actual != PIN["revision"]:
        raise ValueError(f"Refusing to change an existing checkout at {actual}")
    run(["git", "diff", "--exit-code"], cwd=source)
    run(["git", "diff", "--cached", "--exit-code"], cwd=source)
    run(["git", "submodule", "update", "--init", "--depth", "1", *PIN["submodules"]], cwd=source)
    config = source / "bootstrap.toml"
    expected = (HERE / "bootstrap.toml").read_bytes()
    if config.exists() and config.read_bytes() != expected:
        raise ValueError(f"Refusing to replace an existing {config}")
    config.write_bytes(expected)
    require_checkout(source)
    run([sys.executable, "x.py", "build", "--jobs", str(jobs), "--stage", "1", "compiler/rustc",
         "library", "src/tools/rustdoc", "src/tools/cargo"], cwd=source)
    # Bootstrap leaves Cargo in the tools directory; include that source-built binary
    # in the linked sysroot so rustup cannot fall back to an upstream Cargo.
    host_build = source / "build" / PIN["host"]
    shutil.copy2(host_build / "stage1-tools-bin/cargo", host_build / "stage1/bin/cargo")
    verify_compiler(source)


def verify_compiler(source):
    require_checkout(source)
    stage = source / "build" / PIN["host"] / "stage1"
    rustc = stage / "bin/rustc"
    version = run([rustc, "-vV"], capture=True)
    print(version, end="")
    fields = dict(line.split(": ", 1) for line in version.splitlines() if ": " in line)
    if fields.get("commit-hash") != PIN["revision"]:
        raise ValueError("The built compiler does not identify the pinned Ferrocene source")
    if fields.get("host") != PIN["host"] or not fields.get("release", "").startswith(PIN["rust_version"]):
        raise ValueError("Unexpected compiler host or release")
    run(["rustup", "toolchain", "link", PIN["toolchain"], stage])
    return stage, version


def json_events(log):
    for line in log.read_text().splitlines():
        try:
            yield json.loads(line)
        except json.JSONDecodeError:
            continue


def validation_findings(log, cargo=False):
    findings = []
    for event in json_events(log):
        diagnostic = event.get("message", {}) if cargo else event
        code = diagnostic.get("code") or {}
        if code.get("code") == "ferrocene::unvalidated":
            findings.append(diagnostic)
    return findings


def rust_environment(stage, target):
    env = dict(os.environ, RUSTC=str(stage / "bin/rustc"), RUSTDOC=str(stage / "bin/rustdoc"),
               CARGO_TARGET_DIR=str(target), CARGO_PROFILE_DEV_DEBUG="0",
               CARGO_PROFILE_TEST_DEBUG="0", CARGO_INCREMENTAL="0")
    for key in ["RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTC_BOOTSTRAP", "RUSTFLAGS",
                "CARGO_ENCODED_RUSTFLAGS", "RUSTDOCFLAGS", "CARGO_ENCODED_RUSTDOCFLAGS"]:
        if env.get(key):
            raise ValueError(f"Unset {key} for the reproducible compatibility trial")
    return env


def coverage(source):
    stage, version = verify_compiler(source)
    tool_pin = json.loads((HERE / "coverage-tool.json").read_text())
    with urllib.request.urlopen(tool_pin["url"], timeout=60) as response:
        data = response.read()
    if hashlib.sha256(data).hexdigest() != tool_pin["sha256"]:
        raise ValueError("cargo-llvm-cov archive checksum mismatch")
    tools = source / "build/fairing-tools"
    tools.mkdir(parents=True, exist_ok=True)
    binary = tools / "cargo-llvm-cov"
    with tarfile.open(fileobj=io.BytesIO(data)) as archive:
        member = archive.getmember("cargo-llvm-cov")
        if not member.isfile():
            raise ValueError("cargo-llvm-cov archive member is not a regular file")
        binary.write_bytes(archive.extractfile(member).read())
    binary.chmod(0o755)
    actual = run([binary, "llvm-cov", "--version"], capture=True).strip()
    if actual != f"cargo-llvm-cov {tool_pin['version']}":
        raise ValueError(f"Unexpected coverage tool: {actual}")
    env = rust_environment(stage, ROOT / "target/ferrocene-coverage")
    llvm_bin = source / "build" / PIN["host"] / "llvm/bin"
    env.update(PATH=os.pathsep.join([str(stage / "bin"), str(tools), env.get("PATH", "")]),
               RUSTUP_TOOLCHAIN=PIN["toolchain"], CARGO=str(stage / "bin/cargo"),
               LLVM_COV=str(llvm_bin / "llvm-cov"), LLVM_PROFDATA=str(llvm_bin / "llvm-profdata"))
    report_dir = ROOT / "target/ferrocene-source-report"
    report_dir.mkdir(parents=True, exist_ok=True)
    result = report_dir / "coverage.json"
    command = [str(stage / "bin/cargo"), "llvm-cov", "--locked", "-p", "fairing", "-p", "fairing-widgets",
               "--all-features", "--json", "--summary-only", "--output-path", str(result)]
    print(f"Running product coverage; log: {report_dir / 'coverage.log'}", flush=True)
    with (report_dir / "coverage.log").open("w") as output:
        completed = subprocess.run(command, cwd=ROOT, env=env, stdout=output, stderr=subprocess.STDOUT)
    if completed.returncode:
        print("\n".join((report_dir / "coverage.log").read_text().splitlines()[-60:]), file=sys.stderr)
    completed.check_returncode()
    exported = json.loads(result.read_text())["data"][0]
    for package in ["fairing", "fairing-widgets"]:
        prefix = str(ROOT / "crates" / package / "src") + os.sep
        if not any(item["filename"].startswith(prefix) for item in exported["files"]):
            raise ValueError(f"Coverage contains no product source for {package}")
    totals = exported["totals"]
    if totals["lines"]["count"] == 0:
        raise ValueError("The coverage report contains no source lines")
    summary = {"tool": tool_pin, "rustc": version, "totals": totals,
               "command": command, "exit_code": completed.returncode,
               "scope": "Product unit/integration tests; line, function and region coverage; excludes doctests, branch and MC/DC"}
    (report_dir / "coverage-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(totals, indent=2), flush=True)


def check_consumer(stage, env, result_dir, report):
    libraries = {}
    for event in json_events(result_dir / "validation-boundary.log"):
        if event.get("reason") == "compiler-artifact":
            name = event.get("target", {}).get("name")
            if name in ["fairing", "fairing_widgets"]:
                for filename in event.get("filenames", []):
                    if filename.endswith(".rlib"):
                        libraries[name] = filename
    if set(libraries) != {"fairing", "fairing_widgets"}:
        raise ValueError("Cargo did not report both source-built product libraries")
    rustc = str(stage / "bin/rustc")
    common = [rustc, "--edition=2021", "--error-format=json", "--out-dir", str(result_dir)]
    control_log = result_dir / "lint-control.log"
    with control_log.open("w") as output:
        control = subprocess.run([*common, "--crate-name", "lint_control", str(HERE / "lint-control.rs")], cwd=ROOT,
                                 env=env, stdout=output, stderr=subprocess.STDOUT)
    if control.returncode != 1 or not validation_findings(control_log):
        raise ValueError("The negative control did not reject an unvalidated call")
    report["lint_negative_control"] = "rejected an unvalidated call as expected"
    command = [*common, str(HERE / "consumer.rs"), "-W", "ferrocene::unvalidated"]
    for directory in sorted({str(Path(library).parent) for library in libraries.values()}):
        command += ["-L", f"dependency={directory}"]
    for name, library in libraries.items():
        command += ["--extern", f"{name}={library}"]
    log = result_dir / "consumer.log"
    with log.open("w") as output:
        consumer = subprocess.run(command, cwd=ROOT, env=env, stdout=output, stderr=subprocess.STDOUT)
    findings = validation_findings(log)
    (result_dir / "consumer-findings.json").write_text(json.dumps(findings, indent=2) + "\n")
    report["checks"].append({"name": "consumer", "command": command, "exit_code": consumer.returncode})
    report["consumer_validation_boundary_findings"] = len(findings)
    if consumer.returncode:
        (result_dir / "summary.json").write_text(json.dumps(report, indent=2) + "\n")
        raise subprocess.CalledProcessError(consumer.returncode, command)
    command = [str(result_dir / "consumer")]
    with (result_dir / "consumer-run.log").open("w") as output:
        executed = subprocess.run(command, cwd=ROOT, env=env, stdout=output, stderr=subprocess.STDOUT)
    report["checks"].append({"name": "consumer-run", "command": command, "exit_code": executed.returncode})
    report["complete"] = executed.returncode == 0
    (result_dir / "summary.json").write_text(json.dumps(report, indent=2) + "\n")
    executed.check_returncode()
    print(f"Consumer compiled and ran; {len(findings)} validation-boundary diagnostics", flush=True)


def check(source):
    stage, version = verify_compiler(source)
    cargo = stage / "bin/cargo"
    cargo_version = run([cargo, "-Vv"], capture=True)
    print(cargo_version, end="")
    cargo_fields = dict(line.split(": ", 1) for line in cargo_version.splitlines() if ": " in line)
    if cargo_fields.get("commit-hash") != PIN["submodules"]["src/tools/cargo"]:
        raise ValueError("The Cargo binary does not identify the pinned Cargo source")
    result_dir = ROOT / "target/ferrocene-source-report"
    result_dir.mkdir(parents=True, exist_ok=True)
    # An inherited wrapper or flags could silently test a different compiler or hide lint findings.
    env = rust_environment(stage, ROOT / "target/ferrocene-source")
    report = {"source": PIN, "rustc": version, "cargo": cargo_version,
              "fairing_revision": run(["git", "rev-parse", "HEAD"], capture=True).strip(),
              "fairing_tree": run(["git", "rev-parse", "HEAD^{tree}"], capture=True).strip(),
              "fairing_tracked_changes": bool(run(["git", "status", "--porcelain", "--untracked-files=no"], capture=True).strip()),
              "bootstrap_sha256": hashlib.sha256((HERE / "bootstrap.toml").read_bytes()).hexdigest(),
              "qualified_distribution": False, "complete": False, "checks": []}
    phases = [
        ("all-targets", ["check", "--workspace", "--all-targets", "--all-features"]),
        ("tests", ["test", "--workspace", "--all-features"]),
        ("minimal", ["check", "-p", "fairing", "--no-default-features", "--features", "mock", "--all-targets"]),
        # This must generate code: part of this lint runs after monomorphization.
        ("validation-boundary", ["rustc", "-p", "fairing", "--lib", "--all-features",
                                 "--message-format=json", "--", "-W", "ferrocene::unvalidated"]),
    ]
    for name, args in phases:
        command = [str(cargo), args[0], "--locked", *args[1:]]
        log = result_dir / f"{name}.log"
        print(f"Running {name}; log: {log}", flush=True)
        with log.open("w") as output:
            completed = subprocess.run(command, cwd=ROOT, env=env, stdout=output, stderr=subprocess.STDOUT)
        report["checks"].append({"name": name, "command": command, "exit_code": completed.returncode})
        if name == "validation-boundary":
            findings = validation_findings(log, cargo=True)
            (result_dir / "validation-findings.json").write_text(json.dumps(findings, indent=2) + "\n")
            report["validation_boundary_findings"] = len(findings)
            print(f"Validation-boundary diagnostics: {len(findings)} (inventory, not a qualification gate)")
        (result_dir / "summary.json").write_text(json.dumps(report, indent=2) + "\n")
        if completed.returncode:
            print("\n".join(log.read_text().splitlines()[-60:]), file=sys.stderr)
            raise subprocess.CalledProcessError(completed.returncode, command)
        print(f"Passed {name}", flush=True)
    check_consumer(stage, env, result_dir, report)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["build", "check", "coverage"])
    parser.add_argument("--jobs", type=int, default=4, help="Compiler build parallelism (default: 4)")
    cache = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
    parser.add_argument("--source-dir", type=Path, default=cache / "fairing/ferrocene" / PIN["revision"])
    args = parser.parse_args()
    if args.jobs < 1:
        parser.error("--jobs must be positive")
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("This trial supports x86_64 Linux only")
    source = args.source_dir.resolve()
    if source == ROOT or ROOT in source.parents:
        parser.error("Keep the compiler checkout outside the fairing repository")
    try:
        if args.action == "build":
            build(source, args.jobs)
        elif args.action == "check":
            check(source)
        else:
            coverage(source)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Ferrocene trial failed: {error}\n")


if __name__ == "__main__":
    main()
