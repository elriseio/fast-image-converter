# golden_v0/expected — recorded WebP golden outputs

These 10 `.webp` files are the recorded outputs of the
`fast-image-converter` binary (default pipeline, no flags) when run
against the matching `.jpg` files in the parent `golden_v0/`
directory. They are the regression ground truth per ADR-0002
and DE-002.

## Recording context

| Field | Value |
|---|---|
| Binary | `fast-image-converter` v0.2.0 (post-DE-001) |
| Host | the host that committed this golden (see `git log --follow -- tests/fixtures/golden_v0/expected/`) |
| libwebp | 1.6.0 (`pkg-config --modversion libwebp`) |
| Quality | 85 (v0 baseline) |
| Resize | per-orientation: portrait 800, landscape 1000, square 800 |
| Encoder | `webp::Encoder` (lossy VP8) |

## Re-recording procedure

If a libwebp ABI change on the host pushes the byte-equivalence
drift above the 0.1 % tolerance (see `tests/golden_v0.rs`
`BYTE_TOLERANCE`), the golden must be re-recorded:

```bash
tmp=$(mktemp -d)
cp tests/fixtures/golden_v0/*.jpg "$tmp/"
./target/release/fast-image-converter "$tmp"
cp "$tmp"/*.webp tests/fixtures/golden_v0/expected/
rm -rf "$tmp"
```

After re-recording:

1. Update `GOLDEN_LIBWEBP_VERSION` in `tests/golden_v0.rs` to
   the new host libwebp version.
2. Run `cargo test --test golden_v0` to confirm the new golden
   passes the byte-equivalence check.
3. Commit `tests/fixtures/golden_v0/expected/*.webp` and the
   `GOLDEN_LIBWEBP_VERSION` update in the same commit.
