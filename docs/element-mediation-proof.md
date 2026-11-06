# Shared callback mediation evidence

The proof unit is the common admission kernel and its opaque payload carriers.
Screens, factories, actions, status items, navigation items, tile panels and tree
leaves use that boundary. Adding a screen or a leaf does not require a new proof
for that instance. Adding an interface operation requires a checked adapter;
introducing a raw storage, invocation or extraction route must fail the compiler
check.

## Runtime structure

`access::mediation` owns executable payloads and their normalized requirements.
Declarations, instances and resident loans hold opaque handles. `ScreenHandle`
provides no `Screen`, dereference or extraction implementation. Its factory is
invoked inside admission; the resulting screen stays private and receives a
second live check before any hook. Generated screen/action/UI adapters call the
same invocation primitive.

A prepared tree leaf borrows its actual `ElementHandle`. Rust prevents replacement
or independent mutation of that receiver while the plan is live. The child index
only contributes to widget identity; dispatch never uses it to look up another
object. The common invoker extends a borrowed `AuthorityPath` in the callback context;
children inherit it automatically. Every leaf entry checks its own requirement and
every ancestor requirement. Plans retain session identity/revision and metadata,
not an authorization result. Denied children are omitted before size accumulation.

In a protected shell, legacy carriers deny entry. Initial library declarations
receive explicit library requirements. Untyped global hooks remain rejected by
`build_protected`; admitted callbacks receive invocation-local extension stores.

## Common proof and automatic checks

| Check | Evidence | Boundary |
| --- | --- | --- |
| Admission is necessary; denied work has no callback effect/result | Kani executes unchanged production `Admission::allows`, common `invoke` and `protected_dispatch` bodies | Context observation and resource-scope contracts are modeled |
| Every ancestor is checked | Verus verifies the unchanged production ancestor-walk body with ghost invariants | Arbitrary finite depth; one stable observation interval and normalized grant-identity contract |
| Opaque protocol ownership and entry cut | Compiler-owned fields, visibility, method-item references, return types and callable substitutions in both feature profiles | Registered-payload confinement; not complete arbitrary heap/erasure analysis |
| Bypass checker rejects new routes | Compiled production mutations in an isolated checkout | Direct calls, raw storage, raw/generic extraction, field exposure, an unguarded adapter and discarded/overwritten context ancestry |

The ancestor proof has no depth, child-count or permission-catalog enumeration.
Dropping the parent during construction, or removing an ancestor step, grant check,
ancestor revision check or token-mode condition must fail a proof obligation.
Compiler facts also check the concrete `Cx` scope constructor, its inherited-path
field and forwarding of that scoped context to the callback. Kernel mutations must fail the named
security assertion; compiler errors and unrelated assertion failures do not count.

The compiler check discovers definitions and operations rather than maintaining a
list of individual protected screens. It follows callable type substitutions and
actual invocation sites. Merely constructing or storing a closure does not imply
that its body executed. Unknown callable entry on a reachable kernel path rejects
the admission cut.

## Remaining whole-library obligations

Complete mediation remains **not proved**. The strict global gate remains nonzero.
The outstanding connection includes protected construction and noninstallation of
legacy hooks, complete payload provenance through type erasure and indirect
function pointers, factory/binding data refinement, and the concrete authority
observation contract. The production ancestor proof establishes the walk under its
context contract; it does not prove arbitrary concurrent or stateful narrowing-policy behavior
or the Rust atomic memory model. Compact floating-point geometry is covered by
product regressions, not an unbounded layout theorem.

Authenticators/backends and arbitrary effects inside an admitted application body
remain trusted. The guarantee concerns admission to library-managed callbacks;
it does not retract already-disclosed information or cancel admitted work.

## Run

```sh
python3 verification/install.py proofs
python3 verification/central_proofs.py
python3 verification/central_mediation.py
python3 verification/central_mediation_controls.py
# The whole-library claim still rejects, retaining its diagnostic inventory.
python3 verification/mediation.py --require-proved
```

Evidence is written under `target/assurance/central-mediation/`, with source hashes,
compiler inventories, proof logs and mutation results. Old bounded traversal clients
were retired with the borrowed-plan refactor; their receipts do not certify this
implementation. See [the completion specification](central-mediation-spec.md) for
the full target and [the global detector](callback-mediation.md) for its verdict.
