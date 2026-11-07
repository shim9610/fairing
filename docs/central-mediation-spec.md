# Central mediation implementation specification

The completion target is whole-library mediation of application user interface
(UI) callbacks through the protected facade, including arbitrary finite element trees.

## Shared runtime invariant

Application payloads are held by opaque carriers in one mediation module. A
carrier owns both the payload and its normalized requirement. No raw payload,
mutable payload reference, callback pointer or executable factory result may be
returned to the rest of the library. No dereference/extraction API is provided.

Screen, action, status, navigation, tile and element operations use generated
adapters around one checked invocation primitive. Factories remain opaque and
cannot execute before admission; their returned screens stay in opaque carriers
with the same requirement and receive a post-construction check. Dynamic element
metadata is checked against its prepared binding inside the carrier immediately
before dispatch. Applications supply permission values, not authorization booleans.

The primitive reads the calling context's issuer authority and narrowing policy;
it never consumes a stored allow/deny result. Denial produces no callback execution
or result. Callback-local icon/widget stores preserve the existing non-escape
boundary. Authentication/backends and already-admitted arbitrary application
work retain their explicitly stated ownership boundary.

Legacy callbacks are a separate explicit class. They cannot be invoked in the
protected facade. Library-created initial declarations receive explicit library
requirements; registration cannot turn a protected requirement back into legacy.
Untyped global hooks remain rejected. Their absence and callback-local resources
are state invariants, not a list of apparently harmless call sites.

Protected construction consumes a private `CallbackFreeBuilder`, whose compiler
type graph contains only configuration data and explicitly trusted host interfaces.
The conversion clears every untyped hook slot; the compiler inventory checks those
assignments and the preservation of data fields. Procedural wallpapers cannot
enter the private validated wrapper. Its production recursive predicate and
constructor are verified for arbitrary finite nesting, including inactive themes.

## Tree representation and induction

A prepared leaf borrows its actual opaque carrier. It does not store an index
that must later be reconciled with a separately mutable tree. Rust borrowing
therefore preserves receiver identity and prevents structural replacement while
the plan lives. Plans carry requirements and authority identity/revision, never a
reusable permission. Every dispatched operation rechecks live authority.

The recursive proof covers the actual node protocol: denied subtrees produce no
plan; admitted child plans preserve the parent invariants; stale plans allocate
no geometry; each paint checks the same carrier and prepared requirement. The
induction is over arbitrary finite trees/sequences, with no enumeration of sizes
or depths. The common invocation primitive extends the immutable ancestor chain in the
callback context. Children receive that scoped context; tree adapters do not pass
parent paths manually. Every new child entry checks the inherited chain.

## Automatic whole-library enforcement

The compiler checker inventories payload storage, construction, movement and
invocation. Raw application protocol calls or raw callable storage outside the
mediation boundary are errors unless covered by an explicit, checked payload-body,
legacy-unreachable or trusted-host rule. New interface adapters are generated from
the shared invocation contract. New unclassified stores/call sites fail checking;
they are never accepted merely because a test passed or a name is familiar.

The checker must establish carrier non-escape, requirement/receiver matching,
factory requirement preservation, protected construction/registration invariants
and the context used at each entry. Standard library/UI operation contracts and
trusted host interfaces are explicit proof assumptions. Unknown application
forwarding or ownership obligations keep the strict gate nonzero.

## Completion criteria

- [x] All supported registered application payload families use opaque carriers.
- [x] All registered payload operations use the common invocation primitive;
  standard operation adapters are generated.
- [x] Raw payload extraction and new bypass calls fail compiler controls.
- [x] Actual recursive plans preserve identity through borrowed carriers.
- [ ] Arbitrary-tree authority/requirement invariants are proved compositionally.
- [ ] The global checker connects all protected roots and carrier operations,
  with no unclassified application callback/storage/forwarding obligation.
- [ ] The existing strict mediation gate succeeds for the stated property;
  deliberately broken guards, bindings, factories and escapes fail semantically.

`global_mediation.py` joins fresh production admission, ancestor-path,
construction and factory receipts to both compiler feature inventories. It checks
all scoped-context fields, deep return types and pre-admission payload transfers,
including writes through application-state borrows. The strict gate consumes this
report. A successful connection check does not discharge the remaining
protected-root legacy-hook preservation and concrete authority-observer refinement
obligations; each remains explicitly false in the generated obligation report.

Do not replace these criteria with new small-harness counts, mark unknown paths
safe, or label an abstract graph theorem a complete runtime proof.
