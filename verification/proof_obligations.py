#!/usr/bin/env python3
"""Report discharged and open obligations without promoting component proofs to a whole-app theorem."""
import argparse
import hashlib
import json
from pathlib import Path
import re

from protected_bindings import audit as audit_protected, inputs as protected_inputs
from retained_state import audit
from session_bindings import audit as audit_sessions, inputs as session_inputs

ROOT = Path(__file__).resolve().parents[1]
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--require-whole-app', action='store_true')
p.add_argument('--require-components', action='store_true', help='Require all implemented component proofs and source inventory')
a = p.parse_args()
source = ROOT / 'crates/fairing/src/shell/back_route.rs'
caller = (ROOT / 'crates/fairing/src/shell/navigation.rs').read_text()
text = source.read_text()
fields = re.findall(r'pub (\w+): bool,', text)
expected = {
 'faulted': 'self.faulted', 'prompt': 'self.prompt.is_open()',
 'lock': 'self.prompt.is_lock_screen()', 'info': 'self.desktop.info_open()',
 'overlay': '!self.overlay.is_closed()', 'keyboard': 'self.osk.is_shown()',
 'overview': 'self.workspace.is_overview_open()', 'home': 'self.workspace.is_home()',
}
if set(fields) != set(expected) or len(fields) != len(expected):
 raise SystemExit('Back state projection changed; review specification coverage before proving dispatch')
start = caller.index('let group = back_route::classify(back_route::BackState {')
end = caller.index('});', start)
projection = caller[start:end]
for field, expression in expected.items():
 if projection.count(field + ': ' + expression + ',') != 1:
  raise SystemExit('Back projection no longer binds the specified product field: ' + field)
def proved(name, paths):
 receipt_path = ROOT / ('target/assurance/proofs/' + name + '-receipt.json')
 receipt = json.loads(receipt_path.read_text()) if receipt_path.exists() else {}
 expected = {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest() for path in paths}
 return receipt.get('inputs') == expected

checked = proved('back', [source, ROOT / 'verification/kani_back.rs', ROOT / 'verification/verus/back.py'])
protocol = ROOT / 'crates/fairing-widgets/src/interaction/rules.rs'
raw_fields = re.findall(r'pub (\w+): bool,', protocol.read_text().split('pub(crate) enum Group')[0])
if len(raw_fields) != 4 or set(raw_fields) != {'present', 'enabled', 'continuous', 'active'}:
 raise SystemExit('Interaction projection changed; review the independent specification before proving operations')
interaction_checked = proved('interaction', [protocol, ROOT / 'verification/kani_interaction.rs', ROOT / 'verification/verus/interaction.py'])
concrete_checked = proved('concrete', [
 ROOT / 'crates/fairing-widgets/src/interaction/concrete.rs', protocol,
 ROOT / 'verification/kani_concrete.rs', ROOT / 'verification/verus/concrete.py',
 ROOT / 'verification/verus/interaction.py', ROOT / 'verification/state-schema.json',
])
inventory = audit()
session_bindings = audit_sessions()
session_checked = proved("session", session_inputs())
protected_bindings = audit_protected()
protected_checked = proved("protected", protected_inputs())
obligations = [
 {"id": "typed-callback-admission", "status": "discharged" if protected_checked else "open", "scope": "Exact token-mode admission predicate in Kani/Verus, actual generic callback branch in Kani with arbitrary u64 effect/payload clients; denied dispatch preserves client state and yields no callback payload. Sealed registration and positive/compile-fail API clients bind the type boundary; UI adapter source tripwires and product witnesses support this component receipt. The shared kernel and compiler confinement have separate evidence; no complete Rust/egui refinement is claimed."},
 {"id": "session-authority-decisions", "status": "discharged" if session_checked else "open", "scope": "Exact production identity/revision, observation, grant expiry, policy intersection and terminal transition bodies; arbitrary u64 values, Kani and Verus. Kani also checks the actual sequential atomic storage with two permission cells."},
 {"id": "session-terminal-induction", "status": "discharged" if session_checked else "open", "scope": "Unbounded revocation-history induction; conditional SC stable-even-interval lemma. Rust private capability APIs have positive/compile-fail client checks. Concrete atomic trace refinement is guarded/tested, not a full Rust concurrency proof."},
 {'id': 'back-dispatch-partition', 'status': 'discharged' if checked else 'open',
  'scope': 'All 256 valuations of eight Back predicates, nine disjoint groups and specified handler selection.'},
 {'id': 'retained-owner-partition', 'status': 'discharged' if interaction_checked else 'open',
  'scope': 'All 16 raw valuations of present/enabled/continuous/active, five independent disjoint groups.'},
 {'id': 'retained-owner-operations', 'status': 'discharged' if interaction_checked else 'open',
  'scope': 'All five independently specified operations in every raw protocol state: immediate specified result, no refusal of a promised operation, start and finish after arbitrary prior protocol state.'},
 {'id': 'retained-owner-lifetime', 'status': 'discharged' if interaction_checked else 'open',
  'scope': 'All pairs of u64 timestamps and both scope/fresh-press predicates; future and wrapped timestamps never revive ownership.'},
 {'id': 'concrete-retained-partition', 'status': 'discharged' if concrete_checked else 'open',
  'scope': 'Exact product Session<Capture<T>>, Pass and Lifetime types; all stored timestamps and enum variants, seven independent concrete groups, arbitrary payload type T.'},
 {'id': 'concrete-admission-refinement', 'status': 'discharged' if concrete_checked else 'open',
  'scope': 'Actual lease restoration, HoldPhase decoding and six pointer observations refine protocol admission; fresh unowned presses cannot reuse old capture; completion stays latched under arbitrary progress changes.'},
 {'id': 'retained-owner-product-partition', 'status': 'discharged' if concrete_checked else 'open',
  'scope': 'Verus: every sequence length and every joint combination of timestamped capture carriers has total unique membership. The upstream egui map-to-sequence relation is not proved.'},
 {'id': 'capture-payload-noninterference', 'status': 'discharged' if concrete_checked else 'open',
  'scope': 'Generic in both payload types: equal input ownership yields equal admission regardless of all animation/value data; actual capture application preserves the payload.'},
]
components_complete = all(item['status'] == 'discharged' for item in obligations)
# These are actual unverified boundaries, not verifier errors or discharged claims.
boundaries = [
 {'id': 'typed-callback-effects', 'scope': 'Admitted callbacks retain raw egui UI, app/backend references and arbitrary Rust effects. Host hooks, destructors, queued outputs, previously disclosed data, and unwrapped operations are trusted; no whole-program information-flow or revocation-cancellation theorem is established.'},
 {'id': 'session-external-trust-and-concurrency', 'scope': 'Authenticators are trusted to verify credentials and publish authorized permissions. Standard SC atomics, clock/scheduler and source bindings are assumptions. Loom exhausts bounded actual-publication scenarios; a universal concrete Rust atomic trace refinement and arbitrary application effects are not proved.'},
 {'id': 'complete-concrete-abstraction',
  'scope': 'Shell/workspace/gesture state, widget geometry/animation, arbitrary caller fields, and upstream egui memory do not yet have a proved joint abstraction into the component protocols.'},
 {'id': 'input-realization',
  'scope': 'Headless pointer/keyboard witnesses and production-wiring mutations test the adapter; no formal proof covers upstream hit testing, layout, clipping, z-order or focus/capture implementation.'},
 {'id': 'callback-progress',
  'scope': 'The library accepts ordinary Rust screen, painter, backend and on_back callbacks without machine-checked termination/effect contracts.'},
]
report = {'target_guarantee': 'Every reachable state is covered, and every specification-permitted interaction is always executable with its promised outcome.',
          'whole_app_status': 'incomplete', 'implemented_components_status': 'proved' if components_complete else 'incomplete',
          'obligations': obligations, 'backend_inventory': inventory, 'session_bindings': session_bindings, 'protected_bindings': protected_bindings, 'unproved_boundaries': boundaries,
          'type_inventory_scope': 'Compiler-derived named product declarations in minimal/all feature Linux builds; declarations outside the concrete-carrier/protocol models remain unproved'}
out = ROOT / 'target/assurance/proofs/state-obligations.json'
out.parent.mkdir(parents=True, exist_ok=True)
out.write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report, indent=2))
if a.require_components and not components_complete:
 raise SystemExit('MISSING_COMPONENT_EVIDENCE: rerun both verifiers on current product and specification inputs')
if a.require_whole_app:
 raise SystemExit('OPEN_PROOF_OBLIGATIONS: concrete abstraction, input realization and callback progress remain unproved')
