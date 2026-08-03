# Runbook

> **Audience**: operator (human) + developer (on call).
> **Cadence**: append on every incident; retire stale entries
> during quarterly `STATUS.md` review.

## 1. First-30-Minutes Triage

| Symptom | First check | Likely cause | Fix |
|---|---|---|---|
| `error: failed to run custom build command for webp` | `pkg-config --modversion libwebp` | host missing `libwebp-dev` | install via system package manager; see § 2.1 |
| `gallery-compress: not a directory: <path>` (exit 1) | `ls -la <path>` | wrong arg, or `GALLERY_BASE` not set | pass an absolute path or set `GALLERY_BASE` |
| `gallery-compress: cannot read <path>: <errno>` | `ls -la <path>` | permission denied | chmod / chown; or rerun from a directory the user can read |
| `gallery-compress: <file>: <decode error>` (per-file, exit 1) | `file <file>` | corrupt JPEG / non-JPEG with `.jpg` ext | fix the file; for now the binary will skip + count the failure |
| Binary hangs (no output for > 30 s) | `top -p $(pgrep gallery-compress)` | stuck rayon thread; usually I/O wait on a slow disk | wait; if reproducibly stuck, file an issue with `strace -p` capture |
| Empty summary line (0 files processed) | `ls <dir>/*.jpg` | no `.jpg` files (case-sensitive filter) | rename extensions or pass a directory that has `.jpg` files |

## 2. Build-Time Failures

### 2.1 `pkg-config` cannot find `libwebp`

```
error: failed to run custom build command for `webp`
...
thread 'main' panicked at 'called `Result::unwrap()` on an `Err` value: PkgError(EnvError("pkg-config: libwebp not found"))'
```

Fix:

- Debian / Ubuntu: `sudo apt install libwebp-dev`
- Arch: `sudo pacman -S libwebp`
- Homebrew (macOS): `brew install webp`

Re-run `cargo build --release` after install.

### 2.2 `cc` not found

Fix: install a C toolchain. Debian / Ubuntu: `sudo apt install build-essential`.
Arch: `sudo pacman -S base-devel`. Homebrew: `xcode-select --install`.

### 2.3 `image` crate build failure

Cause: the `image` crate's `default-features = false` + `features = ["jpeg"]`
selection intentionally disables PNG / WebP decoders inside `image`
(WebP encoding goes through the dedicated `webp` crate). Adding a new
format may require extending the `features` list in `Cargo.toml` and
rebuilding. **Do NOT touch `Cargo.toml` yourself**; the developer owns
this and will receive a per-format handoff.

## 3. Runtime Incidents

### 3.1 Partial batch failure

The summary line on stderr prints `(processed N candidates, K failed)`.
Exit code is `1` if any file failed. To find the failing files, redirect
stderr:

```
gallery-compress 2025 2> failures.log
grep '^gallery-compress:' failures.log
```

### 3.2 Output bigger than input

For very small source images (e.g. < 50 KB thumbnails), the WebP
container overhead may exceed the savings from re-quantisation. The
operator decision is either to accept the larger output, or to
re-quantise at a lower `quality` (under Wave 3+ via `--quality`).

### 3.3 Source-file removal on partial failure

**Risk**: the v0 baseline removes the source `.jpg` only after a
successful conversion. A decode failure or an encoder panic leaves the
source intact. This is the documented v0 contract.

**Future mitigation** (post-Wave 1, gated on operator request): add
`--keep-source` flag. Captured in `ROADMAP.md` § Wave 4.

## 4. Regression Incidents

TBD — populate on first regression report.

## 5. Active Defect

### AD-001 — Hard-coded absolute host path in `src/main.rs:14-15`

```
const DEFAULT_GALLERY_BASE: &str =
    "<absolute-host-path>/public/images/gallery";
```

**Severity**: Low (functional impact only if the operator runs the
binary with a bare year argument on a different host).
**Privacy impact**: leaks operator host layout in any source
distribution.

**Status**: queued for cleanup. Captured as `AR-002` for the developer
role. Architect MUST NOT touch code.

**Operator workaround**: always pass an absolute path or set
`GALLERY_BASE` explicitly.

## 6. Resolved Defect

_This section is intentionally empty on first publish._

## 7. Source Refs

- `architecture.md` — architecture overview.
- `architecture/STATUS.md` § 3 Captured Trade-offs.
- `components/cli-frontend.md` § exit-code contract.
- `components/converter-core.md` § per-file failure handling.
- `Issues/open/architect/AR-001_initiate_multi_format_cli.md` —
  the canonical initiation proposal.
