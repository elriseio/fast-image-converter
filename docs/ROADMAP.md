# Roadmap

> **Status**: Draft. See `architecture/STATUS.md` for the meta-document.
> Waves are sequenced smallest-first; each wave is independently
> committable per `TASK_PLANNING_GUIDE.md`.

## Active Wave

### Wave 1 — Multi-format CLI scope expansion

**Goal**: extend the v0 `gallery-compress` binary to a generic
configurable input/output image-format converter while preserving
the v0 default behaviour.

**Driver issue**: `Issues/open/architect/AR-001_initiate_multi_format_cli.md`
(proposal) + the downstream developer task to be created in
`Issues/open/developer/`.

**Acceptance criteria** (top-level, will be re-decomposed into
per-wave tasks):

1. The binary accepts `--input-format <fmt>` and
   `--output-format <fmt>` CLI flags with `<fmt> ∈ {jpg, png, webp}`
   in Wave 1; additional formats (gif, bmp, tiff, avif) gated on
   follow-up waves.
2. The default pipeline (no flags) keeps the v0 behaviour exactly:
   JPG → WebP, portrait 800 px, landscape 1000 px, quality 85.
3. The exit-code contract is preserved (0 / 1 / 2).
4. The summary line on stdout remains a single grep-friendly line.
5. The per-file error line on stderr remains a single
   `<path>: <error>` line.

**Sub-tasks (planned decomposition for developer)**:

| Sub-task | Owner | Status |
|---|---|---|
| 1.1 Introduce `format` module + `Codec` trait | developer | queued |
| 1.2 Wire `--input-format` / `--output-format` CLI flags | developer | queued |
| 1.3 Plumb per-format quality / resize policy | developer | queued |
| 1.4 Update README + add `--help` examples | developer (with lamport) | queued |
| 1.5 Smoke test on a 3-format sample | developer + tester | queued |

**Cross-cutting tracks touched**:

- `cli_ergonomics` — `--help` text + flag surface.
- `output_fidelity` — encoder-config audit per format.
- `regression_risk` — v0 default pipeline must be byte-equivalent.

**Out of Wave 1 scope**: GIF animation, ICC profiles, AVIF encoder,
distributed batch mode, library API.

## Planned Waves

### Wave 2 — Format expansion

- Add `bmp`, `tiff`, animated `gif`, `apng` to the input set.
- Add `avif` to the output set (depends on host `libaom` /
  `libavif` availability; gated on operator environment check).
- Per-format quality presets surfaced via `--quality <n>` flag.

### Wave 3 — Resize policy generalisation

- Replace the v0 per-orientation hard policy with a `--resize`
  flag accepting `<W>x<H>` (cap), `<W>x` (max-width), or `none`.
- Keep the v0 policy as `--resize=auto:portrait=800,landscape=1000`
  default.

### Wave 4 — Operator UX

- `--dry-run` mode that prints the candidate list + planned
  pipeline without writing.
- `--keep-source` mode that preserves the input file (v0 baseline
  removes it after a successful conversion).
- `--jobs <N>` flag for capping the rayon thread pool.

## Recently Closed

_This section is intentionally empty on first publish. The architect
will append here as waves close._

## Cross-Cutting Track Anchors

| Track | Anchor doc | Cadence |
|---|---|---|
| `cli_ergonomics` | `components/cli-frontend.md` | per-flag-add |
| `output_fidelity` | `contracts/codec-bounds.md` | per-codec-add |
| `regression_risk` | `RUNBOOK.md` § Regression incidents | per-release |
| `build_health` | `RUNBOOK.md` § Build-time failures | per-host-update |
| `host_path_leak` | `RUNBOOK.md` § Tech-debt hot list | per-cleanup |

## Source Refs

- `architecture.md` — architecture overview.
- `architecture/STATUS.md` — meta-document + captured trade-offs.
- `adr/0001-multi-format-cli-scope.md` — scope decision.
- `adr/0002-preserve-jpg-to-webp-baseline.md` — backward-compat decision.
- `Issues/open/architect/AR-001_initiate_multi_format_cli.md` — driver proposal.
