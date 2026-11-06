#!/usr/bin/env python3
"""Verify the production ancestor walk for arbitrary finite borrowed paths.

Only signatures and ghost contracts are added to the production body. Cx models
one stable issuer observation interval; its authority contract is an explicit
premise, not a proof of the concrete atomic implementation or narrowing policy.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'verification'))
import install

OUT = ROOT / 'target/assurance/central-mediation'


def function(text, marker):
    start = text.index(marker)
    opening = text.index('{', start)
    depth = 1
    end = opening + 1
    while depth:
        depth += (text[end] == '{') - (text[end] == '}')
        end += 1
    return text[start:end]


SUPPORT = '''use vstd::prelude::*;
verus! {
type Stamp = (u64, u64);
// Grant identities model normalized strings; no permission vocabulary is bounded.
struct Gate { ghost key: int }
struct Cx<'a> { token: bool, stamp: Option<Stamp>, grants: &'a Ghost<Set<int>> }
impl Cx<'_> {
    fn token_mode(&self) -> (r: bool) ensures r == self.token, { self.token }
    fn authority_stamp(&self) -> (r: Option<Stamp>) ensures r == self.stamp, { self.stamp }
    #[verifier::external_body]
    fn allows_gate(&self, gate: &Gate) -> (r: bool)
        ensures r == self.grants@.contains(gate.key),
    { unimplemented!() }
}
struct AuthorityPath<'a> { gate: &'a Gate, stamp: Stamp, parent: Option<&'a AuthorityPath<'a>> }
impl<'a> AuthorityPath<'a> {
    spec fn height(&self) -> nat decreases self,
    { match self.parent { None => 1, Some(p) => 1 + p.height() } }
    spec fn entries(&self) -> Set<(Stamp, int)> decreases self,
    {
        match self.parent {
            None => Set::empty().insert((self.stamp, self.gate.key)),
            Some(p) => p.entries().insert((self.stamp, self.gate.key)),
        }
    }
    spec fn valid_entry(entry: (Stamp, int), stamp: Stamp, cx: &Cx<'_>) -> bool {
        entry.0 == stamp && cx.grants@.contains(entry.1)
    }
'''


def generate(text):
    constructor = function(text, "fn new(gate: &'a Gate")
    signature_new = "fn new(gate: &'a Gate, stamp: Stamp, parent: Option<&'a Self>) -> Self"
    contract_new = """fn new(gate: &'a Gate, stamp: Stamp, parent: Option<&'a Self>) -> (result: Self)
        ensures result.entries() == match parent {
            None => Set::empty().insert((stamp, gate.key)),
            Some(p) => p.entries().insert((stamp, gate.key)),
        },
    """
    assert constructor.startswith(signature_new)
    checked_constructor = constructor.replace(signature_new, contract_new, 1)
    assert checked_constructor.replace(contract_new, signature_new, 1) == constructor
    original = function(text, 'pub(crate) fn allows(&self, cx: &Cx')
    signature = "pub(crate) fn allows(&self, cx: &Cx<'_>) -> bool"
    assert original.startswith(signature)
    contract = '''fn allows(&self, cx: &Cx<'_>) -> (allowed: bool)
        ensures allowed ==> cx.token && cx.stamp == Some(self.stamp)
            && forall |e:(Stamp,int)| #[trigger] self.entries().contains(e)
                ==> Self::valid_entry(e, self.stamp, cx),
    '''
    adapted = original.replace(signature, contract, 1)
    loop = 'while next.is_some() {'
    invariant = '''while next.is_some()
            invariant
                cx.token, cx.stamp == Some(self.stamp),
                forall |e:(Stamp,int)| #[trigger] self.entries().contains(e)
                    && !match next { Some(p) => p.entries().contains(e), None => false }
                    ==> Self::valid_entry(e, self.stamp, cx),
            decreases match next { Some(p) => p.height(), None => 0 },
        {'''
    assert adapted.count(loop) == 1
    adapted = adapted.replace(loop, invariant, 1)
    # The complete executable body must round-trip exactly. No replacement of
    # accesses, branch predicates, loop steps or returns is allowed.
    restored = adapted.replace(contract, signature, 1).replace(invariant, loop, 1)
    assert restored == original
    return SUPPORT + checked_constructor + '\n' + adapted + '\n}\n}\n', constructor + '\n' + original


def verify(source, output, verus):
    generated, original = generate(source.read_text())
    output.write_text(generated)
    command = [str(verus), '--crate-type=lib', str(output)]
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
    output.with_suffix('.log').write_text(result.stdout + result.stderr)
    return result, original, command


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, default=ROOT / 'crates/fairing/src/access/mediation.rs')
    args = parser.parse_args()
    OUT.mkdir(parents=True, exist_ok=True)
    receipt = OUT / 'path-proof.json'
    receipt.unlink(missing_ok=True)
    paths = [args.source, Path(__file__), ROOT / 'verification/tools.json']
    def hashes():
        return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
    before = hashes()
    verus = Path(os.environ['VERUS']) if 'VERUS' in os.environ else install.tool('verus')
    result, body, command = verify(args.source, OUT / 'authority-path.rs', verus)
    if result.returncode:
        raise SystemExit((result.stdout + result.stderr)[-12000:])
    controls = []
    for label, old, new in [
        ('omit-parent-at-construction', '            parent,', '            parent: None,'),
        ('skip-ancestor', 'next = path.parent;', 'next = None;'),
        ('omit-grant', 'path.stamp != self.stamp || !cx.allows_gate(path.gate)', 'path.stamp != self.stamp'),
        ('omit-ancestor-revision', 'path.stamp != self.stamp || !cx.allows_gate(path.gate)', '!cx.allows_gate(path.gate)'),
        ('omit-token-mode', '!cx.token_mode() || cx.authority_stamp() != Some(self.stamp)', 'cx.authority_stamp() != Some(self.stamp)'),
    ]:
        assert args.source.read_text().count(old) == 1
        mutant = OUT / (label + '-source.rs')
        mutant.write_text(args.source.read_text().replace(old, new, 1))
        result, _, _ = verify(mutant, OUT / (label + '.rs'), verus)
        diagnostic = result.stdout + result.stderr
        assert result.returncode and ('postcondition not satisfied' in diagnostic or 'invariant not satisfied' in diagnostic), 'Control did not fail a proof obligation: ' + label
        controls.append({'case': label, 'status': 'proof-obligation-rejected'})
    assert hashes() == before, 'Proof sources changed during verification'
    report = {'status': 'unbounded-production-path-verified', 'inputs': before,
        'production_body_sha256': hashlib.sha256(body.encode()).hexdigest(), 'command': command,
        'controls': controls, 'property': 'Successful admission checks every entry in any finite borrowed ancestor chain against one expected stamp and granted identity set, with token mode enabled.',
        'assumptions': 'Stable observation interval contract for Cx/token/grants; its grant observer is an explicit external contract over an abstract set of unbounded mathematical identities. Normalized string equality preserves identity; standard Rust borrowing/Option semantics. Concrete SC atomics, concurrent policy behavior and completeness of all library routes are separate obligations.',
        'global_mediation': 'not-proved'}
    receipt.write_text(json.dumps(report, indent=2) + '\n')
    print(report['status'], '(arbitrary finite depth; semantic mutation controls rejected)')


if __name__ == '__main__':
    main()
