# Element entry mediation evidence

The property has three distinct parts: a callback must pass the library's guard,
the guard must check that receiver's permission, and the authority observation
must still be valid when the callback begins. A proof of `invoke` alone establishes
none of the caller coverage. These checks connect the new element path to those
obligations without promoting it to the whole-library theorem.

## Compiler coverage and unbounded path induction

`verification/element_coverage.py` consumes the compiler inventories and source
receipts produced by the [mediation detector](callback-mediation.md). In both Linux
feature profiles it discovers every compiled direct library call to `Element::measure`
and `Element::paint`. Each must occur in an acyclic closure containing exactly one
such entry. That closure must have one construction, no copy or storage escape,
and one consuming call to `PermissionBinding::run`. Observed method-item references
and indirect calls whose argument types expose an Element trait object are rejected. Generic/external forwarding not represented
as an Element call remains an open compiler-model boundary.

Each callback closure's control-flow blocks form a region. The checker builds
these regions and guard call sites in the actual inventory graph. No public
non-closure root may be inside a region, and every edge entering a region from
outside must originate at the checked guard. It checks this cut
against all emitted graph edges, including recursive and closure-forwarding edges.

`verification/verus/element_coverage.rs` proves by induction that a path avoiding
those guards cannot enter the callback regions. The two lemmas quantify over any
finite path length; there is no recursion-depth cutoff in this graph theorem.
The checked graph instance supplies their root and cut premises. Removing the cut,
root or guard-avoidance premise must fail Verus as a semantic negative control.

Nine actual compiled Rust fixtures exercise the coverage checker: one guarded
client is accepted; direct entry, entry after a guard, foreign forwarding, closure
storage, an indirect function parameter, a function-item route, two entries under
one guard and a repeating entry loop are rejected. These fixtures test caller coverage, not whether a same-named test guard authorizes
correctly.

This establishes entry containment in the extracted graph. The guard's permission
semantics, omitted dynamic/external edges and full compiler-to-program refinement
are separate obligations. It does not prove every callback family in Fairing is
mediated.

## Production traversal under explicit contracts

`verification/element_proofs.py` prepares Kani clients from the production
`ElementTree`, recursive dispatch, complete `PermissionBinding` definitions and
session transition rules. Production function bodies are preserved. The adapters
provide explicit contracts for the surrounding authority, collection and UI APIs:

- Authority is sequential and exposes independent issuer and policy masks, a live
  mode/state and a session identity/revision. Callback effects may revoke, publish
  replacement grants or replace the session. Production session transition and
  identity decisions are reused. Atomic concurrency, clock/scheduler behavior and
  the concrete `Cx` implementation require their own refinement evidence.
- A width-two arena sequence provides the used ordered `Vec` operations. Capacity
  overflow is an assertion failure, not silently truncated work. Allocation failure
  and destructor execution are outside this contract. This does not verify the
  standard allocator or the concrete `Vec` representation.
- The UI adapter records root allocation and supplied rectangles. It does not
  implement egui hashing, clipping, rasterization or input realization.

Each dynamic test element checks its **own** required permission at callback entry,
independently of the production binding. Paint additionally requires a recorded
measurement from the current session identity/revision. The source binding's
`allows/run` bodies and the actual tree control flow decide whether those oracles
are reached; the harness never assumes a callback was authorized.

Six harnesses cover the sequence adapter; a complete leaf traversal with arbitrary
modeled authority/metadata changes; retaining a tree across frames after revocation;
a denied nested tree; publication during measurement; and requirement drift when
both values are granted. The single-leaf proof checks both authorization safety and
whether admitted, stable content is actually measured, painted and allocated.

Fixed nested cases use at most two leaves and two row containers. The permission
domain has three distinct values. Kani's unwinding assertions remain enabled;
container loop bounds are discovered separately from recursive call depth. General
recursive receiver/authority refinement, sibling changes during paint and compact geometry still require
proofs beyond the existing product/mutation regressions. The unbounded graph cut
above covers entry routing, not those state properties.

## Run and evidence

```sh
python3 verification/install.py proofs
# Build current compiler inventories. Its strict global verdict remains nonzero.
python3 verification/mediation.py --require-proved
python3 verification/element_coverage.py
python3 verification/element_coverage_controls.py
python3 verification/element_proofs.py
python3 verification/element_proof_controls.py
```

`target/assurance/elements/coverage.json` records source hashes, the actual graph
cut and compiler inventories, the Verus command and negative controls. Compiler
fixtures retain their logs and summary under `coverage-controls/`. Successful
bounded traversal checking writes `receipt.json` with its inputs, bounds and
contract assumptions; `kani.log` retains individual proof results.
`element_proof_controls.py` mutates the real permission binding and pre-allocation
validation. A negative control counts only when the named security assertion has
`FAILURE` status; an unrelated unwind failure or an undetermined assertion does
not count as a counterexample. Old receipts are removed before checking so a failed rerun cannot leave an apparent fresh pass.

The `element-traversal` CI job runs the bounded clients and production mutations.
The `callback-mediation` CI job runs the coverage evidence after its strict check,
even when that global check fails. This preserves the existing
`UNPROVED_COMPLETE_MEDIATION` result. The next refinement work must connect
receiver/binding identity and live authority for arbitrary trees and extend
coverage to the remaining screen/action/chrome/tile and host boundaries.
