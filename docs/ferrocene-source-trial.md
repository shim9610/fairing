# Trying Ferrocene from public source

This trial builds the public Ferrocene compiler, standard library, Cargo and
rustdoc, then uses them on the existing fairing workspace. It evaluates compiler
compatibility on x86_64 Linux. It does not need a Ferrocene account or a hardware
board. The repository's default Rust toolchain and minimum Rust version stay as
declared in `rust-toolchain.toml` and `Cargo.toml`.

## Source and build scope

[`tools/ferrocene/source.json`](../tools/ferrocene/source.json) pins Ferrocene
commit `2eb9c33acc893467ee92070c839b51dea4b1c054` from `release/1.97`, whose
`src/version` is `1.97.1`. It also pins the LLVM, Cargo and backtrace submodules.
The source tree's Ferrocene version is `26.08.1` and its channel is `beta`; this
trial is not an installation of the qualified Ferrocene 26.08 distribution.

[`bootstrap.toml`](../tools/ferrocene/bootstrap.toml) builds stage 1 with the
source tree's LLVM backend, limited to X86. The compiler and LLVM downloads from
Ferrocene's private CI are disabled. Bootstrap still downloads the upstream Rust
1.96.0 compiler specified in Ferrocene's `src/stage0`, verifying its recorded
checksums. A stage 1 compiler is built by that bootstrap compiler; this trial
does not perform a stage 2 self-hosted rebuild or run the complete compiler
qualification test suite.

The `library` build includes the complete host sysroot, including `proc_macro`
for derive macros, `test` for Cargo's test harness, and the profiling runtime for
coverage instrumentation. Building only `library/std` leaves those additional
crates out of a stage 1 sysroot.

The source checkout lives outside this repository. No Ferrocene source, compiler
binary or new runtime dependency is added to the published fairing crates.

## Run locally

Use x86_64 Linux with Python 3, Git, GCC/G++, Make, CMake, Ninja and rustup on
`PATH`. Install the system development libraries needed by the existing
`runner`/`runner-x11` features, as for the normal all-features build. The source
and compiler build need several gigabytes of disk space and substantially more
time than a normal fairing build.

```sh
python3 tools/ferrocene/run.py build --jobs 4
python3 tools/ferrocene/run.py check
python3 tools/ferrocene/run.py coverage
```

All three commands accept `--source-dir /absolute/path/to/ferrocene` to use an
existing checkout at the exact pinned revisions. The script refuses a different
revision, tracked source modifications or a different bootstrap configuration.
The default location is `$XDG_CACHE_HOME/fairing/ferrocene/<revision>`, falling
back to `~/.cache/fairing/ferrocene/<revision>`.

The build registers a separate rustup toolchain named
`fairing-ferrocene-source-1.97.1`. It places the source-built Cargo in that
toolchain so rustup does not silently substitute another Cargo. The check script
also selects the compiler and rustdoc by absolute path, verifies the compiler
and Cargo commit identities, and rejects inherited compiler wrappers or Rust
flags that could change the trial.

The compatibility checks use `--locked` and a separate
`target/ferrocene-source` directory:

- Workspace checks with every feature and all Cargo targets, including examples,
  on the x86_64 Linux host.
- Workspace tests and documentation tests with every feature.
- The minimal `fairing` feature set (`--no-default-features --features mock`).
- A code-generating library build with `ferrocene::unvalidated` enabled.
- A consumer entry point calling the real configuration and unit-conversion APIs.
- A negative control that must fail specifically on an unvalidated call.

Logs, compiler identities, commands and exit codes are written to
`target/ferrocene-source-report/`. `summary.json` contains the compatibility
results; `validation-findings.json` and `consumer-findings.json` contain the
validation-boundary diagnostics. These are separate from a normal audit result.

The coverage command uses the pinned `cargo-llvm-cov` 0.9.1 Linux binary,
verifies its archive SHA-256, and explicitly selects the LLVM tools built from
the same source as the compiler. It measures the product crates' unit and
integration tests, writing `coverage.json` and `coverage-summary.json`. This
mode measures lines, functions and regions; it excludes documentation tests,
branch coverage and modified condition/decision coverage (MC/DC). Documentation
tests still run in the separate compatibility check. No coverage percentage is
used as a qualification claim or an arbitrary pass threshold.

## Local results — 2026-11-01

The pinned source built successfully on Debian 13, with four build jobs, a
16 GiB memory limit, GCC 14.2.0, CMake 4.1.3 and Ninja
`1.13.0.git.kitware.jobserver-pipe-1`. The resulting
compiler reports Rust 1.97.1, Ferrocene 26.08.1 and LLVM 22.1.6; Cargo reports
1.97.1 at the pinned Cargo commit. The first LLVM source build took 44 minutes.
The library code, Cargo manifests, dependency lockfile and default Rust 1.99
toolchain required no changes for this trial.

| Check | Result |
| --- | --- |
| All workspace features and Cargo targets | Passed |
| Workspace tests and documentation tests | 1,811 passed; 16 ignored |
| Minimal feature set | Passed |
| Unannotated library lint inventory | 0 diagnostics |
| Consumer compile and execution | Passed; 100 validation-boundary diagnostics |
| Lint negative control | Rejected the unvalidated call as expected |
| Instrumented product unit/integration tests | 1,451 passed |
| Coverage: lines | 45,008 / 48,998 (91.86%) |
| Coverage: functions | 4,806 / 5,276 (91.09%) |
| Coverage: regions | 74,925 / 81,551 (91.88%) |

The consumer diagnostics include `ShellConfig::default`, `ShellConfig::validate`,
`round_u8` and calls reached through generic instantiations. The count includes
repeated diagnostics from separate lint passes and instantiations. Treat it as
an inventory of validation evidence needed at those call boundaries. No
`prevalidated` attributes were added to production code.

Coverage figures describe the instrumented test build, including inline unit
tests. The export contains 122 product source files and one standard-library
file. Documentation tests, branch coverage and MC/DC are outside
these figures. These results are local; the new hosted Ferrocene workflow has
not been executed.

## CI

The separate `Ferrocene source compatibility` workflow runs manually and weekly
on the public repository. It performs a clean source build before the same
compatibility checks, with two build jobs and a four-hour timeout. It uploads
the compiler identities, exit codes, logs and lint inventories, including on
failure. It uses no Ferrocene account, private CI cache or self-hosted hardware.
The existing PR audit continues to check the standard toolchain independently.
Compatibility build outputs are released before coverage to bound disk use;
the logs and result files are retained.

The workflow requires a successful source build, workspace checks and nonempty
coverage exports for both product crates.
Validation-boundary warnings remain an explicit inventory; a green job does not
mean that all APIs are validated.

## Interpreting the validation-boundary lint

Ferrocene's `unvalidated` lint checks calls from validated roots, such as a
program's `main` function or items annotated `#[ferrocene::prevalidated]`.
Enabling it on an ordinary, unannotated library can produce zero diagnostics
without checking all of that library's function bodies. A zero count alone is
therefore not evidence that fairing or its dependencies are validated.

The consumer fixture supplies a concrete entry point and reports the boundary
at the APIs it calls. It is a small sample, not a complete application or a
traversal of every library API. The negative control verifies that the compiler
really rejects a known unvalidated call. No production item is marked
`prevalidated` to silence diagnostics.

A successful source build and passing fairing tests establish compatibility for
the recorded configuration and tested behavior. Official qualification evidence,
standard-library certification scope and the final device's regulatory evidence
are separate requirements. This source trial provides none of those by itself.
Board and debug-probe tools are excluded: fairing supplies library interfaces,
not a concrete MCU board, firmware or probe connection.

## References

- [Pinned Ferrocene source](https://github.com/ferrocene/ferrocene/tree/2eb9c33acc893467ee92070c839b51dea4b1c054)
- [Public-source build setup and private-cache options](https://github.com/ferrocene/ferrocene/blob/2eb9c33acc893467ee92070c839b51dea4b1c054/ferrocene/doc/internal-procedures/src/setup-local-env.rst)
- [Lint implementation and its default scope](https://github.com/ferrocene/ferrocene/blob/2eb9c33acc893467ee92070c839b51dea4b1c054/compiler/rustc_lint/src/ferrocene/mod.rs)
- [Validated-root selection](https://github.com/ferrocene/ferrocene/blob/2eb9c33acc893467ee92070c839b51dea4b1c054/compiler/rustc_middle/src/middle/codegen_fn_attrs/ferrocene.rs)
