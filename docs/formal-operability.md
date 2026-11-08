# Formal operability obligations

The target theorem is stronger than a regression campaign:

1. Every reachable concrete state belongs to exactly one specified state group.
2. Each group has an independently defined set of promised interactions.
3. Every valid invocation of a promised interaction reaches its specified outcome;
   previous interaction history cannot leave a hidden blocker.

**This whole-application theorem is not proved.** The executable status report is
`target/assurance/proofs/state-obligations.json`. The implemented components have their own required evidence gate. A green component-proof job does
not discharge the remaining concrete/UI boundaries. Run
`python3 verification/proof_obligations.py --require-whole-app` to require this
stronger claim; it currently fails with `OPEN_PROOF_OBLIGATIONS`.

The [issuer-controlled session component](session-tokens.md#security-evidence-and-assumptions)
also has exact decision proofs, terminal-history induction and a conditional sequentially consistent (SC) trace
lemma. Its concrete atomic implementation has bounded Loom evidence and source tripwires;
this does not discharge the whole-application boundaries below.

The [compiler-derived callback mediation verifier](callback-mediation.md) now
checks for paths outside the local dispatcher. Its confirmed unauthenticated
custom-painter route is now blocked by protected-builder validation. Untyped hook
families are rejected, and protected callbacks cannot persist icon/widget painters
in shared shell stores. Regression and mutation controls cover those repairs.
The [common mediation gate](protected-contracts.md) proves the shared monitor
and automatically checks all library-managed protected payload entries. The broad
legacy diagnostic still reports conservative candidates outside that certificate.
This routing result does not discharge the whole-application operability theorem above.

## Statement and required proof order

Let `S` include the shell, widgets, egui memory/areas/focus/capture and application
callback state, not only visible screen names. Let `R` be reachable states and
`C_g(s)` be the specification's group predicates. Coverage/disjointness requires:

`for every s in R: exactly one g satisfies C_g(s)`.

Define `Promised(g, a)` from the operation specification. It must not be inferred
from implementation refusal, missing hit targets or a stale disabled flag. Otherwise
an inaccessible control could simply be declared unavailable, making the theorem
vacuous. Every declared operation and supported variant must be accounted for.

Next prove that every concrete transition preserves the abstraction relation and
that the renderer/input implementation realizes the abstract invocation. Finally,
prove the operation's outcome or progress bound for every permitted invocation.
Input delivery, frame opportunities and required external response contracts must
be explicit. An internal stale blocker cannot be excluded as an environment failure.

Current headless pointer witnesses exercise the concrete correspondence on finite
configurations. They do not replace this refinement proof. Likewise, modeling a
callback as immediately returning assumes termination; the library accepts ordinary
Rust callbacks and has not proved their termination or effects.

## Discharged component: typed callback admission

Typed application declarations require `Protected<P, D>`, whose private binding
stores the application value and its normalized grant identifier. `build_protected`
requires token mode and exposes no mutable legacy registration. Let `A` be the current token/policy permission observation.
The production admission predicate satisfies `entry = token_mode && A`.
The actual generic dispatch branch returns `None` without invoking its body when
entry is false. Kani checks that branch with arbitrary u64 effect/payload clients,
and Verus checks the exact admission predicate. Compiler clients establish the
registration/type boundary, including mandatory external element metadata; source
tripwires and product witnesses cover erased screen/action/chrome/tile adapters and the post-factory recheck. The common mediation gate also consumes source-bound
kernel proofs and compiler-discovered carrier, declaration and routing contracts.

The resulting component contract is absence of a new callback invocation through
denied protected dispatch. It does not classify arbitrary app data, erase prior
outputs, cancel work already admitted or prove every raw UI/backend call mediated.
Admitted callbacks retain their original UI/app/backend context. The protected
adapters and the complete renderer do not have a full Rust/egui refinement proof.
See [typed requirements](session-tokens.md#typed-permission-requirements).

## Discharged component: Back dispatch partition

`Shell::back_in` reads an explicit `BackState` and calls the exact production
classifier/dispatcher in `shell/back_route.rs`. It retains the original priority:

| Group | Independent membership condition after higher priorities are excluded | Back dispatch contract |
| --- | --- | --- |
| Faulted | Fault quarantine is set | No normal dispatch |
| Locked | Prompt open and lock purpose | Back intentionally ignored |
| Unlock | Prompt open and unlock purpose | Cancel prompt |
| Info | Icon information popover open | Close popover |
| Overlay | Overlay not closed | Request overlay close |
| Keyboard | Keyboard shown | Request keyboard hide |
| Overview | Overview open | Request overview close |
| Home | Workspace at home | Back intentionally ignored |
| Workspace | Remaining operational cases | Invoke workspace Back handling |

These are **nine groups of the Back projection**, not nine groups covering the
entire application's interaction semantics. The raw projection has eight independent
Boolean predicates. Kani covers all 256 valuations, including inconsistent/dominated
combinations, rather than assuming away inconvenient overlaps. It proves:

- Exactly one independent specification predicate holds and the classifier returns it.
- Every specification-permitted Back selects the specified handler, never Ignore.
- Once higher owners are absent, arbitrary old predicate values cannot latch the
  classifier into a previous owner.

Verus checks the unchanged executable classifier/dispatcher bodies against independent
membership and target specifications. Its classifier contract also proves uniqueness.
The enums have exhaustive matches; a new group cannot silently fall through a
wildcard. The coverage audit checks the complete field set and the actual product
field bindings. The source receipt prevents reporting an old proof for modified code.

Both verifiers must reject removal of the keyboard group and refusal of its promised
Back operation. Native controls separately break the live keyboard predicate and
omit the actual hide call; a real field/keyboard test must reject each mutation.

This proves handler selection. It does not prove animation completion, a pointer's
arrival at the Back control, or arbitrary `Screen::on_back` behavior. A callback can
consume Back by design; its own promised outcome requires a separate contract.

## Discharged component: retained interaction ownership

`fairing-widgets/src/interaction/rules.rs` is the production lifecycle kernel.
Its four raw predicates are independent: `present`, `enabled`, `continuous` and
`active`. No reachable-state assumption removes an inconvenient valuation.

| Group | Independent membership predicate | Promised operations |
| --- | --- | --- |
| Absent | `!present` | Observe, Cancel |
| Disabled | `present && !enabled` | Observe, Cancel |
| Expired | `present && enabled && !continuous` | Observe, Cancel, Start |
| Idle | `present && enabled && continuous && !active` | Observe, Cancel, Start |
| Active | `present && enabled && continuous && active` | Observe, Cancel, Start, Continue, Finish |

Kani checks all 16 raw valuations and all five operations. Verus verifies the
unchanged executable bodies against independent membership, admission and outcome
specifications; membership is both complete and unique. `Start` depends only on
present/enabled, so stale history cannot make a promised start unavailable.
`Finish` ends an active continuous operation. `Cancel` always retires activity.
The outcome is computed immediately without a retry loop or external callback.
Since these properties quantify over arbitrary raw predecessor states, they also
hold after any history projected into this protocol; the theorem is not restricted
to the finite histories run by the native tests.

Both verifiers also cover every pair of u64 timestamps. Ownership lasts through the
same pass and its immediate successor; future timestamps and counter wrap cannot
revive it. A fresh press retires an earlier press-scoped session, while mounted
regions remain valid for their one-pass observation window. Four separate mutations
must be rejected by both tools: accepting future timestamps, preserving an expired
owner, refusing a new start because of old state, and keeping activity after finish.

The egui adapter stores a last-seen stamp with each payload. Reading does not renew
ownership: only the drawing owner stores it again. It rejects expired/disabled
payloads before cloning, and removes them. Product bindings cover:

- Dropdown open/query sessions, search focus and retired popup areas.
- Pattern stroke capture, row long-press suppression and button hold completion.
- Numeric drafts, wheel animation, keyboard requests and password reveal state.
- Press-scoped drag claims and mounted drag-exclusion rectangles.

Numeric drafts additionally bind to the exact original f64 bits and displayed seed.
A caller update, including one too small to change the displayed decimal, supersedes
an older draft. Disabling a field through a parent UI also retires editing state.
These changes keep public API signatures and caller-owned selections/buffers intact.

## Concrete carrier correspondence

The product now stores the types in `interaction/concrete.rs` directly:
`Session<T>` contains the real last-seen stamp and payload; `Pass` contains the
current pass, enablement, scope and fresh-press observation. `HoldPhase` is the
actual cached hold state. `Capture<T>` contains the actual input ownership bit
and the complete animation/value payload. These are not verifier-only replicas.

Both tools verify twelve unchanged production function bodies. Lease membership
is independently specified with integer arithmetic over every pair of u64 values,
including future stamps, gaps and the maximum value. The timestamped capture has
exactly one of seven groups: Disabled, Missing, Future, Expired, ReplacedPress,
Idle or Captured. Restoration yields the original borrowed payload exactly when
its lease is live; it does not renew the stamp or clone a payload.

The actual hold phase and six pointer observations are converted to protocol
states/operations by verified functions. Admission is not inferred from cache
presence or implementation refusal. The independent promise allows a fresh owned
start regardless of old storage, requires active ownership for continuation or
completion, and cancels previous ownership on a fresh unowned press even when
stale dragged/stopped flags coexist. Kani found the latter combination during
refinement; both affected harnesses pass after the production operation mapping
was corrected.

Completion stays latched for an admitted press independently of later animation
progress. A native witness changes the actual ring animation after completion;
a precise mutation restoring the old progress-dependent latch must refire and
fail. This avoids assuming monotonic animation progress in the input contract.

Verus proves noninterference for arbitrary payload **types**, not a finite set of
sample animation values. Equal input ownership gives equal admission irrespective
of all animation/value data, and applying a capture result preserves that payload.
It also proves total/unique membership for sequences of timestamped captures of
arbitrary length and all their joint state combinations. This composition theorem
does not establish that egui's heterogeneous map has been mapped to those sequences.

The egui storage lookup/downcast/clone, response observation and actual assignment
of the result are concrete integration boundaries. Miri exercises the real storage
adapter. Native mutations bypass restoration, corrupt the fresh-press binding and
omit capture-result application; matching public widget witnesses must reject them.
The derived Clone implementations are not called by the Verus proof and have no
Verus Clone specification; the proof covers borrowed selection and value moves.

## Compiler-derived type inventory

`verification/state_schema.py` asks the pinned compiler for private and hidden type
metadata for both product crates, with minimal and all features on Linux x86_64.
It compares module-level named struct/enum/union/type-alias declarations, generic bounds,
field type and variant payload against `verification/state-schema.json`. No numeric
compiler IDs or source positions are kept. Added/removed fields or variants fail
CI until their state correspondence is reviewed; stripped or unresolved metadata
fails extraction rather than silently omitting an entry.

The current profiles contain 814 and 686 declarations respectively; 20 declarations
are direct inputs to the lifecycle/concrete-carrier/session decision proofs. These counts describe
type declarations, not reachable states or verified application behavior. Other
declarations remain explicitly unmapped. The inventory includes configuration,
rendering and builder types as well as retained state and does not enumerate
function-local types and anonymous callback environments, arbitrary integrator types or upstream internals.
It is an omission guard and a worklist for further refinement, not the requested
whole-state completeness theorem. Compiler runs record the exact source inputs;
a local `--skip-build` report is explicitly marked as not freshly compiled.

## Backend inventory and concrete witnesses

`verification/retained-state.json` classifies the retained egui backend sites in the
widget crate. `retained_state.py` rejects an added/removed access site or lifecycle
call until that inventory is reviewed. It explicitly records geometry state in
expandable content and paint-only caches as separate categories. This is a source
inventory tripwire, not a proof over arbitrary Rust fields or upstream egui internals.

Native witnesses exercise all seven dropdown trigger forms, all four openers and
all 16 ordered pairs of outside-dismissal, owner withdrawal, own disabling and
parent disabling: 448 histories, with the promised peer click/reopen/selection
suffix checked after both prefixes (896 suffix probes). The same context and ids
survive every prefix; the harness never clears hidden memory between probes.
These concrete probes allow layout/animation frames to settle; they do not establish
a zero-frame input-realization theorem.
Other witnesses check stale numeric drafts (including update concurrent with blur
and changes below displayed precision), interrupted pattern completion/restart,
keyboard request expiry/disable, a new hold after an old completed hold, and
a still-held pointer while a button is withdrawn or disabled. A returning button
requires a fresh Start; an idle cached entry cannot authorize Continue. Zero-duration
holds complete exactly once per fresh press. A wheel keeps animation and input
capture separate; an old held drag cannot change a returning wheel, while a fresh
drag still works.
Adapter unit tests check that consumers cannot renew absent owners, expired payloads
are not cloned, and fresh press/mount lifetimes differ as specified.

Native mutation controls separately bypass the actual lease, remove numeric
caller-value validation, treat an idle hold entry as active, and bypass wheel capture admission. Passing abstract proofs cannot hide disconnected product
wiring: these broken adapters must fail their concrete contract tests. Generated
protocol histories, Miri and the shell fuzzer also check the production kernel.

## Evidence gates and remaining boundaries

`proof_obligations.py --require-components` must pass after both verifiers run. It
checks receipts against the current product source **and** independent verifier
specifications, the exact raw predicate fields and Back product bindings, and the
backend inventory. A stale receipt or changed inventory fails CI. The status report
records the discharged Back/protocol obligations separately from concrete carrier
partition/refinement, unbounded capture-product partition and payload noninterference.

The requested whole-application theorem still needs:

- A proved joint abstraction for shell/workspace/gesture state, widget geometry and
  animation, caller state, and upstream egui memory. The two component projections
  do not constitute this complete concrete partition.
- Formal input realization through layout, clipping, z-order, hit testing and
  upstream focus/capture. The concrete witnesses and mutation controls are tests.
- Machine-checked termination/effect contracts for arbitrary application, painter,
  backend and `on_back` callbacks.

`--require-whole-app` deliberately rejects that stronger claim. This rejection
means the concrete boundaries remain unproved; it is not a counterexample from
Kani or Verus. Neither the state inventory nor more finite test histories can
substitute for those missing refinement and callback contracts.
