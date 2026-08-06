# Contracts

> **Canonical registry** of cross-component contracts in the
> `fast-image-converter` architecture. Each contract lives in its own
> per-contract document with shape, invariants, and enforcement.

## Registry

| # | Contract | Document | Direction | Status |
|---|---|---|---|---|
| 1 | `codec-bounds` | `codec-bounds.md` | `format-codecs` ↔ `converter-core` | draft |
| 2 | `report-shape` | `report-shape.md` | `cli-frontend` → `report-stream` (NDJSON; `--json` mode) | draft |

## Cross-References

- `docs/architecture.md` § 5 Contracts.
- `docs/components/converter-core.md` § Concurrency Contract.
- `docs/components/format-codecs.md` § 4 Invariants.
- `docs/adr/0002-preserve-jpg-to-webp-baseline.md` — fidelity
  contract.
