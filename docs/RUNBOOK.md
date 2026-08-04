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
| pkg-config  | any         | system package manager                |
| C compiler  | any         | `build-essential` / `base-devel` / Xcode CLT |
| cargo-audit | ^0.22       | install once: `make audit-install`    |

The CI workflow installs the same packages under Ubuntu 24.04
(`sudo apt-get install -y libwebp-dev pkg-config build-essential`).

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
3. Install `libwebp-dev`, `pkg-config`, `build-essential`.
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

## 8. Source Refs

- `architecture.md` — architecture overview.
- `architecture/STATUS.md` § 3 Captured Trade-offs.
- `components/cli-frontend.md` — exit-code contract.
- `components/converter-core.md` — per-file failure handling.
- `Issues/open/architect/AR-001_initiate_multi_format_cli.md` —
  the canonical initiation proposal.
