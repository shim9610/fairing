#!/usr/bin/env python3
"""Require public API/model tests to reject broken production wiring in disposable copies."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "target/assurance/controls"
OUT.mkdir(parents=True, exist_ok=True)
results = []

with tempfile.TemporaryDirectory(prefix="fairing-contract-controls-") as temporary:
    checkout = Path(temporary) / "source"
    shutil.copytree(ROOT, checkout, ignore=shutil.ignore_patterns(".git", "target", "__pycache__", "*.pyc"))
    def run(name, test, *, loom=False, failure=False, marker=""):
        env = dict(os.environ, CARGO_INCREMENTAL="0")
        env["CARGO_TARGET_DIR"] = str(ROOT / ("target/assurance-control-loom" if loom else "target/assurance-controls"))
        env.pop("RUSTFLAGS", None)
        if loom:
            env["RUSTFLAGS"] = "--cfg fairing_loom"
        command = ["cargo", "test", "--locked", "--manifest-path", str(checkout / "verification/Cargo.toml")]
        if loom:
            command += ["--no-default-features", "--features", "loom"]
        command += ["--lib", test, "--", "--exact"]
        path = OUT / (name + ".log")
        with path.open("w") as log:
            result = subprocess.run(command, cwd=checkout, env=env, stdout=log, stderr=subprocess.STDOUT)
        text = path.read_text()
        if failure:
            if result.returncode == 0 or "test result: FAILED" not in text or marker not in text:
                raise SystemExit(f"Broken production code was not rejected by its contract: {path}")
        elif result.returncode or "1 passed" not in text:
            raise SystemExit(f"Contract baseline failed: {path}")
        results.append({"name": name, "command": command, "exit_code": result.returncode,
                        "expected_failure": failure, "log": str(path)})
        (OUT / "summary.json").write_text(json.dumps(results, indent=2) + "\n")
        print(name + (": rejected broken production code" if failure else ": passed"), flush=True)

    scenarios = [
        ("launch-gates", "api::regressions::direct_and_queued_paths_recheck_the_current_policy",
         "crates/fairing/src/access/mod.rs",
         [("self.policy.allows(&self.session.subject, gate)", "true"),
          ("self.policy.allows(&self.session.subject, &Gate::from(name))", "true")],
         False, "a revoked screen remained open"),
        ("notification-cap", "api::regressions::persistent_to_dismissible_updates_enforce_the_notification_cap",
         "crates/fairing/src/notify/center.rs",
         [("self.enforce_cap();\n            return CenterChange::Updated;", "return CenterChange::Updated;")],
         False, "an update left dismissible notifications above the cap"),
        ("inbox-delivery", "schedules::modeled_concurrent_drain_preserves_each_message_and_sender_order",
         "crates/fairing/src/inbox.rs",
         [("self.rx.try_recv()", "self.rx.try_recv().and_then(|_| Err(TryRecvError::Empty))")],
         True, "assertion `left == right` failed"),
    ]
    for name, test, relative, changes, loom, marker in scenarios:
        path = checkout / relative
        original = path.read_text()
        run(name + "-baseline", test, loom=loom)
        changed = original
        for old, new in changes:
            if changed.count(old) != 1:
                raise SystemExit(f"Review mutation boundary in {relative}")
            changed = changed.replace(old, new, 1)
        path.write_text(changed)
        (OUT / (name + ".mutation.json")).write_text(json.dumps({"path": relative, "changes": changes}, indent=2) + "\n")
        try:
            run(name + "-negative", test, loom=loom, failure=True, marker=marker)
        finally:
            path.write_text(original)
