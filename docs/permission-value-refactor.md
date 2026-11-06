# Permission values and framework-owned UI dispatch

## Status and checkpoint

This is the implementation specification for issue #11. Runtime checkpoint:
`5789bbc271eb508ee9fa6032142c9b720011d7e4`. That revision fixes the confirmed
painter bypass, but complete mediation remains unproved. This specification is
committed before changing the runtime so the refactor can be reviewed against it.

The immediate milestone is the execution architecture and its connected tests.
A completed architecture is a prerequisite for the later global mediation proof;
it must not be reported as that proof itself.

## Ownership and permission vocabulary

The application defines its permission enum or small value struct. Fairing does
not prescribe domain permissions, role names, enum ordering or an administrator
hierarchy. Different variants of the same enum can name different permissions.

The application implements a value-to-grant mapping. Its returned grant identifier
names an entry in the issuer-controlled token catalog. The authenticator expands
roles into independent grants; Fairing compares the requested identifier with the
current token and any narrowing policy. The mapping cannot mint or modify a token.
Missing, revoked, expired or unpublished grants deny admission.

Replace the type-wide `Permission::GATE` constant with a value method. The value
is stored in an opaque `PermissionBinding<P>`, together with its normalized grant
identifier. `Protected::new(permission, body)` declares an explicit value. Typed
issuer grant/snapshot helpers take a value as well. Existing split controller/token
ownership, revocation and publication semantics remain in force.

Permission values must be cheap to copy. Declaration/mapping methods are metadata
contracts: they do not receive UI, app/backend access or mutable session authority,
and must be side-effect-free. Arbitrary user implementations of those metadata
methods are not automatically proved pure by Rust trait conformance. Extension
analysis must reject effects or unresolved purity obligations. The implemented
[metadata checker](permission-metadata-check.md) applies this requirement to a
restricted subset of compiled workspace implementations.

## Mandatory element contract

Every protected element exposes a required-permission value through a mandatory
trait method; there is no default allow or optional requirement. External screen
implementations can implement this declaration contract and be registered through
a framework API that creates the guarded adapter automatically. Closure-based
screen/action/status/nav/tile declarations use an opaque value-bearing adapter
that implements the same contract.

A registered declaration has a fixed permission binding. Changing its requirement
requires replacing that registration. A dynamically traversed child element may
supply its current value; a changed identifier between measurement and paint
invalidates the prepared entry instead of executing with the old requirement.
Factories require a binding before user construction and retain the post-factory
recheck.

User-owned implementations describe and implement elements. Normal execution is
owned by Fairing: users register elements/trees; they do not have to call a guard
before every child themselves. Safe client APIs cannot extract callbacks or a
mutable legacy shell from a protected registration.

## Framework-owned element tree

Introduce a typed element interface and an owned tree for custom UI. Each leaf
returns its application permission value and implements measurement and painting;
input/click handling performed in its paint callback has the same admission
boundary. Library-owned row/column containers recursively lay out their children.
A tree can be registered as a protected screen, and custom widgets/painters can
be supplied as leaf elements without opening an untyped global hook.

For each frame, Fairing performs:

1. Read requirement metadata and check the current token/policy before invoking
   user measurement or layout-sensitive code.
2. Omit denied children from measurement, the layout plan, spacing and hit areas.
   An entirely denied subtree consumes no child layout space.
3. Build row/column geometry in library code from admitted measurements only.
4. Revalidate permission, requirement identity and authority revision before using
   prepared geometry. Discard stale plans rather than reuse old admission.
5. Check live authority immediately before each paint/input/action callback.
   A denied callback receives no UI/app/backend references and supplies no result.

A measurement callback may revoke or replace grants. Previously measured siblings
must not paint using that stale plan. Permission changes schedule/rebuild the next
frame; already-admitted work and output are not retroactively undone. Layout work
is bounded; invalidation must not trigger an unbounded retry loop.

Measurement results must be finite and nonnegative. Invalid measurements are
omitted without invoking paint. Layout records contain framework-owned indices
and rectangles, not reusable authorization capabilities. Structural node paths and
normalized requirements namespace widget IDs independently of visible sibling
order. Protected callbacks keep the original UI/app/backend access and
callback-local extension stores.

## Existing surfaces and compatibility

Screen/action declarations, status/nav items and tile panels must all accept value
bindings and continue internal dispatch checks. Hidden custom nav items must be
removed before cell sizing, while disabled built-in Back behavior is retained.
Status items must be filtered before width measurement. Permission-dependent
registrations must not retain hit rectangles from an earlier allowed frame.

Legacy `Shell` remains an explicit compatibility API. Protected construction keeps
rejecting untyped global builder hooks; they must not be silently treated as public.
The new element tree provides a typed path for custom layout and paint. Raw egui
calls inside an admitted user callback, trusted host/authentication/backend work
and destructors retain their documented ownership boundary. This specification
covers Fairing-owned protected element entry, not all arbitrary Rust execution.

## Verification and acceptance

- [x] One user enum supplies distinct read/write permissions, with independent
  token grants, replacement, expiry and terminal revocation.
- [x] Omitted declaration methods/types fail compilation; external implementation
  clients compile using only the public API. Users do not invoke child guards.
- [x] All existing protected declaration adapters preserve the chosen value and
  guard every supported hook, including factories and lifecycle callbacks.
- [x] Denied element measurement/paint callbacks are never called; denied entries
  allocate no space, gaps, input regions or output shapes.
- [x] Nested row/column layouts compact after denial/revocation; changed metadata,
  revocation during measurement and invalid sizes reject stale paint plans.
- [x] Live paint callbacks use the current token; sibling revocation cannot execute
  a later callback on an earlier admission result.
- [x] Real production mutations removing admission, metadata checks or layout
  filtering are rejected by meaningful regressions.
- [x] Update public API/migration/security documentation and compiler type schemas.
  Re-run relevant product, component-proof and assurance checks; bind receipts to
  the final sources.

The compiler MIR detector and strict complete-mediation gate remain in place.
Newly unknown paths remain obligations. Source inventories, contract tests and a
local decision proof must not be relabeled as a global absence theorem. Extend
consumer analysis and the whole-library proof after this execution path exists.
No merge, release or branch-protection changes are included.
