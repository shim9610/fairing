# Retained-context interaction histories

These tests exercise production widgets and the shell through real egui input.
They supplement the existing component proofs and rendered reachability tests.
They do not establish completeness over all UI state or all event traces.

## Contracts

Every generated subject retains the same Context, widget ids, focus/area storage
and AnimationStore throughout its prefix and recovery suffix. No test clears
those stores, restarts the shell, or requests focus to satisfy a suffix.

The independent suffix requires:

- An outside tap dismisses an open popup, after pointer release.
- An enabled peer accepts one subsequent click.
- The text field accepts pointer focus and replacement text.
- The numeric field commits the newly entered value on blur.
- A fresh hold completes exactly once, including continued holding afterward.
- A caller wheel update remains authoritative at rest, and a fresh drag changes it.
- The dropdown reopens, accepts a rendered option click and closes after selection.

The suffix explicitly requires visible/enabled controls and frame opportunities.
It uses normal user cancellation and documented caller value updates. Its numeric
and wheel updates are public inputs, not writes into retained widget storage.
It does not allow a widget's unexpected disabled state or a missing hit target
to excuse a promised operation.

The final selected option, text, numeric bits, wheel selection and new hold count
are compared with a separate pristine execution of the same suffix. Its asserted
public outcome is cached once per widget profile to avoid redrawing an identical
reference for every generated history; subject Contexts are never shared or reset. Historical
callback counts and caller values are intentionally distinguished from the new
operation's effects. Each result also has a direct expected-outcome assertion:
agreement between two faulty runs is not sufficient.

## Input grammar and timing

The low nibble of each history byte selects an operation:

| Opcode | Operation |
| --- | --- |
| 0 | Tap dropdown trigger |
| 1 | Tap outside |
| 2 / 3 / 4 | Withdraw / disable / parent-disable owners for two passes, then restore |
| 5 | Tap text field |
| 6 | Deliver text input |
| 7 | Press hold button |
| 8 | Release pointer outside |
| 9 | Press wheel |
| 10 | Move pointer upward |
| 11 | Escape |
| 12 | Change numeric and wheel caller values |
| 13 | Tap numeric field |
| 14 | Tab |
| 15 | Wait sixteen passes |

Bits 4–5 select frame intervals of 1/120, 1/60, 1/15 or 1/4 second.
Bits 6–7 select zero through three additional passes after the operation.
A generated prefix contains zero through twenty-four operations. The fixed corpus
covers all seven trigger forms and four openers with ordered interruption pairs,
plus mixed capture/focus/caller-update histories.

Recovery uses 60 Hz input. Popup animation gets twenty-four passes (400 ms).
Wheel settling gets 120 passes (two seconds). These are executable test bounds
for this fixture and theme, not new public latency promises. A separate immediate
dismissal check permits no settling passes or retries between the consumed outside
click and the next peer click.

The shell fixture additionally exercises actual on-screen key clicks after
popup, field disable/withdrawal, overlay, resize and sleep histories, both with
and without animation. It checks digits/Latin layout switching and verifies that
typing affects only the intended field before and after popup selection.

The outside-click target is reserved blank space at the fixture's top-right.
The bottom-right is inside a full-width Sheet and is not a valid outside target.
A fixed regression opens each of the 28 profiles at all four frame intervals
before dismissal (112 cases), including the reduced Sheet/Tile history [0].
The dedicated workflow uses the same 512 cases and seed as the full assurance
and coverage runs.

## Authentication timing boundaries

A separate production-shell test checks 24 combinations of just-before/exactly-at/
just-after session expiry, fresh input, same-gate replacement queued in that pass,
and two queue capacity/budget profiles. An earlier challenge's grant must not
survive expiry or authorize the replacement challenge. A fresh current-token
retry must still execute exactly once. Both the old reader response and a later
duplicate are retained by the test reader; cancellation cannot make them vanish
from the test environment.

## Reproduction and failure reduction

Run:

```sh
mkdir -p target/assurance/ui-histories
PROPTEST_CASES=512 PROPTEST_RNG_SEED=20261101 \
  cargo test --locked --manifest-path verification/Cargo.toml \
  --lib widget_lifecycle:: -- --nocapture
```

The generated test uses proptest sequence/value shrinking with at most 2,048
shrink iterations. Its failure report includes the profile and reduced byte
sequence; the opcode table makes that sequence replayable. Regressions are
persisted under target/assurance/ui-histories and uploaded with CI logs.
Shrinking stops at the configured budget. Add fixed regressions to source so they
survive artifact expiry.

The dedicated UI interaction histories workflow runs the native suite. Existing
Rust assurance also runs these tests through its integration test workspace.
Three native semantic controls mutate production code in disposable checkouts:
focus-isolation clears the currently focused field instead of only the retiring
owner; popup-retirement leaves the retired area interactable; deadline-reply skips
the pre-dispatch expiry check. Each baseline must pass. Each mutation must exit
unsuccessfully and its log must contain both `test result: FAILED` and the
control's expected diagnostic marker. The checker does not tie that marker to
the failing assertion; the log must be inspected to establish that relationship.
A compilation failure alone does not count as detection.

No additional product dependency or public API is introduced.

## Scope

These checks exercise finite histories, selected timings and fixture layouts.
They check observable input recovery, ownership isolation and results. They do
not prove every upstream egui state, arbitrary callbacks, GPU/native event-loop
behavior, or every possible scheduling trace. Existing authentication/deadline,
Miri, Loom, fuzzing and component-proof jobs retain their individual scopes.
