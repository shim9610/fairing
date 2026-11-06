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

Use `Shell::builder(config).build_protected(ctx)` when application declarations
must carry a permission type. It requires `config.access.session_tokens = true`
and returns `ProtectedShell`, whose `add` accepts only `Protected<P, D>`.
The ordinary `Shell` remains the legacy registration API; the protected facade
exposes no mutable shell or conversion back to it. Configure the authenticator and
host backends before construction; these are trusted authority/infrastructure APIs.

`build_protected` rejects untyped painter/layout hooks, widget-painter bundles,
image loaders and procedural wallpapers before attaching backends or executing user
hooks. Both branches of nested themed wallpapers are checked. Data-only wallpapers
remain available. Use typed screen/action/status/nav/tile declarations for application
UI. The legacy `build` API continues to support unmediated host customization.

Protected callbacks receive their original app/backend references, with callback-local
icon and widget-painter stores. Customizations installed through `cx.icons` or
`cx.widgets().painters` last only for that invocation, including on panic; they cannot
install a shell hook that runs after revocation. Keep an application-owned `IconSet`
or `WidgetPainters` and use it inside an admitted callback when persistence is needed.
Custom icon IDs from a callback-local store must not be retained across calls.

```rust
use fairing::{action, screen, Cx, LaunchAction, ProtectedShell, Shell, ShellConfig};
use fairing::access::{Permission, Protected, SessionGrant};

struct ResultsView;
impl Permission for ResultsView {
    const GATE: &'static str = "results.view";
}
struct ResultsExport;
impl Permission for ResultsExport {
    const GATE: &'static str = "results.export";
}

fn build(ctx: &fairing::egui::Context) -> fairing::Result<ProtectedShell> {
    let mut config = ShellConfig::default();
    config.access.session_tokens = true;
    let mut shell = Shell::builder(config).build_protected(ctx)?;

    shell.add(Protected::<ResultsExport, _>::new(action("export", |cx| {
        // Only admitted callbacks receive the original app and backend context.
        let _ = cx.app::<usize>();
    })));
    let mut export_button = Protected::<ResultsExport, _>::new(
        |ui: &mut fairing::egui::Ui, cx: &mut Cx<'_>| {
            if ui.button("Export").clicked() {
                cx.launch(LaunchAction::run("export"));
            }
        },
    );
    shell.add(Protected::<ResultsView, _>::new(screen("results", move |ui, cx| {
        ui.heading("Results");
        export_button.render(ui, cx);
    })));

    // After credential verification, the issuer passes these to SessionController::issue.
    let _grants = vec![
        SessionGrant::base_for::<ResultsView>(),
        SessionGrant::base_for::<ResultsExport>(),
    ];
    Ok(shell)
}
```

`Permission::GATE` identifies the issuer grant. Types sharing the same string name
the same permission; choosing the correct requirement and role expansion remains
policy configuration. There is no implicit administrator or public bypass.
`SessionGrant::temporary_for::<P>(until)` and `inactive_for::<P>()` use the same
requirement type. A snapshot can query `allows_permission::<P>()`.

Every `Protected<P, T>` privately owns a `PermissionBinding<P>` and its body.
The sealed `PermissionBound` trait exposes the requirement, not a cached permit.
The binding consults the *calling context's current session* on every `run` or
`render`; it does not retain a previous user's token or a successful Boolean.
Default legacy/one-level contexts cannot admit typed callbacks.

Protected registration supports screens, actions, status items, nav items and
overlay tiles. It replaces any earlier `.gate(...)` on the declaration with the
type's requirement. Screen/action icons use `Visibility::Hidden`. Each callback
also has its own guard after type erasure: screen UI/back/lifecycle/result hooks,
actions, status/nav UI and tile-panel bodies. A protected factory defers user
construction until its first authorized screen hook and rechecks after construction;
a constructor that revokes the session cannot admit the following hook.
Denied lifecycle hooks, including `Destroyed`, are skipped; use host events and
trusted destructors for cleanup that must occur after revocation.

The exact `protected_dispatch::admitted` body is checked by Kani and Verus.
Kani also compiles the actual generic `invoke` branch with arbitrary u64 effect
and payload clients: denial does not call the body, change the modeled client
effect or return its payload; admission calls once and forwards its result.
Compiler clients reject missing permission traits, untyped registration, mutable
shell escape and attempts to forge a stored permit. Product witnesses and source
tripwires bind these decisions to the UI adapters; those adapters do not have a
complete Rust/egui refinement proof.

Admitted callbacks retain raw egui UI, app state and backend access. They can
perform arbitrary Rust effects. Protect each nested scope or registered operation
that needs a different requirement; the facade cannot infer data sensitivity from
an arbitrary `ui.label` or backend call. It does not erase previously drawn pixels,
stored values, queued toasts or completed effects. Host infrastructure callbacks,
legacy `Shell`, arbitrary destructors and external business authorization remain
trusted boundaries. This is callback admission, not whole-program information-flow
confinement or cancellation of already-admitted work.

Bindings contain only a type marker. Admission still performs live token/policy
checks; it is not a zero-cost authority check. Gate construction borrows a static
name without allocation. Registration adds callback wrappers, and factory instances
have lazy construction storage. There is no new dependency or lock in callback
admission; catalog lookup and atomic observation retain the session read bounds
above. No universal latency or throughput guarantee is claimed.
