# Architecture diagrams

The SVG files are the rendered documentation artifacts. Their DOT sources are
kept beside them so ownership, boundaries and labels can be edited and reviewed
with the documentation.

| Figure | Subject | Used by |
|---|---|---|
| [System architecture](system-architecture.svg) | Host/application ownership, shell state, backend integration and presentation | [Architecture](../architecture.md#1-layers) |
| [Runtime flow](runtime-flow.svg) | Logic-only updates, shared state processing, UI frames and events | [Architecture](../architecture.md#4-one-frame) |
| [Security architecture](security-architecture.svg) | Issuer authority, token observations, typed registration and common admission | [Security architecture](../security-architecture.md#1-authority-and-trust-boundaries) |
| [Authentication flow](authentication-flow.svg) | Library-issued requests, built-in prompts and host-owned login responses | [Security architecture](../security-architecture.md#3-authentication-and-session-publication) |
| [Session lifecycle](session-lifecycle.svg) | Publication, busy reads, expiry, issuer revocation and local detachment | [Security architecture](../security-architecture.md#3-authentication-and-session-publication) |

Solid arrows describe construction, calls or execution flow. Dashed arrows
represent data/publication, wake notifications or optional continuation as
labelled in each figure. Colored regions indicate ownership/responsibility, not
process isolation. The session figure is a lifecycle view, not the formal state
machine itself.

Regenerate all figures from the repository root with Graphviz `dot` installed:

```sh
python3 tools/render_architecture.py
```

The renderer uses the Python standard library and Graphviz. It adds SVG titles,
descriptions and an accessible image role, and removes the external SVG DTD.
No script, external font or image resource is needed to display the output.
Graphviz/font versions can alter layout; inspect the rendered SVGs after editing.
