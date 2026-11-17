# Execution and layout boundary review

This review compares the implementation at `6f5c1d9` with the source hashes in
[the measurement record](../verification/benchmarks/mediation-2026-11-16.json).
The public API decision inventory matches its reviewed ledger in minimal and all
feature profiles.

## Type boundary

`ExecutionToken<F>` owns the admitted operation and consumes itself before calling
it. The private callback field has no constructor outside the dispatcher, payload
getter, clone or borrowed execution method. Action, UI and factory payloads live
inside `mediation::execution`; callers can dispatch but cannot borrow their raw
bodies. Screen storage retains the authorization check after factory construction.

`LayoutToken<P>` holds data authorization, frame, observation time and session
identity/revision. It has no execution method. Data validation can reuse the token
while those observations match. Unversioned custom access policies are rechecked.
Measurement and paint are executable application callbacks and each gets a fresh,
consumed execution attempt. An admitted callback may finish; this change does not
make arbitrary application effects transactional.

Rust compilation rejects private payload borrowing, execution-token fabrication,
repeated execution, borrowed execution and layout-token execution. The compiler
route checker also inspects adapters inside the private execution boundary.
See [the common contract](protected-contracts.md#execution-and-layout-types).

## Two verifier omissions

Both omissions were reproduced using deliberately modified internal adapters at
the baseline. They were verifier false acceptances: the modified code was accepted
with an UNSAT (unsatisfiable) routing verdict, but its first callback revoked authority and a
second callback still entered. These reproductions do not establish that the
unmodified baseline exposed that runtime path through its public API.

| Omission | Why it was missed | Regression control |
| --- | --- | --- |
| An ordinary helper calls the payload twice | The helper summary recorded whether it could enter a payload, losing the number/order of entries | `helper-repeat` |
| A nested mediated callback returns, then the adapter calls the raw payload | Returning from nested mediation incorrectly left the caller's older admission usable | `nested-return` |

The checker now computes fresh/consumed admission effects through helpers,
closures, matching call returns and recursive fixed points. A payload entry
consumes admission; a nested mediated operation starts fresh and returns consumed.
An access-policy callback invalidates prior admission. Both compiled reproductions
must produce `callback-without-fresh-admission`.

## Back default test

`default_back_action_is_pop` previously compared enum constants (`Pop == Pop` and
`Pop != Consumed`). It never called `Screen::on_back`; changing the real default
to `Consumed` still passed that test. The test now constructs a screen using the
trait default and calls `on_back` with a real context fixture. The `back-default`
production mutation changes the default to `Consumed` and must fail with
`default Screen::on_back must pop`.

## CPU measurement

The same public-API benchmark was built in release mode before and after the
change using Rust 1.99.1 on an AMD EPYC 9V74 virtual host (five visible CPUs,
four-CPU quota). Both binaries were pinned to CPU 4. Other local validation jobs
were paused during measurement. Five rounds alternated binary order. Each round
contained seven samples: 20,000 action invocations per sample, or 80 egui frames
per sample after 12 warmup frames. The table reports the median of the five round
medians; raw round medians and sample ranges are in the linked record.

| Workload | Before | After |
| --- | ---: | ---: |
| Protected action dispatch | 218.40 ns | 212.57 ns |
| 64 leaves, depth 1 | 134.56 µs | 112.27 µs |
| 256 leaves, depth 1 | 492.11 µs | 410.20 µs |
| 64 leaves, depth 8 | 268.00 µs | 178.37 µs |
| 64 leaves, depth 32 | 828.85 µs | 436.00 µs |
| 256 leaves, depth 8, half denied | 521.02 µs | 337.75 µs |

The layout implementation validates the complete plan once, then each visited
node's layout token, rather than recursively revalidating each subtree at
every paint node. Executable callbacks still recheck live authority, including
ancestor requirements.
The execution wrapper adds no heap allocation; each layout plan stores additional
frame/time/revision metadata. This benchmark measures CPU dispatch and headless
egui layout/shape generation, with the default access policy.

Work was checked as well as time. In every round each binary executed 140,000
action callbacks. The five layout cases respectively executed 35,840, 143,360,
35,840, 35,840 and 71,680 measurement callbacks, with the same paint counts. Denied
leaves account for the final case's lower count.

Reproduce the benchmark with:

```sh
cargo build --locked --release --manifest-path verification/Cargo.toml --example mediation_bench
taskset -c 4 verification/target/release/examples/mediation_bench
```

For a baseline checkout, copy the same benchmark source into its verification
examples directory before building. The measurement record includes both binary
hashes, the benchmark hash and candidate product-source hashes.

## Validation

| Check | Observed result |
| --- | --- |
| Strict workspace audit | Passed formatting, Clippy, 1,989 workspace unit/integration/doc tests, documentation and ARM64 build checks |
| Common kernel and compiled routing | Kani/Verus kernel/path evidence current; all-feature and minimal routing queries UNSAT with no errors or unguarded payload entries |
| Compiled contract controls | 40 cases: three accepted extensions, 32 semantic rejections and five Rust type-boundary rejections; batches used identical product/checker source hashes |
| Layout lifetime proof | Five source-bound Kani harnesses and ten assertion-rejection controls; compiled plan/root verdict-use checks |
| Production mutation controls | 47 passing baselines and 47 expected semantic rejections, including the real Back-default mutation |
| Native API contract coverage | 81 tests passed; new execution storage (66/66 lines, 15/15 functions) and layout token (28/28 lines, 3/3 functions) covered; all reviewed per-module floors passed |
| Minimal feature regressions | Ten element tests and the actual Back-default test passed |
| Inventory and metadata | Public API decision ledger and rebuilt state inventory matched; 20 existing proof-input type declarations unchanged; workspace permission metadata and downstream acceptance/rejection fixtures passed |

The existing production proof pipeline also completed with no unexpected outcome.
The six admission-effect unit tests pass normally and under Python optimization.
Coverage percentages describe this API test suite, not branch coverage or a proof
of arbitrary application behavior. The [assurance guide](rust-assurance.md)
describes test ownership and retained evidence.

## Additional Miri observation

The existing Miri test
`protected_scope_uses_current_context_and_never_caches_admission` passes. An
expanded run of `admitted_callbacks_cannot_install_persistent_icon_or_widget_callbacks`
fails while drawing its button label: egui's glyph rasterizer reaches
`vello_cpu 0.2.0`, `fine/lowp/mod.rs:361`, where `bytemuck::cast_slice::<u8, u32>`
reports `TargetAlignmentGreaterAndInputNotAligned`. The failure occurs in glyph
rasterization after the protected scope has returned. This expanded GUI Miri
check is not counted as a successful execution-token check.
