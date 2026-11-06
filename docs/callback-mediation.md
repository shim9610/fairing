# Callback mediation verification

The target is that every application callback entry passes a current check for
its required permission. The local `invoke(false, callback)` proof does not show
that every caller uses `invoke`. The current library violates the broader target:
an unauthenticated `ProtectedShell` can invoke a custom status painter that changes
app state and renders private text. This verifier detects that failure without
changing library execution paths.

## Run

```sh
python3 verification/install.py proofs
python3 verification/mediation.py --require-proved
python3 verification/mediation_controls.py
```

The pinned Kani compiler supplies `rustc_driver`; the existing verified Verus
bundle supplies Z3. Both minimal and all-feature Linux profiles of `fairing` and
`fairing-widgets` are analyzed. No new dependency is added.

`--require-proved` currently exits nonzero with `COMPLETE_MEDIATION_VIOLATED`.
The `callback-mediation` CI job preserves this failed security verdict and uploads
the evidence; it does not turn the finding into a successful security check.
Detector controls run separately even after this expected property failure.

For detector development, `--expect-counterexample` succeeds only when the detector finds the known compiler
path and reproduces it in the real library. Its success means **a violation was
detected**, not that the security property holds. `--require-proved` rejects the
current state. `--skip-build` requires matching source and inventory receipts.

The standalone concrete check exits nonzero when it reproduces the violation:

```sh
cargo run --locked --manifest-path verification/Cargo.toml --example mediation_probe
```

The probe uses synthetic app data, a headless egui context and a real
`ProtectedShell` without a token or external services. Its explicit
`--expect-counterexample` option instead validates the known failure.

## Compiler inventory and reachability

The driver enumerates the compiler's local Mid-level Intermediate Representation
(MIR) body keys and emits every
function/method/closure body: blocks, successors, call targets, operand types,
closure identities, trait identities and source positions. Non-function keys are
counted separately; totals must agree. Discovery uses compiler data, including
Fn/FnMut/FnOnce language items, project protocol traits and function pointers.

The analyzer constructs a conservative interprocedural graph with control-flow
continuations, direct calls and possible forwarding of closure arguments through
external functions such as egui and `catch_unwind`. It analyzes public-visibility
entry points and, separately, public `ProtectedShell` methods. Public visibility
can overapproximate external reachability, including methods on private types.

The dispatcher body is excluded from unmediated reachability, while the caller's
continuation remains reachable. A guarded call does not authorize a later sibling
callback. The local dispatcher contract is an explicit assumption, not a complete
application proof.

Generated finite Horn clauses (logical reachability rules) ask whether a callback site is reachable outside
that scope. Z3 computes the least fixed point without a path-length cutoff in the
finite graph. An independent traversal must agree with the solver. Every witness
must follow actual graph edges from an entry root.

Branch feasibility, closure forwarding and call/return contexts are
overapproximated. A satisfiable (`sat`) query means a **candidate**, not necessarily
a feasible Rust execution; `unsat` means no such path in that graph. Unresolved generic/virtual calls, external bodies, destructors and
unsupported MIR terminators are reported. They are never silently marked safe.
An `unsat` graph result does not discharge those boundaries or the unproved full
compiler/model correspondence. Matching the required gate to the correct session
and proving the freshness of every check are also outside this graph analysis;
it cannot certify whole-application mediation.

## Confirmed current counterexample

```text
ProtectedShell::frame_with -> Shell::frame_with -> frame callback
-> Shell::frame_inner -> Shell::draw_chrome -> draw_status_bar
-> StatusBar::ui_with -> custom painter
```

The concrete probe confirms this path: in 32 frames without a session token and
with the probe permission denied, the painter runs 32 times, changes the supplied
app state 32 times and places a synthetic private string in the output shapes.
The native observation and compiler path must agree before the report says
`violated-confirmed`. Other graph findings remain candidates.

## Evidence and controls

`target/mediation/summary.json` records verdicts, source hashes and profile counts.
Profiles retain compiler JSON/version, input receipts, solver input/logs, callback
and unresolved-boundary inventories, candidate traces and the matched painter
path. The native probe log records its actual observation.

Detector controls compile independent Rust fixtures through the same driver.
Guarded-only and unreachable callbacks must yield `unsat`; direct calls, calls
after a guarded call, function pointers and virtual methods must yield `sat`.
Tampered compiler output must be rejected. These controls test detection and do
not fix the library's bypasses.
