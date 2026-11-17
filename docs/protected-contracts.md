# Common protected mediation contract

The proof unit is the common mediation kernel. The compiler checks that every
library-managed protected payload entry uses it. Individual screens, permission
values and tree leaves do not require individual proofs or verifier registrations.
The [security architecture](security-architecture.md) places this contract in the
issuer, host and application trust boundaries.

## One declaration, runtime adapter and coverage

`access/mediation/contract.rs` defines `protected_contract!`. Carrier declarations
emit the actual opaque storage and its associated `Payload` contract together.
Declaration conversions emit the sealed registration implementation and its
contract together. Operation declarations emit an adapter that invokes the shared
monitor. Permission binding methods also use one generated implementation.

For example, after defining a new payload protocol, one operation declaration is
sufficient inside the private `access/mediation/execution.rs` module:

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
private helpers. Each admission permits at most one protected payload entry.
`verification/dispatch_effects.py` tracks fresh/consumed admission through ordinary
helpers, closures, call returns and recursive fixed points. A payload entry
consumes admission. A nested mediated operation starts with fresh admission, but
its return leaves the caller consumed; it cannot restore an earlier grant.
Policy callbacks also invalidate a prior admission. Thus two calls hidden in an
ordinary helper and a raw call after nested mediation are rejected.

External callable consumers require an explicit at-most-once contract:
`Option::map`, `Option::and_then` and `bool::then`. Iterator consumers such as
`for_each` and lazy `map` cannot reuse one admission for repeated payload calls.
Compiled controls cover these cases and accept a valid single forward.

## Execution and layout types

The private dispatcher creates an `ExecutionToken<F>` only after admission and
immediately consumes it with `execute(self)`. Its callback field is private;
it has no clone, borrowing execution or public constructor. The executable action,
UI and factory storage lives in the private `mediation::execution` module.
Callers receive dispatch methods; they cannot borrow the raw payload through
`Callback::invoke`. `ScreenHandle` delegates to opaque screen storage, including
the post-factory recheck. The compiler route checks still inspect code inside
this private execution boundary.

Element geometry uses a separate `LayoutToken<P>`. It carries permission metadata,
frame, observation time and authority identity/revision, with no executable body
or dispatch method. Repeated data validation is allowed only while those values
match. An unversioned custom access policy is queried again rather than cached.
The whole plan is validated before painting; each visited node validates its own
layout token. Measurement and paint callbacks each obtain a fresh single-use
execution attempt, including inherited ancestor requirements. The shared
`SessionToken` remains live across attempts; consuming an execution token does
not log the session out.

Compiled negative controls require Rust to reject a private payload loan, a
forged execution token, repeated execution, execution through a shared borrow and
layout-token execution. Separate semantic controls mutate code inside the
execution module, where Rust field privacy alone cannot detect a bad adapter.

`verification/layout_proofs.py` adds five Kani harnesses over the unchanged
layout-token construction, validity and metadata bodies. The harness also uses
the production permission-binding methods, context observers, Access authority
and cacheability methods, and session snapshot/identity/revision methods.
It checks minting against both authority observations, binding preservation,
frame/time/session identity/revision matching, reusable unchanged data, custom
policy rechecks, and rejection of revoked, expired or incoherent observations.
Ten mutations must fail their specific security assertions, including elapsed-time
overflow incorrectly wrapping to zero.

The proof represents `Instant` with symbolic nanosecond ticks and models atomic
read schedules through the production `session_rules::observe` decision.
Frame/time observations, the existing permission-decision contract and immutable
Access configuration within one layout remain premises. This extends layout
lifetime evidence; the session concurrency proofs retain their separate scope.
The receipt in `target/assurance/layout-token/proof.json` binds the complete input
set and hashes. Strict mediation verification requires this receipt alongside
the existing kernel/path receipts.

`verification/layout_bindings.py` connects the verdict to compiled callers.
`Plan::validate` and `Plan::paint` must check their own token using the calling
context and return false without further work on refusal. Root space allocation
and painting must use the same validated plan, context and stamp, and every path
to those uses must cross a true validation result. A use-site cycle must cross
fresh validation again. Unknown root calls and field writes are rejected; the
root routine uses explicit observation, preparation, egui metadata and repaint
interfaces. Five compiled mutations remove the node/root guards, invert the verdict or
write context fields before checking; all must be rejected. These rules cover the shared plan
routines, without per-widget proof declarations.

The [boundary review and CPU measurements](execution-layout-review.md) record
the two verifier regressions, type controls and baseline comparison.

## Contract boundaries

Authoritative gate/stamp observations and pure permission metadata obey their
interfaces. The checker binds concrete context forwarding to the original owner;
the existing session and metadata checks cover their respective contracts.
Ordinary Rust privacy/borrowing, standard containers, compiler extraction and
solver correctness are trusted. The certificate proves monitor use and the
monitor's stated contract. The application authenticator verifies credentials.

Admitted application bodies retain raw egui, app and backend access. Their
arbitrary effects, destructors, already-disclosed output and explicit standalone
legacy APIs retain the documented trust boundary. The certificate is not a
whole-program information-flow or cancellation theorem. Protected construction
continues to reject untyped hooks, and admitted scopes use local extension stores.

## Run

```sh
python3 verification/install.py proofs
python3 verification/test_validation.py
python3 verification/test_dispatch_effects.py
python3 -O verification/test_validation.py
python3 verification/mediation.py --require-proved
python3 verification/contract_controls.py
python3 verification/mediation_controls.py
```

The strict command above succeeds only when both compiled feature profiles pass the
contract checker and the source-bound kernel receipts are current. Evidence is
written to `target/assurance/protected-contracts/` and
`target/assurance/central-mediation/`. The broad legacy reachability diagnostic
remains available without `--require-proved`; its candidates are not the protected
contract certificate.

Receipt validation checks the complete expected input set as well as hashes and
proof status. Acceptance checks remain active under Python `-O`/`-OO` and
`PYTHONOPTIMIZE`; CI runs negative receipt controls normally and with `-O`.
`--profile all` and `--profile minimal` each check one profile. Receipts bind local
artifacts to source inputs and assume the tools and evidence files are trusted.
