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
emitted hidden diagnostic APIs. Foreign glob reexports remain explicit references;
their upstream bodies are not expanded into Fairing's owned surface.

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
| Ordinary `desktop_mut` | Trusted legacy customization, retained for grid/painter editing. Prefer `set_dock_placement`, existing wallpaper loading or typed declaration replacement for those operations |
| Ordinary `status_bar_mut`, `nav_bar_mut` | Retained for live bar/spec customization. Replacing declarations through `add` remains the registration path; new operations must preserve layout/authorization invalidation |
| Ordinary `metrics_spec_mut`, `services_mut`, `access_mut`, `osk_mut` | Retained trusted host controls for device metrics, backend ownership, legacy policy and keyboard integration. Their concrete types are explicit compatibility commitments, not protected callback grants |
| Registry store | Private implementation; external queries move to `ShellView` and no declaration store is borrowed |
| Protected observation | `observe()` lends `ShellView` only. No mutable shell/component conversion, backend loan or extension-store access is exposed |
| Hidden prompt/animation/gesture/notification diagnostics | Retain existing read-only signatures as supported test instrumentation; adding a feature gate would itself require migration. Future observers expose only the required values |
| Runner/harness operations | Sealed `HostedShell` supports both modes using the same orchestration. External implementations are intentionally prohibited |
| Recovery factories | Trusted host construction; the protected factory returns `ProtectedShell`. It is not an application draw callback. It closes old admission first and never replays old work |
| Built-in settings | `add_all_protected` and `add_all_protected_with` map IDs to application permission values at registration. The mapper is trusted host setup; metadata mappings and issuer grants still follow their documented contracts. Per-operation write checks remain separate |
| Public services/screens/elements/authenticator/policy/painter traits | External implementations are supported. Required-method additions break callers; object safety and default-method authority need explicit review. Admitted callback effects and external credential verification remain trusted |
| Public free functions, widgets, layout helpers and standalone legacy controls | Preserve their existing public paths and documented behavior. They remain direct application APIs; adding a protected host does not sandbox calls made independently by application code |
| egui/widget reexports | Preserve public paths; incompatible upstream type changes require an explicit dependency/API migration |

Sealing restricts implementors; object safety and auto traits remain observable
contracts. Preserve object safety when extending `HostedShell`, for example by
constraining generic convenience methods to `Self: Sized`. Non-exhaustive structs
also require `..` in downstream destructuring patterns. Existing field types,
serialization semantics and implemented public traits remain compatibility commitments.

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
