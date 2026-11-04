# Release archive checks

Run from the repository root with Python 3.12+, rustup and the normal all-feature
Rust/system build inputs:

```sh
python3 tools/package_check.py
```

For an uncommitted local review, add `--allow-dirty`. CI runs without that option
after the strict audit and rejects dirty package inputs. This command builds and
tests archives; it does not publish to a registry.

## What is checked

1. `cargo package --locked -p fairing-widgets -p fairing --all-features` creates
   and verifies both release archives. Cargo stages the sibling crate locally
   while checking the unpublished pair.
2. The script extracts the `.crate` files into a temporary directory outside the
   checkout. It runs `cargo test` on the extracted widgets crate and all-feature
   `cargo test` on the extracted shell, including unit, integration and API doctests.
3. A standalone application is created beside those extracts. Its source is
   `tools/package_consumer.rs`: it uses the public bridge traits, submits a shell
   command, and verifies rendered screen text through the headless harness.
4. The consumer runs with no default features, the default feature set, and all
   public features. Cargo metadata must identify both dependencies as the extracted
   archives, never the source checkout. The external project also checks formatting.

Before the first release, `fairing-widgets` is unavailable on crates.io. Temporary
manifests therefore patch that sibling to its extracted archive and update only
temporary lockfiles, seeded from the package lockfile. Other dependencies keep their
pinned versions and registry identities. Package construction uses the repository
lockfile, and extracted tests/consumers use offline,
locked resolution after their temporary lockfiles are prepared. Repository manifests,
lockfiles and Cargo source configuration are not rewritten.
The script rejects new third-party identities/versions in temporary resolution and
checks that the repository lockfile hash is unchanged.

## Guide tests and package tests

The full guide reads `docs/guide/` outside the `fairing` crate directory. Those
pages and `src/guide_probe.rs` are intentionally absent from the release archive.
The audit sets `RUSTDOCFLAGS="--cfg fairing_repo_docs"` for its all-feature tests;
the Ferrocene compatibility test command uses the same explicit gate.

For a focused repository-guide run:

```sh
RUSTDOCFLAGS="--cfg fairing_repo_docs" \
  cargo test --locked -p fairing --all-features --doc
```

Ordinary archive doctests do not enable that cfg, and still test the public API
examples. The package script overrides inherited repository rustdoc flags with
`-D warnings` so excluded pages cannot be enabled accidentally. Font-warning tests
register valid egui-bundled font data in the Strong family and also exercise
unreadable-font rejection; they do not require the repository's example fonts.

## Evidence and scope

Logs, archive SHA-256 hashes, toolchain identity and command exit codes are retained
under `target/package-check/`, including a `summary.json`. The CI job uploads this
directory as the `package-check` artifact even on failure.

These checks cover the current host toolchain, packaged test suite and headless
consumer. They do not open a window, validate GPU presentation, publish either
crate, or test downloading the unpublished sibling from crates.io. For the first
release, publish the widgets dependency before the shell.
