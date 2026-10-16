//! K06: rule areas, planes, wires and vias against KiCad's DSN export.

mod common;

use std::path::Path;

use common::{copper, parity};
use freerouting::board::RoutingBoard;
use freerouting::board::items::AreaKind;
use freerouting::board::structure::FixedState;
use freerouting::io::specctra::{BoardReadResult, build_board};
use freerouting_dsn::Pcb;
use freerouting_dsn::ast::{KeepoutKind, Shape, WireType, WiringItem};
use freerouting_geometry::{IntPoint, Point, PolylineShapeOps};

/// Reads like KiCad's export does, which drops the copper zones footprints own, so that the
/// trees compare.
fn load(board: &Path) -> (freerouting_kicad::KicadRead, Pcb) {
    let text = std::fs::read_to_string(board).unwrap();
    let pro = std::fs::read_to_string(board.with_extension("kicad_pro")).ok();
    let options = freerouting_kicad::ReadOptions {
        drop_footprint_zones: true,
        ..Default::default()
    };
    let read = freerouting_kicad::read_with("board.dsn", &text, pro.as_deref(), options)
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

#[test]
fn copper_equals_the_export() {
    for board in all_boards() {
        let (read, export) = load(&board);
        let n = name_of(&board);
        for (cat, d) in [
            (
                "rule areas",
                copper::structure_rule_areas(&read.pcb, &export, &read.report),
            ),
            (
                "image rule areas",
                copper::image_rule_areas(&read.pcb, &export, &read.report),
            ),
            ("planes", copper::planes(&read.pcb, &export)),
            ("wires", copper::wires(&read.pcb, &export)),
            ("vias", copper::wire_vias(&read.pcb, &export)),
            ("via padstacks", copper::via_padstacks(&read.pcb, &export)),
            ("via list", copper::via_lists(&read.pcb, &export)),
            ("network", parity::network(&read.pcb, &export)),
        ] {
            assert!(
                d.is_empty(),
                "{n}: {cat}: {:#?}",
                d.iter().take(3).collect::<Vec<_>>()
            );
        }
    }
}

#[test]
#[ignore]
fn summary() {
    for board in all_boards() {
        let (read, export) = load(&board);
        println!(
            "{}: {} | export planes {} wires {}",
            name_of(&board),
            read.report.to_string().replace('\n', "; "),
            export.structure.as_ref().unwrap().planes.len(),
            export.wiring.as_ref().unwrap().items.len()
        );
    }
}

/// Every corpus board that has a KiCad 10 export (`$FR_KICAD_CORPUS`).
/// `cargo test --release -p freerouting-kicad --test copper corpus_copper -- --ignored --nocapture`
#[test]
#[ignore = "needs $FR_KICAD_CORPUS (tools/kicad-reader/fetch-corpus.sh)"]
fn corpus_copper() {
    let mut bad = 0;
    for board in common::corpus_boards("") {
        let stem = board.file_stem().unwrap().to_str().unwrap().to_owned();
        let dsn = board.with_file_name(format!("{stem}.kicad10.dsn"));
        let Ok(export) = std::fs::read_to_string(&dsn) else {
            continue;
        };
        if std::fs::metadata(&board).map(|m| m.len()).unwrap_or(0) > 20_000_000
            && std::env::var_os("FR_ALL").is_none()
        {
            println!("{stem}: skipped (over 20 MB; set FR_ALL=1)");
            continue;
        }
        let text = String::from_utf8_lossy(&std::fs::read(&board).unwrap()).into_owned();
        let pro = std::fs::read_to_string(board.with_extension("kicad_pro")).ok();
        let options = freerouting_kicad::ReadOptions {
            drop_footprint_zones: true,
            ..Default::default()
        };
        let Ok(read) = freerouting_kicad::read_with("board.dsn", &text, pro.as_deref(), options)
        else {
            println!("{stem}: read error");
            continue;
        };
        let Ok(export) = Pcb::parse(&export) else {
            continue;
        };
        let mut notes = Vec::new();
        for (cat, d) in [
            (
                "rule areas",
                copper::structure_rule_areas(&read.pcb, &export, &read.report),
            ),
            (
                "image rule areas",
                copper::image_rule_areas(&read.pcb, &export, &read.report),
            ),
            ("planes", copper::planes(&read.pcb, &export)),
            ("wires", copper::wires(&read.pcb, &export)),
            ("vias", copper::wire_vias(&read.pcb, &export)),
            ("via padstacks", copper::via_padstacks(&read.pcb, &export)),
            ("via list", copper::via_lists(&read.pcb, &export)),
            ("network", parity::network(&read.pcb, &export)),
        ] {
            if !d.is_empty() {
                notes.push(format!("{cat}={}", d.len()));
                if std::env::var_os("FR_VERBOSE").is_some() {
                    for l in d.iter().take(2) {
                        println!(
                            "    [{cat}] {}",
                            l.chars().take(600).collect::<String>().replace('\n', " | ")
                        );
                    }
                }
            }
        }
        if !notes.is_empty() {
            bad += 1;
        }
        println!(
            "{stem}: planes {} wires {} vias {} areas {} arcs {}; {}",
            read.report.planes,
            read.report.wires,
            read.report.vias,
            read.report.rule_areas.len(),
            read.report.track_arcs_flattened,
            if notes.is_empty() {
                "OK".to_owned()
            } else {
                notes.join(" ")
            }
        );
    }
    println!("boards with differences: {bad}");
}

fn built(read: &freerouting_kicad::KicadRead) -> Box<RoutingBoard> {
    match build_board(&read.pcb) {
        BoardReadResult::Success { board, .. } => board,
        _ => panic!("build_board failed"),
    }
}

fn synthetic(name: &str) -> (freerouting_kicad::KicadRead, Pcb) {
    load(&common::fixtures().join("synthetic").join(name))
}

fn warnings_with<'a>(read: &'a freerouting_kicad::KicadRead, what: &str) -> Vec<&'a str> {
    read.warnings
        .iter()
        .map(|w| w.message.as_str())
        .filter(|m| m.contains(what))
        .collect()
}

/// (kind, layer name) of the areas of the board that no footprint owns, sorted.
fn board_areas(board: &RoutingBoard) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = board
        .items()
        .filter(|i| i.component_id() == 0)
        .filter_map(|i| i.as_area().map(|a| (a, i)))
        .filter(|(a, _)| !a.is_conduction_area())
        .map(|(a, _)| {
            (
                format!("{:?}", a.kind),
                board.layer_structure.layers[a.layer() as usize]
                    .name
                    .clone(),
            )
        })
        .collect();
    v.sort();
    v
}

/// K06.2: one `Obstacle` per layer for each tracks-and-vias area; `ViaObstacle` for vias
/// only; `Obstacle` plus a warning for tracks only; an added `ComponentObstacle` for each area
/// banning footprints; nothing for pads only, copper pour only and placement only.
#[test]
fn rule_areas_map_by_flags() {
    let (read, _) = synthetic("rule-areas.kicad_pcb");
    let by_name = |n: &str| {
        read.report
            .rule_areas
            .iter()
            .find(|a| a.name.as_deref() == Some(n))
            .unwrap_or_else(|| panic!("no rule area {n}"))
    };
    let emitted = |n: &str| by_name(n).emitted.clone();
    assert_eq!(emitted("tracks-vias"), ["keepout"]);
    assert_eq!(emitted("vias"), ["via_keepout"]);
    assert_eq!(emitted("tracks"), ["wire_keepout"]);
    assert_eq!(emitted("footprints"), ["place_keepout"]);
    assert_eq!(
        emitted("tracks-footprints"),
        ["wire_keepout", "place_keepout"]
    );
    assert_eq!(emitted("pads-footprints"), ["place_keepout"]);
    for none in ["pads", "pour", "placement"] {
        assert!(emitted(none).is_empty(), "{none}");
    }
    assert_eq!(
        by_name("all-copper").layers,
        ["F.Cu", "B.Cu", "In1.Cu", "In2.Cu"]
    );
    assert_eq!(
        by_name("inner").layers,
        ["In1.Cu", "In2.Cu"],
        "layer-id order"
    );
    // one warning per area for the three cases the board cannot express
    assert_eq!(
        warnings_with(&read, "bans tracks but not vias").len(),
        4,
        "tracks-only: 2 board + 2 footprint areas"
    );
    let nothing = warnings_with(&read, "no keepout emitted");
    assert_eq!(nothing.len(), 3, "{nothing:?}");
    assert!(
        nothing
            .iter()
            .any(|m| m.contains("'pour'") && m.contains("plane on its layers is not cut"))
    );
    assert!(
        nothing
            .iter()
            .any(|m| m.contains("'pads'") && m.contains("only pads"))
    );
    assert!(nothing.iter().any(|m| m.contains("'placement'")));

    let board = built(&read);
    let o = |k: &str, l: &str| (k.to_owned(), l.to_owned());
    let mut want = vec![
        // tracks-vias, tracks, tracks-footprints, hole, all-copper[F], f-and-b[F]
        o("Obstacle", "F.Cu"),
        o("Obstacle", "F.Cu"),
        o("Obstacle", "F.Cu"),
        o("Obstacle", "F.Cu"),
        o("Obstacle", "F.Cu"),
        o("Obstacle", "F.Cu"),
        o("Obstacle", "B.Cu"),
        o("Obstacle", "B.Cu"), // all-copper, f-and-b
        o("Obstacle", "In1.Cu"),
        o("Obstacle", "In2.Cu"), // all-copper
        o("ViaObstacle", "F.Cu"),
        o("ViaObstacle", "B.Cu"), // vias
        o("ViaObstacle", "In1.Cu"),
        o("ViaObstacle", "In2.Cu"), // inner
        o("ComponentObstacle", "F.Cu"),
        o("ComponentObstacle", "F.Cu"), // footprints, tracks-footprints
        o("ComponentObstacle", "B.Cu"), // pads-footprints
    ];
    want.sort();
    assert_eq!(board_areas(&board), want);
}

/// K06.2: a trace and a via inside the pads-and-footprints area are not blocked by it, while
/// it blocks another component's placement area, the only thing it can block.
#[test]
fn pads_and_footprints_area_blocks_only_placement() {
    let (read, _) = synthetic("rule-areas.kicad_pcb");
    let mut board = built(&read);
    let area_id = board
        .items()
        .find(|i| {
            i.component_id() == 0
                && i.as_area().is_some_and(|a| {
                    a.kind == AreaKind::ComponentObstacle
                        && board.layer_structure.layers[a.layer() as usize].name == "B.Cu"
                })
        })
        .expect("the pads-and-footprints area")
        .id();
    // inside it: x 140..150 mm, y -20..-30 mm (board units are 0.1 um)
    let at = IntPoint::new(1_450_000, -250_000);
    let b_cu = board.layer_structure.get_no("B.Cu");
    let before = board.item_ids();
    board.insert_trace_points(
        &[
            Point::Int(IntPoint::new(1_420_000, -250_000)),
            Point::Int(IntPoint::new(1_480_000, -250_000)),
        ],
        b_cu,
        1_250,
        &[],
        0,
        FixedState::Unfixed,
    );
    let trace_id = *board
        .item_ids()
        .iter()
        .find(|i| !before.contains(i))
        .expect("trace inserted");
    let padstack = board.library.padstacks.iter().next().map(|p| p.id).unwrap();
    let via_id = board.insert_via(padstack, at, &[], 0, FixedState::Unfixed, true);
    let area = board.item(area_id).unwrap();
    assert!(!board.is_obstacle(board.item(trace_id).unwrap(), area));
    assert!(!board.is_obstacle(board.item(via_id).unwrap(), area));
    let other = board
        .items()
        .find(|i| {
            i.component_id() > 0
                && i.as_area()
                    .is_some_and(|a| a.kind == AreaKind::ComponentObstacle)
        })
        .expect("a footprint's placement area");
    assert!(board.is_obstacle(area, other));
}

/// Two reads give the same tree.
#[test]
fn reads_are_deterministic() {
    for name in ["rule-areas.kicad_pcb", "arcs.kicad_pcb", "vias.kicad_pcb"] {
        let path = common::fixtures().join("synthetic").join(name);
        let text = std::fs::read_to_string(path).unwrap();
        let a = freerouting_kicad::read(&text, None).unwrap();
        let b = freerouting_kicad::read(&text, None).unwrap();
        assert_eq!(a.pcb, b.pcb, "{name}");
        assert_eq!(a.warnings, b.warnings, "{name}");
        assert_eq!(a.report, b.report, "{name}");
    }
}

fn mini(body: &str) -> String {
    format!(
        "(kicad_pcb (version 20240108) (generator \"pcbnew\") (generator_version \"8.0\")\n\
         (layers (0 \"F.Cu\" signal) (31 \"B.Cu\" signal) (44 \"Edge.Cuts\" user))\n\
         (net 0 \"\") (net 1 \"N1\")\n{body})\n"
    )
}

const SQUARE: &str = "(polygon (pts (xy 0 0) (xy 10 0) (xy 10 10) (xy 0 10)))";

/// KiCad's defaults (`ZONE_SETTINGS`): tracks, vias and pads not allowed; a `(keepout)` block
/// resets pads and footprints to allowed (older files omit them).
#[test]
fn rule_area_flag_defaults_follow_kicad() {
    let text = mini(&format!(
        "(zone (net 0) (net_name \"\") (layer \"F.Cu\") (name \"placement only\") (placement (enabled yes)) {SQUARE})\n\
         (zone (net 0) (net_name \"\") (layer \"F.Cu\") (name \"old\") (keepout (copperpour not_allowed)) {SQUARE})\n\
         (zone (net 0) (net_name \"\") (layer \"B.Cu\") (name \"tracks\") (keepout (tracks not_allowed) (vias allowed)) {SQUARE})\n"
    ));
    let r = freerouting_kicad::read(&text, None).unwrap();
    let a = &r.report.rule_areas;
    // a placement area without a keepout block keeps the constructor's flags
    assert_eq!(
        (a[0].flags.tracks, a[0].flags.vias, a[0].flags.pads),
        (true, true, true)
    );
    assert_eq!(a[0].emitted, ["keepout"]);
    // `(keepout)` resets pads and footprints; tracks and vias stay banned
    assert_eq!(
        (
            a[1].flags.pads,
            a[1].flags.footprints,
            a[1].flags.copper_pour
        ),
        (false, false, true)
    );
    assert_eq!(a[1].emitted, ["keepout"]);
    assert_eq!(a[2].emitted, ["wire_keepout"]);
}

/// Warnings the reader gives for copper it cannot use.
#[test]
fn unusable_copper_gives_warnings() {
    let text = mini(&format!(
        "(zone (net 0) (net_name \"\") (layers \"F.Cu\" \"Bogus.Cu\") (name \"z\") (keepout (tracks not_allowed) (vias not_allowed)) {SQUARE})\n\
         (zone (net 0) (net_name \"\") (layer \"F.Cu\") (name \"thin\") (keepout (tracks not_allowed)) (polygon (pts (xy 0 0) (xy 1 1))))\n\
         (segment (start 0 0) (end 1 1) (width 0.2) (layer \"F.SilkS\") (net 1))\n\
         (via (at 5 5) (size 0.8) (drill 0.4) (layers \"F.Cu\" \"Nope.Cu\") (net 1))\n\
         (arc (start 0 0) (end 2 0) (width 0.2) (layer \"F.Cu\") (net 1))\n"
    ));
    let r = freerouting_kicad::read(&text, None).unwrap();
    assert_eq!(
        warnings_with(&r, "layer 'Bogus.Cu' is not a layer of the board").len(),
        1
    );
    assert_eq!(warnings_with(&r, "fewer than three corners").len(), 1);
    assert_eq!(
        warnings_with(&r, "not a copper layer of the board").len(),
        1
    );
    assert_eq!(warnings_with(&r, "layers are not copper layers").len(), 1);
    assert_eq!(warnings_with(&r, "without a mid point").len(), 1);
    // the usable part of the first zone survives on F.Cu
    assert_eq!(r.report.rule_areas.len(), 1);
    assert_eq!(r.report.rule_areas[0].layers, ["F.Cu"]);
}

/// K06.3: after `build_board`, the absolute polygon of every footprint rule area equals the
/// file's coordinates (within one board unit), for footprints at 0, 30 and 135 degrees on both
/// sides.
#[test]
fn footprint_areas_keep_the_files_coordinates() {
    use freerouting_kicad::{footprints::read_footprints, header, nets::NetTable, sexpr::Tree};
    let path = common::fixtures().join("synthetic/rule-areas.kicad_pcb");
    let text = std::fs::read_to_string(path).unwrap();
    let (read, _) = synthetic("rule-areas.kicad_pcb");
    let board = built(&read);
    let tree = Tree::parse(&text).unwrap();
    let hdr = header::read_header(&tree).unwrap();
    let root = header::board(&tree).unwrap();
    let nets = NetTable::build(&tree, hdr.version);
    let fps = read_footprints(root, &nets, &mut Vec::new()).unwrap();
    let count = 4;
    let areas: Vec<_> = board
        .items()
        .filter(|i| i.component_id() > 0)
        .filter_map(|i| i.as_area().map(|a| (i, a)))
        .collect();
    let mut checked = 0;
    for fp in fps.iter().filter(|f| f.reference != "C1") {
        for z in &fp.zones {
            let want: Vec<(f64, f64)> = z
                .outline
                .iter()
                .map(|&(x, y)| (f64::from(x) / 100.0, -f64::from(y) / 100.0))
                .collect();
            let (routing, place) = freerouting_kicad::rule_areas::kinds_of(&z.flags);
            let mut kinds = Vec::new();
            match routing {
                Some(KeepoutKind::ViaKeepout) => kinds.push(AreaKind::ViaObstacle),
                Some(_) => kinds.push(AreaKind::Obstacle),
                None => {}
            }
            if place {
                kinds.push(AreaKind::ComponentObstacle);
            }
            for p in freerouting_kicad::zones::cu_stack_order(z.layers, count) {
                for kind in &kinds {
                    let hit = areas.iter().any(|(_, a)| {
                        a.kind == *kind && a.layer() == p as i32 && {
                            let border = match a.area() {
                                freerouting_geometry::Area::PolylineArea(pa) => {
                                    pa.get_border().clone()
                                }
                                freerouting_geometry::Area::Shape(
                                    freerouting_geometry::Shape::Polyline(ps),
                                ) => ps.clone(),
                                other => panic!("{other:?}"),
                            };
                            let corners: Vec<(f64, f64)> = match &border {
                                freerouting_geometry::PolylineShape::Tile(t) => (0..t
                                    .border_line_count())
                                    .map(|i| t.corner(i).to_float())
                                    .map(|f| (f.x, f.y))
                                    .collect(),
                                freerouting_geometry::PolylineShape::Polygon(g) => g
                                    .corners()
                                    .iter()
                                    .map(|c| c.to_float())
                                    .map(|f| (f.x, f.y))
                                    .collect(),
                            };
                            corners.len() == want.len()
                                && want.iter().all(|w| {
                                    corners.iter().any(|c| {
                                        (c.0 - w.0).abs() <= 1.0 && (c.1 - w.1).abs() <= 1.0
                                    })
                                })
                        }
                    });
                    assert!(
                        hit,
                        "{}: {kind:?} on layer {p} not at the file's coordinates {want:?}",
                        fp.reference
                    );
                    checked += 1;
                }
            }
        }
    }
    assert!(checked >= 12, "{checked}");
}

/// K06.4: the planes of the synthetic board, the netless zones' made-up nets at the head of the
/// network, and the copper zone a footprint owns (kept as a plane by default, dropped like
/// KiCad's export with `drop_footprint_zones`).
#[test]
fn planes_and_netless_zones() {
    let (dropped, export) = synthetic("rule-areas.kicad_pcb");
    let text =
        std::fs::read_to_string(common::fixtures().join("synthetic/rule-areas.kicad_pcb")).unwrap();
    let kept = freerouting_kicad::read(&text, None).unwrap();
    let layer = |p: &freerouting_dsn::ast::Plane| match &p.area.shape {
        Shape::Polygon { layer, .. } => layer.name().to_owned(),
        _ => panic!(),
    };
    let planes = &dropped.pcb.structure.as_ref().unwrap().planes;
    let got: Vec<(String, String)> = planes.iter().map(|p| (p.net.clone(), layer(p))).collect();
    // zones in board (file) order, layers in layer-id order; the netless zones count per plane
    assert_eq!(got.len(), 6);
    assert!(got.contains(&("N1".to_owned(), "F.Cu".to_owned())));
    assert!(got.contains(&("N2".to_owned(), "F.Cu".to_owned())));
    assert!(got.contains(&("N2".to_owned(), "B.Cu".to_owned())));
    let nonet: Vec<&(String, String)> = got
        .iter()
        .filter(|g| g.0.starts_with("@:no_net_"))
        .collect();
    assert_eq!(nonet.len(), 3);
    let names: Vec<&str> = dropped
        .pcb
        .network
        .as_ref()
        .unwrap()
        .nets
        .iter()
        .map(|n| n.name.as_str())
        .collect();
    assert_eq!(
        &names[..3],
        ["@:no_net_0", "@:no_net_1", "@:no_net_2"],
        "{names:?}"
    );
    assert!(names[3..].iter().all(|n| !n.starts_with("@:")));
    // KiCad's export agrees (planes, order, nets, layers)
    assert!(copper::planes(&dropped.pcb, &export).is_empty());
    let en: Vec<&str> = export
        .network
        .as_ref()
        .unwrap()
        .nets
        .iter()
        .map(|n| n.name.as_str())
        .collect();
    assert_eq!(&en[..3], &names[..3]);
    // holes become windows
    assert!(
        planes
            .iter()
            .any(|p| p.net == "N2" && p.area.windows.len() == 1)
    );
    // the footprint's zone: dropped with a warning, or a plane after the board's own
    assert_eq!(
        dropped.report.footprint_zones_dropped,
        [("C1".to_owned(), Some("fp-copper".to_owned()))]
    );
    assert_eq!(
        warnings_with(&dropped, "copper zone 'fp-copper' of footprint C1 dropped").len(),
        1
    );
    let planes = &kept.pcb.structure.as_ref().unwrap().planes;
    assert_eq!(planes.len(), 7);
    assert_eq!(
        (planes[6].net.as_str(), layer(&planes[6])),
        ("N2", "F.Cu".to_owned())
    );
    assert_eq!(
        kept.report.footprint_zones_kept,
        [("C1".to_owned(), Some("fp-copper".to_owned()))]
    );
    assert_eq!(
        warnings_with(
            &kept,
            "copper zone 'fp-copper' of footprint C1 kept as a plane"
        )
        .len(),
        1
    );
    assert_eq!(
        &planes[..6],
        &dropped.pcb.structure.as_ref().unwrap().planes[..]
    );
}

fn length(points: &[freerouting_dsn::ast::Point]) -> f64 {
    points
        .windows(2)
        .map(|w| (w[1].x - w[0].x).hypot(w[1].y - w[0].y))
        .sum()
}

fn wires(pcb: &Pcb) -> Vec<(&WireType, &Vec<freerouting_dsn::ast::Point>)> {
    pcb.wiring
        .as_ref()
        .unwrap()
        .items
        .iter()
        .filter_map(|i| match i {
            WiringItem::Wire(w) => match (&w.shapes[0], w.wire_type.as_ref()) {
                (Shape::Path { points, .. }, Some(t)) => Some((t, points)),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// K06.5: on `Issue180-Test` (86 wires of 102 segments), every wire equals the export: nets,
/// layers, widths, types (15 locked wires are `fix`, the others `route`) and points.
#[test]
fn issue180_wires_equal_the_export() {
    let (read, export) = load(&common::fixtures().join("Issue180-Test/Test.kicad_pcb"));
    assert!(copper::wires(&read.pcb, &export).is_empty());
    let w = wires(&read.pcb);
    assert_eq!(w.len(), 86);
    assert_eq!(w.iter().filter(|(t, _)| **t == WireType::Fix).count(), 15);
    assert_eq!(
        w.iter()
            .filter(|(t, _)| **t == WireType::Other("route".to_owned()))
            .count(),
        71
    );
    assert_eq!(read.report.track_arcs_flattened, 0);
}

/// The analytic length of the arc through three points, by the chord and the angle
/// (an independent formula from the reader's).
fn analytic_arc_length(s: (f64, f64), m: (f64, f64), e: (f64, f64)) -> f64 {
    let d = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).hypot(a.1 - b.1);
    let (a, b, c) = (d(m, e), d(s, e), d(s, m));
    // circumradius R = abc / (4 K), K from Heron
    let p = (a + b + c) / 2.0;
    let k = (p * (p - a) * (p - b) * (p - c)).sqrt();
    let r = a * b * c / (4.0 * k);
    // the arc through the middle point: angle at the centre = 2 * asin(chord / 2R), or the
    // reflex angle when the middle point is on the far side
    let half = (b / (2.0 * r)).asin();
    let mid_to_chord_far = {
        // distance of m from the chord's midpoint line: reflex when m is farther from the
        // chord's midpoint than R
        let mid = ((s.0 + e.0) / 2.0, (s.1 + e.1) / 2.0);
        d(m, mid) > r
    };
    if mid_to_chord_far {
        r * (std::f64::consts::TAU - 2.0 * half)
    } else {
        r * 2.0 * half
    }
}

/// K06.5: arcs of 10, 90 and 270 degrees (and -90) at radii 0.1 to 20 mm: each arc wire
/// keeps its end points exactly and its length is within 0.1 % of the analytic length.
#[test]
fn arcs_follow_the_curve() {
    let (read, export) = synthetic("arcs.kicad_pcb");
    assert_eq!(read.report.track_arcs_flattened, 16);
    assert!(
        copper::wires(&read.pcb, &export).is_empty(),
        "the chord's points are on our path"
    );
    let text =
        std::fs::read_to_string(common::fixtures().join("synthetic/arcs.kicad_pcb")).unwrap();
    // (start) (mid) (end) of every arc, in mm
    let mut arcs = Vec::new();
    let mut rest = text.as_str();
    while let Some(i) = rest.find("(arc") {
        rest = &rest[i..];
        let end = rest.find("(layer").unwrap();
        let nums: Vec<f64> = rest[..end]
            .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
            .filter(|t| !t.is_empty() && t.chars().any(|c| c.is_ascii_digit()))
            .filter_map(|t| t.parse().ok())
            .collect();
        // [start x y, mid x y, end x y, width]
        arcs.push(((nums[0], nums[1]), (nums[2], nums[3]), (nums[4], nums[5])));
        rest = &rest[end..];
    }
    assert_eq!(arcs.len(), 16);
    let w = wires(&read.pcb);
    for (s, m, e) in arcs {
        let to_dsn = |p: (f64, f64)| (p.0 * 1000.0, -p.1 * 1000.0);
        let (sd, ed) = (to_dsn(s), to_dsn(e));
        let wire = w
            .iter()
            .find(|(_, pts)| {
                (pts[0].x - sd.0).abs() < 1e-6
                    && (pts[0].y - sd.1).abs() < 1e-6
                    && pts.len() >= 2
                    && (pts[pts.len() - 1].x - ed.0).abs() < 1e-6
                    && (pts[pts.len() - 1].y - ed.1).abs() < 1e-6
            })
            .unwrap_or_else(|| panic!("no wire from {sd:?} to {ed:?}"));
        let want = analytic_arc_length(to_dsn(s), to_dsn(m), to_dsn(e));
        let got = length(wire.1);
        assert!(
            (got - want).abs() <= want * 0.001,
            "arc {s:?} {m:?} {e:?}: length {got} vs analytic {want}"
        );
    }
}

/// K06.5: a segment, an arc and a segment in file order are one wire with connected ends; the
/// merge starts a new wire when the net, the width or the layer changes or the path breaks,
/// and skips net 0.
#[test]
fn consecutive_tracks_merge_into_wires() {
    let text = mini(
        "(segment (start 0 0) (end 5 0) (width 0.2) (layer \"F.Cu\") (net 1))\n\
         (arc (start 5 0) (mid 6.41421356 0.58578644) (end 7 2) (width 0.2) (layer \"F.Cu\") (net 1))\n\
         (segment (start 7 2) (end 7 9) (width 0.2) (layer \"F.Cu\") (net 1) locked)\n\
         (segment (start 7 9) (end 7 12) (width 0.3) (layer \"F.Cu\") (net 1))\n\
         (segment (start 7 12) (end 8 12) (width 0.3) (layer \"B.Cu\") (net 1))\n\
         (segment (start 8 12) (end 9 12) (width 0.3) (layer \"B.Cu\") (net 0))\n\
         (segment (start 8 12) (end 9 12) (width 0.3) (layer \"B.Cu\") (net 1))\n\
         (segment (start 20 20) (end 21 20) (width 0.3) (layer \"B.Cu\") (net 1))\n",
    );
    let r = freerouting_kicad::read(&text, None).unwrap();
    let w = wires(&r.pcb);
    // 1: seg+arc (route; the locked flag is the third item's), 2: locked seg (starts a
    // new wire? no: same net, width, layer and connected, so it joins), 3: width change,
    // 4: layer change, 5: after the net-0 segment the path still ends at 8,12 -> joins; 6: break
    assert_eq!(w.len(), 4, "{w:#?}");
    let first = w[0].1;
    assert!(first.len() > 4, "seg + flattened arc + seg: {first:?}");
    assert_eq!((first[0].x, first[0].y), (0.0, 0.0));
    assert_eq!(
        (first[first.len() - 1].x, first[first.len() - 1].y),
        (7000.0, -9000.0)
    );
    assert_eq!(
        *w[0].0,
        WireType::Other("route".to_owned()),
        "the type is the first item's"
    );
}

/// K06.6: the synthetic 6-layer board gives the export's padstacks and spans.
#[test]
fn via_padstacks_and_spans() {
    let (read, export) = synthetic("vias.kicad_pcb");
    assert!(copper::wire_vias(&read.pcb, &export).is_empty());
    assert!(copper::via_padstacks(&read.pcb, &export).is_empty());
    assert!(copper::via_lists(&read.pcb, &export).is_empty());
    let vias: Vec<_> = read
        .pcb
        .wiring
        .as_ref()
        .unwrap()
        .items
        .iter()
        .filter_map(|i| match i {
            WiringItem::Via(v) => Some(v),
            _ => None,
        })
        .collect();
    assert_eq!(vias.len(), 7);
    let names: std::collections::BTreeSet<&str> =
        vias.iter().map(|v| v.padstack.as_str()).collect();
    for n in [
        "Via[0-5]_800:400_um",
        "Via[0-2]_700:300_um",
        "Via[1-3]_600:300_um",
        "Via[0-1]_300:100_um",
        "Via[3-5]_700:300_um",
    ] {
        assert!(names.contains(n), "{n} in {names:?}");
    }
    let lib = read.pcb.library.as_ref().unwrap();
    let span = |n: &str| {
        lib.padstacks
            .iter()
            .find(|p| p.name == n)
            .unwrap()
            .shapes
            .len()
    };
    assert_eq!(span("Via[0-5]_800:400_um"), 6);
    assert_eq!(span("Via[0-2]_700:300_um"), 3);
    assert_eq!(span("Via[1-3]_600:300_um"), 3);
    assert_eq!(span("Via[0-1]_300:100_um"), 2);
    assert_eq!(span("Via[3-5]_700:300_um"), 3);
    // the list: the class via first, then new ones in file order
    let list = &read
        .pcb
        .structure
        .as_ref()
        .unwrap()
        .via
        .as_ref()
        .unwrap()
        .padstacks;
    assert_eq!(list[0], "Via[0-5]_600:300_um");
    assert_eq!(list.len(), 6);
    // the locked via is fixed, the rest are routed
    let fixed = vias
        .iter()
        .filter(|v| v.via_type == Some(WireType::Fix))
        .count();
    assert_eq!(fixed, 1);
}

/// K06.6: `Issue180-Test` has 627 vias, 624 of them locked (`(type fix)`), and the export's
/// padstacks.
#[test]
fn issue180_vias_are_fixed() {
    let (read, export) = load(&common::fixtures().join("Issue180-Test/Test.kicad_pcb"));
    assert!(copper::wire_vias(&read.pcb, &export).is_empty());
    assert_eq!(read.report.vias, 627);
    let fixed = read
        .pcb
        .wiring
        .as_ref()
        .unwrap()
        .items
        .iter()
        .filter(|i| matches!(i, WiringItem::Via(v) if v.via_type == Some(WireType::Fix)))
        .count();
    assert_eq!(fixed, 624);
}

/// K06.7: every custom pad counts as a convex hull; the report prints.
#[test]
fn report_counts_hulls_and_prints() {
    let (read, _) = synthetic("pads.kicad_pcb");
    assert_eq!(read.report.custom_pad_hulls, 3);
    let (read, _) = synthetic("rule-areas.kicad_pcb");
    let text = read.report.to_string();
    assert!(text.contains("rule areas: 19"), "{text}");
    assert!(text.contains("pads-footprints (line "));
    assert!(text.contains("emitted: place_keepout"));
    assert!(
        text.contains("footprint copper zones: 0 kept as planes, 1 dropped"),
        "{text}"
    );
}

/// The sum of the track lengths pcbnew computes (`tools/kicad-reader/track_length.py`, KiCad
/// 10.0.7, 2,084 tracks with a net, 200 arcs) for `CM5_MINIMA_3`, in nanometres.
const CM5_KICAD_LENGTH_NM: f64 = 2_785_801_604.0;

/// K06.5: `CM5_MINIMA_3` (out of the repository, 200 track arcs): the total trace length after
/// `build_board` is within 0.1 % of KiCad's own.
#[test]
fn cm5_trace_length_matches_kicad() {
    let boards = common::corpus_boards("demos/cm5_minima");
    let Some(board) = boards.first() else {
        eprintln!("CM5_MINIMA_3 is not in the corpus; skipped");
        return;
    };
    let (read, _) = load(board);
    assert_eq!(read.report.track_arcs_flattened, 200);
    let b = built(&read);
    // board units are 0.1 um
    let mm = b.cumulative_trace_length() / 10_000.0;
    let want = CM5_KICAD_LENGTH_NM / 1e6;
    println!("CM5_MINIMA_3: build_board {mm:.3} mm, KiCad {want:.3} mm");
    assert!(
        (mm - want).abs() <= want * 0.001,
        "{mm} mm vs KiCad {want} mm"
    );
    // and the tree itself
    let tree_mm: f64 = wires(&read.pcb).iter().map(|(_, p)| length(p)).sum::<f64>() / 1000.0;
    assert!(
        (tree_mm - want).abs() <= want * 0.001,
        "{tree_mm} mm vs KiCad {want} mm"
    );
}

/// K06.2/K06.7: on `canary-pwr` (out of the repository) the fan inlet keep-out becomes one
/// `ComponentObstacle` on B.Cu that blocks neither traces nor vias, the report lists it with its
/// flags and `place_keepout`, and KiCad's own export (read through the DSN path) still shows the
/// full keepout.
#[test]
fn canary_pwr_fan_inlet() {
    let boards = common::corpus_boards("canary-pwr");
    let Some(board) = boards.first() else {
        eprintln!("canary-pwr is not in the corpus; skipped");
        return;
    };
    let (read, export) = load(board);
    for (cat, d) in [
        (
            "rule areas",
            copper::structure_rule_areas(&read.pcb, &export, &read.report),
        ),
        ("planes", copper::planes(&read.pcb, &export)),
        ("wires", copper::wires(&read.pcb, &export)),
        ("vias", copper::wire_vias(&read.pcb, &export)),
        ("via padstacks", copper::via_padstacks(&read.pcb, &export)),
        ("via list", copper::via_lists(&read.pcb, &export)),
    ] {
        assert!(d.is_empty(), "canary-pwr {cat}: {d:?}");
    }
    let a = &read.report.rule_areas;
    assert_eq!(a.len(), 1);
    assert!(
        a[0].name
            .as_deref()
            .unwrap()
            .starts_with("Fan inlet keep-out")
    );
    assert_eq!(a[0].layers, ["B.Cu"]);
    assert_eq!(a[0].emitted, ["place_keepout"]);
    assert!(a[0].flags.pads && a[0].flags.footprints && !a[0].flags.tracks && !a[0].flags.vias);
    let text = read.report.to_string();
    assert!(
        text.contains("Fan inlet keep-out") && text.contains("emitted: place_keepout"),
        "{text}"
    );
    assert!(
        text.contains("pads not_allowed") && text.contains("footprints not_allowed"),
        "{text}"
    );
    assert_eq!(read.report.vias, 41);
    // the built board
    let mut board = built(&read);
    let areas = board_areas(&board);
    assert_eq!(
        areas,
        vec![("ComponentObstacle".to_owned(), "B.Cu".to_owned())]
    );
    // KiCad's export, read through the DSN path, keeps the full keepout at (136.5, -53) mm
    let k = &export.structure.as_ref().unwrap().keepouts;
    assert_eq!(k.len(), 1);
    assert_eq!(k[0].kind, KeepoutKind::Keepout);
    let Shape::Polygon { layer, points, .. } = &k[0].area.shape else {
        panic!()
    };
    assert_eq!(layer.name(), "B.Cu");
    assert!(
        points.iter().any(|p| p.x == 136_500.0 && p.y == -53_000.0),
        "{points:?}"
    );
    // probes inside the area: neither a trace nor a via is blocked
    let area_id = board
        .items()
        .find(|i| {
            i.as_area()
                .is_some_and(|a| a.kind == AreaKind::ComponentObstacle)
        })
        .unwrap()
        .id();
    let before = board.item_ids();
    let b_cu = board.layer_structure.get_no("B.Cu");
    board.insert_trace_points(
        &[
            Point::Int(IntPoint::new(1_370_000, -530_000)),
            Point::Int(IntPoint::new(1_390_000, -530_000)),
        ],
        b_cu,
        1_250,
        &[],
        0,
        FixedState::Unfixed,
    );
    let trace = *board
        .item_ids()
        .iter()
        .find(|i| !before.contains(i))
        .unwrap();
    let padstack = board
        .library
        .padstacks
        .iter()
        .find(|p| p.name.starts_with("Via["))
        .map(|p| p.id)
        .unwrap();
    let via = board.insert_via(
        padstack,
        IntPoint::new(1_365_000, -530_000),
        &[],
        0,
        FixedState::Unfixed,
        true,
    );
    let area = board.item(area_id).unwrap();
    assert!(!board.is_obstacle(board.item(trace).unwrap(), area));
    assert!(!board.is_obstacle(board.item(via).unwrap(), area));
}

/// An `(arc ...)` in a zone's `pts` becomes a polyline with the ends exact (KiCad appends the
/// arc's polyline, at 1 um), and counts in the report.
#[test]
fn zone_outline_arcs_are_polylines() {
    let text = mini(
        "(zone (net 0) (net_name \"\") (layer \"F.Cu\") (name \"round\") (keepout (tracks not_allowed) (vias not_allowed))\n\
         (polygon (pts (xy 0 0) (xy 10 0) (arc (start 10 0) (mid 15 5) (end 10 10)) (xy 0 10))))\n",
    );
    let r = freerouting_kicad::read(&text, None).unwrap();
    assert_eq!(r.report.zone_arcs_flattened, 1);
    let k = &r.pcb.structure.as_ref().unwrap().keepouts[0];
    let Shape::Polygon { points, .. } = &k.area.shape else {
        panic!()
    };
    // 4 corners, the arc's interior points, and the repeated first point; (10,0) is not doubled
    assert!(points.len() > 8, "{points:?}");
    assert_eq!((points[1].x, points[1].y), (10_000.0, 0.0));
    assert_eq!(
        points
            .iter()
            .filter(|p| p.x == 10_000.0 && p.y == 0.0)
            .count(),
        1
    );
    assert!(points.iter().any(|p| p.x == 10_000.0 && p.y == -10_000.0));
    // every interior point is on the circle through (10,0), (15,5), (10,10): centre (10,5), r 5 mm
    for p in points.iter().filter(|p| p.x > 10_000.0) {
        let d = (p.x - 10_000.0).hypot(-p.y - 5_000.0);
        assert!((d - 5_000.0).abs() <= 1.0, "{p:?} is {d} from the centre");
    }
}
