# Common protected mediation contract

The proof unit is the common mediation kernel. The compiler checks that every
library-managed protected payload entry uses it. Individual screens, permission
values and tree leaves do not require individual proofs or verifier registrations.

## One declaration, runtime adapter and coverage

`access/mediation/contract.rs` defines `protected_contract!`. Carrier declarations
emit the actual opaque storage and its associated `Payload` contract together.
Declaration conversions emit the sealed registration implementation and its
contract together. Operation declarations emit an adapter that invokes the shared
monitor. Permission binding methods also use one generated implementation.

For example, after defining a new payload protocol, one operation declaration is
sufficient inside the mediation module:

```rust,ignore
protected_contract!(operation Callback<dyn TrendView>, render(ui: &mut egui::Ui),
    |body, cx| body.render(ui, cx));
```

There is no Python carrier/protocol list to update. The compiler discovers the
contract implementations, associated payload types, actual carrier instantiations
and generic payload bounds from compiled Rust. It also compares the declared
payload contract with the actual executable field types, including nested containers;
a generated marker with a false payload type is rejected. The same rule discovers the new
protocol and its invocation sites. A direct call that bypasses the generated
adapter is rejected. The extension controls compile both dynamic and generic
new protocols against the unchanged checker and unchanged kernel proof.

Application-defined `Element` implementations use the existing public
`ElementTree::leaf` and protected registration APIs. They inherit this boundary;
application code does not insert guards around every child.

## Kernel and routing evidence

Kani checks unchanged production admission, dispatch, payload selection and
binding bodies. Refusal has no payload entry, effect or result; binding preserves
the requested identifier. Verus checks the production ancestor constructor/walk
for arbitrary finite depth and an unbounded permission-identity set. Narrowing
permission decisions may reject nondeterministically.

The compiler checker derives its targets from the runtime contracts. It checks
raw protocol storage/references, opaque field visibility, deep return types,
payload movement through private generic helpers and captures, context ownership,
resource isolation and authority-observer forwarding. Unknown payload type shapes
reject the contract. Unclassified executable storage, payload export, unclassified
application-trait dispatch, reachable unknown forwarding and unsupported reachable
control flow reject the certificate.

The resulting control-flow and callable-flow graph retains caller continuations
after a guarded call. The common dispatch boundary is the only admission cut.
Only its source-proved kernel caller may create that cut; an adapter that calls the lower-level dispatcher
with a literal `true` is rejected.
Horn reachability must report `unsat` for protected payload entry outside that
cut, and an independent traversal must agree. Loops and recursion have no path
length enumeration bound. Solver identifiers use the smallest bit width that
uniquely represents every compiled node; unused identifier values add no edges
and this does not bound path length. Every monitor helper is an analysis root, including
private helpers. Bridge closures may contain at most one protected entry and
cannot cycle through it. A new adapter that duplicates or loops a payload call
is rejected; composition uses separately mediated operations. Adding an operation
does not grant admission to its caller's later operations.

## Contract boundaries

Authoritative gate/stamp observations and pure permission metadata obey their
interfaces. The checker binds concrete context forwarding to the original owner;
the existing session and metadata checks cover their respective contracts.
Ordinary Rust privacy/borrowing, standard containers, compiler extraction and
solver correctness are trusted. This certificate proves monitor use and the
monitor's stated contract; it does not certify an external credential verifier.

Admitted application bodies retain raw egui, app and backend access. Their
arbitrary effects, destructors, already-disclosed output and explicit standalone
legacy APIs retain the documented trust boundary. The certificate is not a
whole-program information-flow or cancellation theorem. Protected construction
continues to reject untyped hooks, and admitted scopes use local extension stores.

## Run

```sh
python3 verification/install.py proofs
python3 verification/mediation.py --require-proved
python3 verification/contract_controls.py
python3 verification/mediation_controls.py
```

The strict command succeeds only when both compiled feature profiles pass the
contract checker and the source-bound kernel receipts are current. Evidence is
written to `target/assurance/protected-contracts/` and
`target/assurance/central-mediation/`. The broad legacy reachability diagnostic
remains available without `--require-proved`; its candidates are not the protected
contract certificate.
