# Joint UI state specification

The [canonical concrete-state specification](concrete-state-representation.md)
defines the full lossless state space. Its source-bound representation check is
separate from the operational abstraction and progress obligations below.

[`verification/verus/ui_state.rs`](../verification/verus/ui_state.rs) defines a
joint model for shell policy, widgets, focus, popups, capture, animation and input
surfaces. Run `python3 verification/ui_specification.py` to verify its twelve
theorems and ten semantic controls. This command checks the declared model;
the total projection from concrete product state is a separate open obligation.

## State representation

`UiState<V,R>` retains arbitrary value and lease payload types. Maps and sequences
have arbitrary finite sizes, without a fixed widget count, popup depth or history
length. Widget identifiers are scoped to an interaction context; cross-context
and viewport ownership must be supplied by the concrete projection.

| Dimension | Retained information | State groups |
| --- | --- | --- |
| Shell | Back priority predicates, modal scope, configured recovery availability | Faulted, Locked, Unlock, Info, Overlay, Keyboard, Overview, Home, Workspace |
| Widget policy | Mounted/enabled state, input scope, content/authentication/recovery role, declared capabilities | Absent, Disabled, Faulted, Locked, OutsideModal, Ready |
| Focus | Exact optional owner identity, separate keyboard owner | None, This, Other, Dangling |
| Popup | Intended open owners and ordered retained popup entries | Closed, MissingSurface, StaleSurface, Open |
| Pointer capture | A map from pointer identities to owner identities | Free, This, Other, Dangling |
| Animation | Phase, exact progress bits and epoch | Missing, Hidden, Entering, Shown, Leaving |
| Input surface | Exact rectangle/clip bits, layer, painted state and actual point-routing function | Missing, Hidden, Blocked, Reachable |

Values, keyboard ownership, pass and timestamped lease payloads remain in the
joint state even when a particular group predicate does not inspect them. A
group is a product of the seven classifications and the widget capabilities.
It is not merely a visible screen name. Identity `0` is a valid owner; absence
uses `Option`, not a sentinel. Duplicate popup entries, missing surfaces,
contradictory shell predicates and dangling owners are included in the raw domain.
The partition proof has no well-formedness or reachability precondition.

## Promised operations

`ready` depends on declared widget policy and current shell policy. It does not
depend on retained popup state, capture, lease age, animation or actual hit-test
success. Authentication widgets remain eligible in their active lock scope.
Recovery widgets become eligible during a fault only when the developer has
configured that recovery path. The projection must bind these roles to the real
owners; untrusted input cannot assign itself a privileged role.

| Operation | Contract eligibility | Required outcome |
| --- | --- | --- |
| Activate | Ready, activation capability | Activation attributed to the target |
| Focus | Ready, focus capability | Exact target owns focus |
| Text | Ready, edit capability, keyboard invocation, exact focused target | Edit attributed to that target |
| OpenPopup | Ready, popup capability | Target is intended open and becomes the last popup entry, preserving peer order |
| DismissPopup | Always available as internal cleanup | Remove all target entries and its open intent |
| BeginCapture | Ready, drag capability, fresh pointer invocation | Target owns that pointer; other pointer bindings remain |
| EndCapture | Always available as internal cleanup | Release only this target's ownership; preserve a replacement owner |
| Cancel | Always available as internal cleanup | Retire target focus, keyboard, popup and capture ownership; preserve peer owners |

Cleanup availability permits resource/state retirement, not normal callback
dispatch during quarantine. Capabilities are the declared operation contract,
not inferred from whether the implementation happened to accept a request.
Other operations, such as authentication replies and backend commands, retain
their existing separate specifications and still need composition with this model.

## Proved model properties

- Every modeled state, target and pointer has exactly one joint group. This also
  quantifies over arbitrary collection sizes and payloads.
- Equal groups imply equal operation eligibility, including invocation modality
  and fresh-press requirements.
- Changing only retained presentation/history cannot withdraw a promised
  operation. Focus is retained as a distinct contract input for text delivery.
- Text has one recipient; changing focus identity changes the relevant promise.
- A promised pointer operation with no actual hit region violates input
  realization. The missing region cannot redefine the operation as unpromised.
- Fault quarantine preserves only cleanup for ordinary widgets, while configured
  recovery and active authentication controls retain their respective eligibility.
- Cancellation has a specified successor from every modeled state. Releasing an
  old capture cannot cancel its replacement owner.

Ten mutations must fail with semantic proof violations: erased focus,
hit-test-dependent promises, stale popup misclassification, wrong capture owner,
lost animation phase, lost fault priority, blocked recovery, blocked authentication,
retained cancelled focus and removal of peer capture. Compiler errors and solver
resource failures do not satisfy the controls.

## Concrete proof obligations

`abstraction_obligation` requires **every reachable concrete state** to produce a
model state and preserve the required observations. Returning `None` for a stale
or inconvenient concrete state fails this obligation. The observation functions
must come from the operation contracts; choosing a constant observation would
not represent text ownership or the other promised behavior.

`input_realizable` requires an intended pointer region to exist and every intended
point to reach the actual target after clipping, z-order, gesture and input
interception. The point-routing functions are abstract interfaces here. Their
correspondence to egui geometry/routing must be proved by the concrete adapter.

`product_progress_obligation` requires a nonempty set of invocation traces and a
specified completion bound for **every** trace, not merely one successful path.
Each invocation emits one completion event; every emitted completion must satisfy
the outcome contract. A prior incorrect result or duplicate completion is not
excused by a later successful result. The bound counts invocation trace steps.
The trace relation must cover actual executions, including stale blockers.
Frame/input delivery and external response assumptions must be explicit; internal
deadlock cannot be removed from the trace relation as an environment failure.
The `Edited` effect currently specifies attribution, not text contents; buffer
editing and arbitrary caller callbacks require their own effect contracts.

The existing carrier-only product projection still loses focus identity and is
rejected by the native witness plus Verus/Kani check. The new model contains that
identity, but this does not itself connect all product fields to it. Shell/access,
workspace, gesture geometry, popup child ownership, viewport state, backend queues
and callback effects remain concrete composition work. The completeness CI gate
therefore remains failed. The component proof job separately requires a fresh
joint-model receipt, including source/tool hashes and semantic-control logs.
