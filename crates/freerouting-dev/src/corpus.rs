//! The corpus manifest (`tools/corpus/manifest.json`, F03.3) and the check
//! that keeps unlicensed boards out of the repository.
//!
//! The corpus of open KiCad boards lives outside the repository
//! (`$FR_CORPUS_DIR`, fetched by `tools/corpus/fetch.sh`). Only the manifest
//! is committed: per board the repo, the 40-hex commit, the path, the sha256,
//! the SPDX licence read from the repo at that commit, the attribution, the
//! layer count, the tier and whether the board has planes. A board may be
//! committed only if its licence is in `tools/corpus/allowed-licenses.txt`;
//! [`committed_violations`] finds any file in a tree whose content is a
//! manifest board with another licence.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use freerouting::quality::feasibility::report::sha256_hex;

use crate::json_read::Json;

/// One board of the manifest.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: String,
    pub repo: String,
    pub commit: String,
    pub path: String,
    pub sha256: String,
    pub license: String,
    pub attribution: String,
    pub layers: u32,
    pub tier: String,
    pub has_planes: bool,
    pub committable: bool,
}

/// Parses the manifest; the first malformed entry is the error.
pub fn parse_manifest(text: &str) -> Result<Vec<Entry>, String> {
    let json = Json::parse(text)?;
    let Some(Json::Array(boards)) = json.get("boards") else {
        return Err("no `boards` array".into());
    };
    boards.iter().map(parse_entry).collect()
}

fn text_field(entry: &Json, key: &str) -> Result<String, String> {
    match entry.get(key) {
        Some(Json::String(s)) if !s.is_empty() => Ok(s.clone()),
        _ => Err(format!(
            "entry {}: `{key}` missing or empty",
            entry.get("id").and_then(Json::as_str).unwrap_or("?")
        )),
    }
}

fn parse_entry(entry: &Json) -> Result<Entry, String> {
    let bool_field = |key: &str| match entry.get(key) {
        Some(Json::Bool(b)) => Ok(*b),
        _ => Err(format!(
            "entry {}: `{key}` must be true or false",
            text_field(entry, "id").unwrap_or_default()
        )),
    };
    let layers = match entry.get("layers") {
        Some(Json::Number(n)) if n.fract() == 0.0 && *n > 0.0 => *n as u32,
        _ => {
            return Err(format!(
                "entry {}: `layers` must be a count",
                text_field(entry, "id")?
            ));
        }
    };
    Ok(Entry {
        id: text_field(entry, "id")?,
        repo: text_field(entry, "repo")?,
        commit: text_field(entry, "commit")?,
        path: text_field(entry, "path")?,
        sha256: text_field(entry, "sha256")?,
        license: text_field(entry, "license")?,
        attribution: text_field(entry, "attribution")?,
        layers,
        tier: text_field(entry, "tier")?,
        has_planes: bool_field("has_planes")?,
        committable: bool_field("committable")?,
    })
}

/// Whether `text` is `n` lower-case hex digits.
pub fn is_hex(text: &str, n: usize) -> bool {
    text.len() == n && text.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

/// Whether `license` is an SPDX licence identifier (or `LicenseRef-none`):
/// letters, digits, `.`, `-` and `+`, starting with a letter or digit.
pub fn is_spdx_or_none(license: &str) -> bool {
    !license.is_empty()
        && license.as_bytes()[0].is_ascii_alphanumeric()
        && license
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'+'))
        && !license.starts_with("LicenseRef-")
        || license == "LicenseRef-none"
}

/// What is wrong with the manifest as a whole and with its entries.
pub fn manifest_problems(entries: &[Entry], allowed: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut ids = BTreeMap::new();
    for e in entries {
        if ids.insert(e.id.clone(), ()).is_some() {
            problems.push(format!("{}: duplicate id", e.id));
        }
        if !is_hex(&e.commit, 40) {
            problems.push(format!(
                "{}: commit must be 40 hex digits, got {:?}",
                e.id, e.commit
            ));
        }
        if !is_hex(&e.sha256, 64) {
            problems.push(format!("{}: sha256 must be 64 hex digits", e.id));
        }
        if !is_spdx_or_none(&e.license) {
            problems.push(format!(
                "{}: license {:?} is neither an SPDX id nor LicenseRef-none",
                e.id, e.license
            ));
        }
        if e.committable != allowed.contains(&e.license) {
            problems.push(format!(
                "{}: committable is {} but license {} {} in allowed-licenses.txt",
                e.id,
                e.committable,
                e.license,
                if allowed.contains(&e.license) {
                    "is"
                } else {
                    "is not"
                }
            ));
        }
        if !matches!(e.layers, 2 | 4 | 6) {
            problems.push(format!("{}: layers {} is not 2, 4 or 6", e.id, e.layers));
        }
        if !matches!(e.tier.as_str(), "easy" | "medium" | "hard") {
            problems.push(format!("{}: tier {:?}", e.id, e.tier));
        }
    }
    problems
}

/// Reads `allowed-licenses.txt`: one SPDX id per line, `#` comments.
pub fn parse_allowed(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if !matches!(name, ".git" | "target" | "node_modules") {
                walk(&path, out);
            }
        } else if name.ends_with(".kicad_pcb")
            || name.ends_with(".kicad_pro")
            || name.ends_with(".kicad_dru")
        {
            out.push(path);
        }
    }
}

/// Files under `root` whose content is a manifest board (by sha256) with a
/// licence that is not allow-listed, or any `.kicad_pcb` named like a
/// manifest board's file in a non-allowed entry. Returns one message each.
pub fn committed_violations(root: &Path, entries: &[Entry]) -> Vec<String> {
    let mut files = Vec::new();
    walk(root, &mut files);
    let mut out = Vec::new();
    for path in files {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let hash = sha256_hex(&bytes);
        if let Some(e) = entries.iter().find(|e| e.sha256 == hash) {
            if !e.committable {
                out.push(format!(
                    "{} is the corpus board {} ({}), whose licence {} is not allow-listed; \
                     boards with that licence are fetched, never committed",
                    path.display(),
                    e.id,
                    e.repo,
                    e.license
                ));
            }
        }
    }
    out
}
