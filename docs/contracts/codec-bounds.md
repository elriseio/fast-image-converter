---
project_slug: convert-to-webp
doc_slug: contract_codec_bounds
doc_type: contract_doc
applicable_roles: [architect, developer, tester]
version: 1
source_artifacts:
  - docs/components/converter-core.md
  - docs/components/format-codecs.md
  - src/main.rs:62-66, 101-136
summary: "Contract between format-codecs and converter-core: Codec trait shape, error semantics, byte-count guarantees, deterministic-encoding requirement."
tags: [contract, codec, trait, interface]
---

# Contract: `codec-bounds`

## 1. Direction

`format-codecs` is invoked by `converter-core` per file. The
contract runs in one direction:

```
converter-core  --(Codec::convert_one)-->  format-codecs
converter-core  <--(Result<Counts, CodecError>)--  format-codecs
```

## 2. Inputs

| Field | Type | Source |
|---|---|---|
| `src` | `&Path` | `converter-core` (the candidate file) |
| `params` | `CodecParams { quality: u8, resize: ResizePolicy }` | `converter-core` (forwarded from CLI flags + defaults) |

## 3. Outputs

| Variant | Payload | When |
|---|---|---|
| `Ok(ConversionReport)` | `{ in_bytes: u64, out_bytes: u64 }` | decode, resize, encode, write, source-delete all succeeded |
| `Err(CodecError::Decode)` | `{ path, message }` | the codec could not produce a `DynamicImage` |
| `Err(CodecError::Encode)` | `{ path, message }` | the encoder could not produce output bytes |
| `Err(CodecError::Io)` | `{ path, kind: WriteSource | WriteDest | DeleteSource, message }` | filesystem-level failure |

## 4. Invariants

- **INV-CB-1**: on success, `in_bytes` equals
  `fs::metadata(src).len()` **before** the codec touches the file.
- **INV-CB-2**: on success, `out_bytes` equals
  `fs::metadata(dst).len()` **after** the codec writes the file.
- **INV-CB-3**: on success, the source file is removed (v0
  behaviour). This is the v0 baseline contract; a future
  `--keep-source` flag (Wave 4) gates the removal but is not in
  Wave 1.
- **INV-CB-4**: on `Err`, the source file is left untouched
  (decode / encode failure). On `Err(CodecError::Io::WriteDest)`,
  the partially-written output file may be left on disk and the
  source may or may not be removed depending on where the I/O
  error occurred.
- **INV-CB-5**: the codec MUST NOT panic on any input file. Any
  internal panic is a bug; converter-core wraps the worker in a
  rayon catch_unwind (planned Wave 2; v0 propagates panics).
- **INV-CB-6**: deterministic encoding. For the same input file
  + same `CodecParams`, repeated invocations MUST produce
  byte-identical output (modulo host `libwebp` version pinning).
  Tested via the golden batch per `adr/0002-preserve-jpg-to-webp-baseline.md`.

## 5. Error Propagation

`converter-core` translates each `CodecError` variant into:

| Codec variant | Stderr line |
|---|---|
| `Decode` | `<src>: <message>` |
| `Encode` | `<src>: <message>` |
| `Io::WriteSource` | not reachable; codec does not write source |
| `Io::WriteDest` | `<src>: <message>` |
| `Io::DeleteSource` | `<src>: <message>` (note: dst is already on disk in this case; operator may need cleanup) |

## 6. Enforcement

- INV-CB-1 / INV-CB-2: enforced by `converter-core`'s end-to-end
  smoke test (per-file in/out byte count check).
- INV-CB-6: enforced by the golden-batch regression test per
  ADR-0002.

## 7. Open Questions (architect hand-off to developer)

- **Q1**: should `Err(CodecError::Io::WriteDest)` clean up the
  partially-written output file? Current v0 behaviour: leave it.
  Wave 1 decision needed.
- **Q2**: should the codec report the encoded image's pixel
  dimensions? Useful for the summary line in Wave 3+; not in
  Wave 1.

## 8. Future Work

- Wave 2: add `catch_unwind` wrapper in `converter-core`.
- Wave 2: add streaming-encode variant for very large images.
