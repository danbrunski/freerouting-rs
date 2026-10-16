//! K05: the reader's tree against the tree of KiCad's own DSN export.

mod common;

use std::path::Path;

use common::parity;
use freerouting_dsn::Pcb;

fn load(board: &Path) -> (freerouting_kicad::KicadRead, Pcb) {
    let text = std::fs::read_to_string(board).unwrap();
    let pro = std::fs::read_to_string(board.with_extension("kicad_pro")).ok();
    let read = freerouting_kicad::read(&text, pro.as_deref())
        .unwrap_or_else(|e| panic!("{}: {e}", board.display()));
    let stem = board.file_stem().unwrap().to_str().unwrap();
    let export = board.with_file_name(format!("{stem}.kicad10.dsn"));
    let export = Pcb::parse(&std::fs::read_to_string(&export).unwrap()).unwrap();
    (read, export)
}

fn all_boards() -> Vec<std::path::PathBuf> {
    common::committed_boards()
        .into_iter()
        .chain(common::synthetic_boards())
        .collect()
}

fn name_of(board: &Path) -> String {
    board.file_name().unwrap().to_string_lossy().into_owned()
}

/// K05.1: the layer list (names, types, order) equals the export's `structure` layers.
#[test]
fn layers_equal_the_export() {
    for board in all_boards() {
        let (read, export) = load(&board);
        let d = parity::layers(&read.pcb, &export);
        assert!(d.is_empty(), "{}: {d:?}", name_of(&board));
    }
}

/// K05.2: net names, their order and every net's pin list equal the export's
/// `network`; `Issue742` is the name dialect (codes by first appearance).
#[test]
fn nets_and_pins_equal_the_export() {
    for board in all_boards() {
        let (read, export) = load(&board);
        let d = parity::network(&read.pcb, &export);
        assert!(d.is_empty(), "{}: {d:?}", name_of(&board));
        assert!(
            !read.pcb.network.as_ref().unwrap().nets.is_empty(),
            "{}",
            name_of(&board)
        );
    }
}

/// K05.3: per class width, clearance, `use_via` and net set equal the export
/// (the order of the non-default classes is KiCad's `std::unordered_map`
/// order and is compared by name); the default rules and via list too.
#[test]
fn net_classes_rules_and_vias_equal_the_export() {
    for board in all_boards() {
        let (read, export) = load(&board);
        let d = parity::classes(&read.pcb, &export);
        assert!(d.is_empty(), "{}: {d:?}", name_of(&board));
        let d = parity::structure_rules(&read.pcb, &export);
        assert!(d.is_empty(), "{}: {d:?}", name_of(&board));
    }
}

/// K05.3: a pattern, an explicit assignment, a regular expression and a net
/// matching two classes give the classes KiCad exports; the KiCad 6 project
/// (schema 2, a class's `nets` list) too.
#[test]
fn synthetic_projects_give_kicad_classes() {
    let names = |board: &str| -> Vec<(String, Vec<String>)> {
        let (read, export) = load(&common::fixtures().join("synthetic").join(board));
        assert!(parity::classes(&read.pcb, &export).is_empty());
        let mut v: Vec<_> = read
            .pcb
            .network
            .as_ref()
            .unwrap()
            .classes
            .iter()
            .map(|c| (c.name.clone(), c.nets.clone()))
            .collect();
        v.sort();
        v
    };
    let s = |a: &[&str]| a.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    assert_eq!(
        names("classes.kicad_pcb"),
        vec![
            ("Fast".to_owned(), s(&["A1", "A2"])),
            ("Fast,Wide".to_owned(), s(&["C"])),
            ("Wide,Default".to_owned(), s(&["B", "D", "E"])),
            ("kicad_default".to_owned(), s(&["/sig1"])),
        ]
    );
    assert_eq!(
        names("classes6.kicad_pcb"),
        vec![
            ("Fast".to_owned(), s(&["A1", "A2", "C"])),
            ("Wide,Default".to_owned(), s(&["/sig1", "B"])),
            ("kicad_default".to_owned(), s(&["D", "E"])),
        ]
    );
}

/// K05.4: boundary area within 0.1 % and the same vertex count; a synthetic
/// concave notched outline keeps its notch and a slot gives one signal keepout.
#[test]
fn outlines_equal_the_export() {
    for board in all_boards() {
        let (read, export) = load(&board);
        let (d, info) = parity::boundary(&read.pcb, &export);
        assert!(d.is_empty(), "{}: {d:?}", name_of(&board));
        println!("{}: {info}", name_of(&board));
        let (a, b) = (
            parity::outline_holes(&read.pcb),
            parity::outline_holes(&export),
        );
        assert_eq!(a.len(), b.len(), "{}: outline holes", name_of(&board));
    }
}

#[test]
fn notched_outline_and_slot() {
    let (read, export) = load(&common::fixtures().join("synthetic/outline.kicad_pcb"));
    let st = read.pcb.structure.as_ref().unwrap();
    let freerouting_dsn::ast::Shape::Path { points, .. } = &st.boundaries[0].shapes[0] else {
        panic!("boundary is a path");
    };
    // the notch (80,60) -> (80,45) -> (70,45) -> (70,60) is still there
    let has = |x: f64, y: f64| {
        points
            .iter()
            .any(|p| (p.x - x * 1000.0).abs() < 1.0 && (p.y + y * 1000.0).abs() < 1.0)
    };
    assert!(has(80.0, 45.0) && has(70.0, 45.0) && has(80.0, 60.0) && has(70.0, 60.0));
    assert_eq!(
        parity::outline_holes(&read.pcb).len(),
        1,
        "one slot, one keepout"
    );
    assert_eq!(parity::outline_holes(&export).len(), 1);
    // the export has the same vertex count
    let freerouting_dsn::ast::Shape::Path { points: theirs, .. } =
        &export.structure.as_ref().unwrap().boundaries[0].shapes[0]
    else {
        panic!()
    };
    assert_eq!(points.len(), theirs.len());
}

/// K05.5: image names, the image of each reference, the component grouping and
/// order, and every place's position, side and rotation equal the export.
#[test]
fn placement_equals_the_export() {
    for board in all_boards() {
        let (read, export) = load(&board);
        let d = parity::placement(&read.pcb, &export);
        assert!(d.is_empty(), "{}: {d:?}", name_of(&board));
        let im = parity::images(&read.pcb, &export);
        assert!(im.names.is_empty(), "{}: {:?}", name_of(&board), im.names);
    }
}

/// K05.5: back-side parts at 0, 30, 90 and 225 degrees (the 10.0.7 export:
/// file 90 gives `back 270`, 0 gives `back 180`).
#[test]
fn back_side_rotations() {
    let (read, _) = load(
        &common::fixtures().join("Issue368-CorneyIslandWireless/corney_island_wireless.kicad_pcb"),
    );
    let place = |r: &str| {
        read.pcb
            .placement
            .as_ref()
            .unwrap()
            .components
            .iter()
            .flat_map(|c| c.places.iter())
            .find(|p| p.name == r)
            .unwrap()
            .location
            .unwrap()
    };
    assert_eq!(
        (place("D16").side, place("D16").rotation),
        (freerouting_dsn::ast::Side::Back, 270.0)
    );
    assert_eq!(
        (place("S16").side, place("S16").rotation),
        (freerouting_dsn::ast::Side::Back, 180.0)
    );
    let (read, _) = load(&common::fixtures().join("synthetic/backside.kicad_pcb"));
    let rot = |r: &str| {
        let l = read
            .pcb
            .placement
            .as_ref()
            .unwrap()
            .components
            .iter()
            .flat_map(|c| c.places.iter())
            .find(|p| p.name == r)
            .unwrap()
            .location
            .unwrap();
        (l.side, l.rotation)
    };
    use freerouting_dsn::ast::Side::{Back, Front};
    // pcbnew's Flip left the footprints at 180, 150, 90 and -45 degrees in the
    // file; the 10.0.7 export says back 0, 330, 270 and 135
    assert_eq!(rot("B1"), (Back, 0.0));
    assert_eq!(rot("B2"), (Back, 330.0));
    assert_eq!(rot("B3"), (Back, 270.0));
    assert_eq!(rot("B4"), (Back, 135.0));
    assert_eq!(rot("F1"), (Front, 30.0));
    assert_eq!(rot("F2"), (Front, -135.0));
}

/// K05.6: pin names, padstack names, positions and rotations equal the export;
/// non-custom padstack shapes equal within `%.6g` rounding; custom pads
/// (the hull of the pad, KiCad's `EXPORT_CUSTOM_PADS_CONVEX_HULL`) within 1 um.
#[test]
fn pads_equal_the_export() {
    for board in all_boards() {
        let (read, export) = load(&board);
        let d = parity::padstacks(&read.pcb, &export);
        assert!(d.is_empty(), "{}: {d:?}", name_of(&board));
        let im = parity::images(&read.pcb, &export);
        assert!(im.pins.is_empty(), "{}: {:?}", name_of(&board), im.pins);
        assert!(
            im.keepouts.is_empty(),
            "{}: {:?}",
            name_of(&board),
            im.keepouts
        );
    }
}

/// K05.6: the synthetic pads board holds one pad of each shape (rotated, with
/// offset, chamfers, a custom pad, a copper-less hole and a repeated number).
#[test]
fn synthetic_pads_cover_every_shape() {
    let (read, _) = load(&common::fixtures().join("synthetic/pads.kicad_pcb"));
    let lib = read.pcb.library.as_ref().unwrap();
    let names: Vec<&str> = lib.padstacks.iter().map(|p| p.name.as_str()).collect();
    for prefix in [
        "Round[A]",
        "Rect[T]",
        "Oval[T]",
        "Trapz[T]",
        "RoundRect[T]",
        "Cust[T]",
    ] {
        assert!(
            names.iter().any(|n| n.starts_with(prefix)),
            "{prefix} in {names:?}"
        );
    }
    assert!(
        names.iter().any(|n| n.contains("[300.000000,-200.000000]")),
        "offset in the name"
    );
    let image = &lib.images[0];
    assert!(
        image.keepouts.len() >= 2,
        "the NPTH hole is a keepout per copper layer"
    );
    let pins: Vec<&str> = image.pins.iter().map(|p| p.name.as_str()).collect();
    assert!(pins.contains(&"1") && pins.contains(&"1@1"), "{pins:?}");
}

/// K05.7: outline counts equal the export's, and each outline's points are
/// within 1 um (no exceptions on the committed boards).
#[test]
fn image_outlines_equal_the_export() {
    for board in all_boards() {
        let (read, export) = load(&board);
        let im = parity::images(&read.pcb, &export);
        assert!(
            im.outlines.is_empty(),
            "{}: {:?}",
            name_of(&board),
            im.outlines
        );
        assert!(
            im.outline_points.is_empty(),
            "{}: {:?}",
            name_of(&board),
            im.outline_points
        );
    }
}

/// K05.8: the tree prints as DSN and parses back to an equal tree.
#[test]
fn tree_round_trips_through_dsn_text() {
    for board in all_boards() {
        let (read, _) = load(&board);
        let text = read.pcb.to_dsn();
        let back = Pcb::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", name_of(&board)));
        assert_eq!(back, read.pcb, "{}", name_of(&board));
    }
}

/// K05.8: `build_board` accepts every board.
#[test]
fn build_board_succeeds() {
    for board in all_boards() {
        let (read, _) = load(&board);
        match freerouting::io::specctra::build_board(&read.pcb) {
            freerouting::io::specctra::BoardReadResult::Success { .. } => {}
            _ => panic!("{}: build_board failed", name_of(&board)),
        }
    }
}

fn mini_board(body: &str) -> String {
    format!(
        "(kicad_pcb (version 20240108) (generator \"pcbnew\") (generator_version \"8.0\")\n\
         (layers (0 \"F.Cu\" signal) (31 \"B.Cu\" signal) (44 \"Edge.Cuts\" user))\n{body})\n"
    )
}

/// K05.5: KiCad's two refusals, with its messages.
#[test]
fn footprint_reference_errors() {
    let fp = |r: &str, v: &str| {
        format!(
            "(footprint \"l:f\" (layer \"F.Cu\") (at 10 10) (property \"Reference\" \"{r}\") (property \"Value\" \"{v}\")\n\
             (pad \"1\" smd rect (at 0 0) (size 1 1) (layers \"F.Cu\")))\n"
        )
    };
    let strict = freerouting_kicad::ReadOptions {
        strict_references: true,
    };
    let e = freerouting_kicad::read_with("b.dsn", &mini_board(&fp("", "10k")), None, strict)
        .unwrap_err();
    assert_eq!(
        e.message,
        "Footprint with value of '10k' has an empty reference designator."
    );
    let two = mini_board(&format!("{}{}", fp("R1", "a"), fp("R1", "b")));
    let e = freerouting_kicad::read_with("b.dsn", &two, None, strict).unwrap_err();
    assert_eq!(
        e.message,
        "Multiple footprints have the reference designator 'R1'."
    );
    // KiCad 10.0.7 (the corpus exports) uniquifies instead
    let r = freerouting_kicad::read(&two, None).unwrap();
    let names: Vec<String> = r
        .pcb
        .placement
        .unwrap()
        .components
        .iter()
        .flat_map(|c| c.places.iter().map(|p| p.name.clone()))
        .collect();
    assert_eq!(names, ["R1", "R1_1"]);
    assert_eq!(
        r.warnings
            .iter()
            .filter(|w| w.message.contains("exported as 'R1_1'"))
            .count(),
        1
    );
    let r = freerouting_kicad::read(&mini_board(&fp("", "x")), None).unwrap();
    assert_eq!(
        r.pcb.placement.unwrap().components[0].places[0].name,
        "REF**"
    );
}

/// K05.1: the stackup and the pads' `die_length` go to `extras`.
#[test]
fn extras_hold_stackup_and_die_length() {
    let text = mini_board(
        "(setup (stackup (layer \"F.Cu\" (type \"copper\") (thickness 0.035))\n\
           (layer \"dielectric 1\" (type \"core\") (thickness 1.5) (material \"FR4\") (epsilon_r 4.5) (loss_tangent 0.02)\n\
             (addsublayer (thickness 0.5) (material \"FR4\") (epsilon_r 3.5) (loss_tangent 0.02)))\n\
           (layer \"B.Cu\" (type \"copper\") (thickness 0.035))))\n\
         (footprint \"l:f\" (layer \"F.Cu\") (at 10 10) (property \"Reference\" \"U1\")\n\
           (pad \"1\" smd rect (at 0 0) (size 1 1) (layers \"F.Cu\") (die_length 1.25)))\n",
    );
    let r = freerouting_kicad::read(&text, None).unwrap();
    let st = &r.extras.stackup;
    assert_eq!(st.len(), 3);
    assert_eq!(st[0].thickness_nm, Some(35_000));
    assert_eq!(st[1].thickness_nm, Some(2_000_000), "sheets add up");
    assert_eq!(st[1].sheets, 2);
    assert_eq!(st[1].epsilon_r, Some((4.5 * 1.5 + 3.5 * 0.5) / 2.0));
    assert_eq!(r.extras.die_lengths.len(), 1);
    assert_eq!(r.extras.die_lengths[0].nm, 1_250_000);
    assert_eq!(r.extras.die_lengths[0].pin, "1");
}

/// K05.1: `canary-pwr` (out of the repository, in the corpus): F.Cu, Gnd.Cu,
/// Power.Cu, B.Cu with types signal, power, power, signal; four 0.035 mm
/// copper layers and dielectrics of 0.1, 1.24 and 0.1 mm with epsilon_r 4.5;
/// structure parity with its export.
#[test]
fn canary_pwr_structure() {
    let boards = common::corpus_boards("canary-pwr");
    let Some(board) = boards.first() else {
        eprintln!("canary-pwr is not in the corpus; skipped");
        return;
    };
    let (read, export) = load(board);
    use freerouting_dsn::ast::LayerType::{Power, Signal};
    let layers: Vec<(String, Option<freerouting_dsn::ast::LayerType>)> = read
        .pcb
        .structure
        .as_ref()
        .unwrap()
        .layers
        .iter()
        .map(|l| (l.name.clone(), l.layer_type.clone()))
        .collect();
    assert_eq!(
        layers,
        vec![
            ("F.Cu".to_owned(), Some(Signal)),
            ("Gnd.Cu".to_owned(), Some(Power)),
            ("Power.Cu".to_owned(), Some(Power)),
            ("B.Cu".to_owned(), Some(Signal)),
        ]
    );
    let st = &read.extras.stackup;
    let copper: Vec<_> = st.iter().filter(|l| l.kind == "copper").collect();
    assert_eq!(copper.len(), 4);
    assert!(copper.iter().all(|l| l.thickness_nm == Some(35_000)));
    let diel: Vec<_> = st
        .iter()
        .filter(|l| l.kind == "core" || l.kind == "prepreg")
        .collect();
    assert_eq!(
        diel.iter()
            .map(|l| l.thickness_nm.unwrap())
            .collect::<Vec<_>>(),
        vec![100_000, 1_240_000, 100_000]
    );
    assert!(diel.iter().all(|l| l.epsilon_r == Some(4.5)));
    for (cat, d) in [
        ("layers", parity::layers(&read.pcb, &export)),
        ("network", parity::network(&read.pcb, &export)),
        ("classes", parity::classes(&read.pcb, &export)),
        ("rules", parity::structure_rules(&read.pcb, &export)),
        ("placement", parity::placement(&read.pcb, &export)),
        ("padstacks", parity::padstacks(&read.pcb, &export)),
        ("boundary", parity::boundary(&read.pcb, &export).0),
    ] {
        assert!(d.is_empty(), "canary-pwr {cat}: {d:?}");
    }
}

/// K05.8: `build_board` succeeds on every board of `$FR_KICAD_CORPUS`; prints
/// the read time of each.
/// `cargo test --release -p freerouting-kicad --test structure corpus_builds -- --ignored --nocapture`
#[test]
#[ignore = "needs $FR_KICAD_CORPUS (tools/kicad-reader/fetch-corpus.sh)"]
fn corpus_builds() {
    let boards = common::corpus_boards("");
    if boards.is_empty() {
        eprintln!("no corpus; skipped");
        return;
    }
    let mut slowest = (0u128, String::new(), 0u64);
    let mut failures = Vec::new();
    for board in boards {
        let bytes = std::fs::read(&board).unwrap();
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let pro = std::fs::read_to_string(board.with_extension("kicad_pro")).ok();
        let t = std::time::Instant::now();
        let read = match freerouting_kicad::read(&text, pro.as_deref()) {
            Ok(r) => r,
            Err(e) if e.kind == freerouting_kicad::ErrorKind::UnsupportedVersion => {
                println!("{}: skipped, {e}", board.display());
                continue;
            }
            Err(e) => {
                failures.push(format!("{}: read: {e}", board.display()));
                continue;
            }
        };
        let ms = t.elapsed().as_millis();
        println!(
            "{}: {} bytes read in {ms} ms",
            board.file_name().unwrap().to_string_lossy(),
            bytes.len()
        );
        if bytes.len() as u64 > slowest.2 {
            slowest = (
                ms,
                board.file_name().unwrap().to_string_lossy().into_owned(),
                bytes.len() as u64,
            );
        }
        match freerouting::io::specctra::build_board(&read.pcb) {
            freerouting::io::specctra::BoardReadResult::Success { .. } => {}
            _ => failures.push(format!("{}: build_board failed", board.display())),
        }
    }
    println!(
        "largest board: {} ({} bytes) read in {} ms",
        slowest.1, slowest.2, slowest.0
    );
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Every corpus board that has a KiCad 10 export: one summary line per board.
/// `cargo test -p freerouting-kicad --test structure corpus_parity -- --ignored --nocapture`
#[test]
#[ignore = "needs $FR_KICAD_CORPUS (tools/kicad-reader/fetch-corpus.sh)"]
fn corpus_parity() {
    let boards = common::corpus_boards("");
    if boards.is_empty() {
        eprintln!("no corpus; skipped");
        return;
    }
    let only = std::env::var("FR_ONLY").ok();
    let mut total_bad = 0;
    for board in boards {
        let stem = board.file_stem().unwrap().to_str().unwrap().to_owned();
        if only
            .as_ref()
            .is_some_and(|o| !board.to_string_lossy().contains(o.as_str()))
        {
            continue;
        }
        if std::fs::metadata(&board).map(|m| m.len()).unwrap_or(0) > 20_000_000
            && std::env::var_os("FR_ALL").is_none()
        {
            println!("{stem}: skipped (over 20 MB; set FR_ALL=1)");
            continue;
        }
        let dsn = board.with_file_name(format!("{stem}.kicad10.dsn"));
        let Ok(export) = std::fs::read_to_string(&dsn) else {
            continue;
        };
        let text = std::fs::read(&board)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap();
        let pro = std::fs::read_to_string(board.with_extension("kicad_pro")).ok();
        let t = std::time::Instant::now();
        let read = match freerouting_kicad::read(&text, pro.as_deref()) {
            Ok(r) => r,
            Err(e) => {
                println!("{stem}: READ ERROR {e}");
                total_bad += 1;
                continue;
            }
        };
        let ms = t.elapsed().as_millis();
        let export = match Pcb::parse(&export) {
            Ok(e) => e,
            Err(e) => {
                println!("{stem}: export does not parse: {e}");
                continue;
            }
        };
        let im = parity::images(&read.pcb, &export);
        let (bd, info) = parity::boundary(&read.pcb, &export);
        let cats: Vec<(&str, usize)> = vec![
            ("layers", parity::layers(&read.pcb, &export).len()),
            ("network", parity::network(&read.pcb, &export).len()),
            ("classes", parity::classes(&read.pcb, &export).len()),
            ("rules", parity::structure_rules(&read.pcb, &export).len()),
            ("placement", parity::placement(&read.pcb, &export).len()),
            ("padstacks", parity::padstacks(&read.pcb, &export).len()),
            ("boundary", bd.len()),
            ("imgnames", im.names.len()),
            ("pins", im.pins.len()),
            ("outlines", im.outlines.len()),
            ("outpts", im.outline_points.len()),
            ("keepouts", im.keepouts.len()),
        ];
        let bad: Vec<String> = cats
            .iter()
            .filter(|(_, n)| *n > 0)
            .map(|(c, n)| format!("{c}={n}"))
            .collect();
        if !bad.is_empty() {
            total_bad += 1;
            if std::env::var_os("FR_VERBOSE").is_some() {
                for (cat, v) in [
                    ("network", parity::network(&read.pcb, &export)),
                    ("classes", parity::classes(&read.pcb, &export)),
                    ("rules", parity::structure_rules(&read.pcb, &export)),
                    ("placement", parity::placement(&read.pcb, &export)),
                    ("padstacks", parity::padstacks(&read.pcb, &export)),
                    ("boundary", bd.clone()),
                    ("pins", im.pins.clone()),
                    ("outlines", im.outlines.clone()),
                    ("outpts", im.outline_points.clone()),
                    ("keepouts", im.keepouts.clone()),
                ] {
                    for l in v.iter().take(2) {
                        println!(
                            "    [{cat}] {}",
                            l.chars().take(700).collect::<String>().replace('\n', " | ")
                        );
                    }
                }
            }
        }
        println!(
            "{stem}: {} ms, {} nets, {} images; {}; {}",
            ms,
            read.pcb.network.as_ref().unwrap().nets.len(),
            read.pcb.library.as_ref().unwrap().images.len(),
            if bad.is_empty() {
                "OK".to_owned()
            } else {
                bad.join(" ")
            },
            info.chars().take(80).collect::<String>()
        );
    }
    println!("boards with differences: {total_bad}");
}
