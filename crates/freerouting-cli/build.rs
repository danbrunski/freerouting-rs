//! Bakes the git commit into the binary for `--version` and the result
//! manifest (C08.2). No dependency: it runs `git`.
//!
//! `FREEROUTING_GIT_SHA` overrides the lookup (Java reads the same
//! variable); without git, or outside a checkout, the value is `unknown`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=FREEROUTING_GIT_SHA");
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let root = manifest_dir.join("../..");
    watch_head(&root);
    let sha = std::env::var("FREEROUTING_GIT_SHA")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| git_sha(&root))
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=FREEROUTING_GIT_SHA={sha}");
}

/// `git rev-parse --short=9 HEAD` in `root`.
fn git_sha(root: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short=9", "HEAD"])
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!sha.is_empty()).then_some(sha)
}

/// Rebuild when HEAD moves: `.git/HEAD` changes on a checkout, the ref it
/// names on a commit.
fn watch_head(root: &Path) {
    let head = root.join(".git/HEAD");
    println!("cargo:rerun-if-changed={}", head.display());
    if let Ok(text) = std::fs::read_to_string(&head)
        && let Some(reference) = text.trim().strip_prefix("ref: ")
    {
        println!(
            "cargo:rerun-if-changed={}",
            root.join(".git").join(reference).display()
        );
    }
}
