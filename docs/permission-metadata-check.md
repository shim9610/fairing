# Checking application permission declarations

`Permission::gate` maps an application-owned value to a grant identifier.
`PermissionBound::required_permission` returns an element's requirement. These
methods execute before admission, so they must provide metadata without causing
effects or diverging. Ordinary Rust trait checks enforce their signatures, but do
not establish that contract.

## Run against an application workspace

From a Fairing checkout, install the pinned compiler development components and
check the application manifest:

```sh
python3 verification/install.py proofs
python3 verification/check_extension.py \
  --manifest-path /path/to/application/Cargo.toml --all-features
```

For a particular configuration, use `--no-default-features` and/or `--features`
instead. Run every supported production configuration that needs evidence. The
tool uses the pinned nightly compiler and requires a current application lockfile.
It does not run application callbacks. It refuses to replace an existing
`RUSTC_WORKSPACE_WRAPPER` silently.

The checker rebuilds workspace production targets with a compiler wrapper. It
identifies implementations by the actual Fairing trait definition, including
through public re-exports. It examines their compiled mid-level intermediate
representation (MIR) and recursively checks statically available helper bodies.

## Accepted subset and rejection

Simple enum matches, field copies, local scalar operations and constructors such
as `Gate::borrowed` are supported. Accepted control-flow and helper-call graphs
are acyclic; helper depth is bounded. Available constant-evaluation MIR is examined
when an imported constant function has no exported runtime MIR.

The analysis rejects writes through references, raw-pointer operations, drops,
loops, recursion, panics/asserts, unresolved trait or function-pointer calls,
unavailable helper bodies and unsupported MIR operations. Rejection can indicate
an effect or code outside this deliberately conservative subset. It is not
permission to assume an unresolved method is pure. Keep metadata implementations
small and direct, or extend the checker and its controls before accepting them.

`FAIRING_METADATA_EFFECT` diagnostics identify rejected implementations. Ordinary
compiler errors also reject missing declaration methods or incompatible types.
The consumer controls exercise one accepted enum/field client and six rejected
clients: direct and transitive I/O, a loop, a static write, an effectful mapper and
a missing declaration method.

## Evidence and scope

The default output is `target/metadata-check/report.json` and `compiler.log` in the
Fairing checkout; select another directory with `--output`. The report records the
command, examined methods and hashes of compiler-driver inputs and workspace Rust
sources/manifests/lockfile. A source change during checking rejects the result.
No examined implementations produces a nonzero `no-implementations` result.

`metadata-contracts-checked` covers compiled **production workspace implementations**
and statically available helpers for the selected configuration. Test targets,
inactive configurations and implementations imported from dependencies outside the
workspace are not certified. Include owned implementation crates as workspace
members or check their workspaces separately.

This result establishes the restricted metadata contract and ordinary Rust type
conformance. It does not validate business permission mappings, prove that every
callback is mediated, or constrain arbitrary admitted callback effects. The
[complete-mediation gate](callback-mediation.md) remains separate and unproved.
CI runs the library check and consumer rejection controls in `permission-metadata`.
