# Formal operability obligations

The target theorem is stronger than a regression campaign:

1. Every reachable concrete state belongs to exactly one specified state group.
2. Each group has an independently defined set of promised interactions.
3. Every valid invocation of a promised interaction reaches its specified outcome;
   previous interaction history cannot leave a hidden blocker.

**This whole-application theorem is not proved.** The executable status report is
`target/assurance/proofs/state-obligations.json`. A green component-proof job does
not discharge its open obligations. Run
`python3 verification/proof_obligations.py --require-whole-app` to require this
stronger claim; it currently fails with `OPEN_PROOF_OBLIGATIONS`.

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

## Remaining obligations

The status report keeps these open:

- Complete concrete-state partition beyond the Back projection, including retained
  widget and egui state and application state behind callbacks.
- A complete operation catalogue with independent preconditions and outcomes.
- Preservation of the concrete-to-abstract relation across every retained-state transition.
- Concrete input realization: z-order, clipping, focus, capture and widget hit testing.
- Termination/progress of the permitted operations, including contracted callbacks.

Adding more finite interaction histories improves bug detection but does not close
these obligations. Successful Back proofs must not be presented as the target
whole-application no-stuck-state guarantee.
