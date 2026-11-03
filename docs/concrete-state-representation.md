# Concrete state representation

This specification addresses one obligation: every concrete product state has a
lossless representation. It does not define permitted operations or prove that
an input produces a transition. Those are separate obligations.

## Canonical specification

The canonical state is a nominally typed, labelled product/sum of the actual
product fields and enum variants, together with the complete referenced store
at the same instant. A native type's exact identity includes its generic and
const parameters. Every struct field is retained. Every enum variant has a
distinct constructor and retains all of its fields. Type aliases are transparent.

The field domains contain the complete native values, not summaries such as
lengths, hashes, focus-present flags, or context identifiers. Collections retain
their full contents and order. References retain identity and aliasing; their
referents remain in the unchanged store. In particular, an egui Context handle
alone is insufficient: the shared context memory is part of the store. The
representation is a mathematical instantaneous state view, not a frozen heap
snapshot or a new runtime API.

The existing operational model in ui_state.rs is a separate candidate abstraction.
Its finite flags are not substituted for this canonical specification, and this
proof does not establish that they are a sufficient operational abstraction.

## Obligation and implementation binding

For every native state x, the representation and independent reconstruction must
satisfy reconstruct(represent(x)) = Some(x). The reverse law establishes that each
canonical representation denotes exactly its corresponding state. Consequently,
distinct states cannot collapse, and any observation of the reconstructed full
state agrees with that of the original. These statements quantify over arbitrary
field values, collection contents, root counts and store identities.

verification/state_correspondence.py checks the source-to-logic binding:

1. Parse both product libraries, including private and function-local structs and
   enums. Record source identities, fields, variants, module reachability and cfg
   guards.
2. In disposable source copies, generate an exhaustive borrowed field projection
   for every datatype. The normal product source and API are unchanged.
3. Compile those projections with the actual generic parameters and where
   clauses. Capture production rustc expansion without cfg(doc), and obtain
   independent private rustdoc type metadata.
4. Compare the actual expanded method with its attested body. Accept only a
   closed grammar of exhaustive patterns, field borrows, labels and sum
   constructors. There are no getter callbacks, clones, allocations or
   user-provided projection functions.
5. Construct the concrete logical datatype from the compiler's declaration,
   independently of the projection's output. Translate the checked native body
   into the borrowed logical projection. Verus checks its reconstruction and
   normalization postconditions, then the canonical inverse laws for arbitrary
   field domains.
6. Prove that every production feature selection activates only layouts for
   which those checks succeeded. The minimal, all, osk-only and overlay-only
   compilations cover the four independently conditional ShellBuilder layouts.
   Feature closure and module/field/variant guards are checked universally over
   requested feature masks; an unsupported active cfg or state-generating macro
   fails closed.
7. Compose the per-type inverse laws through nominal identities, arbitrary root
   sequences, heterogeneous sums, stores and unchanged referenced state.

Field payloads remain parametric. Changing their Rust types does not invalidate
an algebraic product/sum inverse, but the new native type and its actual
projection must still compile. This is not permission to discard field values.

## Controls and evidence

A successful run writes target/assurance/state-correspondence/summary.json only
after fresh native builds, all Verus batches and all negative controls pass.
The receipt records source/proof hashes, per-profile projections and aliases,
configuration coverage, command results and evidence hashes. Development
--skip-build never emits a fresh receipt.

Required negative controls remove an actual Shell output field, relabel an actual
Lifetime variant, omit the two mixed feature layouts, and alter an independently
expanded projection body. Semantic controls must fail with a Verus postcondition
counterexample; compiler errors, timeouts and solver resource failures do not
count as counterexamples. The altered expanded body must fail the binding check.

Run:

    python3 verification/state_correspondence.py

The public-repository-only Concrete state correspondence workflow runs this
command and retains receipts, generated theorems and logs.

## Trust and boundaries

The trusted base includes Rust's datatype and borrowing semantics, the pinned
compiler and Verus/vstd/solver, and the reviewed restricted extractor/translator.
This is source-level representation verification, not a proof of the compiler,
allocator, operating system or third-party implementation. Native binding is
checked on Linux x86_64. Product cfg expressions that could introduce an
unreviewed target-specific layout are rejected rather than generalized silently.

This deliberately retains complete external and shared state instead of claiming
to have verified its internal behavior. It does not establish a useful finite
behavioral quotient, correctness of the operational groups, permission rules,
input reachability, progress, deadlock freedom, panic freedom or UI recovery.
