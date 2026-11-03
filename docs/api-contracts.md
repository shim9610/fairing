# Public API contracts and verification

The process/IPC implementation is removed. Whole stock-egui heap-exhaustion recovery
is not established; see [the exact owned-path scope](memory-limits.md).

The assurance suite checks the contracts below against the production sources
and public APIs. The table distinguishes exhaustive decision proofs from
generated or bounded executions. A tool installation or a green placeholder
test is insufficient: production mutations must also fail the relevant checks.

The stronger whole-application operability theorem is incomplete. Component proofs
and finite witnesses must not be presented as complete concrete-state coverage or
universal interaction progress; see [formal operability obligations](formal-operability.md).

## Contract map

| Documented contract | Production path | Executable evidence |
| --- | --- | --- |
| Runtime unwind faults latch dispatch off, deny gates and discard partial viewport/platform output; no automatic callback replay | Shell runtime boundaries, runner adapters, Access quarantine, BoundedInbox::close_admission | runtime_faults integration tests inject UI/borrowed-state/direct/queued/backend faults; runner tests inject event/raw-UI faults; approved-payload cleanup and subprocess controls for aborting/double-panicking unknown destructors. Requires unwind; no business rollback or universal OOM claim. |
| Shell replacement closes old admission before construction, never replays old work, and leaves dispatch blocked on failure; recovery UI and restart policy are independent | Shell::restart_with, RecoveryUi, runner recovery callbacks | runtime_faults exercises factory errors/unwinds, rejected replacements, old-shell disposal faults and real button input; runner tests exercise automatic, request-based and failing recovery callbacks. No business/context rollback; unwind-only boundary. |
| Icon storage failure reclaims older cache entries or omits the path; dismissal event allocation failure preserves notification/unread state; failed command factories release their slot | try_flatten, IconCache::try_get, Shell::try_dismiss_notification, ShellHandle::try_send_fallible_with | Real 16/64 MiB one-byte-exhaustion tests exercise zero-allocation cache hits, cold-icon refusal, reclamation, dismissal retry and no rejection logger calls. Stock-egui frame exhaustion is a separate failing negative control, not an acceptance result. |
| Ordered gate levels; explicit assignments and unknown-gate defaults; one level/off bypasses policy | `Access::allows`, `LevelPolicy::allows/required` ([access guide §1–4](guide/05-access-control.md)) | Kani `ordered_gates_are_monotone_and_reflexive`; Verus `level_allows`; generated `public_access` includes Unicode/empty/unknown names and real level names preceding top/bottom aliases |
| A grant must name an existing level, cannot demote an unlock session, and may equal the current level only on a lock screen | `Shell::apply_outcome` → `access::rules::grant_allowed` ([access guide §5](guide/05-access-control.md)) | Kani `grants_cannot_use_unknown_or_lower_levels`; Verus `grant_allowed`; `public_grants` exercises actual table lookup, prompt purpose and continuation dispatch |
| Direct and queued actions, screens and setting writes check their gate when executed; changing policy revokes open screens | `Shell::launch/set_setting`, `ShellHandle`, `revalidate_access` ([access guide §2.3](guide/05-access-control.md)) | `public_shell_ordering` tracks independent expected action counts/settings through up to 32 operations; same-sender subject/launch order; gate-bypass mutation must fail screen revocation |
| Authentication continuation re-enters the launch gate; a session boundary cancels the old check before polling | `apply_outcome`, `observe_subject_change`, `request_unlock`, `cancel_prompt` ([access guide §5](guide/05-access-control.md)) | `public_grants`; fixed `session_boundaries_cancel_before_polling_and_deadlines_are_inclusive` covers logout, same-level identity replacement, gate retargeting, lock and Back |
| Overdue session timeout, temporary elevation and idle lock precede queued privileged work and fresh input | Frame state/expiry and access timers ([access guide §7](guide/05-access-control.md)) | `public_expiry` and fixed cases at deadline −1 ms, exactly at the deadline and +1 ms, with and without fresh input; check action refusal, unchanged setting and screen eviction |
| Correlated replies cannot complete a canceled/replaced challenge or replay a continuation; queue batches preserve execution-time gates and FIFO admission | `AuthRequestId::is_active` → `response_is_current`, `Shell::drain_commands/tick_access` | Kani/Verus check exact token matching; `transitions` compares the real shell with an independent oracle across 8,748 matrix traces and 128 generated traces, a Miri replay and twelve deadline cases; omitted correlation, an excessive batch and late expiration must fail semantic controls |
| PIN and pattern attempts share the stricter allowance and longer lockout; refused grants restore their cleared attempt history; reopening does not reset lockout | `PinTable::submit/refused` ([access guide §5](guide/05-access-control.md)) | `public_authentication` uses an independent attempt/deadline model for up to 64 operations; Miri fixed sequences include lock expiry equality and mixed methods |
| Failure accounting cannot wrap; the attempt limit is inclusive | `PinTable::fail` → `next_failure/lockout_reached` | Kani `failure_accounting_never_wraps`, `lockout_is_inclusive_and_cannot_be_disabled_by_more_failures`; Verus contracts on both real functions; product regression at `u32::MAX` |
| Accepted Boolean settings coalesce to the latest target and wait for the previous backend acknowledgement | `set_setting` → pending Boolean handling ([services guide §5](guide/06-services.md)) | Existing Kani/Verus ACK proofs plus `public_backend_ordering`: actual `ShellHandle`, setting gates and a recording delayed Wi-Fi backend; no overlapping writes and exact independent expected dispatch list |
| Inbox has only nonblocking receive APIs and preserves messages and per-sender order | `Inbox::pair/try_recv/drain` ([screen guide](guide/02-screens.md)) | Two Loom scenarios: early/final drain, and a concurrent consumer with two senders/three messages; native and Miri run the same source with real channels; a drop-message mutation must fail |
| Bounded admission shares capacity across clones, preserves rejected ownership, skips rejected factories and returns slots; update budget preserves accepted order | `Inbox::bounded`, `ShellHandle::try_send*`, builder limits ([memory limits](memory-limits.md)) | `public_memory` FIFO oracle; native/Miri close races and factory unwinding; Loom atomic-admission scenarios; shell cap-2/budget-1 integration regression |
| Message-based mocks default to bounded storage and finite polls; explicit unbounded storage keeps finite work, full batches schedule continuation, and zero budgets pause without spinning | `MockControl::try_send`, mock `with_inbox` constructors, `MockInbox`, `Backend::next_wake` ([service guide](guide/06-services.md#bounded-service-channels)) | Product `services::mock::inbox_tests`: all six default capacities and per-poll limits, FIFO continuation, pause/resume with existing controls, rejected payload ownership/disconnect, timed scan progress under backlog and invalid capacity |
| Inline pool handles remain pool-specific and removed generations never revive | `FixedPool`, `next_generation` ([memory limits](memory-limits.md)) | `public_memory` live/stale/foreign-key model, Miri and API fuzzer; Kani four-operation two-slot sequences and all u64 generations; Verus exact generation body; both reject generation wrap |
| Display retention refuses or defers optional work under pressure; diagnostic allocator stays outside product graphs | `try_toast/try_notify`, ToastQueue, NotificationCenter ([memory limits](memory-limits.md)) | `public_display` and hard-cap notification properties, native/Miri fixed cases, real 16/64 MiB exhausted-arena factory/text failures, continued preallocated queue work and display resumption; BOM gate rejects diagnostic allocator edges in either product |
| Newest-only inline display retention and clipped chronological views; bounded CPU preview/warning have no allocation or font dependency | `memory::DisplayWindow`, `paint_image_preview`, `paint_display_warning` ([memory guide](memory-limits.md)) | Native FIFO oracle including zero capacity, privacy and geometry cases; generated/Miri/fuzz API contracts; Kani four arbitrary samples in a two-slot window; Verus exact viewport arithmetic over usize inputs; both reject a broken clipping body; real 16/64 MiB exhaustion keeps CPU warning/preview and 10,000 work commands running with zero allocations |
| Pending display loss never evicts an accepted device command; optional backdrop processing refuses allocation failure | `ToastQueue` KeepLatest, Shell builder/status, actual card-frost source | Latest-display/command integration regression; invalid/duplicate input retention tests; actual exhausted-arena oldest-text replacement, skipped text/blur work; bounded/invalid-image processor tests |
| Notification ids deduplicate; source groups stay contiguous; persistent items resist dismissal; hard caps include persistent entries, refusing new overflow; gated content hides | `NotificationCenter`, `shows_content` ([notification source](../crates/fairing/src/notify/center.rs)) | `public_notifications` checks invariants after up to 64 operations; fixed persistent-flood/refusal regressions; Miri, libFuzzer and a cap-removal mutation |
| Back routing covers every valuation of its observed predicates, selects exactly one group and dispatches each independently permitted Back to its specified target | `Shell::back_in` → `back_route::classify/dispatch` | Kani three harnesses and Verus contracts on exact source; all 256 predicate valuations and nine disjoint groups; both reject a missing group and a refused permitted operation; native field/keyboard controls reject wrong snapshot binding and an omitted hide call. This is dispatch, not whole-UI operability. |
| Every service polls once with the supplied time and the minimum reported deadline wins | `Services::poll/custom` ([services guide §1](guide/06-services.md)) | `public_services`, two independently recorded backend types; Miri fixed deadline/missing-deadline cases |
| UTC conversion saturates, calendar fields are valid, formatting clears a reused buffer and changing hour convention preserves shape | `WallTime`, `ClockFormat` ([services guide: clocks](guide/06-services.md)) | `public_calendar` with arbitrary u64/i32 inputs, inverse Gregorian conversion and all formats; Miri boundary values and API fuzzer |
| Pixel conversion clamps; changing a container changes fractional references without changing physical, finger or text terms | `round_u8/round_i8`, `Dim`, `Scale::in_container` ([unit source](../crates/fairing-widgets/src/unit.rs)) | `public_units` uses arbitrary f32 bit patterns, NaN/infinities and arbitrary u16 dimensions; Miri/fuzzer; compile-fail anchor contracts remain in product documentation tests |
| Invalid ratios/grid dimensions fail validation; malformed configuration and credential debug/error output do not reveal entered secrets | `ShellConfig::from_toml`, `Access::from_config`, `Credential::Debug` ([configuration guide](guide/07-config-reference.md)) | Generated `public_configuration`, `credential_debug_is_independent_of_contents`, parser/redaction properties, Miri fixed cases and parser/API fuzz targets |
| Tool dependencies stay outside product packages; dependency inventories match the locked target/feature graph | Published manifests, primary lockfile and package BOMs | Input approvals/audit/deny; eight product and eight diagnostic/utility CycloneDX BOMs, official schema validation and reconciliation with Cargo normal/build edges |

Test names in this table are in
[`verification/src/api.rs`](../verification/src/api.rs),
[`src/lib.rs`](../verification/src/lib.rs) and the
[`Kani harnesses`](../verification/kani_access.rs). Verus checks the exact
production bodies in [`access/rules.rs`](../crates/fairing/src/access/rules.rs);
the adapter adds contracts and visibility only. Kani/Verus also check the
original ACK kernel.

## Bounds and assumptions

- Kani proves seventeen harnesses. Verus verifies twelve production functions and
  an additional quiet-pass progress obligation.
  Pool sequence proof is bounded to four operations on a two-slot u8 pool; the generation
  decision covers all u64 values. Kani treats pool-identity atomics sequentially;
  it does not prove concurrent identity creation. The access rules cover all u16 levels, all u32 counter values and every
  optional attempt limit. They are exhaustive decision proofs, not a proof of
  the complete egui application.
- The command/reply oracle checks 8,748 fixed matrix traces and 128 generated traces
  of at most 96 operations (CI overrides the generated count to 512), plus twelve
  queued-work/session-deadline scenarios.
  It compares observable effects after every operation and requires a successful retry
  without restarting away the current state. One fixed trace also runs under Miri;
  ASan fuzzing reuses the oracle. See [the exact matrix](ui-reachability.md#commandreply-transition-oracle).
- Native tests contain fourteen properties with 512 cases each and four shell
  properties with a default of 64 cases each, using seed 20261101. CI sets
  `PROPTEST_CASES=512`, overriding every generated property's default. Operation sequences cap
  at 32 shell/backend operations or 64 non-UI operations. Fixed cases ensure
  the important deadline/credential boundaries execute regardless of the seed.
- UI reachability adds 192 fixed viewport/history/modal/motion/input/density combinations,
  96 generated histories by default (512 in CI, up to 16 operations), twelve required pointer-input edges and
  24 refusal/lockout/recovery configurations. The actual
  shell must resume ordinary UI on a quiet pass and satisfy each edge after intentional
  blockers stand aside. Kani/Verus prove the shared input decision, not the rendered
  correspondence; native witnesses check that separately. See [UI reachability](ui-reachability.md).
- Miri interprets twenty-five assurance tests and the production supported-payload cleanup test
  with normal isolation, validity and race
  checks. It covers the actual Access, PinTable, notification, calendar, unit,
  service, fixed-pool and bounded/unbounded Inbox APIs, plus a fixed logic-only
  command/reply/replacement trace. It does not interpret the complete shell rendering
  path; generated cases run natively.
- Loom exhausts its four bounded scenarios without a preemption/permutation/time
  cap. The larger scenario has four threads including the caller. The branch
  safety limit is 1,000 and exceeding it fails. Loom 0.7.2 does not model the exact
  empty/disconnected error distinction. The bounded transport substitutes a model
  channel (Loom has no sync_channel) while retaining production admission atomics.
  A close-after-drain model avoids the stub's failed-send/destruction miscount;
  real concurrent close uses native channels and Miri. These are not proofs of
  std's channel internals or of the optional allocator.
- The delayed backend model acknowledges one command at a time. Accepted,
  previously authorized intent may finish after a later subject change; newly
  denied writes do not change that intent. A fresh failure is terminal in the
  ACK-kernel model. Integrators must honor these backend assumptions.
- Authentication replies carry an opaque Fairing-issued request token. The shell checks
  that token against the active request and consumes terminal responses once. Backends
  retain the original token with asynchronous work, including work completing after cancel.
  Tokens correlate trusted backend results; they do not authenticate a network peer.

- Generated deadline cases exercise the deterministic frame-only timeline.
  Integer-second deadlines are exact; the ±1 ms probes allow less than 1 µs
  of interval rounding in the frame's f32 representation. Existing product
  background/logic-only regressions continue in the strict audit.
- ASan (AddressSanitizer) campaigns run 20,000 ACK inputs, 10,000 parser inputs,
  10,000 public non-UI API inputs and 2,000 shell/backend/grant inputs.
  They provide bounded bug-finding evidence, not an exhaustive proof.

Rendering, arbitrary gestures, every optional runner/platform combination,
integrator callbacks, schedulability and hardware behavior are covered only
to the extent of the existing product tests and audits. They have no new
universal proof here. SBOM validation establishes inventory structure and
graph consistency, not runtime behavior.

## Controls and coverage

`verification/proofs.py` requires both verifiers to reject twelve mutations:
reversed ACK condition, stale-response acceptance, reversed level order, accepting
an unknown grant, wrapping failure count, excluding lockout equality, wrapping a
pool generation, broken display clipping, excessive input deferral, modal capture, a missing Back group and ignoring a permitted Back operation.
The controls require a semantic verification error; compilation errors fail
the control instead of counting as a successful rejection.

`verification/controls.py` copies the sources into a disposable directory,
checks a passing baseline, then breaks production gate delegation,
notification cap enforcement, bounded factory admission, Inbox delivery, input progress,
modal priority, response correlation, the per-update command budget, deadline ordering, Back snapshot binding and Back handler execution separately. The matching
public API/Loom test must fail with its expected assertion. Working sources
and published dependencies remain intact.

`verification/coverage.py` reports product code executed by the contract suite.
It fails if any of fourteen claimed production modules has no executed source
lines. cargo-llvm-cov's dependency selection must explicitly instrument both
product crates; its default dependency-path filter excludes non-registry
dependencies. The command therefore disables that default report filter and
the script selects canonical product source paths from the raw export.
This is distinct from the whole product unit/integration coverage in the
Ferrocene workflow. Both raw and per-file exports are retained, with no
arbitrary certification threshold. The latest API-suite totals are retained in the workflow coverage artifact;
this targeted figure is separate from the full product test suite and is not
claimed as whole-API coverage. Fresh coverage artifacts record totals per commit; earlier percentages describe
the previous allocator prototype. The isolated diagnostic allocator is outside
product API coverage.

## Defects found while checking the contracts

The original failure count used `failures += 1`. Kani produced an overflow
counterexample at `u32::MAX`. Saturating accounting preserves the attempt
history, including the restoration performed by `refused`.

The notification cap was applied only to newly inserted ids. With a cap of
one, insert two persistent ids and update one to be dismissible: the old
implementation retained two items even though one could be dropped.
proptest reduced this to three push/update operations. Updates initially reapplied the oldest-non-persistent eviction rule. The later
memory-pressure correction makes the cap hard even for persistent entries,
refusing protected overflow before insertion. Both rules have product regressions
and the hard-cap bypass must fail its negative control.

## Run

See [the tool installation and workflow guide](rust-assurance.md) first.

```sh
python3 verification/check-inputs.py
python3 verification/install.py all
python3 verification/proofs.py
PROPTEST_CASES=512 PROPTEST_RNG_SEED=20261101 \
  cargo test --locked --manifest-path verification/Cargo.toml --lib
RUSTFLAGS='--cfg fairing_loom' \
  cargo test --locked --manifest-path verification/Cargo.toml \
  --no-default-features --features loom --lib
MIRIFLAGS='-Zmiri-seed=0' cargo +nightly-2026-10-30 miri test \
  --locked --manifest-path verification/Cargo.toml --lib
python3 verification/controls.py
python3 verification/fuzz.py
python3 verification/coverage.py
```

CI uploads contract controls, per-module coverage, proof controls, Miri/Loom
logs, fuzz corpora and BOMs. A passing run establishes the recorded contracts
within these scopes and assumptions; it is not a device certification.

## Panic payload contract

Only `String`, `&'static str` and `recovery::PanicCode` payloads are supported for
cleanup. Expected API failures use typed return values. Unknown payload destructors
are not invoked: those objects are retained and counted, including across shell
replacement. This preserves dispatch quarantine without claiming bounded retention.
`PanicCleanupStats::released` replaces the earlier experimental `cleanup_panics`
counter; `last_code` is a process-wide diagnostic, not a per-shell fault association.
The numeric payload type provides no helper for initiating a panic inside Fairing.
The dedicated Miri test checks supported payload cleanup with leak checking enabled;
unknown-payload controls run natively in subprocesses because retaining those objects
is intentional and not a leak-free contract. A local negative control which executed
unknown destructors was rejected by the aborting-destructor regression.

## Gesture ownership and current geometry

Region placement uses current shell/pane geometry and the keyboard band. Guards receive
current area state before user drawing and are retired when inactive or removed. The
gesture engine retains press ownership; a region-owned press cannot also advance a hidden
corner sequence. Knock travel records maximum observed displacement, including out-and-back
strokes. The emergency hold remains independent of region ownership.

Since egui hit-tests previous-pass widgets before `Shell::frame`, a changed region map
claiming a fresh press causes ordinary UI callbacks to be omitted for that pass. Guards
and region callbacks still run; backend/authorization processing and queued work have
already run, and a repaint is requested. Tests cover immediate press/release over an
existing button, moving a region, stale held-button repeat, resize/policy/keyboard changes,
region removal before release and ordinary input resuming after omission. This does not
replay input or provide atomic rerouting to widgets outside a claimed region.

## UI progress and required input paths

The production `shell/input_route.rs` decision is shared by the shell, Kani, Verus
and Miri. The required six-edge UI fixture uses real egui buttons, Cancel, on-screen
PIN keys and a region action; it never substitutes a host command or hardware-key
shortcut for a required edge. Generated region/rotation histories precede the
witnesses. A missing edge witness, sticky deferral or stale guard covering a modal
fails the suite. The latter two defects are deliberate negative controls in CI.
[Scope, fairness conditions and extension rules](ui-reachability.md) describe which
progress properties are proved and which widget paths are checked dynamically.
