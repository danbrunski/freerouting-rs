//! `freerouting-dev tidy` (Q15.1): the five measures on boards with known
//! copper, and the same copper read from a session and from a `.kicad_pcb`.
//!
//! Each fixture is generated from one description (pins and copper in
//! millimetres, KiCad's frame) as a DSN, a session and a `.kicad_pcb`, so the
//! three files hold the same copper by construction. The DSN is in micrometres
//! with `(resolution um 10)` and y negated, like KiCad's export; the session is
//! in tenths of a micrometre.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use freerouting::drc::json::JsonValue;
use freerouting_dev::tidy::{TidyReport, distribution, tidy_board};
use freerouting_dev::tidy_input::{import_routed, routed_board};

const VIA: &str = "Via[0-1]_800:400_um";

#[derive(Clone)]
enum Copper {
    Wire(&'static str, Vec<(f64, f64)>),
    Via(f64, f64),
}

struct Net {
    name: &'static str,
    pins: Vec<(f64, f64)>,
    copper: Vec<Copper>,
}

fn net(name: &'static str, pins: &[(f64, f64)], copper: Vec<Copper>) -> Net {
    Net {
        name,
        pins: pins.to_vec(),
        copper,
    }
}

fn f(points: &[(f64, f64)]) -> Copper {
    Copper::Wire("F.Cu", points.to_vec())
}

fn b(points: &[(f64, f64)]) -> Copper {
    Copper::Wire("B.Cu", points.to_vec())
}

/// Millimetres in KiCad's frame to the DSN's micrometres (y negated).
fn dsn_xy(x: f64, y: f64) -> (i64, i64) {
    ((x * 1000.0).round() as i64, (-y * 1000.0).round() as i64)
}

fn dsn_text(nets: &[Net], via_name: &str) -> String {
    let mut s = String::from(
        "(pcb tidy.dsn\n  (parser (string_quote \")(space_in_quoted_tokens on)\n    \
         (host_cad \"freerouting-rs Q15 test fixture\"))\n  (resolution um 10)\n  (unit um)\n  \
         (structure\n    (layer F.Cu (type signal) (property (index 0)))\n    \
         (layer B.Cu (type signal) (property (index 1)))\n    \
         (boundary (path pcb 0  0 -100000  100000 -100000  100000 0  0 0  0 -100000))\n    ",
    );
    writeln!(
        s,
        "(via \"{via_name}\")\n    (rule (width 200) (clearance 200))\n  )"
    )
    .unwrap();
    s.push_str("  (placement\n    (component TH\n");
    let mut names = Vec::new();
    for net in nets {
        for (x, y) in &net.pins {
            let (dx, dy) = dsn_xy(*x, *y);
            let name = format!("P{}", names.len());
            writeln!(s, "      (place {name} {dx} {dy} front 0)").unwrap();
            names.push(name);
        }
    }
    s.push_str("    )\n  )\n  (library\n    (image TH (pin TH_PAD 1 0 0))\n");
    s.push_str("    (padstack TH_PAD (shape (circle F.Cu 1600)) (shape (circle B.Cu 1600)) (attach off))\n");
    writeln!(
        s,
        "    (padstack \"{via_name}\" (shape (circle F.Cu 800)) (shape (circle B.Cu 800)) (attach off))\n  )"
    )
    .unwrap();
    s.push_str("  (network\n");
    let mut next = 0;
    let mut all = Vec::new();
    for net in nets {
        let pins: Vec<String> = (0..net.pins.len())
            .map(|i| format!("P{}-1", next + i))
            .collect();
        next += net.pins.len();
        writeln!(s, "    (net {} (pins {}))", net.name, pins.join(" ")).unwrap();
        all.push(net.name);
    }
    writeln!(
        s,
        "    (class kicad_default {}\n      (circuit (use_via \"{via_name}\"))\n      \
         (rule (width 200) (clearance 200))\n    )\n  )\n)",
        all.join(" ")
    )
    .unwrap();
    s
}

fn ses_xy(x: f64, y: f64) -> (i64, i64) {
    let (dx, dy) = dsn_xy(x, y);
    (dx * 10, dy * 10)
}

fn ses_text(nets: &[Net]) -> String {
    let mut s = String::from(
        "(session tidy\n  (base_design tidy)\n  (placement (resolution um 10))\n  (was_is)\n  \
         (routes\n    (resolution um 10)\n    (parser (host_cad \"test\") (host_version \"1\"))\n    \
         (library_out)\n    (network_out\n",
    );
    for net in nets {
        writeln!(s, "      (net {}", net.name).unwrap();
        for copper in &net.copper {
            match copper {
                Copper::Wire(layer, points) => {
                    write!(s, "        (wire (path {layer} 2000").unwrap();
                    for (x, y) in points {
                        let (sx, sy) = ses_xy(*x, *y);
                        write!(s, " {sx} {sy}").unwrap();
                    }
                    s.push_str("))\n");
                }
                Copper::Via(x, y) => {
                    let (sx, sy) = ses_xy(*x, *y);
                    writeln!(s, "        (via \"{VIA}\" {sx} {sy})").unwrap();
                }
            }
        }
        s.push_str("      )\n");
    }
    s.push_str("    )\n  )\n)\n");
    s
}

fn kicad_text(nets: &[Net]) -> String {
    let mut s = String::from(
        "(kicad_pcb\n\t(version 20260206)\n\t(generator \"pcbnew\")\n\t(generator_version \"10.0\")\n\t\
         (general (thickness 1.6) (legacy_teardrops no))\n\t(paper \"A4\")\n\t(layers\n\t\t\
         (0 \"F.Cu\" signal)\n\t\t(2 \"B.Cu\" signal)\n\t\t(25 \"Edge.Cuts\" user)\n\t)\n",
    );
    for net in nets {
        for copper in &net.copper {
            match copper {
                Copper::Wire(layer, points) => {
                    for pair in points.windows(2) {
                        writeln!(
                            s,
                            "\t(segment (start {} {}) (end {} {}) (width 0.2) (layer \"{layer}\") \
                             (net \"{}\") (uuid \"00000000-0000-4000-8000-{:012}\"))",
                            pair[0].0,
                            pair[0].1,
                            pair[1].0,
                            pair[1].1,
                            net.name,
                            s.len()
                        )
                        .unwrap();
                    }
                }
                Copper::Via(x, y) => {
                    writeln!(
                        s,
                        "\t(via (at {x} {y}) (size 0.8) (drill 0.4) (layers \"F.Cu\" \"B.Cu\") \
                         (net \"{}\") (uuid \"00000000-0000-4000-8000-{:012}\"))",
                        net.name,
                        s.len()
                    )
                    .unwrap();
                }
            }
        }
    }
    s.push_str(")\n");
    s
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("tidy")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Files {
    dsn: PathBuf,
    ses: PathBuf,
    kicad: PathBuf,
}

fn write_files(dir: &str, nets: &[Net], via_name: &str) -> Files {
    // tests run in parallel: one directory per test
    let test = std::thread::current()
        .name()
        .unwrap_or("t")
        .replace("::", "_");
    let dir = scratch(&format!("{test}-{dir}"));
    let files = Files {
        dsn: dir.join("tidy.dsn"),
        ses: dir.join("tidy.ses"),
        kicad: dir.join("tidy.kicad_pcb"),
    };
    std::fs::write(&files.dsn, dsn_text(nets, via_name)).unwrap();
    std::fs::write(&files.ses, ses_text(nets)).unwrap();
    std::fs::write(&files.kicad, kicad_text(nets)).unwrap();
    files
}

/// The six connections of the main board.
fn main_nets() -> Vec<Net> {
    vec![
        net(
            "straight",
            &[(5.0, 5.0), (25.0, 5.0)],
            vec![f(&[(5.0, 5.0), (25.0, 5.0)])],
        ),
        net(
            "ell",
            &[(5.0, 10.0), (25.0, 20.0)],
            vec![f(&[(5.0, 10.0), (25.0, 10.0), (25.0, 20.0)])],
        ),
        net(
            "zigzag",
            &[(5.0, 40.0), (25.0, 40.0)],
            vec![f(&[
                (5.0, 40.0),
                (10.0, 40.0),
                (15.0, 45.0),
                (20.0, 40.0),
                (25.0, 40.0),
            ])],
        ),
        net(
            "jog",
            &[(5.0, 30.0), (25.0, 30.2)],
            vec![f(&[(5.0, 30.0), (10.0, 30.0), (10.2, 30.2), (25.0, 30.2)])],
        ),
        net(
            "acute",
            &[(5.0, 50.0), (10.0, 55.0)],
            vec![f(&[(5.0, 50.0), (20.0, 50.0), (10.0, 55.0)])],
        ),
        net(
            "viapair",
            &[(5.0, 60.0), (25.0, 60.0)],
            vec![
                f(&[(5.0, 60.0), (10.0, 60.0)]),
                Copper::Via(10.0, 60.0),
                b(&[(10.0, 60.0), (20.0, 60.0)]),
                Copper::Via(20.0, 60.0),
                f(&[(20.0, 60.0), (25.0, 60.0)]),
            ],
        ),
    ]
}

fn score(files: &Files, routed: &Path) -> TidyReport {
    let (board, imported) = routed_board(&files.dsn, routed).unwrap();
    assert_eq!(imported.summary.errors_encountered, 0, "{imported:?}");
    tidy_board(&board)
}

fn connection<'a>(
    report: &'a TidyReport,
    net: &str,
) -> &'a freerouting_dev::tidy_copper::Connection {
    let all: Vec<_> = report
        .measured
        .connections
        .iter()
        .filter(|c| c.net == net)
        .collect();
    assert_eq!(all.len(), 1, "{net}: {:?}", report.measured.connections);
    all[0]
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-3 * expected.abs().max(1.0),
        "{actual} != {expected}"
    );
}

#[test]
fn a_straight_track_has_no_detour_bends_or_vias() {
    let files = write_files("main", &main_nets(), VIA);
    let r = score(&files, &files.ses);
    let c = connection(&r, "straight");
    close(c.length_mm, 20.0);
    close(c.detour, 1.0);
    assert_eq!((c.bends, c.acute, c.jogs, c.vias), (0, 0, 0, 0));
    assert_eq!(c.bends_per_10mm, 0.0);
}

#[test]
fn an_l_has_one_bend_and_its_detour() {
    let files = write_files("main", &main_nets(), VIA);
    let c = score(&files, &files.ses).measured.connections;
    let c = c.iter().find(|c| c.net == "ell").unwrap();
    close(c.length_mm, 30.0);
    close(c.straight_mm, 500.0_f64.sqrt());
    close(c.detour, 30.0 / 500.0_f64.sqrt());
    assert_eq!((c.bends, c.acute, c.jogs, c.vias), (1, 0, 0, 0));
    close(c.bends_per_10mm, 10.0 / 30.0);
}

#[test]
fn a_zig_zag_counts_three_bends_and_none_acute() {
    let files = write_files("main", &main_nets(), VIA);
    let r = score(&files, &files.ses);
    let c = connection(&r, "zigzag");
    let length = 10.0 + 2.0 * 50.0_f64.sqrt();
    close(c.length_mm, length);
    close(c.detour, length / 20.0);
    // 45, 90 and 45 degrees: a right angle is not acute
    assert_eq!((c.bends, c.acute, c.jogs, c.vias), (3, 0, 0, 0));
    close(c.bends_per_10mm, 30.0 / length);
}

#[test]
fn a_short_diagonal_between_two_bends_is_a_jog() {
    let files = write_files("main", &main_nets(), VIA);
    let r = score(&files, &files.ses);
    let c = connection(&r, "jog");
    // the 0.283 mm diagonal is shorter than two 0.2 mm track widths
    assert_eq!((c.bends, c.acute, c.jogs, c.vias), (2, 0, 1, 0));
    close(c.length_mm, 5.0 + 0.08_f64.sqrt() + 14.8);
}

#[test]
fn a_corner_sharper_than_a_right_angle_is_acute() {
    let files = write_files("main", &main_nets(), VIA);
    let r = score(&files, &files.ses);
    let c = connection(&r, "acute");
    assert_eq!((c.bends, c.acute, c.jogs, c.vias), (1, 1, 0, 0));
    close(c.length_mm, 15.0 + 125.0_f64.sqrt());
    close(c.straight_mm, 50.0_f64.sqrt());
}

#[test]
fn a_via_pair_counts_two_vias_and_splits_the_runs() {
    let files = write_files("main", &main_nets(), VIA);
    let r = score(&files, &files.ses);
    let c = connection(&r, "viapair");
    close(c.length_mm, 20.0);
    close(c.detour, 1.0);
    assert_eq!((c.bends, c.acute, c.jogs, c.vias), (0, 0, 0, 2));
}

#[test]
fn the_board_score_is_the_median_and_nearest_rank_p90() {
    let files = write_files("main", &main_nets(), VIA);
    let r = score(&files, &files.ses);
    assert_eq!(r.measured.connections.len(), 6);
    assert_eq!(r.measured.skipped, 0);
    let [detour, bends, acute, jogs, vias] = r.measures.map(Option::unwrap);
    let jog_detour = (5.0 + 0.08_f64.sqrt() + 14.8) / 400.04_f64.sqrt();
    let zig = (10.0 + 2.0 * 50.0_f64.sqrt()) / 20.0;
    close(detour.median, (jog_detour + zig) / 2.0);
    close(detour.p90, (15.0 + 125.0_f64.sqrt()) / 50.0_f64.sqrt());
    let per10 = |bends: f64, length: f64| bends * 10.0 / length;
    close(
        bends.median,
        (per10(1.0, 30.0) + per10(1.0, 15.0 + 125.0_f64.sqrt())) / 2.0,
    );
    close(bends.p90, per10(3.0, 10.0 + 2.0 * 50.0_f64.sqrt()));
    assert_eq!((acute.median, acute.p90), (0.0, 1.0));
    assert_eq!((jogs.median, jogs.p90), (0.0, 1.0));
    assert_eq!((vias.median, vias.p90), (0.0, 2.0));
}

#[test]
fn distributions_use_the_documented_percentiles() {
    assert_eq!(distribution(&[]), None);
    let one = distribution(&[4.0]).unwrap();
    assert_eq!((one.median, one.p90), (4.0, 4.0));
    let ten: Vec<f64> = (1..=10).map(f64::from).collect();
    let d = distribution(&ten).unwrap();
    assert_eq!((d.median, d.p90), (5.5, 9.0));
    let eleven: Vec<f64> = (1..=11).rev().map(f64::from).collect();
    let d = distribution(&eleven).unwrap();
    assert_eq!((d.median, d.p90), (6.0, 10.0));
}

#[test]
fn the_same_copper_from_a_session_and_a_kicad_pcb_scores_the_same() {
    let files = write_files("main", &main_nets(), VIA);
    let from_ses = score(&files, &files.ses);
    let from_kicad = score(&files, &files.kicad);
    assert_eq!(from_ses.measured.connections.len(), 6);
    assert_eq!(from_ses, from_kicad);
    assert_eq!(
        from_ses.to_json(true).to_pretty_string(),
        from_kicad.to_json(true).to_pretty_string()
    );
}

#[test]
fn routed_copper_replaces_the_copper_the_board_already_has() {
    // A board that carries the routed copper already, like an input with
    // wiring of its own that the session repeats: the session or the
    // `.kicad_pcb` imported again replaces it. (An exact second copy does not
    // change these measures; a stale input trace the router ripped does,
    // `tests/session_import.rs`.)
    let files = write_files("again", &main_nets(), VIA);
    let once = score(&files, &files.ses);
    for routed in [&files.ses, &files.kicad] {
        let (mut board, first) = routed_board(&files.dsn, &files.ses).unwrap();
        let wiring = |b: &freerouting::board::BasicBoard| {
            b.items().filter(|i| i.is_trace() || i.is_via()).count()
        };
        let copper = wiring(&board);
        assert_eq!(
            copper,
            first.summary.wires_imported + first.summary.vias_imported
        );
        import_routed(&mut board, routed).unwrap();
        assert_eq!(wiring(&board), copper, "{}", routed.display());
        assert_eq!(tidy_board(&board), once, "{}", routed.display());
    }
}

#[test]
fn a_via_padstack_the_dsn_lacks_takes_the_first_via_type() {
    // the DSN names its via type VIA600, the .kicad_pcb's vias get KiCad's name
    let files = write_files("renamed", &main_nets(), "VIA600");
    let mut board = freerouting_dev::load_board(&files.dsn).unwrap();
    let imported = import_routed(&mut board, &files.kicad).unwrap();
    assert_eq!(imported.via_substitutions, 2);
    assert_eq!(imported.summary.vias_imported, 2);
    assert_eq!(imported.summary.errors_encountered, 0);
    assert_eq!(tidy_board(&board).measured.connections.len(), 6);
}

#[test]
fn a_multi_pin_net_is_its_pin_to_pin_legs() {
    // a Y: the spanning tree is P-R and Q-R (16.2 mm each), not the 20.4 mm P-Q
    let tee = vec![net(
        "tee",
        &[(5.0, 70.0), (25.0, 70.0), (15.0, 78.0)],
        vec![
            f(&[(5.0, 70.0), (15.0, 72.0)]),
            f(&[(15.0, 72.0), (25.0, 70.0)]),
            f(&[(15.0, 72.0), (15.0, 78.0)]),
        ],
    )];
    let files = write_files("tee", &tee, VIA);
    let r = score(&files, &files.ses);
    let leg = 104.0_f64.sqrt() + 6.0;
    assert_eq!(
        r.measured.connections.len(),
        2,
        "{:?}",
        r.measured.connections
    );
    for c in &r.measured.connections {
        close(c.length_mm, leg);
        assert_eq!((c.bends, c.acute, c.jogs, c.vias), (1, 0, 0, 0));
        assert!(c.to.starts_with("P2") || c.from.starts_with("P2"), "{c:?}");
    }
    assert_eq!(r.measured.nets, 1);
    assert_eq!(score(&files, &files.kicad), r);
}

#[test]
fn unrouted_pins_and_stubs_make_no_connection() {
    let open = vec![net(
        "stub",
        &[(5.0, 5.0), (25.0, 5.0)],
        vec![f(&[(5.0, 5.0), (15.0, 5.0)])],
    )];
    let files = write_files("stub", &open, VIA);
    let r = score(&files, &files.ses);
    assert!(r.measured.connections.is_empty());
    assert!(r.measures.iter().all(Option::is_none));
    // a measure over no connections is left out of the JSON
    let json = r.to_json(false).to_pretty_string();
    assert!(
        json.contains("\"connections\": 0") && json.contains("\"measures\": {}"),
        "{json}"
    );
}

fn dev(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_freerouting-dev"))
        .arg("tidy")
        .args(args)
        .output()
        .unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn the_command_prints_a_table_and_the_json() {
    let files = write_files("main", &main_nets(), VIA);
    let dsn = files.dsn.to_str().unwrap();
    let (code, out, err) = dev(&[dsn, files.ses.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("6 connection(s) on 6 net(s), 0 skipped\n"),
        "{out}"
    );
    for label in ["detour", "bends/10mm", "acute", "jogs", "vias/conn"] {
        assert!(out.contains(label), "{out}");
    }

    let json_path = files.dsn.with_file_name("out.json");
    let (code, _, err) = dev(&[
        dsn,
        files.kicad.to_str().unwrap(),
        "--json",
        json_path.to_str().unwrap(),
        "--connections",
    ]);
    assert_eq!(code, 0, "{err}");
    let text = std::fs::read_to_string(&json_path).unwrap();
    let json = freerouting_dev::json_read::Json::parse(&text).unwrap();
    assert_eq!(
        json.get("format").and_then(|v| v.as_str()),
        Some("freerouting-tidy-1")
    );
    assert_eq!(json.get("connections").and_then(|v| v.as_f64()), Some(6.0));
    let vias = json.get("measures").and_then(|m| m.get("vias")).unwrap();
    assert_eq!(vias.get("p90").and_then(|v| v.as_f64()), Some(2.0));
    assert!(text.contains("connection_list"));

    let (code, _, err) = dev(&[dsn]);
    assert_eq!(code, 2);
    assert!(err.contains("usage"), "{err}");
}

#[test]
fn json_numbers_are_plain_decimals() {
    let files = write_files("main", &main_nets(), VIA);
    let r = score(&files, &files.ses);
    let JsonValue::Object(members) = r.to_json(false) else {
        panic!("not an object")
    };
    assert!(members.iter().any(|(k, _)| k == "measures"));
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// Java's route references (`fixtures/reference/route`, bounded runs: some nets
/// stay open, and only the routed pin pairs are connections) pin the counts
/// the Q15.3 baseline table reports.
#[test]
fn java_route_references_are_scored() {
    for (board, connections, nets, detour_median, vias_p90) in [
        ("Issue690-ecc83", 8, 5, 1.1211, 0.0),
        ("Issue690-sonde_xilinx", 38, 21, 1.174, 0.0),
        ("Issue093-interf_u", 162, 99, 1.081, 2.0),
    ] {
        let dsn = fixtures().join(format!("{board}.dsn"));
        let ses = fixtures().join(format!("reference/route/{board}.ses"));
        let (loaded, imported) = routed_board(&dsn, &ses).unwrap();
        assert_eq!(imported.summary.errors_encountered, 0, "{board}");
        let report = tidy_board(&loaded);
        assert_eq!(report.measured.connections.len(), connections, "{board}");
        assert_eq!(report.measured.nets, nets, "{board}");
        assert_eq!(report.measured.skipped, 0, "{board}");
        let [detour, _, acute, _, vias] = report.measures.map(Option::unwrap);
        assert!(
            (detour.median - detour_median).abs() < 1e-3,
            "{board}: {detour:?}"
        );
        assert!(detour.median >= 1.0 && detour.p90 >= detour.median);
        assert_eq!(acute.p90, 0.0, "{board}");
        assert_eq!(vias.p90, vias_p90, "{board}");
    }
}
