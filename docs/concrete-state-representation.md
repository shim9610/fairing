# Concrete UI state verification status

Whole-UI-state specification completeness is **unproved**. Passing the source
structure or focus component jobs must not be reported as completion of that
obligation.

The required scope is Fairing, Fairing Widgets and the egui state they use.
Application-provided Screen implementations and closures are external code.
Their permitted effects through references, public APIs and shared handles must
be included in the model. Their implementation and termination are separate
from this state-coverage obligation.

## Implemented evidence

verification/state_correspondence.py checks compiler-bound field/variant
reconstruction for product source datatypes and configuration coverage. Its
generic referenced World parameter does not establish that all concrete shared
runtime state has been collected or connected. The receipt labels whole UI
coverage unproved.

verification/native_ui_state.py compiles a disposable copy of the locked egui
archive, checked against Cargo.lock's archive checksum. It independently captures
production compiler expansion. Actual Focus, FocusWidget and FocusDirection
definitions are checked against the record specification, including previous
and deferred focus, event filters, modal layers and navigation state.

The independent focus-record laws and current-focus query composition are
verified. A mutation changes the actual native Focus::focused body, recompiles
egui and must fail the observation-preservation proof. The cache representation
is a parameter: a concrete encoding of its storage is not proved. In particular,
unordered map contents alone are insufficient when iteration order affects
directional navigation ties.

verification/import_state_types.py uses the pinned compiler interface to inspect
private transitive fields, closure captures and MIR local slots. This is a
dependency/root audit, not a whole-state abstraction theorem.

## External input boundary

A callback's effect is not restricted to its pane rectangle. Ui exposes a shared
Context; public APIs can change shared egui state. Cx exposes mutable Services
and IconSet references and methods for requests, animation and nested drawing.
ShellHandle can enqueue commands from other threads.

Because callbacks receive mutable Cx, public reference slots may themselves be
replaced, including slots whose pointees are read-only. The model must
distinguish an immutable pointee from an immutable field slot and must not assume
that public session/theme/pane metadata remains equal to authoritative private
shell state. WidgetCx also exposes configurable references and frame/scope
values.

These are public API effects, not a requirement to verify arbitrary application
code. Memory-unsafe interference outside Rust/API contracts is outside this
source-level scope.

## Missing proof

A total actual-runtime-state-to-specification projection is not implemented.
It must connect the retained source state, shared context/storage, object
identities and aliases, and relevant active invocation/loan state. The declared
UI groups must preserve the observations required by their operation contracts.
A source type inventory, a per-record inverse law or a generic unchanged store
cannot stand in for that connection.

Input realization and operation execution/progress remain separate obligations;
they must not be used as reasons to leave this first obligation ambiguous.
