#!/usr/bin/env python3
"""Report discharged and open obligations without promoting component proofs to a whole-app theorem."""
import argparse
import hashlib
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--require-whole-app', action='store_true')
a = p.parse_args()
source = ROOT / 'crates/fairing/src/shell/back_route.rs'
caller = (ROOT / 'crates/fairing/src/shell/mod.rs').read_text()
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
receipt_path = ROOT / 'target/assurance/proofs/back-receipt.json'
receipt = json.loads(receipt_path.read_text()) if receipt_path.exists() else {}
checked = receipt.get('source_sha256') == hashlib.sha256(source.read_bytes()).hexdigest()
obligations = [
 {'id': 'back-dispatch-partition', 'status': 'discharged' if checked else 'open',
  'scope': 'All 256 valuations of the eight observed Back predicates, nine disjoint groups, specified handler selection. Not widget operability.'},
 {'id': 'whole-state-partition', 'status': 'open',
  'scope': 'All retained shell/widget state, egui memory/areas/focus/capture, and integrator callback state must refine a complete semantic partition.'},
 {'id': 'whole-operation-specification', 'status': 'open',
  'scope': 'Every promised public interaction needs independent preconditions/outcomes. Back dispatch does not define all UI operations.'},
 {'id': 'state-and-transition-refinement', 'status': 'open',
  'scope': 'Every real retained-state transition must preserve the abstraction relation; a matching dispatch table alone is insufficient.'},
 {'id': 'concrete-input-refinement', 'status': 'open',
  'scope': 'egui hit testing, clipping, z-order, focus and capture must realize the specified invocation, beyond finitely executed witnesses.'},
 {'id': 'termination-and-progress', 'status': 'open',
  'scope': 'Specified operations need a proved result/progress bound. Arbitrary on_back/UI callbacks have no verified termination/effect contracts.'},
]
complete = all(item['status'] == 'discharged' for item in obligations)
report = {'target_guarantee': 'Every reachable state is covered, and every specification-permitted interaction is always executable with its promised outcome.',
          'whole_app_status': 'proved' if complete else 'incomplete', 'obligations': obligations}
out = ROOT / 'target/assurance/proofs/state-obligations.json'
out.parent.mkdir(parents=True, exist_ok=True)
out.write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report, indent=2))
if a.require_whole_app and not complete:
 raise SystemExit('OPEN_PROOF_OBLIGATIONS: whole-app operability has not been proved')
