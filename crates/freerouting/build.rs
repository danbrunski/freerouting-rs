//! Bakes the git commit into the engine crate for `freerouting::build_info`
//! (R01.2). No dependency: it runs `git`.
//!
//! `FREEROUTING_GIT_SHA` (set by CI) wins when it is at least 12 hex digits;
//! else `git rev-parse --short=12 HEAD` in the workspace root, but only when
//! the root has its own `.git` (a source tarball unpacked inside another
//! repository must not pick that repository's commit); else `unknown`. It
//! never fails the build, and the value is never read by routing code.
//!
//! Only non-debug builds rerun when HEAD moves: a changed `rustc-env` value
//! recompiles the whole engine crate and everything above it, which would
//! add a full rebuild to every commit of the test loop. A debug build
//! therefore keeps the sha of the commit it was first built at (until
//! `build.rs` or the environment variable changes); release builds, which
//! are the ones that get shipped, always carry the current one.

use std::path::{Path, PathBuf};
use std::process::Command;

const SHA_LEN: usize = 12;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=FREEROUTING_GIT_SHA");
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let root = manifest_dir.join("../..");
    if std::env::var("PROFILE").as_deref() != Ok("debug") {
        watch_head(&root);
    }
    let sha = std::env::var("FREEROUTING_GIT_SHA")
        .ok()
        .and_then(|s| normalize(&s))
        .or_else(|| git_sha(&root))
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=FREEROUTING_GIT_SHA={sha}");
}

/// The first 12 characters of `text` when it is at least 12 hex digits.
fn normalize(text: &str) -> Option<String> {
    let text = text.trim();
    (text.len() >= SHA_LEN && text.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| text[..SHA_LEN].to_ascii_lowercase())
}

/// `git rev-parse --short=12 HEAD` in `root`, if `root` is a checkout.
fn git_sha(root: &Path) -> Option<String> {
    if !root.join(".git").exists() {
        return None;
    }
    let output = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    normalize(&String::from_utf8(output.stdout).ok()?)
}

/// The directory holding `HEAD`: `.git`, or the target of a `.git` file (a
/// linked worktree).
fn git_dir(root: &Path) -> Option<PathBuf> {
    let dot_git = root.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let text = std::fs::read_to_string(&dot_git).ok()?;
    let target = text.trim().strip_prefix("gitdir: ")?;
    Some(root.join(target))
}

/// Rebuild when HEAD moves: `HEAD` changes on a checkout, the ref it names
/// on a commit. Without a checkout nothing is watched, so the value is
/// computed once.
fn watch_head(root: &Path) {
    let Some(dir) = git_dir(root) else { return };
    let head = dir.join("HEAD");
    println!("cargo:rerun-if-changed={}", head.display());
    if let Ok(text) = std::fs::read_to_string(&head)
        && let Some(reference) = text.trim().strip_prefix("ref: ")
    {
        // A linked worktree keeps its refs in the common directory.
        let common = std::fs::read_to_string(dir.join("commondir"))
            .ok()
            .map(|c| dir.join(c.trim()))
            .unwrap_or_else(|| dir.clone());
        println!(
            "cargo:rerun-if-changed={}",
            common.join(reference).display()
        );
    }
}
