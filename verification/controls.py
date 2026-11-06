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
CONTROL_NAMES = [
    "permission-value", "element-measure", "element-layout", "element-revision", "element-identity", "screen-requirement", "screen-metadata-recheck", "nav-permission-layout",
    "protected-builder", "protected-resources", "protected-wallpaper", "protected-dispatch", "protected-erasure", "protected-factory",
    "session-policy-recheck", "session-revoke", "session-authority", "session-policy", "session-temporary", "session-detach", "configuration-validation", "back-handler", "back-projection", "auth-correlation", "command-budget", "expiry-order", "ui-progress", "modal-input",
    "bounded-admission", "launch-gates", "notification-cap", "inbox-delivery", "retained-lifetime", "numeric-owner", "hold-admission", "wheel-admission", "restore-delegation", "pointer-projection", "capture-application", "hold-completion-latch", "focus-isolation", "popup-retirement", "deadline-reply",
]
selection = parser.add_mutually_exclusive_group()
selection.add_argument("--only", action="append", choices=CONTROL_NAMES,
                       help="Run selected controls; the default runs the complete suite")
selection.add_argument("--exclude", action="append", choices=CONTROL_NAMES,
                       help="Omit controls owned by another CI job")
args = parser.parse_args()
ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "target/assurance/controls"
if args.only:
    OUT = OUT / ("selected-" + "-".join(sorted(set(args.only))))
elif args.exclude:
    OUT = OUT / ("excluded-" + "-".join(sorted(set(args.exclude))))
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
        elif test.startswith("unit/"):
            command = ["cargo", "test", "--locked", "--manifest-path", str(checkout / "Cargo.toml"),
                       "-p", "fairing", "--no-default-features", "--features", "mock,overlay,settings,osk",
                       "--lib", test.removeprefix("unit/"), "--", "--exact"]
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
        ("permission-value", "unit/access::protected::tests::protected_scope_uses_current_context_and_never_caches_admission",
         "crates/fairing/src/access/protected.rs",
         [("gate: required.gate(),", "gate: Gate::borrowed(\"results.view\"),")],
         False, "left: Some(42)"),
        ("element-measure", "unit/elements::tests::an_authorized_parent_does_not_authorize_child_measurement",
         "crates/fairing/src/elements/dispatch.rs",
         [("if !binding.allows(cx) || cx.authority_stamp() != Some(stamp)", "if cx.authority_stamp() != Some(stamp)"),
          ("Node::Leaf(element) => binding.run(cx, |cx| element.measure(available, cx))?", "Node::Leaf(element) => element.measure(available, cx)")],
         False, "denied child measurement executed through an authorized parent"),
        ("element-layout", "unit/elements::tests::nested_layout_excludes_denied_sizes_and_gaps_and_reflows_after_grant_changes",
         "crates/fairing/src/elements/dispatch.rs",
         [("if plans.is_empty() { 0.0 } else { *gap }", "if index == 0 { 0.0 } else { *gap }")],
         False, "denied children reserved layout space or gaps"),
        ("element-revision", "unit/elements::tests::publication_during_measurement_invalidates_layout_even_if_the_gate_stays_allowed",
         "crates/fairing/src/screen/cx.rs",
         [("self.access.authority_stamp()", "self.access.authority_stamp().map(|(id, _)| (id, 0))")],
         False, "publication reused geometry from an old authority revision"),
        ("element-identity", "unit/elements::tests::nested_layout_excludes_denied_sizes_and_gaps_and_reflows_after_grant_changes",
         "crates/fairing/src/elements/dispatch.rs",
         [(".id_salt(path.with(plan.binding.gate().as_str()).value())", "// stable identity deliberately removed")],
         False, "element widget identities collided"),
        ("screen-requirement", "product/permission_values/external_screens_need_no_manual_guard_and_requirement_drift_fails_closed",
         "crates/fairing/src/screen/protected.rs",
         [("body.required_permission().gate() == *binding.gate()", "true")],
         False, "changed screen declaration retained an earlier permission"),
        ("screen-metadata-recheck", "unit/screen::protected::tests::metadata_cannot_revoke_authority_and_then_admit_a_screen_callback",
         "crates/fairing/src/screen/protected.rs",
         [(".then(|| binding.run(cx, |cx| callback(body, cx)))", ".then(|| Some(callback(body, cx)))")],
         False, "metadata revocation admitted a screen callback"),
        ("nav-permission-layout", "product/permission_values/denied_custom_nav_values_do_not_reserve_cells_before_layout",
         "crates/fairing/src/chrome/nav_bar.rs",
         [("if parts.access.uses_session_tokens()", "if false && parts.access.uses_session_tokens()")],
         False, "denied custom navigation reserved a cell"),

        ("protected-builder", "product/protected_boundary/all_untyped_extension_families_are_rejected_before_execution",
         "crates/fairing/src/shell/builder.rs",
         [("self.validate_protected_hooks()?;", "// validation deliberately removed")],
         False, "accepted"),
        ("protected-resources", "unit/access::protected::tests::admitted_callbacks_cannot_install_persistent_icon_or_widget_callbacks",
         "crates/fairing/src/access/protected.rs",
         [("cx.with_protected_resources(callback)", "callback(cx)")],
         False, "callback escaped its admission scope"),
        ("protected-wallpaper", "product/protected_boundary/inactive_and_nested_wallpaper_branches_cannot_smuggle_a_painter",
         "crates/fairing/src/shell/builder.rs",
         [("wallpaper_has_callback(dark) || wallpaper_has_callback(light)", "wallpaper_has_callback(dark)")],
         False, "is_err()"),

        ("protected-dispatch", "unit/access::protected::tests::protected_scope_uses_current_context_and_never_caches_admission",
         "crates/fairing/src/access/protected_dispatch.rs",
         [("if allowed {", "if true || allowed {")],
         False, "left: Some(42)"),
        ("protected-erasure", "unit/access::protected::tests::erased_action_and_chrome_callbacks_keep_their_typed_binding",
         "crates/fairing/src/access/protected.rs",
         [("binding.run(cx, &mut body);", "body(cx);")],
         False, "erased callback lost its typed guard"),
        ("protected-factory", "unit/access::protected::tests::factory_is_guarded_and_revocation_during_construction_blocks_all_hooks",
         "crates/fairing/src/access/protected.rs",
         [("binding.run(cx, |cx| callback(&mut **body, cx))", "Some(callback(&mut **body, cx))")],
         False, "factory revocation admitted a screen callback"),
        ("session-policy-recheck", "product/session_tokens/policy_callback_revocation_is_rechecked_before_a_protected_effect",
         "crates/fairing/src/access/mod.rs",
         [("let issued = token_permission(self.token.as_ref(), self.token_now, gate.as_str());", "let issued = first;")],
         False, "policy callback revocation admitted a stale protected effect"),
        ("session-revoke", "product/session_tokens/controller_revoke_and_disposal_invalidate_all_readers_without_resurrection",
         "crates/fairing/src/access/session_shared.rs",
         [("self.revoked.store(next.revoked, Ordering::SeqCst);", "self.revoked.store(false, Ordering::SeqCst);")],
         False, "SessionReadError::Revoked"),
        ("session-authority", "product/session_tokens/token_mode_denies_legacy_bypasses_and_supports_one_level_multi_permission_sessions",
         "crates/fairing/src/access/mod.rs",
         [(".is_some_and(|snapshot| snapshot.allows_name(name))", ".is_some_and(|snapshot| snapshot.allows_name(name)) || token.is_none()")],
         False, "single-level bypass opened token mode"),
        ("session-policy", "product/session_tokens/custom_policy_cannot_add_unissued_rights_and_logout_drops_tokens_without_methods",
         "crates/fairing/src/access/session_rules.rs",
         [("!quarantined && issued && policy", "!quarantined && issued")],
         False, "local policy failed to narrow token rights"),
        ("session-temporary", "product/session_tokens/independent_permissions_and_temporary_expiry_never_restore_removed_base_rights",
         "crates/fairing/src/access/session_rules.rs",
         [("now < temporary_until", "temporary_until != 0")],
         False, "expired elevation restored old rights"),
        ("session-detach", "product/session_tokens/issuer_revocation_preempts_a_full_command_queue_and_closes_protected_content",
         "crates/fairing/src/access/mod.rs",
         [("Err(SessionReadError::Revoked | SessionReadError::Expired) => {\n                let from = self.reset(now);", "Err(SessionReadError::Revoked | SessionReadError::Expired) => {\n                let from = self.session.subject.level;")],
         False, "revoked session token was retained"),
        ("configuration-validation", "properties::parsed_configuration_satisfies_public_validation",
         "crates/fairing/src/config.rs",
         [("config.validate()?;", "// validation deliberately removed by the negative control")],
         False, "columns above 12 were accepted"),
        ("deadline-reply", "transitions::deadlines::expiry_cancels_old_replies_before_replacement_and_fresh_input",
         "crates/fairing/src/shell/background.rs",
         [("self.expire_access(now);", "if false { self.expire_access(now); }")],
         False, "expired challenge accepted a stale grant"),
        ("focus-isolation", "widget_lifecycle::histories::closed_menu_cleanup_preserves_the_other_fields_focus",
         "crates/fairing-widgets/src/interaction/mod.rs",
         [("ctx.memory_mut(|memory| memory.surrender_focus(id));",
           "ctx.memory_mut(|memory| { if let Some(focused) = memory.focused() { memory.surrender_focus(focused); } });")],
         False, "history blocked text"),
        ("popup-retirement", "widget_lifecycle::histories::outside_dismissal_consumes_one_click_and_releases_the_next",
         "crates/fairing-widgets/src/interaction/mod.rs",
         [("state.interactable = false;", "state.interactable = true;")],
         False, "dismissed popup blocked the next peer click"),
        ("wheel-admission", "widget_lifecycle::a_wheel_requires_a_fresh_drag_after_its_owner_is_interrupted",
         "crates/fairing-widgets/src/interaction/concrete.rs",
         [("Some(saved) => saved.captured,", "Some(_saved) => true,")],
         False, "an old drag changed a returned wheel"),
        ("hold-admission", "widget_lifecycle::an_old_held_pointer_cannot_arm_a_returned_button",
         "crates/fairing-widgets/src/interaction/concrete.rs",
         [("Some(HoldPhase::Armed | HoldPhase::Done)", "Some(HoldPhase::Idle | HoldPhase::Armed | HoldPhase::Done)")],
         False, "an old held pointer armed a retired button"),
        ("retained-lifetime", "widget_lifecycle::every_interruption_prefix_preserves_popup_and_peer_operations",
         "crates/fairing-widgets/src/interaction/concrete.rs",
         [("else if !rules::adjacent(saved.seen, pass.now)", "else if false")],
         False, "a retired popup revived"),
        ("restore-delegation", "widget_lifecycle::every_interruption_prefix_preserves_popup_and_peer_operations",
         "crates/fairing-widgets/src/interaction/mod.rs",
         [('select(\n                    Some(saved),\n                    Pass {\n                        now,\n                        enabled,\n                        lifetime,\n                        fresh_press,\n                    },\n                )', "Some(&saved.payload)")],
         False, "a retired popup revived"),
        ("pointer-projection", "widget_lifecycle::a_zero_duration_hold_completes_once_per_fresh_press",
         "crates/fairing-widgets/src/interaction/mod.rs",
         [("fresh, down, held:", "fresh: false, down, held:")],
         False, "zero-duration hold must complete exactly once per fresh press"),
        ("capture-application", "widget_lifecycle::a_wheel_requires_a_fresh_drag_after_its_owner_is_interrupted",
         "crates/fairing-widgets/src/widgets/wheel.rs",
         [("drum = crate::interaction::concrete::apply_capture(drum, transition);", "let _ = crate::interaction::concrete::apply_capture(drum, transition);")],
         False, "a fresh wheel drag remained blocked after interruption"),
        ("hold-completion-latch", "widget_lifecycle::a_completed_hold_cannot_refire_when_its_animation_progress_changes",
         "crates/fairing-widgets/src/interaction/concrete.rs",
         [("else if matches!(previous, HoldPhase::Done)", "else if matches!(previous, HoldPhase::Done) && full")],
         False, "a completed hold re-fired when animation progress changed"),
        ("numeric-owner", "widget_lifecycle::stale_numeric_drafts_cannot_override_authoritative_values",
         "crates/fairing-widgets/src/widgets/number_field.rs",
         [("d.seed_bits == self.value.to_bits() && d.seed == figure(*self.value, self.decimals)", "true"),
          ("draft.seed_bits == self.value.to_bits() && draft.seed == shown", "true"),
          ("d.seed_bits == self.value.to_bits() && d.seed == shown", "true")],
         False, "a stale numeric draft overwrote the authoritative value"),
        ("back-handler", "product/m7_osk_fixes/a_tap_on_empty_space_after_back_does_not_reopen",
         "crates/fairing/src/shell/navigation.rs",
         [("back_route::BackTarget::HideKeyboard => {\n                self.osk.hide();",
           "back_route::BackTarget::HideKeyboard => {")],
         False, "back did not close it"),
        ("back-projection", "product/m7_osk_fixes/a_tap_on_empty_space_after_back_does_not_reopen",
         "crates/fairing/src/shell/navigation.rs",
         [("keyboard: self.osk.is_shown(),", "keyboard: false,")],
         False, "Back must retain the active field"),
        ("expiry-order", "transitions::deadlines::expiry_preempts_backlogged_commands_at_the_inclusive_boundary",
         "crates/fairing/src/shell/background.rs",
         [("self.expire_access(now);\n        let commands_pending = self.drain_commands(app);",
           "let commands_pending = self.drain_commands(app);\n        self.expire_access(now);")],
         False, "expiry/queue ordering diverged"),
        ("auth-correlation", "transitions::regressions::replacement_and_duplicate_reply_history_matches_the_state_oracle",
         "crates/fairing/src/shell/authentication.rs",
         [("if response.request.is_active(self.auth_request)", "if self.auth_request.is_some()")],
         False, "subject diverged"),
        ("command-budget", "transitions::regressions::finite_budget_separates_gate_retarget_from_reply_polling",
         "crates/fairing/src/shell/navigation.rs",
         [("for _ in 0..self.commands_per_update", "for _ in 0..=self.commands_per_update")],
         False, "queue reordered/evicted"),
        ("ui-progress", "reachability::regressions::actual_ui_edges_remain_reachable_after_input_obstacles",
         "crates/fairing/src/shell/render.rs",
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
         [("self.policy.allows(&self.session.subject, gate)\n    }", "true\n    }"),
          ("self.policy.allows(&self.session.subject, &Gate::from(name))\n    }", "true\n    }")],
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
        if args.exclude and name in args.exclude:
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
