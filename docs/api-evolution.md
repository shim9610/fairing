# Public API evolution policy

The reference checkout for this change is `217373f`. This policy covers both
product crates and all supported public feature combinations. The first release
is still unpublished; intentional source changes are listed in the migration
guide rather than attributed to a published upgrade.

## Inventory and comparison

Run with the repository's pinned Rust toolchain and normal build dependencies:

```sh
python3 tools/api_surface.py --output target/api-surface/current.json
```

The script uses rustdoc's unstable JSON output with `RUSTC_BOOTSTRAP=1` only for
these documentation commands. It records the compiler identity, extractor hash,
source/manifests/lockfile hashes and four package/feature profiles. It inventories
exported paths, inherent public methods, public fields, variants, trait items and
emitted hidden diagnostic APIs. Local and foreign glob reexports remain explicit
references; the individual names they export are not expanded. Foreign item
bodies are not expanded into Fairing's owned surface.

Create a baseline checkout and compare using the **same extractor and compiler**:

```sh
git worktree add --detach target/api-baseline 217373f
python3 tools/api_surface.py --root target/api-baseline \
  --output target/api-surface/baseline.json
python3 tools/api_surface.py --compare target/api-surface/baseline.json \
  --output target/api-surface/comparison.json
```

The baseline checkout needs its ordinary Cargo/system inputs. The output reports
added, removed and structurally changed declarations per profile. Review every
removal/change against the decisions below and the migration guide. This tool
reports changes; its exit status does not certify semantic-version compatibility.
Private representation, auto traits, macro expansion in downstream crates,
layout, behavioral and timing changes need separate review and consumer tests.
Keep the report and build logs with the implementation PR's evidence.

## Decisions by integration surface

| API group | Decision and reason |
| --- | --- |
| Config module structs in both products | Extensible configuration; non-exhaustive structs retain public fields for live customization. All have `Default` and synchronous consuming `configure` methods. Adding optional configuration uses a default; existing field/serialization semantics remain supported |
| Runner `Options` and connection `InboxConfig` | Non-exhaustive with `Default`/`configure`, preserving their existing fields while allowing future options |
| `SettingsConfig` and other builders with private state | Retain existing constructors/builders; private state already prevents exhaustive external construction |
| Session capabilities and bindings | Keep private ownership/state; a reader cannot mutate issuer authority. New convenience methods must preserve issuance/catalog and revocation rules |
| Session grants | Constructors and getters replace public fields; the full constructor supports independent base and additional temporary grants |
| Session errors/declaration kind enums | Non-exhaustive; future failure and registration categories can be added without forcing exhaustive downstream matches |
| Closed value models such as levels, clock formats, geometry and gesture decisions | Keep their existing representation and matching contract. Exhaustiveness is useful for these models; extending one requires explicit compatibility review |
| Declaration builders and element traits | Supported application integration APIs. Applications retain permission vocabularies and callback bodies; registration and dispatch stay library-owned |
| Ordinary desktop mutation | Remove `desktop_mut`. Use named background, page, dock, legibility and painter operations; icon placement still uses declaration add/remove and `set_badge` |
| Ordinary status/navigation mutation | Remove `status_bar_mut` and `nav_bar_mut`. Commit owned item specs/slot lists or use named presentation operations. Status writes invalidate derived placements; bar heights update metrics instead of temporary resolved fields |
| Metrics, backends, access and keyboard mutation | Remove the four remaining component loans. Use owned metrics replacement, attached backend replacement, concrete `ServiceRequest`, policy/legacy-subject operations and keyboard operations. No edit closure receives the original component |
| Registry store | Private implementation; external queries move to `ShellView` and no declaration store is borrowed |
| Protected observation | `observe()` lends `ShellView` only. No mutable shell/component conversion, backend loan or extension-store access is exposed |
| Hidden prompt/animation/gesture/notification diagnostics | Retain existing read-only signatures as supported test instrumentation; adding a feature gate would itself require migration. Future observers expose only the required values |
| Runner/harness operations | Sealed `HostedShell` supports both modes using the same orchestration. External implementations are intentionally prohibited |
| Recovery factories | Trusted host construction; the protected factory returns `ProtectedShell`. It is not an application draw callback. It closes old admission first and never replays old work |
| Built-in settings | `add_all_protected` and `add_all_protected_with` map IDs to application permission values at registration. The mapper is trusted host setup; metadata mappings and issuer grants still follow their documented contracts. Per-operation write checks remain separate |
| Public services/screens/elements/authenticator/policy/painter traits | External implementations are supported. Required-method additions break callers; object safety and default-method authority need explicit review. Admitted callback effects and external credential verification remain trusted |
| Public free functions, widgets, layout helpers and standalone legacy controls | Preserve their existing public paths and documented behavior. They remain direct application APIs; adding a protected host does not sandbox calls made independently by application code |
| egui/widget reexports | Preserve public paths; incompatible upstream type changes require an explicit dependency/API migration |

Runner event/recovery callbacks are trusted host code, like credential issuers
and backend implementations. They are not registered protected UI payloads;
login/bootstrap and replacement must remain available without a live UI session.

Sealing restricts implementors; object safety and auto traits remain observable
contracts. Preserve object safety when extending `HostedShell`, for example by
constraining generic convenience methods to `Self: Sized`. Non-exhaustive structs
also require `..` in downstream destructuring patterns. Existing field types,
serialization semantics and implemented public traits remain compatibility commitments.

## Individual decisions and consumer evidence

[The decision ledger](api-decisions.json) indexes each compiler-exported canonical
contract, including fields, variants and trait methods. Shared usage forms and
rationales are stored once and referenced explicitly by each entry. Each entry records its
retain/change/add/remove decision, current usage form, reason, replacement,
compatibility impact, feature-dependent contract hashes and the explicit exported
aliases captured by the inventory.
The baseline is `217373f`. The [host migration decisions](host-api-decisions.md)
explain the individual component operations and their consumers.

Consumer-reference candidates in the ledger are literal identifier matches in
repository tests, examples, assurance clients and the package consumer. They are
review aids, not type resolution or evidence that every API has a test. The
standalone package consumer is compiled and executed in minimal/default/all
configurations; execution records remain separate from this design ledger.

After generating a fresh compiler inventory, run:

```sh
python3 tools/api_decisions.py
```

This fails on missing/changed decisions, missing explicit aliases from the inventory or any of the
seven removed mutable Shell loans, even if a ledger entry tries to approve one.
To review a deliberate API change, regenerate with `--write --baseline <inventory>`
and inspect the ledger diff; CI never updates the decisions automatically.

## Required evidence for future changes

Compare the compiler inventory using one compiler/extractor version and review
changes under minimal/all features. Compile and execute external consumers with
minimal/default/all supported features, including configuration/grants, custom
traits and protected host lifecycle. Include meaningful compile-fail clients for
private capability and sealed host boundaries.

New protected entry operations use the common contract and compiler discovery.
Run the source-bound kernel/routing gate and the accepted-extension/rejected-bypass
controls when changing these paths; update extraction deliberately when moving
code. A local guard proof does not replace automatic coverage. Run the strict
audit, relevant metadata/product regressions, package consumers and documentation
examples. Record each result against the actual source/toolchain, preserving the
existing distinction between component proofs and whole-application claims.
