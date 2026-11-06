# Issuer-controlled device sessions

Enable `[access] session_tokens = true` to require a live issuer-controlled session
and explicit operation grants. The authenticator owns a `SessionController`;
Fairing retains an `Option<SessionToken>`. Tokens contain no credentials and expose
no mutation API. The controller is not cloneable. Token clones read one shared
session, and controller disposal terminally revokes all of them.

## Issue after authentication

The existing `Authenticator` trait handles credentials, request cancellation and
responses. After verifying credentials, retain the controller and return
`AuthOutcome::Session(token)`. Asynchronous results must retain their original
`AuthRequestId`; a canceled or superseded request cannot install its token.

```rust
use fairing::access::{
    AuthOutcome, AuthRequestId, AuthResponse, SessionController, SessionError,
    SessionGrant, Subject,
};
use fairing::services::Waker;
use std::time::Instant;

// Call only after the authenticator has verified this subject.
fn finish_verified_login(
    request: AuthRequestId,
    subject: Subject,
    now: Instant,
    waker: Waker,
) -> Result<(SessionController, AuthResponse), SessionError> {
    let grants = vec![
        SessionGrant::base("measurement.run"),
        SessionGrant::base("results.view"),
        SessionGrant::base("results.export"),
        SessionGrant::inactive("device.configure"),
    ];
    let (controller, token) = SessionController::issue(subject, grants, None, now, waker)?;
    Ok((controller, request.respond(AuthOutcome::Session(token))))
}
```

The grant list is also the controller's immutable gate catalog. Include inactive
entries for gates that may be granted later. `replace_grants(grants, expires_at,
now)` replaces the complete grant set: omitted catalog gates lose their base and
temporary permissions. Duplicate or unknown gates and unrepresentable deadlines
reject the update before publication. A new gate or a different user requires a
new pair. This bounds each controller's authority to its issued catalog.

Use the shell's monotonic `now` when issuing and updating sessions. Scheduled
session and temporary-grant deadlines participate in Fairing's next-wake selection.
The controller wakes the UI after publication or revocation. Issuance and updates
allocate on the issuer side; token reads allocate nothing and do not wait or retry.
Their work is bounded by the issued catalog, not by a universal execution-time bound.

In `routing` mode, or without a built-in prompt method, token mode emits
`AccessEvent::SessionRequested { request, gate }`. The host authenticates and calls
`Shell::accept_auth_response(request.respond(AuthOutcome::Session(token)))`.
`Shell::cancel_authentication` invalidates that challenge. Unsolicited, stale and
consumed responses cannot install a session. The normal `UnlockRequested` event
remains available for presentation/auditing.

## Multiple roles and independent permissions

Roles belong to the authenticator's account/policy model. Expand all assigned roles
into a deduplicated gate set before issuance. An engineer may receive configuration
permissions, an analyst may receive result/export permissions, and one user may
receive both. Neither role must rank above the other. Administrator permissions are
explicit too; the name does not imply every gate. `Subject.level` remains validated
identity/display metadata and can be used by an explicitly installed local policy.

Token mode does not use the default numeric `LevelPolicy` to infer grants.
An integrator-installed `AccessPolicy` can only narrow the issued permissions:

`allowed = !quarantined && current_token_allows(gate) && local_policy_allows(gate)`.

Without an explicitly installed policy, the last term is true. Issuer permission
is checked again after a custom policy callback; revocation during that callback
cannot authorize a later protected effect. Missing tokens,
unlisted gates, expired permissions, revoked sessions and incoherent publication
reads deny protected work. A declaration without an explicit gate still uses its id; include explicit
grants for any screen that should be reachable. A single configured level does not bypass token
checks, and combining token mode with `mode = "off"` is a configuration error.
Legacy `AuthOutcome::Granted(Subject)` and direct `set_subject` do not grant token
permissions. Direct subject replacement detaches the token instead.

Reading account entries requires the current `settings.credentials` grant; token
detachment clears the cached list and management note. Opening that screen does
not grant its write operations. Token mode also
requires the applicable `settings.credentials.set_secret`, `.set_level`, `.add` or
`.remove` grant. Each operation rechecks the current token, including within one
batch. The authenticator's `CredentialAdmin` enforces account/role-specific rules
using `set_actor`; the existing level-editing screen is not a general role editor.

## Changes, expiration and logout

A controller can replace permissions without a new login. A snapshot is bound to
one publication revision: later checks through that snapshot deny after its revision
changes. The subject and session identity never change within a pair. Identities
and publication revisions never wrap; exhaustion refuses issuance or revokes the
controller rather than reusing authority.

`SessionGrant::temporary(gate, until)` grants an additional permission until the
exclusive deadline. A base permission can coexist with it. Expiration checks the
current base grant; it never restores a copied pre-elevation permission set. A
whole-session deadline ends the observing session and cannot be renewed after
expiry at the publisher's supplied clock. Token mode uses these issuer deadlines
instead of the legacy `unlock_mode` temporary-subject restoration.

Fairing observes token state before rendering, during logic-only updates, before
queued commands, and at direct protected-operation boundaries. It revalidates open
screens after permission publication even when the numeric level has not changed.
Issuer revocation does not enter the bounded command queue. A frame already in
progress can finish its admitted rendering pass after a concurrent change; the
next observed update closes denied screens. Physical presentation latency and scheduler progress
are host/platform responsibilities.

`SessionController::revoke()` is terminal. Fairing drops revoked/expired tokens,
invalidates pending authentication and denies further protected work. Fairing's
logout, lock, timeout, subject replacement, authenticator replacement and fault
quarantine detach local authority before notifying
`Authenticator::session_ended(session_id, now)`. Implement that callback to revoke
only the matching controller; an old session's notification must not revoke a newer
controller. Local detachment already denies work even if notification fails.
Other readers of the pair are revoked by its controller owner.

A busy publication denies the current read but does not drop the token. At a
protected operation's check, the successful read is its authorization observation;
a subsequent concurrent revocation does not undo effects already admitted. Backend
business authorization and cancellation of an already running device operation
remain integration contracts.

Use independent controller/token pairs for web and device sessions. A device logout
must not revoke an unrelated web session. Do not share a pair between domains that
need independent login state. Session IDs are process-local opaque identities, not
network authentication tokens or cryptographic nonces.

## Security evidence and assumptions

Typed application declarations and nested scopes have an additional callback-entry
contract described below. This does not change the issuer/storage assumptions.

Kani and Verus verify the exact six production functions in
`access/session_rules.rs`, not reimplemented decision functions. They cover all
u64 identity/revision/deadline values, the total observation partition, issued-grant
and local-policy intersection, and terminal authority transitions. Three additional Kani harnesses compile the
actual atomic publisher/readers with two permission cells and arbitrary u64
values, checking stored permissions, stale snapshots and terminal revocation.
These storage executions are sequential; concurrent executions have separate
Loom/SC evidence. Verus also proves
an arbitrary-length terminal-history lemma and a stable-even-interval lemma over
monotone sequentially consistent (SC) revision traces.

The SC trace lemma is conditional on the trace/publisher discipline. Structural
source tripwires bind the production publisher and readers to those decisions;
they are review guards, not a complete Rust concurrency refinement proof. Loom
exhausts two bounded scenarios using the actual publication atomics: mixed-version
permission reads and revocation racing publication. Native public-API histories,
512-case generated histories, Miri and the API fuzzer exercise the real capabilities.
Positive and compile-fail clients check controller non-clonability and the reader's
lack of revoke/promotion APIs. Negative controls must reject semantic defects in
both verifiers, three additional concrete-storage mutations in Kani, and
separate defects through product witnesses.

The authenticator is trusted to verify credentials, expand roles correctly and use
its controller only for authorized changes. Controller updates are authoritative;
do not publish a canceled authentication attempt directly into a live controller.
Return a new request-bound session through the authentication response path instead.
Rust type privacy restricts safe client APIs; it does not isolate malicious code in
the same process. Standard atomics, monotonic clock inputs, waking/scheduling and
arbitrary host callbacks are explicit assumptions. This evidence does not establish
complete application security, OS isolation or cryptographic authentication.

## Typed permission requirements

Enable `config.access.session_tokens` and call `build_protected`. Define your own
permission enum or small value struct and implement `Permission::gate(self)` to
map each value to its issuer-controlled token-grant identifier. Different variants
of one enum can name independent grants. Fairing owns the token comparison and
callback dispatch; permission metadata does not grant authority.

```rust
use fairing::{action, Gate, LaunchAction, ProtectedShell, Shell, ShellConfig};
use fairing::access::{Permission, Protected, SessionGrant};
use fairing::elements::{element, ElementTree};

#[derive(Clone, Copy)]
enum AppPermission { ViewResults, ExportResults }
impl Permission for AppPermission {
    fn gate(self) -> Gate {
        Gate::borrowed(match self {
            Self::ViewResults => "results.view",
            Self::ExportResults => "results.export",
        })
    }
}

fn build(ctx: &fairing::egui::Context) -> fairing::Result<ProtectedShell> {
    let mut config = ShellConfig::default();
    config.access.session_tokens = true;
    let mut shell = Shell::builder(config).build_protected(ctx)?;
    shell.add(Protected::new(AppPermission::ExportResults, action("export", |cx| {
        let _ = cx.app::<usize>();
    })));
    shell.add_elements("results", ElementTree::column(AppPermission::ViewResults, 8.0, vec![
        element(AppPermission::ViewResults, fairing::egui::vec2(200.0, 32.0), |ui, _| {
            ui.heading("Results");
        }),
        element(AppPermission::ExportResults, fairing::egui::vec2(200.0, 48.0), |ui, cx| {
            if ui.button("Export").clicked() { cx.launch(LaunchAction::run("export")); }
        }),
    ])?);
    // The issuer supplies these values after credential verification.
    let _grants = [SessionGrant::base_for(AppPermission::ViewResults),
                   SessionGrant::base_for(AppPermission::ExportResults)];
    Ok(shell)
}
```

`SessionGrant::temporary_for(value, until)` and `inactive_for(value)` use the same
mapping. A token snapshot can query `allows_permission(value)`. Values mapping to
the same grant identifier intentionally alias; role expansion and business-policy
classification belong to the application. There is no implicit administrator or
public bypass.

### External elements and framework-owned layout

Implement `PermissionBound` with a mandatory `required_permission` method returning
your enum value. Custom widgets/painters additionally implement `elements::Element`
(`measure` and `paint`). Construct `ElementTree::leaf`, `row` or `column`, then call
`ProtectedShell::add_elements`. For ordinary `Screen` implementations with the same
metadata contract, call `add_screen`. Neither API requires application code to call
a guard around each dispatched hook or child.

Fairing excludes denied children before measuring and allocating row/column space,
gaps or input regions. It builds geometry in library code, validates the plan, then
checks live authority before each paint callback. Measurement callbacks may change
issuer state: a changed session identity or publication revision discards the
prepared plan. Already-admitted output is not retroactively undone. Invalid sizes
are omitted. Stable structural node paths and grant identifiers namespace widget
IDs so hiding a sibling does not move another widget into its retained state.

A registered root screen keeps its original requirement until replacement; a
screen that reports a different identifier is not executed through that binding.
Child requirements are read per plan and checked again before paint. Declaration
and value-mapping methods are side-effect-free metadata contracts. Rust compilation
enforces their presence and type compatibility, not purity of arbitrary user code.
The [extension checker](permission-metadata-check.md) checks a conservative,
terminating subset of compiled workspace metadata implementations. Run it in the
application workspace; dependency implementations outside that workspace remain
outside its certificate.

### Binding and existing declarations

`Protected::new(value, declaration)` supports screen/action/status/nav/tile
registration. Its private `PermissionBinding<P>` holds the copied user value and
normalized identifier, without caching authority. The declaration's old `.gate`
is replaced by that identifier. Denied screen/action icons are hidden. Status/nav
callbacks and tile panels retain live guards after erasure; token-mode nav cells
exclude denied custom declarations before sizing. Disabled built-in Back remains
visible. Factories are bound before construction and checked again afterward.
Denied lifecycle hooks, including `Destroyed`, are skipped; host events/destructors
handle cleanup that must run after revocation.

The original app/backend references remain available after admission. Icon/widget
extension stores lent by a protected callback are local to that invocation, even on
panic. Keep application-owned stores inside your element when persistence is needed;
do not retain custom icon IDs from callback-local stores for a later call.

Legacy `Shell` remains available. Protected construction rejects untyped global
builder hooks, including procedural wallpapers and image loaders, before setup;
data-only wallpapers remain supported. Use the element tree for typed custom UI.

### Evidence and scope

Kani and Verus check the exact admission decision, and Kani checks the actual
callback branch over arbitrary effect/payload clients. Public compiler clients
check missing metadata, missing mappings, incompatible tree permission types and
private storage. Product and mutation tests cover enum-value separation, external
registration, pre-layout filtering, authority/metadata invalidation, stable widget
identity and per-callback rechecks. These establish the implemented component and
execution-path evidence; [complete mediation](callback-mediation.md) remains unproved.

Raw egui/app/backend effects inside an admitted callback, trusted authentication,
backends and destructors remain documented boundaries. The library cannot infer
arbitrary data sensitivity or cancel work already admitted. Permission lookup is
live, not a zero-cost authority check. Bindings normalize identifiers at creation;
tree plans allocate ordinary vectors and revalidate metadata/authority. No new lock,
dependency or universal latency guarantee is introduced.
