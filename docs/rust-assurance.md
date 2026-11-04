# Rust assurance checks

The `Rust assurance` workflow runs the checks below on pull requests, main
pushes, a weekly schedule and manual requests. Its input gate checks the separate
verification lockfile and licences before the tool workspace is compiled. The
existing strict audit, MSRV and example-tour workflow continues independently.
The [API contract map](api-contracts.md) links each public guarantee to its
production path, proof/property, bounds and negative control.
The [Ferrocene source trial](ferrocene-source-trial.md) also supplies the existing
source-built compiler and matching LLVM coverage workflow.

## Tools and the code they check

| Tool | Pin | Scope |
| --- | --- | --- |
| Kani | 0.68.0; nightly 2026-08-21 | Thirty harnesses: ACK/access and response correlation, pool/display, production input admission, Back dispatch and retained-owner partition/operation/lifetime contracts |
| Verus | 0.2026.10.26.d17fbd0; Rust 1.99.1 | Contracts on twenty-nine production functions plus quiet-pass progress and four concrete-carrier composition/noninterference obligations |
| Miri | nightly 2026-10-30 | Twenty-eight assurance tests, six production interaction-adapter tests and the production payload-cleanup test; rendering witnesses run natively |
| Loom | 0.7.2 | Four scenarios: Inbox ordering/consumer and bounded admission/close-after-drain |
| proptest | 1.12.0 | CI: 512 cases per property, including shell ordering, transition traces and UI reachability; seed 20261101 |
| cargo-fuzz | 0.13.2; libfuzzer-sys 0.4.13 | ASan: 20,000 ACK, 10,000 parser, 10,000 API and 2,000 shell inputs |
| cargo-llvm-cov | 0.9.1 | Product and API-contract line/function/region coverage; latter requires execution of the declared production modules |
| cargo-cyclonedx | 0.5.9 | Library package dependency BOMs for default/all features on Linux x86_64 and aarch64 |

Assurance tool archives and CycloneDX schemas have pinned SHA-256 values in
[`verification/tools.json`](../verification/tools.json). Coverage shares its
archive pin with [`tools/ferrocene/coverage-tool.json`](../tools/ferrocene/coverage-tool.json). Downloads are checked
before extraction. Rustup installs the exact compiler versions needed by the
verifiers. Python schema-validator dependencies are pinned in
[`requirements.txt`](../verification/requirements.txt).

The unpublished `verification` workspace has its own lockfile. It reuses the
primary lockfile's existing package versions and adds 23 tool-only packages.
The dated entries in `deps.allow`, `verification/check-inputs.py`, cargo-audit
and the scoped `verification/deny.toml` guard those inputs. NCSA is permitted
there for libFuzzer; the product workspace's licence policy is unchanged.
CODEOWNERS assigns the entire verification directory to its owner; required-review
enforcement is a separate repository setting. None of the test tools
becomes a dependency of a published fairing crate.

## Shared production code and properties

`crates/fairing/src/shell/bool_ack.rs` contains the decision used by
`Shell::reconcile_bool`. The extraction preserves backend-read order, failure
tracking and command dispatch in the shell. The checked properties are:

- An unacknowledged command cannot dispatch a second target.
- A queued dispatch uses the latest desired value and differs from the
  acknowledged value.
- An already-failed backend cannot reject a later command solely because that
  failure remains visible.
- Waiting preserves the observed failure state; a newly reported terminal
  failure releases the pending command.

Kani explores all valid Boolean/optional-Boolean input combinations of this
function. The Verus adapter adds a contract and widens declaration visibility in
a generated standalone crate. It asserts that the function body is copied
unchanged. It adds no external-body assumptions or prevalidated production
annotations. Both tools must also reject a generated variant with the
acknowledgement condition reversed; a compiler error does not satisfy that
negative control.

proptest and the protocol fuzzer run up to 128 operations against this actual
kernel, using a backend model with one reported Boolean and one outstanding
command. Inputs include coalesced intents, acknowledgements, repeated snapshots
and terminal failures. Configuration properties call the real
`ShellConfig::from_toml` and check validation and secret redaction in syntax
errors. The configuration fuzzer accepts up to 4,096 bytes and includes valid
and malformed seeds.

These proofs cover the decision function and the stated backend model. They do
not prove the surrounding shell's authorization policy, clock progression,
callback execution or an integrator's hardware implementation. In particular,
the model treats a new failure notification as terminal for the pending command.

The Inbox harness includes the production source. Normal tests and Miri use
`std::sync::mpsc`; `--cfg fairing_loom` selects Loom's channel implementation for
the model tests. All four bounded scenarios are explored without a permutation
or preemption cap. A branch-limit violation fails the test. Loom 0.7.2 does not
model the exact disconnected-channel `try_recv` result, so its result is limited
to delivery, per-sender order and nonblocking drains in these scenarios.

Miri keeps its normal isolation, validity and race checks enabled. It interprets
fixed regression inputs and real channel threads. Random-input generation runs
natively under proptest and libFuzzer; interpreting the generators themselves
was unnecessarily expensive. Miri's assurance cases are separate from the full
product test suite and the generated-input campaigns.

## SBOM scope and validation

The generator produces CycloneDX 1.5 JSON. Official schemas at the pinned
CycloneDX 1.5 source commit validate every output, and the dependency references
are constructed from Cargo's resolved package graph.

The raw cargo-cyclonedx export can contain inactive optional dependencies. For
example, its all-features fairing export contained 193 components, including
an inactive WGPU stack. The script retains that export as `*.raw.cdx.json` and
projects package components and dependency edges onto:

```sh
cargo tree --locked -p <package> --target <triple> \
  --edges normal,build --prefix depth --format '{p}' [--all-features]
```

The derived `*.cdx.json` files carry the selected target, feature profile and
edge kinds as component properties, pass schema validation again, and have no
unresolved dependency references. A missing package in the original export
fails generation. The approved product lockfile must remain unchanged.

There are eight product BOMs: two libraries × two targets × two feature profiles.
Four diagnostic allocator and four xtask BOMs describe the unpublished tools.
The default standalone allocator graph is empty; its global feature is lab-only.
The BOM gate rejects allocator diagnostic dependencies in either product graph. These are
resolved library/package dependency inventories. A final device application
must generate its own BOM with its own lockfile, features, native dependencies
and firmware. The all-features fairing graph legitimately includes `image`
through eframe/arboard; the default library graph excludes it.

## Local results — 2026-11-01

The memory-pressure correction adds hard persistent-notification retention,
bounded pending toasts and fallible factories/display admission. The production
global allocator feature is withdrawn; the unpublished lab demonstrates actual
fallible recovery and the remaining unsupported infallible abort separately.
Local correction validation passes all 14 strict audit stages (1,835 tests passed,
16 ignored), 36 native API tests, nineteen Miri tests, four Loom scenarios and
all four mutation controls. Normal/16 MiB/64 MiB trials pass real exhausted-arena
refusal, preallocated work and deferred display recovery. Sixteen BOMs pass
schema/graph validation; neither product graph includes the diagnostic allocator.
Fresh hosted artifacts provide per-commit fuzz/coverage totals rather than
reusing the earlier allocator-prototype coverage figures.
The [contract map](api-contracts.md) records the exact guarantees and assumptions.

## Run locally

Use Linux x86_64, Python 3.12+, rustup and the normal Rust/system build tools.
Run the repository's strict audit as described in CONTRIBUTING.md as well.

```sh
python3 verification/check-inputs.py
cargo audit --file verification/Cargo.lock
cargo deny --manifest-path verification/Cargo.toml --config verification/deny.toml check
python3 verification/install.py all
python3 verification/proofs.py
python3 verification/memory.py
PROPTEST_CASES=512 PROPTEST_RNG_SEED=20261101 \
  cargo test --locked --manifest-path verification/Cargo.toml --lib
RUSTFLAGS='--cfg fairing_loom' \
  cargo test --locked --manifest-path verification/Cargo.toml \
  --no-default-features --features loom --lib
```

The nightly pin is read from `verification/tools.json` in CI. With the recorded
pin, the interpreter and fuzz commands are:

```sh
MIRIFLAGS='-Zmiri-seed=0' cargo +nightly-2026-10-30 miri test \
  --locked --manifest-path verification/Cargo.toml --lib
python3 verification/controls.py
python3 verification/fuzz.py
python3 verification/coverage.py
python3 -m venv /tmp/fairing-sbom-python
/tmp/fairing-sbom-python/bin/python -m pip install -r verification/requirements.txt
/tmp/fairing-sbom-python/bin/python verification/sbom.py
```

The tool cache defaults to `$XDG_CACHE_HOME/fairing-assurance` or
`~/.cache/fairing-assurance`; `FAIRING_ASSURANCE_TOOLS` can select another cache.
Proof logs and controls, fuzz commands/corpora, raw/derived BOMs and schema
results are retained under `target/assurance/`. CI uploads them on failure too.
Generated fuzz inputs and crash artifacts never enter the published crates.

All results are evidence for these configurations and bounded checks. The
tools and a passing workflow provide no device certification or complete
security proof. Board/probe tests remain excluded from this library workflow.

## Memory-specific checks

The `memory-modes` job exercises the real opt-in allocator separately from Miri
and the API state models. Kani checks bounded pool sequences and every u64
generation; Verus adds postconditions to the exact generation body. Both reject
a generated wrapping-add defect. proptest/Miri/libFuzzer compare the public
pool and bounded queue with independent live-key and FIFO models.

The Loom bounded-channel case uses the real admission atomics with a model
channel because Loom 0.7.2 lacks sync_channel. Its channel stub miscounts failed
sends racing receiver destruction; that race runs with real channels natively
and under Miri. The close-after-drain Loom case checks closed admission. See
[the contract map](api-contracts.md) for sequence limits and assumptions.


## Same-process bounded display recovery — 2026-11-02

The renderer process/IPC implementation has been removed. The fixed-display CI
job executes the exact product memory source on Linux and Windows. Its Windows
step additionally runs the same-process 16 MiB exhausted-arena trial; Linux's
memory-modes job runs both 16 and 64 MiB trials.

The trials exhaust the arena until a one-byte reserve fails. Inline newest-sample
retention, bounded borrowed-image preview, warning rasterization and 10,000
preallocated work commands must continue with zero allocations in their measured
interval. Other checks require actual old-text reclamation before replacement
and omitted text/real product frost work after fallible failure. These are
same-process CPU/storage guarantees, not an egui frame or GPU-presentation proof.

Kani checks both individual and bulk insertion of four arbitrary samples into a
two-slot window, its chronological newest suffix and zero capacity. Verus checks
the exact display_skip production body for every usize input. Both must reject a
zero-skip mutation semantically. The public property, Miri and ASan API suites
execute display retention/raster bounds; coverage requires this production module.
The standard shell's compiled-stencil notice and preservation of accepted device
commands have integration regressions. Stock egui/font/tessellation/native
allocation recovery is outside the supported whole-renderer guarantees.

## Input progress and rendered transition witnesses

[UI reachability](ui-reachability.md) connects the actual input decision to an explicit
twelve-edge state graph and real pointer actions. Kani/Verus prove the one-quiet-pass
progress condition on production source; native egui tests require input witnesses
after region/rotation histories. Mutation controls reject sticky UI deferral and
regions improperly remaining active over a modal. These checks distinguish a logically
permitted method call from a reachable rendered control, with stated fairness and
configuration bounds. The corpus includes 192 fixed rendered configurations and
24 refusal/lockout/recovery configurations. Generated trace defaults are 128 for the
command/reply oracle and 96 for rendered paths; CI overrides both to 512. The independent queued-command/reply
oracle adds 8,748 fixed traces, 128 generated traces, twelve deadline scenarios and
an interpreted Miri trace. The shell ASan target calls the same oracle with inputs
up to 96 bytes; native controls must reject omitted correlation, budget overflow and
expiration after command dispatch. `controls.py --only expiry-order` replays one
control during development; CI runs the complete baseline/mutation list in
`verification/controls.py`, including retained-owner and UI-history controls.

## Formal operability status

The production Back dispatcher has a source-bound, total/disjoint partition proof
over its eight predicates and nine groups, plus specified target selection. Both
verifiers reject a missing group and a refused permitted operation. Native controls
also reject an incorrect product snapshot and an omitted handler.

The [whole-application obligations](formal-operability.md) remain open beyond this
projection. The proof artifact includes `state-obligations.json`; requiring the
whole-app theorem currently fails with `OPEN_PROOF_OBLIGATIONS`. The CI check for
this refusal guards against promoting component proofs to a whole-app guarantee.
