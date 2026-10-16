//! Shared helpers for the integration tests. Not every test binary uses every helper.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

pub const JAVA_COMMIT: &str = "cda540f";

/// The workspace root.
pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `fixtures/kicad/`.
pub fn fixtures() -> PathBuf {
    repo().join("fixtures/kicad")
}

/// The committed boards, as paths under `fixtures/kicad/`, in a fixed order.
pub fn committed_boards() -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect(&fixtures(), &mut out);
    out.sort();
    out
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "kicad_pcb") {
            out.push(p);
        }
    }
}

/// `$FR_KICAD_CORPUS`, default `target/kicad-corpus`, when it exists.
pub fn corpus_dir() -> Option<PathBuf> {
    let d = std::env::var_os("FR_KICAD_CORPUS")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo().join("target/kicad-corpus"));
    d.is_dir().then_some(d)
}

/// All `.kicad_pcb` files under a corpus subfolder, sorted.
pub fn corpus_boards(sub: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(d) = corpus_dir() {
        collect(&d.join(sub), &mut out);
    }
    out.sort();
    out
}

/// `git show <JAVA_COMMIT>:<path>` in the Java repo next to this one, or
/// `None` when the repo or the commit is missing.
pub fn java_show(path: &str) -> Option<Vec<u8>> {
    let java = repo().join("../freerouting");
    let out = Command::new("git")
        .arg("-C")
        .arg(java)
        .arg("show")
        .arg(format!("{JAVA_COMMIT}:{path}"))
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}
