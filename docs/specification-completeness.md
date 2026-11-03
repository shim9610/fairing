# State specification completeness

`python3 verification/specification.py` verifies the expanded joint model and
audits the existing carrier-only projection. The expanded model has twelve proved
theorems, while the carrier-only projection fails a necessary condition. Total
product refinement is still open, so this strict command returns **2** and its CI
job remains red. The carrier counterexample is a representation gap, not a failing
editor.

## Obligation

Let `C` be reachable concrete product states, `alpha` the product-connected
specified view, and `O` observations needed to state the promised UI behavior.
State representation must be total and its groups disjoint. Additionally, a
claimed complete view must preserve required observations:

`for every a,b in C: alpha(a) = alpha(b) implies O(a) = O(b)`.

Merging states is permitted when the omitted difference is irrelevant. Merely
assigning every state a tag does not satisfy the requested guarantee if the same
tag hides which editor owns input. Here `O` includes the focused editor identity
and the recipient of the same next text event. Focus is retained UI state, not a
fresh environment choice that can be assumed independently of the concrete state.

The repository now has an [expanded joint model](joint-state-specification.md)
with proved model partitions and operation eligibility. Its total projection from
concrete runtime state remains open. The carrier-only product-connected view
checked below still omits focus identity. A collision on this supported subdomain
rejects treating that view as the total projection. Passing this particular check
would not establish totality or adequacy over every other product state.

## Executed counterexample

A test-only fixture draws two real `TextField`s with explicit stable ids and the
same keyboard layout. It first focuses the left editor with an ordinary
press/release, settles and moves the pointer to a neutral location. The public
`Memory::request_focus` API then switches to the right editor **without advancing
the pass or replacing the context**. The typed existing view is unchanged while
concrete focus changes. Context/reference identity therefore cannot explain the
missing state distinction.

Two replays use the identical left-focus prefix, select the intended focus through
the same public API, then deliver the same text event. Each retains its context
from prefix to effect. These replays validate the corresponding text recipients
at equal pass/frame counts; the same-context collision is checked separately.

The existing view contains the actual `Pass`, both `Pointer` records, both ids,
the complete timestamped `Session<Keys>` values and their production lease groups.
Exhaustive field/variant patterns encode all 31 scalar values. They are equal in
the two executions. The contexts instead report different focus ids, and the next
identical `Event::Text("x")` changes the left buffer in one execution and the right
buffer in the other. The fixture checks actual storage, focus, buffer contents
and its encoding before exporting either record.

| History | Existing view | Concrete focus | Next identical text event |
| --- | --- | --- | --- |
| Focus left, settle | V | Left editor | Changes left buffer |
| Focus right, settle | V | Right editor | Changes right buffer |

This view preserves more than the coarse lease/group tags; retaining the current
carrier inputs still does not express focus ownership. The missing distinction
is necessary for the editor's observable behavior.

## Formal check and controls

The tool generates verifier inputs from freshly executed records and runs both
pinned checkers:

- Verus rejects the observation-preservation obligation on the reachable pair.
  Separately it verifies the collision and a generic information-loss lemma:
  no interpretation of the same abstract input can produce both different
  required observations.
- Kani rejects the same obligation. It independently checks the collision and
  quantifies over every possible predicted focus id/recipient. Its failure must
  be the designated assertion, not an unwinding failure or an undetermined check.
- A focus-aware comparison separates these records and passes in both tools.
  This is a control for the known obligation, not proof that a focus-augmented
  specification covers the entire product.

Native execution establishes reachability and the current product-connected view.
The solvers check the resulting mathematical obligation and refutation. This is
an explicit trusted boundary, not an assertion that the whole egui implementation
has been formally verified. Source, lockfile and specification hashes accompany
the records. A fixture/tool error cannot produce a successful completeness verdict;
disappearance of this collision also cannot be promoted to universal completeness.

Artifacts are written under `target/assurance/specification/`: the executed states,
both generated verifier inputs, semantic rejection logs, positive-control logs and
`result.json`. The ordinary component proof jobs remain separate from this strict
completeness gate.
