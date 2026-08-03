# convert-to-webp

Minimal standalone project for the `convert-to-webp` workflow.

Single Rust binary that batch-converts a directory of images
between formats. The default pipeline (no flags) walks a directory
of `.jpg` files and converts each to `.webp` using the system
`libwebp` encoder. Per-orientation size policy matches the
bash+ImageMagick original:

| orientation | rule | quality |
|---|---|---|
| portrait  (h ≥ w) | resize to `800x>` (max width 800, height proportional) | 85 |
| landscape (w > h) | resize to `1000x>` (max width 1000, height proportional) | 85 |
| square    (h == w) | treated as portrait (`800x>`) | 85 |

The source file is removed after a successful conversion. The
script prints a one-line summary of converted bytes.

## Why Rust (vs bash + ImageMagick)

On a 50-file batch of mixed-orientation JPGs (3 MB total) the speedup
on this host (12 cores, libwebp 1.6.0) is:

| engine                  | wall time | user time | sys time |
|---|---|---|---|
| bash + ImageMagick 7.1  | 10.09 s   | 8.60 s    | 2.75 s   |
| Rust + rayon + libwebp  |  1.18 s   | 7.20 s    | 0.77 s   |

~8.5x faster wall time. The CPU work itself is similar; the win comes
from eliminating ~100 process spawns (`identify` + `magick` per image)
and parallelising across cores with `rayon`. Output bytes match within
0.1% (same `libwebp` encoder under the hood).

## Requirements

- Rust toolchain (1.75+; tested with 1.97)
- `libwebp` development files (e.g. `libwebp-dev` on Debian/Ubuntu,
  `libwebp` on Arch, `webp` on Homebrew). At build time `pkg-config`
  must find `libwebp`.
- Standard build toolchain (`cc`, `pkg-config`).

## Build

```bash
cargo build --release
```

Resulting binary: `target/release/convert-to-webp` (~1.6 MB stripped).
The legacy `gallery-compress` name is retained as a backward-
compatible alias; see "Binary rename" below.

## Usage

```bash
# default pipeline (jpg -> webp, no flags)
./target/release/convert-to-webp /tmp/my-images

# by year, against the default GALLERY_BASE
./target/release/convert-to-webp 2025

# explicit format pair
./target/release/convert-to-webp /tmp/my-images --input-format png --output-format webp
./target/release/convert-to-webp /tmp/my-images --input-format webp --output-format png
./target/release/convert-to-webp /tmp/my-images --input-format webp --output-format jpg

# help
./target/release/convert-to-webp --help
```

## Flags

| Flag | Value | Default | Meaning |
|---|---|---|---|
| `--input-format <fmt>` | `jpg`, `png`, `webp` (case-insensitive) | `jpg` | input format (files with this extension are processed) |
| `--output-format <fmt>` | `jpg`, `png`, `webp` (case-insensitive) | `webp` | output format |
| `-h`, `--help` | — | — | print usage to stderr and exit 2 |

The default pair `--input-format jpg --output-format webp` is the v0
`gallery-compress` pipeline preserved bit-for-bit (see ADR-0002).

## Environment

| Var | Default | Meaning |
|---|---|---|
| `GALLERY_BASE` | `<vfatina-home>/public/images/gallery` | Base directory used when the argument is a bare year (e.g. `2025`). Override for other projects. |

## Exit codes

| code | meaning |
|---|---|
| `0` | success (or no candidates found) |
| `1` | runtime error (bad directory, decode/encode failure, ≥1 file failed) |
| `2` | wrong CLI invocation (arg count, unknown flag, bad `--input-format` / `--output-format`, same input/output format) |

## Binary rename

The canonical binary is `convert-to-webp` (matching the project slug).
The legacy v0 name `gallery-compress` is retained as a thin
forwarder that prints a one-time deprecation hint on stderr and
forwards the arguments to the new binary. The deprecation hint
mentions the new name; the legacy name is kept for the next major
version at minimum.

## Layout

```
convert-to-webp/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── .gitignore
├── docs/
│   ├── architecture.md
│   ├── architecture/STATUS.md
│   ├── ROADMAP.md
│   ├── RUNBOOK.md
│   ├── adr/
│   ├── components/
│   └── contracts/
├── src/
│   ├── main.rs
│   └── format.rs
└── tests/
    ├── golden_v0.rs
    └── fixtures/golden_v0/
```

## See also

- `docs/architecture.md` — system architecture overview.
- `docs/architecture/STATUS.md` — architect status meta-document.
- `docs/ROADMAP.md` — active wave + planned waves.
- `docs/RUNBOOK.md` — operator runbook.
- `docs/adr/0001-multi-format-cli-scope.md` — scope decision.
- `docs/adr/0002-preserve-jpg-to-webp-baseline.md` — backward-compat
  baseline decision.
- `docs/components/format-codecs.md` — codec layer specification.
- `docs/components/converter-core.md` — orchestrator specification.
- `docs/components/cli-frontend.md` — CLI layer specification.
- `docs/contracts/codec-bounds.md` — codec ↔ converter-core contract.
