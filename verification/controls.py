#!/usr/bin/env python3
"""Require public API/model tests to reject broken production wiring in disposable copies."""
import argparse
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--only", action="append", choices=[
    "back-handler", "back-projection", "auth-correlation", "command-budget", "expiry-order", "ui-progress", "modal-input",
    "bounded-admission", "launch-gates", "notification-cap", "inbox-delivery", "retained-lifetime", "numeric-owner", "hold-admission", "wheel-admission",
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
    # copytree preserves original timestamps. A cached mutant can be newer than a
    # fresh source copy, so force product/harness freshness before the first baseline.
    # Keep dependency caches; mutation writes/restoration refresh subsequent source mtimes.
    for tree in [checkout / "crates", checkout / "verification"]:
        for source in tree.rglob("*.rs"):
            source.touch()
    def run(name, test, *, loom=False, failure=False, marker=""):
        env = dict(os.environ, CARGO_INCREMENTAL="0")
        env["CARGO_TARGET_DIR"] = str(ROOT / ("target/assurance-control-loom" if loom else "target/assurance-controls"))
        env.pop("RUSTFLAGS", None)
        if loom:
            env["RUSTFLAGS"] = "--cfg fairing_loom"
        command = ["cargo", "test", "--locked", "--manifest-path", str(checkout / "verification/Cargo.toml")]
        if loom:
            command += ["--no-default-features", "--features", "loom"]
        if test.startswith("product/"):
            _, integration, test_name = test.split("/")
            command = ["cargo", "test", "--locked", "--manifest-path", str(checkout / "Cargo.toml"),
                       "-p", "fairing", "--no-default-features", "--features", "mock,overlay,settings,osk",
                       "--test", integration, test_name, "--", "--exact"]
        else:
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
        ("wheel-admission", "widget_lifecycle::a_wheel_requires_a_fresh_drag_after_its_owner_is_interrupted",
         "crates/fairing-widgets/src/widgets/wheel.rs",
         [("active: drum.captured", "active: true")],
         False, "an old drag changed a returned wheel"),
        ("hold-admission", "widget_lifecycle::an_old_held_pointer_cannot_arm_a_returned_button",
         "crates/fairing-widgets/src/widgets/button.rs",
         [("active: matches!(state, Some(RING_ARMED | RING_DONE))", "active: state.is_some()")],
         False, "an old held pointer armed a retired button"),
        ("retained-lifetime", "widget_lifecycle::every_interruption_prefix_preserves_popup_and_peer_operations",
         "crates/fairing-widgets/src/interaction/mod.rs",
         [("let continuous = rules::live(\n                    saved.seen,\n                    now,\n                    matches!(lifetime, Lifetime::Press),\n                    fresh_press,\n                );", "let continuous = true;")],
         False, "a retired popup revived"),
        ("numeric-owner", "widget_lifecycle::stale_numeric_drafts_cannot_override_authoritative_values",
         "crates/fairing-widgets/src/widgets/number_field.rs",
         [("d.seed_bits == self.value.to_bits() && d.seed == figure(*self.value, self.decimals)", "true"),
          ("draft.seed_bits == self.value.to_bits() && draft.seed == shown", "true"),
          ("d.seed_bits == self.value.to_bits() && d.seed == shown", "true")],
         False, "a stale numeric draft overwrote the authoritative value"),
        ("back-handler", "product/m7_osk_fixes/a_tap_on_empty_space_after_back_does_not_reopen",
         "crates/fairing/src/shell/mod.rs",
         [("back_route::BackTarget::HideKeyboard => {\n                self.osk.hide();",
           "back_route::BackTarget::HideKeyboard => {")],
         False, "back did not close it"),
        ("back-projection", "product/m7_osk_fixes/a_tap_on_empty_space_after_back_does_not_reopen",
         "crates/fairing/src/shell/mod.rs",
         [("keyboard: self.osk.is_shown(),", "keyboard: false,")],
         False, "Back must retain the active field"),
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
            pattern = re.compile(r"\s+".join(re.escape(token) for token in old.split()))
            matches = list(pattern.finditer(changed))
            if len(matches) != 1:
                raise SystemExit(f"Review mutation boundary in {relative}")
            match = matches[0]
            changed = changed[:match.start()] + new + changed[match.end():]
        path.write_text(changed)
        (OUT / (name + ".mutation.json")).write_text(json.dumps({"path": relative, "changes": changes}, indent=2) + "\n")
        try:
            run(name + "-negative", test, loom=loom, failure=True, marker=marker)
        finally:
            path.write_text(original)
