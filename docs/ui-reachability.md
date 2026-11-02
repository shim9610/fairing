# UI transition reachability

A state transition can be logically permitted while its input control is hidden,
occluded, stale, disabled or never drawn. Checking only the state-changing method
cannot establish that a user can perform the transition.

The assurance suite checks both **safety** (a conflicting press does not execute two
handlers) and **progress** (input admission does not remain closed after the conflict).
It connects the production decision to real rendered input paths. It does not infer
an application's intended navigation or prove arbitrary user callbacks.

## Production progress contract

`crates/fairing/src/shell/input_route.rs` is called by `Shell::read_gestures`.
Kani and Verus check its exact decision bodies. For ordinary input admission in
an operational shell:

- A fresh press claimed by changed region geometry defers ordinary UI callbacks.
- A prompt, overview or open overlay prevents gesture regions from capturing its input.
- An unchanged region, unclaimed press or absence of a new press does not cause deferral.
- After any history, one pass without a fresh press admits ordinary UI again, even
  if region geometry continues to animate.

The decision has no persistent latch. This local property applies after arbitrary
prefixes; it is not a claim that a finite trace search explored infinite time. The
caller must overwrite its old decision and request a repaint when deferring. Actual
shell witnesses and mutation controls check that wiring separately.

Fault quarantine, deliberate lockout and external authorization waits have separate
contracts; an application must declare whether a recovery control is required.

Progress requires an input/render opportunity. If the host never supplies another UI
pass, or every pass introduces another conflicting fresh press, this contract does
not promise an ordinary rendered frame. Backend response deadlines, permission
policy and application callbacks have their own contracts.

## Required input paths

The executable fixture declares six required edges and observes states from the real
shell. Every edge must receive an actual pointer-input witness; a missing witness
fails the test. Host APIs are used for initial setup, not to satisfy an edge.

| From | Input witness | Required destination |
| --- | --- | --- |
| Public screen | Tap the rendered protected-action button | Unlock prompt |
| Unlock prompt | Tap the rendered Cancel button | Public screen, unchanged subject |
| Unlock prompt | Tap the four on-screen PIN keys | Protected screen, authorized subject |
| Protected screen | Tap the rendered return button | Public screen |
| Public screen | Tap the rendered panel button | Overlay panel |
| Overlay panel | Tap the visible scrim outside the panel | Public screen |

A separate route opens the prompt through a real region gesture. This checks that a
previously active full-screen guard stands aside for the prompt's own input. The
panel-dismiss path is also exercised while a full-screen region is configured behind it.

The corpus uses four viewport profiles, including 480×320, and generated histories
of region activation, movement, removal, quick taps and rotation. A configured region
is allowed to block an underlying control while active. Once it stands aside, or a
modal owns input, that old obstruction must not make the required edge unreachable.
There are 48 fixed profile/history/modal/motion combinations and 32 generated histories.
Both reduced-motion and ordinary animated transitions run through the same required edges.

A witness checks the visible target rectangle, sends actual press/release events and
observes the destination. It allows three attempts, followed by two settling passes
in reduced-motion mode or 24 in animated mode, using the harness's deterministic 60 Hz
clock. This is a bounded input witness, not a wall-clock performance guarantee. It cannot use a hardware-key shortcut, direct grant or host launch as a
substitute. Failures report the edge, viewport, target, layer, prompt/fault status and
frame count. Proptest also reports a minimized operation history.

## Checks that must fail

The contract controls deliberately introduce two product defects:

1. Preserve an old deferral flag with `|=`. A conflict then leaves ordinary UI closed
   even after a quiet pass, and the required input path is unreachable.
2. Ignore the prompt when deciding whether regions are active. A guard can cover the
   prompt's Cancel/PIN controls although the logical authentication API still works.

Both controls must fail semantically, rather than merely fail compilation. Kani and
Verus also reject an input decision that defers on non-conflicting input. This catches
cases where a standalone proof passes but the runtime stopped calling it correctly.

## Scope and extension

The six edges are the fixture's declared requirements, not an inventory of every
Fairing or integrator transition. Add a state observation, required edge and real
input witness when extending this graph. Extend the independent history generator
with the obstruction that previously caused a regression. Keep permissions, deliberate
lockout and configured input capture distinct from unintended inaccessibility.

Kani/Verus prove the decision contract. Headless egui tests establish the concrete
widget/input correspondence for the exercised configurations. Miri interprets the
allocation-free decision but does not render the UI witness corpus. Neither layer
alone proves all layouts, all hardware inputs, arbitrary external callbacks or
unconditional eventual backend completion. The host must keep supplying UI passes;
the viewport and intended target must eventually remain usable during a witness.

Headless witnesses check target geometry and input effects, not pixel contrast, human
discoverability or physical display delivery. Those require separate visual/device checks.
