#!/usr/bin/env python3
"""Verify the production kernel, then require both verifiers to reject a broken acknowledgement check."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

import install

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "target/assurance/proofs"
OUT.mkdir(parents=True, exist_ok=True)


def execute(name, command, env, expected_failure=False, marker=""):
    log = OUT / (name + ".log")
    with log.open("w") as output:
        result = subprocess.run(list(map(str, command)), cwd=ROOT, env=env, stdout=output, stderr=subprocess.STDOUT)
    content = log.read_text()
    if expected_failure:
        if result.returncode == 0 or marker not in content:
            raise SystemExit(f"Negative control did not fail semantically: {log}")
    elif result.returncode:
        print(content[-12000:], file=sys.stderr)
        raise SystemExit(f"Proof failed: {log}")
    print(f"{name}: {'rejected contract violation' if expected_failure else 'verified'}", flush=True)
    return {"name": name, "exit_code": result.returncode, "expected_failure": expected_failure, "log": str(log)}


def hashes(paths):
    return {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest() for path in paths}


def receipt(name, paths, before=None):
    inputs = hashes(paths)
    if before is not None and before != inputs:
        raise SystemExit("Proof inputs changed during verification: " + name)
    (OUT / (name + "-receipt.json")).write_text(json.dumps({"inputs": inputs}, indent=2) + "\n")


verus = install.tool("verus")
kani = install.tool("kani")
environment = dict(os.environ, VERUS=str(verus))
results = [execute("verus", [sys.executable, ROOT / "verification/verus/prove.py"], environment)]
sysroot = subprocess.check_output(["rustup", "run", install.PINS["kani_toolchain"], "rustc", "--print", "sysroot"], text=True).strip()
kani_env = dict(os.environ, RUSTUP_TOOLCHAIN=install.PINS["kani_toolchain"])
kani_env["PATH"] = str(kani.parent) + os.pathsep + kani_env.get("PATH", "")
kani_env["LD_LIBRARY_PATH"] = os.pathsep.join(path for path in [str(Path(sysroot) / "lib"), kani_env.get("LD_LIBRARY_PATH", "")] if path)
results.append(execute("kani", [kani, ROOT / "verification/kani.rs"], kani_env))
results.append(execute("verus-access", [sys.executable, ROOT / "verification/verus/access.py"], environment))
results.append(execute("kani-access", [kani, ROOT / "verification/kani_access.rs"], kani_env))
results.append(execute("verus-memory", [sys.executable, ROOT / "verification/verus/memory.py"], environment))
results.append(execute("kani-memory", [kani, ROOT / "verification/kani_memory.rs"], kani_env))
results.append(execute("verus-display", [sys.executable, ROOT / "verification/verus/display.py"], environment))
results.append(execute("verus-input", [sys.executable, ROOT / "verification/verus/input.py"], environment))
results.append(execute("kani-input", [kani, ROOT / "verification/kani_input.rs"], kani_env))
back_checked_source = ROOT / "crates/fairing/src/shell/back_route.rs"
back_digest = hashlib.sha256(back_checked_source.read_bytes()).hexdigest()
results.append(execute("verus-back", [sys.executable, ROOT / "verification/verus/back.py"], environment))
results.append(execute("kani-back", [kani, ROOT / "verification/kani_back.rs"], kani_env))
if hashlib.sha256(back_checked_source.read_bytes()).hexdigest() != back_digest:
    raise SystemExit("Back source changed while proofs were running")
receipt("back", [back_checked_source, ROOT / "verification/kani_back.rs", ROOT / "verification/verus/back.py"])
interaction_source = ROOT / "crates/fairing-widgets/src/interaction/rules.rs"
interaction_digest = hashlib.sha256(interaction_source.read_bytes()).hexdigest()
results.append(execute("verus-interaction", [sys.executable, ROOT / "verification/verus/interaction.py"], environment))
results.append(execute("kani-interaction", [kani, ROOT / "verification/kani_interaction.rs"], kani_env))
if hashlib.sha256(interaction_source.read_bytes()).hexdigest() != interaction_digest:
    raise SystemExit("Interaction source changed while proofs were running")
receipt("interaction", [interaction_source, ROOT / "verification/kani_interaction.rs", ROOT / "verification/verus/interaction.py"])

concrete_source = ROOT / "crates/fairing-widgets/src/interaction/concrete.rs"
concrete_inputs = [concrete_source, interaction_source, ROOT / "verification/kani_concrete.rs",
                   ROOT / "verification/verus/concrete.py", ROOT / "verification/verus/interaction.py",
                   ROOT / "verification/state-schema.json"]
concrete_before = hashes(concrete_inputs)
results.append(execute("verus-concrete", [sys.executable, ROOT / "verification/verus/concrete.py"], environment))
results.append(execute("kani-concrete", [kani, ROOT / "verification/kani_concrete.rs"], kani_env))
receipt("concrete", concrete_inputs, concrete_before)


# Split session capabilities: exact executable decisions plus independent contracts.
from session_bindings import audit as audit_sessions, inputs as session_inputs

audit_sessions()
session_before = hashes(session_inputs())
results.append(execute("verus-session", [sys.executable, ROOT / "verification/verus/session.py"], environment))
results.append(execute("kani-session", [kani, ROOT / "verification/kani_session.rs"], kani_env))
results.append(execute("session-capability-types", ["cargo", "test", "--locked", "-p", "fairing", "--no-default-features", "--doc", "access::session::"], environment))
if "test result: ok. 4 passed;" not in (OUT / "session-capability-types.log").read_text():
    raise SystemExit("Expected one positive and three compile-fail session capability clients")
receipt("session", session_inputs(), session_before)

# Typed declarations and the exact callback branch are independently checked.
from protected_bindings import audit as audit_protected, inputs as protected_inputs

audit_protected()
protected_before = hashes(protected_inputs())
results.append(execute("verus-protected", [sys.executable, ROOT / "verification/verus/protected.py"], environment))
results.append(execute("kani-protected", [kani, ROOT / "verification/kani_protected.rs"], kani_env))
results.append(execute("protected-capability-types", ["cargo", "test", "--locked", "-p", "fairing", "--no-default-features", "--doc", "access::protected"], environment))
if "test result: ok. 5 passed;" not in (OUT / "protected-capability-types.log").read_text():
    raise SystemExit("Expected one positive and four compile-fail protected clients")
receipt("protected", protected_inputs(), protected_before)
protected_source = (ROOT / "crates/fairing/src/access/protected_dispatch.rs").read_text()
protected_harness = (ROOT / "verification/kani_protected.rs").read_text()
protected_declaration = '#[path = "../crates/fairing/src/access/protected_dispatch.rs"]\nmod protected_dispatch;'
if protected_harness.count(protected_declaration) != 1: raise SystemExit("Review protected callback binding")
for name, old, new in [
    ("protected-token-mode", "token_mode && allowed", "allowed"),
    ("protected-permission", "token_mode && allowed", "token_mode"),
    ("protected-invocation", "if allowed {", "if true || allowed {"),
]:
    if protected_source.count(old) != 1: raise SystemExit("Review protected mutation: " + name)
    mutant = protected_source.replace(old, new, 1)
    source_path = OUT / (name + "-source.rs"); source_path.write_text(mutant)
    harness_path = OUT / (name + "-kani.rs")
    harness_path.write_text(protected_harness.replace(protected_declaration, "mod protected_dispatch {\n" + mutant + "\n}"))
    results.append(execute("kani-" + name, [kani, harness_path], kani_env, expected_failure=True, marker="VERIFICATION:- FAILED"))
    if name != "protected-invocation":
        results.append(execute("verus-" + name, [sys.executable, ROOT / "verification/verus/protected.py", "--source", source_path, "--output", OUT / (name + "-verus.rs")], environment, expected_failure=True, marker="postcondition not satisfied"))

session_source = (ROOT / "crates/fairing/src/access/session_rules.rs").read_text()
session_harness = (ROOT / "verification/kani_session.rs").read_text()
session_declaration = '#[path = "../crates/fairing/src/access/session_rules.rs"]\nmod session_rules;'
shared_declaration = '#[path = "../crates/fairing/src/access/session_shared.rs"]\nmod session_shared;'
if session_harness.count(shared_declaration) != 1: raise SystemExit("Review actual session storage binding")
session_mutation_harness = session_harness.replace(shared_declaration, '#[path = ' + json.dumps(str(ROOT / 'crates/fairing/src/access/session_shared.rs')) + ']\nmod session_shared;')

if session_harness.count(session_declaration) != 1: raise SystemExit("Review session harness binding")
for name, old, new in [
    ("session-revocation", "if revoked {", "if false && revoked {"),
    ("session-expiry", "} else if now >= until {", "} else if now > until {"),
    ("session-publication", "before != after || before % 2 != 0", "before % 2 != 0"),
    ("session-live-authority", "live && (now < base_until", "true && (now < base_until"),
    ("session-temporary-expiry", "now < temporary_until", "temporary_until != 0"),
    ("session-policy", "!quarantined && issued && policy", "!quarantined && issued"),
    ("session-quarantine", "!quarantined && issued && policy", "issued && policy"),
    ("session-terminal", "if state.revoked || now >= until", "if now >= until"),
    ("session-revision", "Some(current + 2)", "Some(current)"),
    ("session-identity", "Some(current + 1)", "Some(current)"),
]:
    if session_source.count(old) != 1: raise SystemExit("Review session mutation: " + name)
    mutant = session_source.replace(old, new, 1)
    source_path = OUT / (name + "-source.rs"); source_path.write_text(mutant)
    results.append(execute("verus-" + name, [sys.executable, ROOT / "verification/verus/session.py", "--source", source_path, "--output", OUT / (name + "-verus.rs")], environment, expected_failure=True, marker="postcondition not satisfied"))
    harness_path = OUT / (name + "-kani.rs"); harness_path.write_text(session_mutation_harness.replace(session_declaration, "mod session_rules {\n" + mutant + "\n}"))
    results.append(execute("kani-" + name, [kani, harness_path], kani_env, expected_failure=True, marker="VERIFICATION:- FAILED"))

# Kani also compiles the real atomic storage. These defects leave the pure kernel
# unchanged and therefore require the concrete-storage harnesses to reject them.
shared_source = (ROOT / "crates/fairing/src/access/session_shared.rs").read_text()
for name, old, new in [
    ("session-storage-revocation", "self.revoked.store(next.revoked, Ordering::SeqCst);", "self.revoked.store(false, Ordering::SeqCst);"),
    ("session-storage-epoch", "self.revision.store(after, Ordering::SeqCst);", "self.revision.store(before, Ordering::SeqCst);"),
    ("session-storage-permission", "let temporary = cell.temporary.load(Ordering::SeqCst);", "let temporary = cell.base.load(Ordering::SeqCst);"),
]:
    if shared_source.count(old) != 1: raise SystemExit("Review concrete storage mutation: " + name)
    mutant = shared_source.replace(old, new, 1)
    harness = session_harness.replace(shared_declaration, "mod session_shared {\n" + mutant + "\n}")
    harness = harness.replace(session_declaration, '#[path = ' + json.dumps(str(ROOT / 'crates/fairing/src/access/session_rules.rs')) + ']\nmod session_rules;')
    path = OUT / (name + "-kani.rs"); path.write_text(harness)
    results.append(execute("kani-" + name, [kani, path], kani_env, expected_failure=True, marker="VERIFICATION:- FAILED"))

source = (ROOT / "crates/fairing/src/shell/bool_ack.rs").read_text()
needle = "if reported == Some(sent)"
if source.count(needle) != 1:
    raise SystemExit("Review the production acknowledgement-check mutation")
mutant = source.replace(needle, "if reported != Some(sent)", 1)
generated = (ROOT / "target/assurance/verus-bool-ack.rs").read_text()
if generated.count(needle) != 1:
    raise SystemExit("Review the Verus negative control")
verus_mutant = OUT / "verus-mutant.rs"
verus_mutant.write_text(generated.replace(needle, "if reported != Some(sent)", 1))
results.append(execute("verus-negative", [verus, "--crate-type=lib", verus_mutant], environment,
                       expected_failure=True, marker="postcondition not satisfied"))
kani_source = (ROOT / "verification/kani.rs").read_text()
declaration = '#[path = "../crates/fairing/src/shell/bool_ack.rs"]\nmod bool_ack;'
if kani_source.count(declaration) != 1:
    raise SystemExit("Review the Kani negative control")
kani_mutant = OUT / "kani-mutant.rs"
kani_mutant.write_text(kani_source.replace(declaration, "mod bool_ack {\n" + mutant + "\n}"))
results.append(execute("kani-negative", [kani, kani_mutant], kani_env,
                       expected_failure=True, marker="VERIFICATION:- FAILED"))
access_source = (ROOT / "crates/fairing/src/access/rules.rs").read_text()
access_harness = (ROOT / "verification/kani_access.rs").read_text()
declaration = '#[path = "../crates/fairing/src/access/rules.rs"]\nmod rules;'
if access_harness.count(declaration) != 1:
    raise SystemExit("Review the access mutation harness")
for name, old, new in [
    ("stale-response", "active == Some(response)", "active.is_some()"),
    ("level-order", "held >= required", "held <= required"),
    ("unknown-grant", "known\n        &&", "true\n        &&"),
    ("counter-wrap", "current.saturating_add(1)", "current.wrapping_add(1)"),
    ("lockout-boundary", "failures >= limit", "failures > limit"),
]:
    if access_source.count(old) != 1:
        raise SystemExit(f"Review the access mutation: {name}")
    mutant = access_source.replace(old, new, 1)
    source_path = OUT / (name + "-source.rs")
    source_path.write_text(mutant)
    results.append(execute("verus-" + name, [sys.executable, ROOT / "verification/verus/access.py",
                           "--source", source_path, "--output", OUT / (name + "-verus.rs")], environment,
                           expected_failure=True, marker="postcondition not satisfied"))
    harness_path = OUT / (name + "-kani.rs")
    harness_path.write_text(access_harness.replace(declaration, "mod rules {\n" + mutant + "\n}"))
    results.append(execute("kani-" + name, [kani, harness_path], kani_env,
                           expected_failure=True, marker="VERIFICATION:- FAILED"))
memory_source = (ROOT / "crates/fairing-widgets/src/memory.rs").read_text()
needle = "current.checked_add(1)"
if memory_source.count(needle) != 1:
    raise SystemExit("Review the generation negative control")
mutant = memory_source.replace(needle, "Some(current.wrapping_add(1))", 1)
# Generated inline modules keep their production child-module source path.
display_source = ROOT / "crates/fairing-widgets/src/memory/display.rs"
mutant = mutant.replace('#[path = "memory/display.rs"]\nmod display;', '#[path = ' + json.dumps(str(display_source)) + ']\nmod display;')
source_path = OUT / "memory-wrap-source.rs"
source_path.write_text(mutant)
results.append(execute("verus-memory-wrap", [sys.executable, ROOT / "verification/verus/memory.py",
                       "--source", source_path, "--output", OUT / "memory-wrap-verus.rs"], environment,
                       expected_failure=True, marker="postcondition not satisfied"))
harness = (ROOT / "verification/kani_memory.rs").read_text()
declaration = '#[path = "../crates/fairing-widgets/src/memory.rs"]\nmod memory;'
if harness.count(declaration) != 1:
    raise SystemExit("Review the generation Kani mutation adapter")
harness_path = OUT / "memory-wrap-kani.rs"
harness_path.write_text(harness.replace(declaration, "mod memory {\n" + mutant + "\n}"))
results.append(execute("kani-memory-wrap", [kani, harness_path], kani_env,
                       expected_failure=True, marker="VERIFICATION:- FAILED"))
# A viewport must select the newest bounded suffix, not the older full window.
display_text = display_source.read_text()
needle = "retained - limit"
if display_text.count(needle) != 1: raise SystemExit("Review display-clipping mutation")
display_mutant = OUT / "display-clip-source.rs"
display_mutant.write_text(display_text.replace(needle, "0", 1))
results.append(execute("verus-display-clip", [sys.executable, ROOT / "verification/verus/display.py",
                       "--source", display_mutant, "--output", OUT / "display-clip-verus.rs"], environment,
                       expected_failure=True, marker="postcondition not satisfied"))
inline = memory_source.replace('#[path = "memory/display.rs"]\nmod display;',
                               '#[path = ' + json.dumps(str(display_mutant)) + ']\nmod display;')
clip_harness = OUT / "display-clip-kani.rs"
clip_harness.write_text(harness.replace(declaration, "mod memory {\n" + inline + "\n}"))
results.append(execute("kani-display-clip", [kani, clip_harness], kani_env,
                       expected_failure=True, marker="VERIFICATION:- FAILED"))
# Reject over-deferral and modal capture in both source-bound input verifiers.
input_source = (ROOT / "crates/fairing/src/shell/input_route.rs").read_text()
input_harness = (ROOT / "verification/kani_input.rs").read_text()
declaration = '#[path = "../crates/fairing/src/shell/input_route.rs"]\nmod input_route;'
if input_harness.count(declaration) != 1:
    raise SystemExit("Review input Kani source binding")
for name, old, new in [
    ("input-route", "geometry_changed && fresh_primary_press && region_claimed", "geometry_changed || fresh_primary_press || region_claimed"),
    ("modal-capture", "overlay_closed && !prompt_open && !overview_open", "overlay_closed && !overview_open"),
]:
    if input_source.count(old) != 1:
        raise SystemExit("Review input mutation: " + name)
    mutant = input_source.replace(old, new, 1)
    input_mutant = OUT / (name + "-source.rs")
    input_mutant.write_text(mutant)
    results.append(execute("verus-" + name + "-negative", [sys.executable, ROOT / "verification/verus/input.py",
                           "--source", input_mutant, "--output", OUT / (name + "-verus.rs")], environment,
                           expected_failure=True, marker="postcondition not satisfied"))
    input_control = OUT / (name + "-kani.rs")
    input_control.write_text(input_harness.replace(declaration, "mod input_route {\n" + mutant + "\n}"))
    results.append(execute("kani-" + name + "-negative", [kani, input_control], kani_env,
                           expected_failure=True, marker="VERIFICATION:- FAILED"))
# Neither a missing state group nor dropping an allowed operation may pass.
back_source = (ROOT / "crates/fairing/src/shell/back_route.rs").read_text()
back_harness = (ROOT / "verification/kani_back.rs").read_text()
declaration = '#[path = "../crates/fairing/src/shell/back_route.rs"]\nmod back_route;'
if back_harness.count(declaration) != 1: raise SystemExit("Review Back source binding")
for name, old, new in [
    ("missing-keyboard-group", "else if s.keyboard", "else if false"),
    ("blocked-keyboard-operation", "BackGroup::Keyboard => BackTarget::HideKeyboard", "BackGroup::Keyboard => BackTarget::Ignore"),
]:
    if back_source.count(old) != 1: raise SystemExit("Review Back mutation: " + name)
    mutant = back_source.replace(old, new, 1)
    source_path = OUT / (name + "-source.rs")
    source_path.write_text(mutant)
    results.append(execute("verus-" + name, [sys.executable, ROOT / "verification/verus/back.py", "--source", source_path,
                           "--output", OUT / (name + "-verus.rs")], environment,
                           expected_failure=True, marker="postcondition not satisfied"))
    harness = OUT / (name + "-kani.rs")
    harness.write_text(back_harness.replace(declaration, "mod back_route {\n" + mutant + "\n}"))
    results.append(execute("kani-" + name, [kani, harness], kani_env,
                           expected_failure=True, marker="VERIFICATION:- FAILED"))

interaction_source_text = interaction_source.read_text()
interaction_harness = (ROOT / "verification/kani_interaction.rs").read_text()
declaration = '#[path = "../crates/fairing-widgets/src/interaction/rules.rs"]\nmod rules;'
if interaction_harness.count(declaration) != 1: raise SystemExit("Review interaction source binding")
for name, old, new in [
    ("lifetime-future", "seen == now || seen.checked_add(1) == Some(now)", "seen >= now || seen.checked_add(1) == Some(now)"),
    ("expired-owner", "else if !s.continuous", "else if false"),
    ("stale-blocks-start", "Operation::Start => s.present && s.enabled", "Operation::Start => s.present && s.enabled && s.continuous"),
    ("finish-stays-active", "active: false,\n                    effect: Effect::Completed", "active: true,\n                    effect: Effect::Completed"),
]:
    if interaction_source_text.count(old) != 1: raise SystemExit("Review lifecycle mutation: " + name)
    mutant = interaction_source_text.replace(old, new, 1)
    path = OUT / (name + "-source.rs"); path.write_text(mutant)
    results.append(execute("verus-" + name, [sys.executable, ROOT / "verification/verus/interaction.py", "--source", path,
                           "--output", OUT / (name + "-verus.rs")], environment,
                           expected_failure=True, marker="postcondition not satisfied"))
    path = OUT / (name + "-kani.rs")
    path.write_text(interaction_harness.replace(declaration, "mod rules {\n" + mutant + "\n}"))
    results.append(execute("kani-" + name, [kani, path], kani_env,
                           expected_failure=True, marker="VERIFICATION:- FAILED"))

concrete_text = concrete_source.read_text()
concrete_harness = (ROOT / "verification/kani_concrete.rs").read_text()
declaration = '#[path = "../crates/fairing-widgets/src/interaction/concrete.rs"]\nmod concrete;'
rules_declaration = '#[path = "../crates/fairing-widgets/src/interaction/rules.rs"]\nmod rules;'
if concrete_harness.count(declaration) != 1 or concrete_harness.count(rules_declaration) != 1:
    raise SystemExit("Review concrete carrier source bindings")
for name, old, new in [
    ("concrete-disabled", "if !pass.enabled", "if false"),
    ("concrete-hold-alias", "Some(HoldPhase::Armed | HoldPhase::Done)", "Some(HoldPhase::Idle | HoldPhase::Armed | HoldPhase::Done)"),
    ("concrete-fresh-boundary", "if pointer.fresh {\n        if pointer.held || pointer.clicked {\n            Operation::Start\n        } else {\n            Operation::Cancel\n        }\n    } else if pointer.stopped", "if pointer.fresh && (pointer.held || pointer.clicked) {\n        Operation::Start\n    } else if pointer.stopped"),
    ("concrete-completion-latch", "else if matches!(previous, HoldPhase::Done)", "else if false"),
    ("concrete-animation-alias", "Some(saved) => saved.captured,", "Some(_saved) => true,"),
]:
    if concrete_text.count(old) != 1: raise SystemExit("Review concrete mutation: " + name)
    mutant = concrete_text.replace(old, new, 1)
    source_path = OUT / (name + "-source.rs"); source_path.write_text(mutant)
    results.append(execute("verus-" + name, [sys.executable, ROOT / "verification/verus/concrete.py", "--source", source_path,
                           "--output", OUT / (name + "-verus.rs")], environment,
                           expected_failure=True, marker="postcondition not satisfied"))
    path = OUT / (name + "-kani.rs")
    harness = concrete_harness.replace(declaration, "mod concrete {\n" + mutant + "\n}")
    harness = harness.replace(rules_declaration, '#[path = ' + json.dumps(str(interaction_source)) + ']\nmod rules;')
    path.write_text(harness)
    results.append(execute("kani-" + name, [kani, path], kani_env,
                           expected_failure=True, marker="VERIFICATION:- FAILED"))
(OUT / "summary.json").write_text(json.dumps(results, indent=2) + "\n")

# A successful component proof must never masquerade as the requested whole-app theorem.
subprocess.run([sys.executable, ROOT / "verification/proof_obligations.py", "--require-components"], check=True)
results.append(execute("whole-app-claim-guard", [sys.executable, ROOT / "verification/proof_obligations.py", "--require-whole-app"],
                       environment, expected_failure=True, marker="OPEN_PROOF_OBLIGATIONS"))
(OUT / "summary.json").write_text(json.dumps(results, indent=2) + "\n")
