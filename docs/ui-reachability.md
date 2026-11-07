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

The executable fixture declares twelve required edges and observes states from the real
shell. Every edge must receive actual pointer input; a missing witness fails the test.
Host APIs establish the initial public screen and do not satisfy a required edge.
The public screen distinguishes the viewer session from an authenticated session.

| From | Input witness | Required destination |
| --- | --- | --- |
| Public screen, viewer | Protected-action button | Unlock prompt |
| Unlock prompt | Cancel button | Public screen, viewer |
| Unlock prompt | Four on-screen PIN keys | Protected screen, authorized |
| Protected screen | Return button | Public screen, authorized |
| Public screen, viewer | Panel button | Overlay panel |
| Overlay panel | Visible scrim | Public screen, viewer |
| Public screen, viewer | Lock button | Lock screen, viewer |
| Lock screen | Continue button | Public screen, viewer |
| Lock screen | Four on-screen PIN keys | Public screen, authorized |
| Public screen, authorized | Protected-action button | Protected screen without another prompt |
| Public screen, authorized | Logout button | Public screen, viewer |
| Public screen, authorized | Lock button | Lock screen, viewer |

Continue is explicitly enabled in this fixture. It must preserve the viewer level;
it is not authentication. The lock PIN must authenticate without executing an old
protected-action continuation. A separate region-gesture route opens an unlock
prompt and verifies that its existing full-screen guard stands aside for PIN/Cancel.

The fixed corpus is the complete product of four viewport profiles (including
480×320), three initial histories, two modal-guard choices, two motion modes, two
pointer-delivery modes and two pixel densities: **192 configurations**. Another
**96 generated histories by default** (512 in CI), at most sixteen operations each,
add held presses, shuffled PIN layouts and a third pixel density. Initial histories vary region
activation, movement, removal, quick taps and rotation. Rotation and guard changes
also occur at the panel, prompt, lock and screen-return stages, including halfway
through entering a PIN. Geometry-only changes must preserve navigation and subject.

A separate **24-configuration refusal/recovery matrix** checks a wrong PIN, the
intentional 320 ms refusal shake, a second refusal causing lockout, rejected input
while locked, Cancel, reopening with the lockout retained, and successful real PIN
input after the recorded deadline. It crosses four viewports, both motion modes and
all three pointer-delivery modes, with shuffled digits and 1.5 pixels per point.
The post-deadline probe includes a 50 ms margin for the frame's f32 time conversion.

Targets come from current widget geometry or rendered text intersected with both
its clip rectangle and the viewport. Input sends actual press/release events:
separate passes, both events in one pass, or a press held across three passes.
A required navigation witness allows three attempts and two settling passes in
reduced-motion mode or 24 in animated mode. App-button counters reject duplicate
or underlying callbacks within one witness; fault quarantine cannot count as a successful edge. A geometry change gets the same bounded
settling opportunity before a target is required. Tests never substitute a hardware
shortcut, direct grant or host launch for the required input.

Failures report the edge, viewport, target, layer, prompt/fault status and frame count;
proptest minimizes the generated history. These are bounded input witnesses, not a
wall-clock performance guarantee. A configured region may intentionally block an
underlying control; it must stand aside before that underlying edge is required.

## Checks that must fail

The rendered-input contract controls deliberately introduce two product defects:

1. Preserve an old deferral flag with `|=`. A conflict then leaves ordinary UI closed
   even after a quiet pass, and the required input path is unreachable.
2. Ignore the prompt when deciding whether regions are active. A guard can cover the
   prompt's Cancel/PIN controls although the logical authentication API still works.

Both controls must fail semantically, rather than merely fail compilation. Kani and
Verus also reject an input decision that defers on non-conflicting input. This catches
cases where a standalone proof passes but the runtime stopped calling it correctly.

## Scope and extension

The twelve edges are the fixture's declared requirements, not an inventory of every
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

## Command/reply transition oracle

[`verification/src/transitions.rs`](../verification/src/transitions.rs) compares a
small independent model with the public shell after every operation. The oracle
tracks issuance ordinals rather than copying production tokens or deriving expected
privilege from observed effects. It checks subject, prompt presence, exact ordered
action executions, setting value, queued count, issued gate sequence, unique tokens
and retained reply count. A test reader deliberately keeps replies after cancellation
and shell replacement.

The fixed matrix contains **8,748 traces**: all capacities 1–3 × all budgets 1–3 ×
all ordered pairs from nine command variants × six response classes × both policy
states. Variants include two challenge gates, two actions, a setting write, Back,
logout and both viewer/admin subject assignments. Replies include current/stale
grants, denial, pending, a non-elevating grant and an unknown-level grant. Two initial
challenges provide real old and current tokens. This exhausts that declared matrix,
not every possible operation sequence.

Another **128 generated traces by default** (512 in CI), each at most 96 operations,
interleave queue admission, updates, replies, policy changes and shell replacement. Each trace ends
with a required successful retry in the same surviving shell after queued work is
settled. Old replies remain queued; progress cannot be satisfied by restarting away
a stuck state. A fixed replacement/duplicate trace also runs under Miri. The shell
ASan fuzzer uses this same oracle, with 2,000 runs and a maximum input length of 96.

Twelve deadline scenarios add actual queued privileged actions spanning a one-second
session timeout. Two capacity/budget profiles, fresh/no input and probes at 999,
1,000 and 1,001 ms check that expiration precedes due queued work and fresh activity,
while pre-deadline work executes exactly in FIFO order. These complement the existing
session/temporary-elevation/idle-lock boundary contracts.

The exact production response-correlation decision is shared with Kani and Verus:
an absent or different active token cannot accept a reply. This proves token matching,
not the whole asynchronous shell. Native mutation controls separately remove the
shell's correlation call, exceed its command budget and move deadline expiration
after command dispatch. All three must fail their semantic assertions. The model does not assume eventual external authentication,
arbitrary callback termination or a trusted network merely because a token matches.
