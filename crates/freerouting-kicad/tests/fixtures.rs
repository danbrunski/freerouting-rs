//! K04.7/K04.8: the committed corpus and its KiCad 10 reference exports.

mod common;

use freerouting_dsn::Pcb;

fn manifest() -> Vec<Vec<String>> {
    let text = std::fs::read_to_string(common::fixtures().join("MANIFEST.tsv")).unwrap();
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap().split('\t').collect();
    assert_eq!(
        header,
        [
            "path",
            "source",
            "sha256",
            "bytes",
            "file_version",
            "kicad_pro",
            "licence",
            "kicad10_dsn",
            "kicad10_dsn_sha256",
            "kicad10_host_version"
        ]
    );
    lines
        .map(|l| l.split('\t').map(str::to_owned).collect())
        .collect()
}

#[test]
fn manifest_lists_exactly_the_committed_boards() {
    let rows = manifest();
    let dir = common::fixtures();
    let mut listed: Vec<_> = rows.iter().map(|r| dir.join(&r[0])).collect();
    listed.sort();
    assert_eq!(listed, common::committed_boards());
    assert_eq!(rows.len(), 7);
    for r in &rows {
        assert_eq!(r.len(), 10, "{r:?}");
        let bytes = std::fs::metadata(dir.join(&r[0])).unwrap().len();
        assert_eq!(r[3], bytes.to_string(), "{}", r[0]);
        assert!(
            r[1].starts_with("freerouting@cda540f:fixtures/"),
            "{}",
            r[1]
        );
        assert_eq!(r[6], "LicenseRef-freerouting-fixture");
        assert_eq!(r[2].len(), 64);
        let has_pro = std::fs::read_dir(dir.join(&r[0]).parent().unwrap())
            .unwrap()
            .flatten()
            .any(|e| e.path().extension().is_some_and(|x| x == "kicad_pro"));
        assert_eq!(r[5], if has_pro { "yes" } else { "no" }, "{}", r[0]);
        // every board has a KiCad 10.0.7 reference export
        assert!(dir.join(&r[7]).is_file(), "{}", r[7]);
        assert!(r[9].starts_with("10.0.7"), "{}: {}", r[0], r[9]);
    }
}

#[test]
fn committed_boards_match_the_java_repo() {
    // git show cda540f:<path> byte for byte (skipped without the Java repo)
    let mut checked = 0;
    for r in manifest() {
        let path = r[1].split_once(':').unwrap().1;
        let Some(show) = common::java_show(path) else {
            continue;
        };
        let ours = std::fs::read(common::fixtures().join(&r[0])).unwrap();
        assert!(show == ours, "{} differs from git show", r[0]);
        checked += 1;
    }
    if checked == 0 {
        eprintln!(
            "Java repo at {} not available; skipped",
            common::JAVA_COMMIT
        );
    }
}

#[test]
fn the_kicad10_exports_parse_as_dsn() {
    for r in manifest() {
        let text = std::fs::read_to_string(common::fixtures().join(&r[7])).unwrap();
        assert!(text.starts_with("(pcb \""), "{}", r[7]);
        assert!(!text.contains('\r'), "{} has CRLF line ends", r[7]);
        let pcb = Pcb::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", r[7]));
        assert!(pcb.structure.is_some(), "{}", r[7]);
    }
}
