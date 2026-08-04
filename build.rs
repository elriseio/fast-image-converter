//! Build-time metadata for the JSON report (DE-005).
//!
//! Two values are baked into the binary at compile time:
//!
//! - `CONVERT_TO_WEBP_BUILD_COMMIT_SHA`: short or full `git
//!   rev-parse HEAD`. Set only when the source tree is a git
//!   checkout and `git` is on `PATH`; absent on release tarball
//!   builds.
//! - `CONVERT_TO_WEBP_LIBWEBP_VERSION`: `pkg-config --modversion
//!   libwebp`. Required by the `webp` crate's own build script
//!   (via `webp-sys`), so this always succeeds in practice.
//!
//! Both values flow into the JSON report's `host.build_commit_sha`
//! and `host.libwebp_version` fields via the `env!` / `option_env!`
//! macros in `src/main.rs::host_meta`.

use std::process::Command;

fn git_commit_sha() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8(output.stdout).ok()?;
    let sha = sha.trim();
    if sha.is_empty() {
        return None;
    }
    Some(sha.to_string())
}

fn libwebp_version() -> Option<String> {
    let output = Command::new("pkg-config")
        .args(["--modversion", "libwebp"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let v = String::from_utf8(output.stdout).ok()?;
    let v = v.trim();
    if v.is_empty() {
        return None;
    }
    Some(v.to_string())
}

fn main() {
    if let Some(sha) = git_commit_sha() {
        println!("cargo:rustc-env=CONVERT_TO_WEBP_BUILD_COMMIT_SHA={sha}");
    }
    if let Some(v) = libwebp_version() {
        println!("cargo:rustc-env=CONVERT_TO_WEBP_LIBWEBP_VERSION={v}");
    }
    // Rebuild the binary when HEAD moves; without this, switching
    // branches in a dirty worktree would leave the previous sha
    // baked in.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/heads/");
}
