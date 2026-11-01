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
| Kani | 0.68.0; nightly 2026-08-21 | Eight harnesses: ACK/access decisions, nonwrapping pool generations and bounded pool sequences |
| Verus | 0.2026.10.26.d17fbd0; Rust 1.99.1 | Postconditions on six real ACK/access/generation functions |
| Miri | nightly 2026-10-30 | Eighteen fixed tests using actual Access/PinTable/notifications/services/time/units/parser/pool/Inbox |
| Loom | 0.7.2 | Four scenarios: Inbox ordering/consumer and bounded admission/close-after-drain |
| proptest | 1.12.0 | Twelve properties × 512 cases plus four shell properties × 64; seed 20261101 |
| cargo-fuzz | 0.13.2; libfuzzer-sys 0.4.13 | ASan: 20,000 ACK, 10,000 parser, 10,000 API and 2,000 shell inputs |
| cargo-llvm-cov | 0.9.1 | Product and API-contract line/function/region coverage; latter checks eleven production modules |
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
CODEOWNERS protects the entire verification directory. None of the test tools
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
was unnecessarily expensive. Miri's eighteen cases are separate from the full
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

There are twelve product BOMs: three libraries × two targets × two feature
profiles. The default standalone allocator graph is empty by design; all-features
selects its allocator dependencies. Four additional xtask BOMs describe the build utility. These are
resolved library/package dependency inventories. A final device application
must generate its own BOM with its own lockfile, features, native dependencies
and firmware. The all-features fairing graph legitimately includes `image`
through eframe/arboard; the default library graph excludes it.

## Local results — 2026-11-01

The expanded public API suite passes 34 native tests, eighteen Miri tests and
four Loom scenarios. Kani passes eight harnesses and Verus verifies six production
functions. Both reject six deliberate decision defects, including generation
wrap. Public API/model controls reject broken gate delegation, notification cap
handling, bounded factory admission and message delivery. `verification/memory.py` passes the normal,
16 MiB and 64 MiB modes, including zero-allocation queue/pool reuse and expected
infallible OOM termination. The [memory guide](memory-limits.md) records allocator
scope, spinlock and exhaustion limits. Full audit, fuzz and coverage results are
retained per commit in CI artifacts. The strict local audit passes all 14 stages
(1,827 tests passed, 16 ignored); three additional memory-guide examples pass.
Rust 1.95 checks all features/targets, and source-built Ferrocene 1.97.1 checks
the fixed-heap example profile. The API-only coverage executes 9,434 of 39,147
product lines (24.10%). Earlier full Ferrocene coverage figures describe the
earlier baseline rather than this allocator change.
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
