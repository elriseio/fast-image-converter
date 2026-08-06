---
project_slug: fast-image-converter
doc_type: adr
applicable_roles: [architect, developer]
version: 1
date: 2026-08-03
status: proposed
supersedes: null
summary: "Preserve the v0 gallery-compress default pipeline (JPG → WebP, portrait 800 / landscape 1000, quality 85) as the no-flags default behaviour, gated by a regression test on a fixed golden batch."
source_artifacts:
  - src/main.rs:11-15 (v0 constants)
  - README.md "Why Rust" benchmark
  - adr/0001-multi-format-cli-scope.md (parent decision)
tags: [adr, backward-compat, baseline, regression]
---

# ADR-0002 — Preserve the JPG→WebP baseline

## Status

**Proposed** (2026-08-03). Awaiting operator approval. Depends on
ADR-0001 acceptance.

## Date

2026-08-03

## Authors

- Architect (system)

## Context

ADR-0001 extends `fast-image-converter` to a multi-format converter. The
v0 binary has been the operator's primary tool for a real gallery
pipeline, and its default behaviour — JPG → WebP, portrait
max-width 800, landscape max-width 1000, quality 85 — is currently
embedded in at least one external operator script.

A naive multi-format rewrite risks regressing the default pipeline
in one or more of:

- **B1 — Policy drift**: the new code path applies a different
  resize rule (e.g. always 1024 max-width, or no resize at all).
- **B2 — Encoder drift**: a different WebP encoder is used (e.g.
  `image::codecs::webp` instead of `webp::Encoder`), producing
  byte-different output for the same inputs.
- **B3 — Quality drift**: the default quality is mapped to a
  different float scale (e.g. 0..100 vs 0..1).
- **B4 — Concurrency drift**: parallel order changes the per-file
  timing distribution; on slow disks this can surface as a
  different total wall-time, which the operator may notice.

## Decision

The v0 pipeline is the **canonical default pipeline** and is
preserved bit-for-bit when the operator runs the binary without
flags. Specifically:

1. **Default flags are equivalent to the v0 pipeline**:
   `fast-image-converter <dir>` ≡ `gallery-compress <dir>`.
2. **A regression test is added**: a fixed golden batch (10 mixed-
   orientation JPGs, ~300 KB total) is committed under
   `tests/fixtures/golden_v0/`. The test asserts that the default-
   pipeline output bytes match the recorded golden output bytes
   within the 0.1 % tolerance already documented in `README.md`.
3. **The v0 constants are preserved literally**:
   - `QUALITY = 85.0`
   - `PORTRAIT_MAX_W = 800`
   - `LANDSCAPE_MAX_W = 1000`
   - Default resize filter = `FilterType::Lanczos3`
4. **The `gallery-compress` binary alias** forwards to
   `fast-image-converter` with the same argv; on first run after the
   rename it prints a one-time stderr hint recommending the new
   name.

## Alternatives Considered

### A1 — Drop the v0 behaviour; new defaults (e.g. max-width 1024)

**Rejected**: silent default change is the worst regression class —
the operator's existing script produces different output without
any signal. Operators catch this only after the next batch lands in
production.

### A2 — Preserve v0 constants but allow quality / resize overrides

**Adopted** (see ADR-0001 § Decision § 3 + § 4) but only via
**explicit flags** — the no-flags path is locked.

### A3 — Re-encode the golden batch on every release

**Rejected**: the golden batch is the regression ground truth;
re-encoding it defeats the purpose. Instead, a separate benchmark
test compares against `libwebp` reference (see `README.md` "Why
Rust") and is allowed to drift within the documented 0.1 % bound.

## Consequences

### Positive

- The operator's existing scripts keep working.
- Regression risk is bounded to the new code path; the default
  path has a regression test.
- The 0.1 % fidelity bound from `README.md` is now enforceable
  rather than aspirational.

### Negative / Risks

- **R1**: a `libwebp` ABI / version change on the host can break
  the byte-equivalence. Mitigated by pinning `libwebp` in the
  build host via `pkg-config --print-variables webp pcfields`
  and recording the host `libwebp` version in the test report.
- **R2**: a fixed golden batch can go stale (e.g. file system
  permissions on the fixture). Mitigated by committing the
  fixtures as read-only and recording their sha256 in the test
  report.

## Follow-up

- `adr/0001-multi-format-cli-scope.md` (parent decision).
- `Issues/open/developer/DE-002_add_regression_golden_batch.md`
  (queued, to be created on operator approval).
