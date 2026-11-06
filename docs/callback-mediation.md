# Callback mediation verification

The target is that every application callback entry passes a current check for
its required permission. The local `invoke(false, callback)` proof does not show
that every caller uses `invoke`. The compiler-derived detector found a real
unauthenticated custom status-painter route in `ProtectedShell`.

That route is now blocked at construction. This repair does **not** establish the
absence of other paths: complete mediation remains **not proved**, and the strict
security check still rejects that claim.

## Repairs and supported API

`ShellBuilder::build_protected` rejects 22 untyped extension families with all
features enabled (17 without `osk`/`overlay`): status/nav painters and layouts,
toast/heads-up painters and layouts, keyboard painters/layouts, shade painters/layouts,
slot/gesture/lock/unlock/recents painters, widget-painter bundles, image loaders and
procedural wallpapers. Validation runs before backend/authenticator attachment or
hook execution. Exhaustive builder/hook field matches require review when fields
are added. Both branches of nested themed wallpapers are inspected; data-only
wallpapers remain supported.

Application UI enters through `Protected<P, D>` declarations. Legacy `build`
retains unmediated host customization. Authenticators and host backends remain
trusted integrations; requiring an authenticated session before calling the
credential verifier would prevent login.

An admitted callback also used to receive shared mutable icon/widget extension
stores. It could install a callback there for later unguarded use. Protected scopes
now lend fresh callback-local stores, with no writes to the shell's stores on normal
return or unwind. Their custom icon IDs must not outlive the call. Persistent
application-owned stores may be used inside an admitted callback. Original app and
backend references remain available. Arbitrary raw egui/host effects and destructors
are outside this entry-mediation contract.

Product regressions check every rejected extension family, nested/inactive wallpaper
branches, legacy customization, and persistent painter injection after normal return
and panic. Three baseline production mutation controls remove builder validation, resource
isolation or the inactive wallpaper branch. Eight additional controls cover value
identity, child measurement admission, compact layout, authority revisions, widget
identity, screen metadata and nav placement. Each must fail its real regression test. These are repair evidence, not a global absence proof.

## Value-based dispatch architecture

The protected execution path uses application-owned enum values and their mapping to
issuer grants. `PermissionBound::required_permission` is mandatory. `add_screen`
creates the guarded adapter for external screens; `add_elements` owns a typed tree
and invokes measurement/paint after live checks. Denied children have no plan entry,
space or gap. Authority revisions and metadata identity invalidate prepared geometry.
Stable structural paths namespace widget IDs independently of visible sibling order.

This is the architecture needed for subsequent coverage/refinement proofs. The
[extension checker](permission-metadata-check.md) checks workspace metadata
getters/mappers against a restricted effect and termination contract. It does not
discharge the existing MIR graph's unresolved protocol calls or supply its missing
ownership/state model; the strict complete-mediation property remains unproved.
See [the recorded specification](permission-value-refactor.md) and
[value-based integration](session-tokens.md#typed-permission-requirements).

The [shared mediation evidence](element-mediation-proof.md) describes opaque
payload ownership, generated adapters, a compiler-derived admission cut and the
unbounded production ancestor-walk proof. Borrowed plans retain the actual leaf
receiver. The [completion specification](central-mediation-spec.md) records the
remaining connection to the whole-library guarantee.

## Run

```sh
python3 verification/install.py proofs
python3 verification/mediation.py --require-proved
python3 verification/mediation_controls.py
```

The pinned Kani compiler supplies `rustc_driver`; the verified Verus bundle supplies
Z3. Minimal and all-feature Linux profiles of `fairing` and `fairing-widgets` are
analyzed. `--require-proved` still exits nonzero with `UNPROVED_COMPLETE_MEDIATION`
after the concrete repair. The `callback-mediation` CI job preserves this verdict
and uploads its evidence. Detector controls run even after the property failure.

`--expect-counterexample` requires a matching compiler path and concrete violation;
it now fails because the known path is blocked. `--skip-build` requires matching
source and inventory receipts. The standalone regression succeeds only when the
specific unsafe builder is rejected before its callback runs:

```sh
cargo run --locked --manifest-path verification/Cargo.toml --example mediation_probe
```

It reports `blocked-at-construction`, zero callback invocations, zero app-state
writes and no private-text output. Reintroducing the original bypass yields
`counterexample-confirmed` and exit code 2. An unrelated construction error is not
accepted as a repair.

## Compiler inventory and reachability

The driver enumerates the compiler's Mid-level Intermediate Representation (MIR)
body keys and emits every function/method/closure body: blocks, successors, call
targets, operand types, closure identities, trait identities and source positions.
Non-function keys are counted separately; totals must agree. Discovery uses compiler
data, including Fn/FnMut/FnOnce language items, project protocol traits and function
pointers.

The conservative graph includes control-flow continuations, direct calls and possible
forwarding of closure arguments through external functions such as egui and
`catch_unwind`. It analyzes public-visibility roots and, separately, public
`ProtectedShell` methods. Public visibility overapproximates external reachability,
including methods on private types.

The dispatcher body is excluded from unmediated reachability while the caller's
continuation remains reachable. A guarded call does not authorize a later sibling
callback. The existing local dispatcher contract is an explicit assumption.

Finite Horn clauses ask whether a callback site is reachable outside that scope.
Z3 computes the least fixed point without a path-length cutoff. An independent
traversal must agree, and every witness must follow actual graph edges from a root.

Branch feasibility, closure forwarding and call/return contexts are overapproximated.
A satisfiable (`sat`) query is a candidate, not necessarily a feasible Rust execution;
`unsat` means no path in that graph. The model does not track the repaired constructor's
heap invariant, so it can still report a custom-painter candidate whose storage cannot
be installed through the protected API.

Unresolved generic/virtual calls, external bodies, destructors and unsupported MIR
terminators are reported. They are never silently classified safe. An `unsat` result
does not discharge those boundaries or the unproved full compiler/model correspondence.
Gate/session matching and freshness are also outside this graph analysis. The detector
currently cannot issue a positive whole-library certificate; fixing the native witness
alone must not make its strict gate green.

## Evidence and controls

`target/mediation/summary.json` records verdicts, source hashes and profile counts.
Profiles retain compiler JSON/version, input receipts, solver input/logs, callback
and unresolved-boundary inventories and candidate traces. The native probe log
records the construction rejection. Evidence from earlier source hashes is rejected.

Detector controls compile independent Rust fixtures through the same driver.
Guarded-only and unreachable callbacks yield `unsat`; direct calls, calls after a
guarded call, function pointers and virtual methods yield `sat`. Tampered inventories
and stale source receipts are rejected. These eight controls test detection, separately
from the production repair mutations.
