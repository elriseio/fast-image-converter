# Components

> **Canonical registry** of components in the `convert-to-webp`
> architecture. Each component has its own per-component document
> with purpose, inputs / outputs, invariants, failure modes, and
> related contracts.

## Registry

| # | Component | Document | Owner | Status |
|---|---|---|---|---|
| 1 | `cli-frontend` | `cli-frontend.md` | developer | draft (v0 reference in `src/main.rs:17-99`) |
| 2 | `converter-core` | `converter-core.md` | developer | draft (v0 reference in `src/main.rs:17-99, 101-136`) |
| 3 | `format-codecs` | `format-codecs.md` | developer | draft (v0 reference in `src/main.rs:11-15, 101-136`) |

## Component Map

```
cli-frontend
     |
     | invokes
     v
converter-core
     |
     | uses
     v
format-codecs
```

`cli-frontend` parses argv + env, then hands control to
`converter-core`, which walks the directory and dispatches per-file
work to `format-codecs`. `format-codecs` exposes a `Codec` trait
(planned, ADR-0001) with one implementation per input/output format
pair; the v0 implementation is the implicit
`jpeg → webp (portrait/landscape/quality=85)` codec.

## Cross-References

- `docs/architecture.md` § 3 Container.
- `docs/contracts/codec-bounds.md` — codec ↔ converter-core contract.
- `docs/adr/0001-multi-format-cli-scope.md` — scope decision.
- `docs/adr/0002-preserve-jpg-to-webp-baseline.md` — backward-compat decision.
