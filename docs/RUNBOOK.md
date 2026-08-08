---
output-policy-exempt: justified-rca-evidence DE-006
---

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

Cause: the `image` crate is pinned with
`default-features = false` + `features = ["jpeg", "png", "webp", "heif"]`
in `Cargo.toml` (per DE-040 / ADR-0004), which enables the JPEG /
PNG / WebP / HEIF decoders inside `image` (WebP encoding goes
through the dedicated `webp` crate, architecturally separate).
Adding a new input format may require extending the `features`
list in `Cargo.toml` and rebuilding. **Do NOT touch `Cargo.toml`
yourself**; the developer owns this and will receive a per-format
handoff.

### 2.4 `pkg-config` cannot find `libheif` (HEIC support, added in DE-040)

```
error: failed to run custom build command for `libheif-sys`
...
thread 'main' panicked at 'called `Result::unwrap()` on an `Err` value: PkgError(EnvError("pkg-config: libheif not found"))'
```

Fix:

- Debian / Ubuntu: `sudo apt install libheif-dev libde265-dev libdav1d-dev`
- Arch: `sudo pacman -S libheif dav1d`
- Homebrew (macOS): `brew install libheif`

Re-run `cargo build --release` after install. HEIC input
support goes through the `libheif-rs` safe wrapper around
`libheif-sys`, which links the system `libheif` C library
together with the `libde265` HEVC decoder and `dav1d` AV1
decoder plugins; all three are required to cover the
iOS 11..16 (HEVC) and iOS 17+ (AV1) HEIC file populations.

**License / patent notes**:

- `libheif`: LGPL-2.1+ (or GPL-2.0+ at the user's option).
  Static linking under LGPL-2.1 requires the operator to
  retain the ability to relink the binary against a modified
  `libheif`. The relink procedure is: (a) obtain the source
  for `libheif` + `libde265` + `dav1d`; (b) recompile with
  the operator's modifications; (c) relink the binary
  against the modified libraries. The combined source tree
  is reproducible from the `libheif-sys` build instructions.
- `libde265`: GPL-2.0+ with a linking exception that permits
  linking from non-GPL applications when `libde265` is used
  as a `libheif` plugin (the binary's use here qualifies). The
  exception text is reproduced in the `libde265` source tree.
- `dav1d`: BSD-2-Clause (permissive, no redistribution
  constraint).
- `libheif-rs` (safe Rust wrapper): MIT.
- HEVC decoder-only use here does not encumber HEVC patent
  claims in the operator's distribution model. Operators in
  jurisdictions with broader HEVC patent claims (notably
  the US, until the patent pool expires) should evaluate the risk
  with their counsel.

**Known limitation**: the `libheif` HEIF decoder
extracts the primary image only from HEIF multi-image
containers (Apple Live Photos, depth-of-field variants).
Matches the v0 baseline behaviour for animated GIF / APNG
(first frame only).

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

_This section is intentionally empty after DE-006 closed AD-001._

## 6. Resolved Defect

### RD-001 — Hard-coded absolute host path (resolved by DE-006)

The `DEFAULT_GALLERY_BASE` constant in `src/main.rs` carried an
absolute host path (`/home/alex/Er/VFSite/vfatina-home/public/images/gallery`)
that leaked operator host layout in source distributions and
broke portability across hosts.

**Resolution** (`DE-006`, commits `f9f940e` + `2deb935`):

- The constant is removed. A positional argument that contains a
  slash is used verbatim (the v0 contract for absolute paths is
  preserved).
- A bare positional argument (e.g. `2025`) now requires
  `GALLERY_BASE` to be set; if unset, the binary prints a usage
  message and exits `2`.
- The README `Environment` table is updated: `GALLERY_BASE` is
  documented as optional with no built-in default.
- No new dependencies were introduced.

**Verification**: `grep -rIn '/home/alex\|/Users' src/` returns
no matches; `cargo build --release` succeeds; `cargo run --
/tmp/some-abs-path` exits `0` on a valid directory and `1` on a
missing one; `cargo test` is green.

**Provenance**: `Issues/done/developer/DE-006_remove_hard_coded_host_path_reopen.md`,
proposal `Issues/done/architect/AR-002_remove_hard_coded_host_path.md`.

## 7. Continuous Quality Gates

This is the merge-gate contract (AR-005). A clean checkout can run
the same gate the CI workflow runs, with no operator-only files
required.

### 7.1 Required Host Tooling

| Tool        | Min version | Source                                |
|-------------|-------------|---------------------------------------|
| rustup      | 1.28+       | <https://rustup.rs>                   |
| Rust        | 1.97.0      | pinned in `rust-toolchain.toml`       |
| libwebp     | 1.0+        | `libwebp-dev` (Debian/Ubuntu), `libwebp` (Arch), `webp` (Homebrew) |
| libheif     | 1.14+       | `libheif-dev` (Debian/Ubuntu, added per DE-040 / ADR-0004), `libheif` (Arch), `libheif` (Homebrew) |
| libde265    | 1.0+        | `libde265-dev` (Debian/Ubuntu, required for HEVC HEIC decode) |
| dav1d       | 1.0+        | `libdav1d-dev` (Debian/Ubuntu, required for AV1 HEIC decode; bundled by `libheif-sys` on some hosts) |
| pkg-config  | any         | system package manager                |
| C compiler  | any         | `build-essential` / `base-devel` / Xcode CLT |
| cargo-audit | ^0.22       | install once: `make audit-install`    |

The CI workflow installs the same packages under Ubuntu 24.04
(`sudo apt-get install -y libwebp-dev libheif-dev libde265-dev
libdav1d-dev pkg-config build-essential`).

### 7.2 Local Verification Command

```sh
scripts/check.sh all
```

Equivalent shorter form (when only one check is in question):

```sh
scripts/check.sh format   # cargo fmt --all -- --check
scripts/check.sh clippy   # cargo clippy --all-targets --all-features -- -D warnings
scripts/check.sh test     # cargo test --all-targets --all-features
scripts/check.sh release  # cargo build --release
scripts/check.sh audit    # cargo audit --deny warnings
```

The script exits non-zero on the first failed check. Each step is
also wired through `make check` for convenience on hosts with a
working `make`. None of the targets read operator-local files
(`Makefile.agent`, `memory.json`, `.symposium/`, `Issues/`); they
all derive inputs from the tracked source tree only.

### 7.3 CI Workflow

`.github/workflows/ci.yml` is the canonical merge gate. It runs on
every pull request and every push to `master` / `main`:

1. Checkout.
2. Install Rust 1.97.0 via `dtolnay/rust-toolchain` (single source
   of truth: `rust-toolchain.toml`).
3. Install `libwebp-dev`, `libheif-dev`, `libde265-dev`, `libdav1d-dev`,
   `pkg-config`, `build-essential` (per DE-040 / ADR-0004; `libheif-dev`
   + `libde265-dev` + `libdav1d-dev` are required for HEIC input).
4. Cache the Cargo registry and `target/` keyed on
   `rust-toolchain.toml` + `Cargo.lock`; cache is **not** keyed on
   PR-supplied content (AR-005 AC-4).
5. Install `cargo-audit` as a standalone binary (NOT a dev-
   dependency per AC-3).
6. Run format, strict Clippy, full tests, release build, advisory
   scan. Failure on any step fails the job.

The workflow uses `permissions: contents: read` (AC-6: least
privilege) and does not publish artefacts on `pull_request` events.

### 7.4 Advisory Exceptions

`cargo audit --deny warnings` is the default. When a transient
advisory cannot be upgraded around, suppress it in
`audit.toml` at the workspace root:

```toml
[advisories]
ignore = [
  "RUSTSEC-2024-0001",   # ticket id, exact version, rationale
]
```

Every ignored advisory must be time-bounded (the ignore is
re-evaluated at each advisory-db refresh) and accompanied by a
rationale that names the affected crate, version range, and the
ticket tracking the upgrade. Empty suppressions are forbidden.

## 8. Release Process

This is the production release contract (AR-008). Releases are
produced by `.github/workflows/release.yml` from a `v*` tag push
on the default branch. No operator-local files participate.

### 8.1 Supported Platforms

| Target triple                        | Status      | Notes                                |
|--------------------------------------|-------------|--------------------------------------|
| `x86_64-unknown-linux-gnu` (Ubuntu)  | **Supported** | release workflow target; CI builds on Ubuntu 24.04 |
| `aarch64-unknown-linux-gnu`          | Best-effort | same toolchain + libwebp packaging; no CI matrix yet |
| `x86_64-apple-darwin` / `arm64-apple-darwin` | Best-effort | macOS Homebrew ships `webp`; cross-compile not yet wired |
| `x86_64-pc-windows-msvc`             | Best-effort | WebP2 dependency via vcpkg or MSYS2; not built in CI |

The canonical release artifact is the Ubuntu 24.04 build.

### 8.2 Release Profile

`Cargo.toml` `[profile.release]` pins:

```toml
opt-level = 3
lto = "thin"
codegen-units = 1
strip = "symbols"
panic = "abort"
```

Trade-offs documented here (AR-008 AC-1):

- `strip = "symbols"` removes the symbol table from the ELF. The
  measured size of `fast-image-converter` is ~1.9 MiB stripped (the v0
  README claimed ~1.6 MiB; the difference is the wider feature
  set: JPEG + PNG + WebP decoders, `rayon`, `libc`, plus `serde_json`
  in tests). After DE-040 (HEIC input via `libheif` + `libde265`
  + `dav1d` static-link) the size delta is expected at +1.5..+2.5
  MiB; the developer PR must record the measured delta with
  `ls -lh target/release/fast-image-converter` before / after.
  The legacy `gallery-compress` forwarder is ~0.37 MiB.
- `panic = "abort"` removes unwinding tables and the `.eh_frame`
  section. A panic in production is a process abort (not a clean
  error path), which is the right trade-off for a batch CLI
  where the unit of failure is the candidate file (handled by
  `CodecError`), not the whole process.
- `lto = "thin"` keeps the per-crate build time tractable while
  still inlining across crate boundaries. `lto = "fat"` would
  shrink the binary by another ~5% but is not worth the wall-
  time cost for CI. With `libheif-sys` added in DE-040, the clean
  build duration delta is expected at +60..+90 s on a 12-core
  host; `sccache` in CI mitigates the cached rebuild case.

### 8.3 Installation

```sh
VERSION="v0.2.0"
TARGET="x86_64-unknown-linux-gnu"

curl -fsSL "https://github.com/<owner>/fast-image-converter/releases/download/${VERSION}/fast-image-converter-${TARGET}.tar.gz" \
  | tar -xz -C /usr/local/bin fast-image-converter gallery-compress

chmod +x /usr/local/bin/fast-image-converter /usr/local/bin/gallery-compress
```

The release tarball layout matches `target/release-dir/`:

```
fast-image-converter        # canonical binary
gallery-compress       # legacy forwarder (ADR-0001 § Decision § 5)
SHA256SUMS             # sha256 checksums
build-manifest.txt     # version, commit, target, rust + libwebp versions
LICENSE                # MIT
```

### 8.4 Checksum Verification

```sh
cd /usr/local/bin
sha256sum -c <<'EOF'
$(curl -fsSL https://github.com/<owner>/fast-image-converter/releases/download/${VERSION}/SHA256SUMS)
EOF
```

Every release's `SHA256SUMS` is generated by the workflow from
the freshly-built binaries. The `build-manifest.txt` records the
exact commit and toolchain versions used to produce the bytes;
operators who require a tighter supply chain can compare the
recorded values against a clean checkout at the same commit.

### 8.5 Rollback

```sh
PREVIOUS="v0.1.0"
curl -fsSL "https://github.com/<owner>/fast-image-converter/releases/download/${PREVIOUS}/fast-image-converter-${TARGET}.tar.gz" \
  | tar -xz -C /usr/local/bin fast-image-converter gallery-compress
sha256sum -c <(curl -fsSL ".../${PREVIOUS}/SHA256SUMS")
```

The release workflow does not auto-delete old tags. Operators
revert to any previously-released tag by reinstalling that tag's
tarball; the `--keep-source` flag and the JSON report shape are
preserved across minor versions, so a rollback at the v0.x
boundary is byte-compatible with the previous minor release.

### 8.6 Release Workflow Contract

`.github/workflows/release.yml`:

- Triggers on `push: tags: [v*]` only. Pull-request events do not
  publish (AR-008 AC-6).
- Permissions: `contents: write` for the GitHub Release
  attachment step; no other token scopes.
- Job 1 (`gate`) re-runs the full AR-005 quality gate (format,
  strict Clippy, all-target tests, dependency advisory) before
  any binary is produced. It also rejects the run if any tracked
  file under `src/`, `tests/`, `docs/`, `scripts/`, `Cargo.toml`,
  `Cargo.lock`, `build.rs`, `Makefile`, `rust-toolchain.toml`, or
  `README.md` contains `/home/alex` or `/Users/alex` (AR-008 AC-4).
- Job 2 (`build`) compiles the release binaries on a clean
  Ubuntu 24.04 runner with the pinned toolchain and `libwebp-dev`
  from `apt`. It measures each binary size, generates the SHA-256
  sums, builds the `build-manifest.txt`, runs the smoke test
  (AR-008 AC-5: `--help`, batch conversion of a small fixture,
  `--single-file` round-trip, exit code validation, legacy
  forwarder), and attaches all artefacts to the GitHub Release.
- The workflow reads `rust-toolchain.toml` as the single source
  of toolchain truth; `Cargo.lock` pins the dependency graph;
  the host-system dependency set matches the gate job
  (`libwebp-dev`, `libheif-dev`, `libde265-dev`, `libdav1d-dev`,
  `pkg-config`, `build-essential`, plus `imagemagick` for the
  smoke-test fixture generation).

### 8.7 Operator-Local File Independence

The release process reads only tracked files. No
`Makefile.agent`, no `memory.json`, no `.symposium/`, no `Issues/`,
no operator-local scratchpads participate. A clean checkout of
the tagged commit plus a working `rustup` + `libwebp-dev` +
`libheif-dev` + `libde265-dev` + `libdav1d-dev` + `pkg-config` +
`build-essential` + `imagemagick` is sufficient (AR-008 AC-8).

## 9. Source Refs

- `architecture.md` — architecture overview.
- `architecture/STATUS.md` § 3 Captured Trade-offs (HEIC
  input-only trade-off row per ADR-0004).
- `components/cli-frontend.md` — exit-code contract.
- `components/converter-core.md` — per-file failure handling.
- `components/format-codecs.md` § 6.4 — HEIC input codec
  family (DE-040 / ADR-0004).
- `Issues/open/architect/AR-001_initiate_multi_format_cli.md` —
  the canonical initiation proposal (Wave 1).
- `Issues/open/architect/AR-003_add_heic_input_support.md` —
  HEIC input proposal (Wave 2.1 driver).
- `Issues/open/developer/DE-040_add_heic_input_codec.md` —
  HEIC input implementation task.
- `docs/adr/0004-add-heic-input-support.md` — HEIC input-only
  decision.
