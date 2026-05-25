//! Build script: stamps the running binary with the current git short SHA
//! and the build timestamp so `GET /version` can return real values.
//!
//! Both are degraded gracefully when the source tree is not a git checkout
//! (e.g. building from a release tarball) or when `git` is not on PATH.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let git_sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=CRONIQ_GIT_SHA={git_sha}");

    let build_time_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    println!("cargo:rustc-env=CRONIQ_BUILD_TIME_UNIX={build_time_unix}");

    // Re-run when HEAD or the current branch ref changes so the embedded SHA
    // tracks new commits. We deliberately do *not* re-run on every build —
    // that would invalidate the cache constantly during `cargo watch` runs.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");
}
