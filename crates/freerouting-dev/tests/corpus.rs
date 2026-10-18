//! The corpus manifest (F03.3): pinned commits and hashes, SPDX licences,
//! the committed-board licence check, the bench set and the fetch script.

use std::path::{Path, PathBuf};
use std::process::Command;

use freerouting_dev::corpus::{
    Entry, committed_violations, is_hex, is_spdx_or_none, manifest_problems, parse_allowed,
    parse_manifest,
};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn manifest() -> Vec<Entry> {
    parse_manifest(&std::fs::read_to_string(repo().join("tools/corpus/manifest.json")).unwrap())
        .unwrap()
}

/// PCBWorld-Bench's D3-C boards, a second manifest beside the 30-board corpus.
fn d3c() -> Vec<Entry> {
    parse_manifest(&std::fs::read_to_string(repo().join("tools/corpus/pcbworld-d3c.json")).unwrap())
        .unwrap()
}

fn allowed() -> Vec<String> {
    parse_allowed(
        &std::fs::read_to_string(repo().join("tools/corpus/allowed-licenses.txt")).unwrap(),
    )
}

#[test]
fn every_entry_has_a_commit_a_sha256_and_an_spdx_id_or_licenseref_none() {
    let entries = manifest();
    let problems = manifest_problems(&entries, &allowed());
    assert!(problems.is_empty(), "{problems:#?}");
    for e in &entries {
        assert!(is_hex(&e.commit, 40) && is_hex(&e.sha256, 64), "{}", e.id);
        assert!(is_spdx_or_none(&e.license), "{}: {}", e.id, e.license);
        assert!(!e.attribution.is_empty() && !e.repo.is_empty() && !e.path.is_empty());
    }
}

#[test]
fn the_manifest_spans_layers_tiers_and_planes() {
    let entries = manifest();
    assert!(
        (28..=34).contains(&entries.len()),
        "about 30 boards, got {}",
        entries.len()
    );
    for layers in [2, 4, 6] {
        assert!(
            entries.iter().any(|e| e.layers == layers),
            "no {layers}-layer board"
        );
    }
    for tier in ["easy", "medium", "hard"] {
        assert!(entries.iter().any(|e| e.tier == tier), "no {tier} board");
    }
    let planes = entries.iter().filter(|e| e.has_planes).count();
    assert!(planes >= 5, "{planes} boards with planes");
    let mut repos: Vec<_> = entries.iter().map(|e| e.repo.as_str()).collect();
    repos.sort();
    repos.dedup();
    assert_eq!(repos.len(), entries.len(), "one board per repository");
}

#[test]
fn the_spdx_check_rejects_what_is_not_an_id() {
    for good in [
        "MIT",
        "Apache-2.0",
        "CERN-OHL-S-2.0",
        "GPL-3.0+",
        "LicenseRef-none",
        "CC0-1.0",
    ] {
        assert!(is_spdx_or_none(good), "{good}");
    }
    for bad in [
        "",
        "see repo",
        "LicenseRef-other",
        "NOASSERTION ",
        "MIT/GPL",
        "-MIT",
    ] {
        assert!(!is_spdx_or_none(bad), "{bad:?}");
    }
}

#[test]
fn manifest_problems_name_each_defect() {
    let mut entries = manifest();
    let allowed = allowed();
    let good = entries[0].clone();
    entries[0].commit = "master".into();
    entries[1].sha256 = "abc".into();
    entries[2].license = "see repo".into();
    entries[3].committable = !entries[3].committable;
    entries[4].layers = 8;
    entries[5].id = good.id.clone();
    entries[0].id = good.id.clone();
    let problems = manifest_problems(&entries, &allowed).join("\n");
    for needle in [
        "commit must be 40 hex",
        "sha256 must be 64 hex",
        "neither an SPDX id",
        "committable is",
        "layers 8",
        "duplicate id",
    ] {
        assert!(problems.contains(needle), "{needle} in {problems}");
    }
}

/// A committed board whose licence isn't allow-listed is refused: copy a
/// corpus board's content into a temporary tree and the check names it.
#[test]
fn a_committed_board_with_a_licence_that_is_not_allow_listed_is_refused() {
    let entries = manifest();
    // Stand-in content for a board we hold no copy of: give an entry the hash of known bytes.
    let bytes = b"(kicad_pcb (version 20260206) (generator \"test\"))\n";
    let hash = freerouting::quality::feasibility::report::sha256_hex(bytes);
    let mut restricted = entries
        .iter()
        .find(|e| !e.committable)
        .expect("a non-committable entry")
        .clone();
    restricted.sha256 = hash.clone();
    let mut permissive = entries
        .iter()
        .find(|e| e.committable)
        .expect("a committable entry")
        .clone();
    permissive.sha256 = hash;

    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("corpus-tree");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("fixtures")).unwrap();
    std::fs::write(dir.join("fixtures/board.kicad_pcb"), bytes).unwrap();
    let refused = committed_violations(&dir, &[restricted.clone()]);
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(refused[0].contains(&restricted.id) && refused[0].contains("not allow-listed"));
    assert!(committed_violations(&dir, &[permissive]).is_empty());
    // A different file is not a corpus board.
    std::fs::write(dir.join("fixtures/board.kicad_pcb"), b"other").unwrap();
    assert!(committed_violations(&dir, &[restricted]).is_empty());
}

#[test]
fn every_d3c_entry_has_a_commit_a_sha256_and_an_spdx_id_or_licenseref_none() {
    let entries = d3c();
    assert!(entries.len() >= 8, "{} D3-C boards", entries.len());
    let problems = manifest_problems(&entries, &allowed());
    assert!(problems.is_empty(), "{problems:#?}");
    for e in &entries {
        assert!(is_spdx_or_none(&e.license), "{}: {}", e.id, e.license);
        assert!(e.id.starts_with("d3c-"), "{}", e.id);
    }
    // One namespace for both manifests: FR_CORPUS_DIR may hold either.
    let ids: Vec<String> = manifest().into_iter().map(|e| e.id).collect();
    assert!(entries.iter().all(|e| !ids.contains(&e.id)));
}

#[test]
fn the_d3c_bench_set_lists_every_d3c_board_and_only_those() {
    let set = std::fs::read_to_string(repo().join("tools/bench/sets/pcbworld-d3c.txt")).unwrap();
    let listed: Vec<String> = set
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    let expected: Vec<String> = d3c()
        .iter()
        .map(|e| format!("target/corpus-d3c/prepped/{}.dsn", e.id))
        .collect();
    assert_eq!(listed, expected);
}

#[test]
fn no_committed_board_in_this_repository_has_a_licence_that_is_not_allow_listed() {
    let boards: Vec<Entry> = manifest().into_iter().chain(d3c()).collect();
    let violations = committed_violations(&repo(), &boards);
    assert!(violations.is_empty(), "{violations:#?}");
    // And git tracks no .kicad_pcb file at all outside the allow-list's reach.
    let output = Command::new("git")
        .args(["ls-files", "*.kicad_pcb"])
        .current_dir(repo())
        .output();
    if let Some(output) = output.ok().filter(|o| o.status.success()) {
        let tracked = String::from_utf8_lossy(&output.stdout);
        for line in tracked.lines() {
            let bytes = std::fs::read(repo().join(line)).unwrap_or_default();
            let hash = freerouting::quality::feasibility::report::sha256_hex(&bytes);
            assert!(
                !boards.iter().any(|e| e.sha256 == hash && !e.committable),
                "{line} is a corpus board with a licence that is not allow-listed"
            );
        }
    }
}

#[test]
fn the_bench_set_lists_every_manifest_board_and_only_those() {
    let set = std::fs::read_to_string(repo().join("tools/bench/sets/kicad-corpus.txt")).unwrap();
    let listed: Vec<String> = set
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    let expected: Vec<String> = manifest()
        .iter()
        .map(|e| format!("target/corpus/prepped/{}.dsn", e.id))
        .collect();
    assert_eq!(listed, expected);
}

/// `fetch.sh` with every file already present and correct downloads nothing,
/// and a second run changes nothing (byte-identical fetches are guaranteed
/// by the commit pin and the hash check).
#[test]
fn fetch_accepts_files_that_match_their_hash_without_the_network() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("corpus-fetch");
    let _ = std::fs::remove_dir_all(&dir);
    let board = b"(kicad_pcb (version 20260206))\n";
    let hash = freerouting::quality::feasibility::report::sha256_hex(board);
    let manifest = format!(
        "{{\"boards\": [{{\"id\": \"t\", \"repo\": \"o/r\", \"commit\": \"{c}\", \"path\": \"a/t.kicad_pcb\", \
         \"sha256\": \"{hash}\", \"companions\": {{}}}}]}}",
        c = "0".repeat(40)
    );
    std::fs::create_dir_all(dir.join("corpus/src/t")).unwrap();
    std::fs::write(dir.join("corpus/src/t/t.kicad_pcb"), board).unwrap();
    std::fs::write(dir.join("manifest.json"), manifest).unwrap();
    let run = || {
        Command::new("bash")
            .arg(repo().join("tools/corpus/fetch.sh"))
            .arg("--manifest")
            .arg(dir.join("manifest.json"))
            .env("FR_CORPUS_DIR", dir.join("corpus"))
            .output()
            .unwrap()
    };
    for _ in 0..2 {
        let out = run();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            std::fs::read(dir.join("corpus/src/t/t.kicad_pcb")).unwrap(),
            board
        );
    }
}

/// The owner's canary-pwr boards are corpus entries too, graded by canary-09.
#[test]
fn the_owner_boards_are_committed_fixtures_graded_by_canary_09() {
    let json = freerouting_dev::json_read::Json::parse(
        &std::fs::read_to_string(repo().join("tools/corpus/manifest.json")).unwrap(),
    )
    .unwrap();
    let Some(freerouting_dev::json_read::Json::Array(owner)) = json.get("owner_boards") else {
        panic!("no owner_boards");
    };
    assert!(owner.len() >= 3);
    for entry in owner {
        let path = entry
            .get("committed_path")
            .and_then(|p| p.as_str())
            .unwrap();
        let bytes = std::fs::read(repo().join(path)).unwrap();
        assert_eq!(
            entry.get("sha256").and_then(|h| h.as_str()),
            Some(freerouting::quality::feasibility::report::sha256_hex(&bytes).as_str()),
            "{path}"
        );
        let note = entry.get("note").and_then(|n| n.as_str()).unwrap();
        assert!(
            note.contains("Graded by canary-09") && note.contains("via_in_pad.py = 0"),
            "{note}"
        );
    }
}
