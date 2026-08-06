---
project_slug: fast-image-converter
doc_slug: adr_0001_multi_format_cli_scope
doc_type: adr
applicable_roles: [architect, developer]
version: 1
date: 2026-08-03
status: proposed
supersedes: null
summary: "Extend fast-image-converter from a single JPG→WebP converter to a generic configurable input/output image-format converter, with the v0 pipeline as the default."
source_artifacts:
  - README.md (v0 baseline behaviour)
  - src/main.rs:11-15 (v0 constants)
  - Issues/open/architect/AR-001_initiate_multi_format_cli.md (driver)
tags: [adr, scope, multi-format, converter]
---

# ADR-0001 — Multi-format CLI scope

## Status

**Proposed** (2026-08-03). Awaiting operator approval.

## Date

2026-08-03

## Authors

- Architect (system)
- Operator (pending approval)

## Context

The `fast-image-converter` project currently hosts a single Rust binary
named `gallery-compress` (v0.2.0) that walks a directory of `.jpg`
files and converts each to `.webp` using `libwebp` via the `webp`
crate, with a hard-coded orientation-based resize policy:

| orientation | rule | quality |
|---|---|---|
| portrait (h ≥ w) | max-width 800 | 85 |
| landscape (w > h) | max-width 1000 | 85 |
| square (h == w) | treated as portrait | 85 |

This single-pipeline design satisfies the original use case (a single
operator with a single photo gallery). It does not satisfy two
emerging use cases:

- **U1**: a different photo source (PNG-only screenshots, mixed JPG +
  PNG + WebP from a third-party uploader) that needs to land in the
  same WebP pipeline.
- **U2**: an asset pipeline that needs to emit **into** formats other
  than WebP (e.g. WebP → AVIF for next-gen, WebP → PNG for archival).

Operator has expressed intent (2026-08-03 chat directive): «инициируй
проект, подготовь все необходимые документы. Это cli утилита для
конвертации изображений между форматами». The literal Russian phrasing
«между форматами» (between formats, plural) is the binding scope
directive.

## Decision

We will extend `fast-image-converter` to a **generic configurable
input/output image-format converter**, with the following scope:

1. **Default pipeline preserved**: no flags → the v0 JPG → WebP
   pipeline runs unchanged.
2. **Wave 1 surfaces**: `--input-format <fmt>`, `--output-format
   <fmt>`, where `<fmt> ∈ {jpg, png, webp}` for Wave 1.
3. **Wave 1 quality policy**: fixed at `85` for all formats (matches
   the v0 WebP quality); overridable via `--quality <n>` from Wave 2.
4. **Wave 1 resize policy**: fixed at the v0 per-orientation policy
   for the default pipeline; overridable via `--resize ...` from
   Wave 3.
5. **Binary name**: under Wave 1 the binary is renamed to
   `fast-image-converter` (matching the project slug); the v0 name
   `gallery-compress` is retained as a backward-compatible alias
   that prints a one-time deprecation hint and then forwards to
   the new binary.

## Alternatives Considered

### A1 — Keep v0 single-pipeline, fork per use case

Spawn a separate `gallery-compress-png`, `gallery-compress-avif`,
etc. **Rejected**: violates the Single-Binary principle; doubles
maintenance; forces operator to remember which fork to call.

### A2 — Replace `gallery-compress` with a shell wrapper around
ImageMagick

**Rejected**: the v0 baseline already demonstrated an 8.5× wall-time
win over ImageMagick on a 50-file batch. Reverting would discard
that gain and re-introduce ~100 process spawns per batch.

### A3 — Library API + thin CLI

Expose `libconvert_to_webp` and have the CLI be a 50-line wrapper.
**Rejected**: introduces an unstable public surface (semver
obligations, breaking-change handling) for a tool with one operator.
Library API is explicitly out of scope per `architecture.md` § 1.

### A4 — Adopt the `image` crate's CLI as-is

`image` provides a `convert` subcommand. **Rejected**: it does not
parallelise across cores, does not honour the per-orientation resize
policy, and does not match the v0 exit-code contract.

### A5 — Adopt `ffmpeg` for everything

**Rejected**: `ffmpeg` brings a 50+ MiB dependency, a different CLI
idiom, and operator confusion ("is it `gallery-compress` or
`ffmpeg`?"). Out of scope per `architecture.md` § 9.

## Consequences

### Positive

- One binary covers U1 + U2 + the original JPG→WebP use case.
- The v0 default pipeline is preserved byte-for-byte (regression
  risk is bounded to the new code path).
- The Codec trait surfaces a natural extension point for Wave 2
  (additional formats).

### Negative / Risks

- **R1**: the multi-format surface area invites feature creep.
  Mitigated by gating additional formats on per-wave ADR updates.
- **R2**: the per-format encoder configuration may produce
  visually different output for the same quality parameter (e.g.
  `image::codecs::png` vs `image::codecs::jpeg`). Mitigated by
  fixing per-format defaults in `components/format-codecs.md` and
  by treating the per-format quality as a coarse knob, not a
  fidelity contract.
- **R3**: the binary-name rename + alias adds a small compat
  burden. Mitigated by keeping `gallery-compress` as a thin
  forwarder for at least one major version.

## Follow-up

- `adr/0002-preserve-jpg-to-webp-baseline.md` — backward-compat
  baseline.
- `Issues/open/architect/AR-001_initiate_multi_format_cli.md` —
  driver proposal.
- `Issues/open/developer/DE-001_implement_multi_format_codecs.md`
  (queued, to be created on operator approval).
