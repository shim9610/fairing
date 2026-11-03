#!/usr/bin/env python3
"""Require public API/model tests to reject broken production wiring in disposable copies."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--only", action="append", choices=[
    "auth-correlation", "command-budget", "expiry-order", "ui-progress", "modal-input",
    "bounded-admission", "launch-gates", "notification-cap", "inbox-delivery",
], help="Run selected controls; omitting this option runs the complete CI suite")
args = parser.parse_args()
ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "target/assurance/controls"
if args.only:
    OUT = OUT / ("selected-" + "-".join(sorted(set(args.only))))
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
        ("expiry-order", "transitions::deadlines::expiry_preempts_backlogged_commands_at_the_inclusive_boundary",
         "crates/fairing/src/shell/background.rs",
         [("self.expire_access(now);\n        let commands_pending = self.drain_commands(app);",
           "let commands_pending = self.drain_commands(app);\n        self.expire_access(now);")],
         False, "expiry/queue ordering diverged"),
        ("auth-correlation", "transitions::regressions::replacement_and_duplicate_reply_history_matches_the_state_oracle",
         "crates/fairing/src/shell/mod.rs",
         [("if response.request.is_active(self.auth_request)", "if self.auth_request.is_some()")],
         False, "subject diverged"),
        ("command-budget", "transitions::regressions::finite_budget_separates_gate_retarget_from_reply_polling",
         "crates/fairing/src/shell/mod.rs",
         [("for _ in 0..self.commands_per_update", "for _ in 0..=self.commands_per_update")],
         False, "queue reordered/evicted"),
        ("ui-progress", "reachability::regressions::actual_ui_edges_remain_reachable_after_input_obstacles",
         "crates/fairing/src/shell/mod.rs",
         [("self.region_frame_deferred =",
           "self.region_frame_deferred |=")],
         False, "UI transition remained unreachable"),
        ("modal-input", "reachability::regressions::actual_ui_edges_remain_reachable_after_input_obstacles",
         "crates/fairing/src/shell/input_route.rs",
         [("overlay_closed && !prompt_open && !overview_open",
           "overlay_closed && !overview_open")],
         False, "UI transition remained unreachable"),
        ("bounded-admission", "api::regressions::memory_capacity_reuse_and_stale_handles",
         "crates/fairing/src/inbox/bounded.rs",
         [("if used >= self.budget.capacity", "if false")],
         False, "a rejected factory allocated/ran"),
        ("launch-gates", "api::regressions::direct_and_queued_paths_recheck_the_current_policy",
         "crates/fairing/src/access/mod.rs",
         [("self.policy.allows(&self.session.subject, gate)", "true"),
          ("self.policy.allows(&self.session.subject, &Gate::from(name))", "true")],
         False, "a revoked screen remained open"),
        ("notification-cap", "api::regressions::persistent_inputs_never_bypass_notification_cap",
         "crates/fairing/src/notify/center.rs",
         [("if self.items.len() == self.max_items", "if false")],
         False, "assertion `left == right` failed"),
        ("inbox-delivery", "schedules::modeled_concurrent_drain_preserves_each_message_and_sender_order",
         "crates/fairing/src/inbox.rs",
         [("self.rx.try_recv()", "self.rx.try_recv().and_then(|_| Err(TryRecvError::Empty))")],
         True, "assertion `left == right` failed"),
    ]
    for name, test, relative, changes, loom, marker in scenarios:
        if args.only and name not in args.only:
            continue
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
