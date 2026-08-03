# convert-to-webp

Minimal standalone project for the `gallery-compress` workflow.

Single Rust binary that walks a directory of `.jpg` files and converts each
to `.webp` using the system `libwebp` encoder. Per-orientation size policy
matches the bash+ImageMagick original:

| orientation | rule | quality |
|---|---|---|
| portrait  (h ≥ w) | resize to `800x>` (max width 800, height proportional) | 85 |
| landscape (w > h) | resize to `1000x>` (max width 1000, height proportional) | 85 |
| square    (h == w) | treated as portrait (`800x>`) | 85 |

The source `.jpg` is removed after a successful conversion. The script
prints a one-line summary of converted bytes.

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

Resulting binary: `target/release/gallery-compress` (~1.6 MB stripped).

## Usage

```bash
# by year, against the default GALLERY_BASE
./target/release/gallery-compress 2025

# arbitrary directory
./target/release/gallery-compress /tmp/my-images
```

## Environment

| Var | Default | Meaning |
|---|---|---|
| `GALLERY_BASE` | `<vfatina-home>/public/images/gallery` | Base directory used when the argument is a bare year (e.g. `2025`). Override for other projects. |

## Exit codes

| code | meaning |
|---|---|
| `0` | success (or no candidates found) |
| `1` | runtime error (bad directory, decode/encode failure, ≥1 file failed) |
| `2` | wrong CLI invocation (arg count != 1) |

## Layout

```
convert-to-webp/
├── Cargo.toml
├── README.md
├── .gitignore
└── src/
    └── main.rs
```