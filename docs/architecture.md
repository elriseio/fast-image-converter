# Architecture

> **Status**: Draft (initiated 2026-08-03 by architect on multi-format scope expansion).
> See `architecture/STATUS.md` for the canonical meta-document.
> See `ROADMAP.md` for the active wave and planned work.

## 1. Purpose

`convert-to-webp` is a command-line utility for **batch image-format
conversion** between raster image formats. The v0 baseline
(`gallery-compress` binary, v0.2.0) covers the single pipeline
**JPG → WebP** with a fixed orientation-based resize policy. The
multi-format initiative (ADR-0001) extends the utility to a generic
converter that accepts a configurable input format and emits a
configurable output format, while preserving the v0 JPG→WebP baseline
as the default pipeline.

The utility targets:

- **Bulk conversion of mixed-orientation image sets** (typical use
  case: web gallery asset pipelines, photo-archive compression).
- **Single-shot conversion** of one file or a small directory.
- **Deterministic, reproducible conversion** with explicit per-format
  quality parameters (no opaque defaults hidden inside a library).
- **Offline operation**: no network calls, no telemetry, no service
  registry lookups. The binary is self-contained.

The utility explicitly does **not** target:

- Per-pixel editing, colour-correction, or watermarking.
- Live preview / interactive UI.
- Containerised transcoding service (FFmpeg-style).
- Cloud storage adapters.
- Distributed batch orchestration.

## 2. Context (C4 Level 1)

```
+-------------------------+
|  Operator (human)       |
|  - runs CLI in shell    |
|  - reads summary output |
+------------+------------+
             | CLI args + env vars
             | (stdin / stdout / stderr / exit codes)
             v
+-------------------------+
|  convert-to-webp        |
|  (Rust CLI binary)      |
+------------+------------+
             | filesystem read / write
             v
+-------------------------+
|  Filesystem             |
|  - input dir + files    |
|  - output dir + files   |
+-------------------------+
```

There is no upstream service, no downstream consumer other than the
filesystem and the operator's shell. The single external dependency
is the system-provided `libwebp` (for WebP encoding), and — once
ADR-0001 lands — system-provided image-format decoders / encoders
exposed via the `image` crate.

## 3. Container (C4 Level 2)

```
+----------------------------------------+
|  convert-to-webp binary                |
|  +----------------------------------+  |
|  | cli-frontend                     |  |   parses argv, env, print usage
|  |  - arg parser                    |  |
|  |  - usage printer                 |  |
|  |  - exit-code mapper              |  |
|  +---------------+------------------+  |
|                  |                      |
|                  v                      |
|  +----------------------------------+   |
|  | converter-core                   |   |   walks dir, schedules jobs
|  |  - directory walker              |   |
|  |  - parallel dispatcher (rayon)   |   |
|  |  - per-file orchestrator         |   |
|  |  - summary printer               |   |
|  +---------------+------------------+   |
|                  |                      |
|                  v                      |
|  +----------------------------------+   |
|  | format-codecs                    |   |   per-format decode / encode /
|  |  - jpeg codec                    |   |   quality + resize policy
|  |  - png codec                     |   |
|  |  - webp codec                    |   |
|  |  - (future: avif, gif, bmp, tiff) |   |
|  +----------------------------------+   |
+----------------------------------------+
```

## 4. Components

For per-component contracts, failure modes, and invariants, see
`components/`:

- `components/cli-frontend.md`
- `components/converter-core.md`
- `components/format-codecs.md`
- `components/README.md` (canonical registry)

## 5. Contracts

For component-to-component contracts, see `contracts/`:

- `contracts/codec-bounds.md` (codec ↔ converter-core)
- `contracts/report-shape.md` (NDJSON wire shape for `--json` mode; DE-005)
- `contracts/README.md` (canonical registry)

## 6. External Dependencies

| Dependency | Version | Purpose | Risk |
|---|---|---|---|
| `image` crate | 0.25 (with `jpeg`, `png`, `webp` features enabled in `Cargo.toml`) | unified decode API across input formats | API churn between minor versions; pin via `Cargo.lock` |
| `webp` crate | 0.3 | WebP encoder binding to `libwebp` | depends on host `libwebp`; build fails if missing |
| `rayon` crate | 1.10 | data-parallel job dispatcher | none observed at v0 baseline |
| `libwebp` (host) | 1.6+ | native encoder | ABI drift; pin via `pkg-config` |

Build requires `pkg-config`, `cc`, and the `libwebp` development
headers. These are host-level SRE concerns and live in
`RUNBOOK.md` § Build-time failures.

## 7. Quality Attributes

| Attribute | Target | Where it is enforced |
|---|---|---|
| Wall-time on 50 mixed-orientation JPGs (3 MB) | < 2 s on 12 cores | `components/converter-core.md` § performance budget |
| Output bytes vs `libwebp` reference | within 0.1 % | `contracts/codec-bounds.md` § fidelity |
| Memory peak for a single 8K image | < 256 MiB | `components/format-codecs.md` § memory budget |
| Binary size (release, stripped) | < 3 MiB | `Makefile` profile + linker |
| Exit-code contract | 0 / 1 / 2 only | `components/cli-frontend.md` § exit codes |

## 8. Decisions

For architectural decisions, see `adr/`:

- `adr/0001-multi-format-cli-scope.md` — extends the utility to a
  generic image-format converter.
- `adr/0002-preserve-jpg-to-webp-baseline.md` — keeps the v0
  JPG→WebP pipeline as the default behaviour.

## 9. Out of Scope

- Lossless re-encoding of already-compressed images.
- Animated GIF / APNG preservation.
- ICC profile / colour-management transforms beyond the
  `image` crate defaults.
- Per-pixel operations (crop, rotate, watermark).
- Cloud or network upload of converted artefacts.
- Distributed execution across machines (the v0 baseline is
  single-host, multi-core; no cluster mode is planned).

## 10. Cross-References

- `architecture/STATUS.md` — meta-document.
- `ROADMAP.md` — active wave, planned waves, cross-cutting tracks.
- `RUNBOOK.md` — operator runbook.
- `components/README.md` — component registry.
- `contracts/README.md` — contract registry.
- `adr/` — decision log.
- `Issues/open/architect/AR-001_initiate_multi_format_cli.md` —
  the canonical initiation proposal.
