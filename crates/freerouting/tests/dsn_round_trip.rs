//! C06.7: the full round trip, the comparison with Java's DSN dumps and the port of
//! `DsnWriterTest`.

mod dsn_support;

use std::path::Path;

use dsn_support::*;
use freerouting::board::BasicBoard;
use freerouting::drc::DesignRulesChecker;
use freerouting::dsn::Pcb;
use freerouting::io::specctra::dsn_writer::pcb_from_board;
use freerouting::scoring::BoardStatistics;

fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<std::sync::Mutex<Option<R>>> =
        items.iter().map(|_| std::sync::Mutex::new(None)).collect();
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(item) = items.get(i) else { break };
                    *results[i].lock().unwrap() = Some(f(item));
                }
            });
        }
    });
    results
        .into_iter()
        .map(|r| r.into_inner().unwrap().unwrap())
        .collect()
}

/// A text with the item ids (`#656`) taken out: they are renumbered.
fn without_ids(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '#' {
            while chars.peek().is_some_and(char::is_ascii_digit) {
                chars.next();
            }
        }
    }
    out
}

/// The full DRC result without item ids (they are renumbered): each finding with its type and
/// its items' descriptions and positions, sorted.
fn drc_result(board: &BasicBoard, name: &str) -> Vec<String> {
    let mut checker = DesignRulesChecker::new(board);
    let report = checker.generate_report(name, "mm");
    let mut lines: Vec<String> = report
        .violations
        .iter()
        .chain(&report.unconnected_items)
        .map(|v| {
            if v.violation_type == "unconnectedItems" {
                // Which two items name an unconnected pair, and the items listed with it,
                // depend on the item ids (the lowest of each group), which are renumbered:
                // the pair is compared by its net.
                let net = v.description.split(['[', ']']).nth(1).unwrap_or("?");
                return format!("unconnectedItems {} net {net}", v.severity);
            }
            // The items of a finding are listed in id order, and the ids are renumbered.
            let mut items: Vec<String> = v
                .items
                .iter()
                .map(|i| format!("{} {:?}", i.description, i.pos))
                .collect();
            items.sort();
            format!(
                "{} {} {} {:?}",
                v.violation_type,
                v.severity,
                without_ids(&v.description),
                items
            )
        })
        .collect();
    lines.sort();
    lines
}

/// `build_board(parse(write_dsn(board)))` has the same statistics (every field the reference
/// comparison looks at, and more) and the same full DRC result as the board.
#[test]
fn round_trip_gives_the_same_statistics_and_drc_result_on_every_fixture() {
    let boards = loaded_boards();
    let results = par_map(&boards, |(name, board)| {
        let again = round_trip(board, name);
        let mut found = Vec::new();
        let (mut a, b) = (BoardStatistics::new(board), BoardStatistics::new(&again));
        // A placed component that owns no item is not written (Java's `writePlacementScope`).
        let owners: std::collections::BTreeSet<i32> =
            board.items().map(|i| i.component_id()).collect();
        a.components.total_count = board
            .components
            .iter()
            .filter(|c| !c.is_placed() || owners.contains(&c.id.0))
            .count();
        if a != b {
            found.extend(diff("statistics", &format!("{a:#?}"), &format!("{b:#?}")));
        }
        found.extend(diff_lines(
            "drc",
            &drc_result(board, name),
            &drc_result(&again, name),
        ));
        (name.clone(), found)
    });
    let mut failures = Vec::new();
    for (name, found) in results {
        if !found.is_empty() {
            failures.extend(found.into_iter().map(|f| format!("{name}: {f}")));
        }
    }
    assert!(boards.len() > 100);
    assert!(
        failures.is_empty(),
        "{} differences:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------
// The comparison with Java's dumps (fixtures/reference/dsn)

fn dump_boards() -> Vec<(String, std::path::PathBuf, std::path::PathBuf)> {
    let dir = fixtures_dir().join("reference/dsn");
    let mut out = Vec::new();
    let mut stack = vec![dir.clone()];
    while let Some(d) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == "dsn") {
                let rel = p
                    .strip_prefix(&dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                // The synthetic cases are inputs of tools/java-reference/cases.
                let input = match rel.strip_prefix("cases/") {
                    Some(case) => fixtures_dir()
                        .join("../tools/java-reference/cases")
                        .join(case),
                    None => fixtures_dir().join(&rel),
                };
                out.push((rel, input, p));
            }
        }
    }
    out.sort();
    out
}

/// The keywords Java's writer spells differently from its own reader (docs/DIVERGENCES.md,
/// freerouting-dsn): the text is rewritten to the spelling the parser reads.
const JAVA_KEYWORDS: &[(&str, &str)] = &[
    ("(snapAngle", "(snap_angle"),
    ("(viaRule", "(via_rule"),
    ("(logicalPart ", "(logical_part "),
    ("(type shoveFixed)", "(type shove_fixed)"),
];

fn java_tree(path: &Path) -> Pcb {
    let mut text = std::fs::read_to_string(path).unwrap();
    for (java, rust) in JAVA_KEYWORDS {
        text = text.replace(java, rust);
    }
    // Java writes an image without a name for a package made without one (Issue145).
    text = text.replace("(image \n", "(image \"<unnamed>\"\n");
    Pcb::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

use freerouting::dsn::ast::{
    Boundary, KeepoutKind, Rule, Shape as DsnShape, Wire, WireType, WiringItem,
};

fn first_difference<T: std::fmt::Debug + PartialEq>(
    what: &str,
    java: &[T],
    rust: &[T],
) -> Vec<String> {
    if java == rust {
        return Vec::new();
    }
    let at = java.iter().zip(rust).take_while(|(a, b)| a == b).count();
    vec![format!(
        "{what}: java {} items, rust {}; first difference at item {at}\n   java: {:?}\n   rust: {:?}",
        java.len(),
        rust.len(),
        java.get(at)
            .map(|x| format!("{x:?}").chars().take(400).collect::<String>()),
        rust.get(at)
            .map(|x| format!("{x:?}").chars().take(400).collect::<String>()),
    )]
}

fn reversed<T: Clone>(items: &[T]) -> Vec<T> {
    items.iter().rev().cloned().collect()
}

/// The differences between Java's tree and Rust's that the checklist cannot explain. Every
/// scope is compared; the differences that are explained (docs/DIVERGENCES.md, "Writing a
/// design file") are removed from both sides first, as noted at each scope.
fn unexplained(java: &Pcb, rust: &Pcb, scale: f64) -> Vec<String> {
    let mut out = Vec::new();
    out.extend(first_difference(
        "parser",
        std::slice::from_ref(&java.parser),
        std::slice::from_ref(&rust.parser),
    ));
    out.extend(first_difference(
        "resolution and unit",
        &[(java.resolution, java.unit)],
        &[(rust.resolution, rust.unit)],
    ));
    let (js, rs) = (
        java.structure.clone().unwrap(),
        rust.structure.clone().unwrap(),
    );

    // Layers: Java writes no `use_net`, and repeats layer 0's width in a layer rule.
    let strip = |layers: &[freerouting::dsn::ast::Layer]| -> Vec<_> {
        layers
            .iter()
            .map(|l| (l.name.clone(), l.layer_type.clone(), l.rules.clone()))
            .collect()
    };
    if js.layers.iter().any(|l| !l.use_net.is_empty()) {
        out.push("layers: Java wrote a use_net".into());
    }
    out.extend(first_difference(
        "layers",
        &strip(&js.layers),
        &strip(&rs.layers),
    ));

    // Boundaries: Rust's `pcb` rectangle is Java's shrunk by the 1000 the reader adds; Rust adds
    // the outline's clearance class when it is not the reader's default.
    let unbound = |b: &Boundary, shrink: f64| -> Boundary {
        let mut b = b.clone();
        b.clearance_class = None;
        if let Some(DsnShape::Rect {
            layer,
            x1,
            y1,
            x2,
            y2,
        }) = b.shapes.first().cloned()
            && layer.name() == "pcb"
        {
            let d = shrink / scale;
            b.shapes[0] = DsnShape::Rect {
                layer,
                x1: x1 + d,
                y1: y1 + d,
                x2: x2 - d,
                y2: y2 - d,
            };
        }
        b
    };
    out.extend(first_difference(
        "boundaries",
        &js.boundaries
            .iter()
            .map(|b| unbound(b, 1000.0))
            .collect::<Vec<_>>(),
        &rs.boundaries
            .iter()
            .map(|b| unbound(b, 0.0))
            .collect::<Vec<_>>(),
    ));

    // The via padstack list: the reader appends the `use_via` names of the classes, so Java's
    // list is Rust's followed by them.
    let network = rust.network.clone().unwrap();
    let mut via_list = rs.via.clone().unwrap_or_default().padstacks;
    via_list.extend(
        network
            .classes
            .iter()
            .flat_map(|c| c.circuit.iter().flat_map(|c| c.use_via.clone())),
    );
    out.extend(first_difference(
        "via padstack list",
        &js.via.clone().unwrap_or_default().padstacks,
        &via_list,
    ));

    // Rules: the default rule is equal; the named clearance rules are single types in Java
    // (its reader skips them) and pairs in Rust.
    out.extend(first_difference(
        "default rule",
        &js.rules[..3.min(js.rules.len())],
        &rs.rules[..3.min(rs.rules.len())],
    ));
    let single = |r: &&Rule| matches!(r, Rule::Clearance { types, .. } if types.len() == 1);
    let pair = |r: &&Rule| matches!(r, Rule::Clearance { types, .. } if types.len() == 2);
    if !js.rules.iter().skip(3).all(|r| single(&r)) || !rs.rules.iter().skip(3).all(|r| pair(&r)) {
        out.push(
            "rules: named clearance rules are not single types in Java and pairs in Rust".into(),
        );
    }

    // Keepouts: written in the opposite order; Rust writes `place_keepout` where Java writes
    // `keepout`, and names a clearance class where the reader's default would be another one.
    let norm_keepouts = |k: &[freerouting::dsn::ast::Keepout]| -> Vec<_> {
        k.iter()
            .map(|k| {
                let mut k = k.clone();
                k.area.clearance_class = None;
                if k.kind == KeepoutKind::PlaceKeepout {
                    k.kind = KeepoutKind::Keepout;
                }
                k
            })
            .collect()
    };
    out.extend(first_difference(
        "keepouts",
        &reversed(&norm_keepouts(&js.keepouts)),
        &norm_keepouts(&rs.keepouts),
    ));

    // Planes: Rust also writes the fixed areas of plane nets on signal layers, which Java
    // writes in the wiring scope.
    let java_wiring = java.wiring.clone().unwrap().items;
    let rust_wiring = rust.wiring.clone().unwrap().items;
    let norm_plane = |p: &freerouting::dsn::ast::Plane| {
        let mut p = p.clone();
        p.area.clearance_class = None;
        p
    };
    let java_planes: Vec<_> = reversed(&js.planes).iter().map(norm_plane).collect();
    let rust_planes: Vec<_> = rs.planes.iter().map(norm_plane).collect();
    let extra: Vec<_> = rust_planes
        .iter()
        .filter(|p| !java_planes.contains(p))
        .collect();
    if java_planes.iter().any(|p| !rust_planes.contains(p)) {
        out.push("planes: a Java plane is missing in Rust".into());
    }
    let java_areas: Vec<&Wire> = java_wiring
        .iter()
        .filter_map(|i| match i {
            WiringItem::Wire(w)
                if !matches!(
                    w.shapes.first(),
                    Some(DsnShape::PolylinePath { .. } | DsnShape::Path { .. })
                ) =>
            {
                Some(w)
            }
            _ => None,
        })
        .collect();
    for p in &extra {
        if !java_areas.iter().any(|w| {
            w.shapes.first() == Some(&p.area.shape)
                && w.net.as_ref().map(|n| &n.name) == Some(&p.net)
        }) {
            out.push(format!(
                "planes: an extra Rust plane of net {} is not a Java wire",
                p.net
            ));
        }
    }

    // Placement, images, padstacks and the part library are equal, except the place keepouts
    // of an image (Java leaves them out) and the names of keepouts.
    let jp = java.placement.clone().unwrap();
    let rp = rust.placement.clone().unwrap();
    out.extend(first_difference(
        "placement",
        &[(jp.flip_style_rotate_first, jp.components.clone())],
        &[(rp.flip_style_rotate_first, rp.components.clone())],
    ));
    let norm_images = |l: &freerouting::dsn::ast::Library| -> Vec<_> {
        l.images
            .iter()
            .map(|i| {
                let mut i = i.clone();
                i.keepouts.retain(|k| k.kind != KeepoutKind::PlaceKeepout);
                for k in &mut i.keepouts {
                    k.area.name = None;
                }
                i
            })
            .collect()
    };
    let (mut jl, rl) = (java.library.clone().unwrap(), rust.library.clone().unwrap());
    for image in &mut jl.images {
        if image.name == "<unnamed>" {
            image.name.clear();
        }
    }
    out.extend(first_difference(
        "images",
        &norm_images(&jl),
        &norm_images(&rl),
    ));
    out.extend(first_difference("padstacks", &jl.padstacks, &rl.padstacks));
    out.extend(first_difference(
        "part library",
        std::slice::from_ref(&java.part_library),
        std::slice::from_ref(&rust.part_library),
    ));

    // Nets: pins in the other order.
    let jn = java.network.clone().unwrap();
    let norm_nets = |n: &freerouting::dsn::ast::Network| -> Vec<_> {
        n.nets
            .iter()
            .map(|n| {
                let mut n = n.clone();
                n.pins.sort();
                n
            })
            .collect()
    };
    out.extend(first_difference(
        "nets",
        &norm_nets(&jn),
        &norm_nets(&network),
    ));

    // Via infos: Java also writes the ones the reader makes for a class that creates its own
    // clearance class (`<padstack>-<class>`); the others are equal.
    let created: Vec<String> = network
        .classes
        .iter()
        .filter(|c| {
            c.rules
                .iter()
                .any(|r| matches!(r, Rule::Clearance { types, .. } if types.is_empty()))
        })
        .map(|c| c.name.clone())
        .collect();
    let java_infos: Vec<_> = jn
        .vias
        .iter()
        .filter(|v| {
            !(network.vias.iter().all(|r| r.name != v.name)
                && created.iter().any(|c| v.name.ends_with(&format!("-{c}"))))
        })
        .cloned()
        .collect();
    out.extend(first_difference("via infos", &java_infos, &network.vias));

    // Via rules: with a `use_via` per class Rust writes none, and the reader makes `default`
    // and one per class.
    if network.via_rules.is_empty() && !jn.via_rules.is_empty() {
        let mut names = vec!["default".to_string()];
        names.extend(
            network
                .classes
                .iter()
                .filter(|c| c.circuit.as_ref().is_some_and(|c| !c.use_via.is_empty()))
                .map(|c| c.name.clone()),
        );
        let java_names: Vec<String> = jn.via_rules.iter().map(|r| r.name.clone()).collect();
        out.extend(first_difference("via rule names", &java_names, &names));
    } else {
        out.extend(first_difference(
            "via rules",
            &jn.via_rules,
            &network.via_rules,
        ));
    }

    // Net classes: a class that creates its clearance class has the clearance rule and no
    // `clearance_class` or via rule; with a `use_via` the class has the names; `use_layer` is
    // left out when it is the signal layers.
    let signal_layers: Vec<String> = rs
        .layers
        .iter()
        .zip(&js.layers)
        .filter(|(l, _)| l.layer_type.as_ref().is_none_or(|t| t.is_signal()))
        .map(|(l, _)| l.name.clone())
        .collect();
    let norm_class = |c: &freerouting::dsn::ast::NetClass, rust: bool| {
        let mut c = c.clone();
        c.rules
            .retain(|r| !matches!(r, Rule::Clearance { types, .. } if types.is_empty()));
        c.clearance_class = None;
        c.via_rule = None;
        if let Some(circuit) = &mut c.circuit {
            circuit.use_via.clear();
            if (rust && circuit.use_layer.is_empty()) || circuit.use_layer == signal_layers {
                circuit.use_layer = signal_layers.clone();
            }
        }
        c
    };
    out.extend(first_difference(
        "net classes",
        &jn.classes
            .iter()
            .map(|c| norm_class(c, false))
            .collect::<Vec<_>>(),
        &network
            .classes
            .iter()
            .map(|c| norm_class(c, true))
            .collect::<Vec<_>>(),
    ));
    out.extend(first_difference(
        "class classes",
        &jn.class_classes,
        &network.class_classes,
    ));

    // Wiring: wires and vias in the other order; the shove-fixed spelling is rewritten; a
    // conduction area carries its fixed state in Rust (Java writes none); the areas that are
    // planes in Rust are not wires.
    let split = |items: &[WiringItem]| {
        let mut wires = Vec::new();
        let mut vias = Vec::new();
        let mut areas = Vec::new();
        for item in items {
            match item {
                WiringItem::Wire(w)
                    if matches!(
                        w.shapes.first(),
                        Some(DsnShape::PolylinePath { .. } | DsnShape::Path { .. })
                    ) =>
                {
                    wires.push(w.clone())
                }
                WiringItem::Wire(w) => {
                    let mut w = w.clone();
                    w.wire_type = None::<WireType>;
                    areas.push(w);
                }
                WiringItem::Via(v) => vias.push(v.clone()),
            }
        }
        (wires, vias, areas)
    };
    let (jw, jv, ja) = split(&java_wiring);
    let (rw, rv, ra) = split(&rust_wiring);
    out.extend(first_difference("wires", &reversed(&jw), &rw));
    out.extend(first_difference("vias", &reversed(&jv), &rv));
    let ja_left: Vec<_> = ja
        .iter()
        .filter(|w| {
            !extra.iter().any(|p| {
                w.shapes.first() == Some(&p.area.shape)
                    && w.net.as_ref().map(|n| &n.name) == Some(&p.net)
            })
        })
        .cloned()
        .collect();
    out.extend(first_difference(
        "conduction areas",
        &reversed(&ja_left),
        &ra,
    ));
    out
}

#[test]
fn rust_tree_equals_the_tree_of_javas_dump_scope_by_scope() {
    let mut failures = Vec::new();
    let mut count = 0;
    for (rel, input, dump) in dump_boards() {
        let freerouting::io::BoardReadResult::Success { board, .. } =
            freerouting::io::read_board(&std::fs::read(&input).unwrap())
        else {
            panic!("{rel} does not load")
        };
        let name = Path::new(&rel)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let rust = pcb_from_board(&board, &name);
        let java = java_tree(&dump);
        failures.extend(
            unexplained(
                &java,
                &rust,
                board.communication.coordinate_transform.scale_factor(),
            )
            .into_iter()
            .map(|f| format!("{rel}: {f}")),
        );
        count += 1;
    }
    assert_eq!(count, 19);
    assert!(
        failures.is_empty(),
        "{} differences:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Ports of the Java tests that use the writer

use freerouting::board::items::ItemId;
use freerouting::io::specctra::dsn_writer::{write_dsn, write_dsn_to};

fn load_fixture(name: &str) -> BasicBoard {
    let path = fixtures_dir().join(name);
    match freerouting::io::read_board(&std::fs::read(path).unwrap()) {
        freerouting::io::BoardReadResult::Success { board, .. } => (**board).clone(),
        _ => panic!("{name} does not load"),
    }
}

fn read_text(text: &str) -> BasicBoard {
    match freerouting::io::read_board_from_str(text) {
        freerouting::io::BoardReadResult::Success { board, .. } => (**board).clone(),
        _ => panic!("text does not load"),
    }
}

/// `DsnWriterTest.writesValidDsnHeader`.
#[test]
fn writes_valid_dsn_header() {
    let board = load_fixture("Issue143-rpi_splitter.dsn");
    let content = write_dsn(&board, "test");
    assert!(
        content.starts_with("(pcb"),
        "DSN output must start with (pcb"
    );
    assert!(
        content.contains("(structure"),
        "DSN output must contain (structure scope"
    );
}

/// `DsnWriterTest.roundtripPreservesLayerCount`.
#[test]
fn roundtrip_preserves_layer_count() {
    let original = load_fixture("Issue143-rpi_splitter.dsn");
    let reloaded = read_text(&write_dsn(&original, "roundtrip"));
    assert_eq!(original.layer_count(), reloaded.layer_count());
}

/// `DsnWriterTest.outputStreamContainsDataAfterWrite`: `write_dsn_to` writes to a stream.
#[test]
fn output_stream_contains_data_after_write() {
    let board = load_fixture("Issue143-rpi_splitter.dsn");
    let mut out = Vec::new();
    write_dsn_to(&board, "flush-test", &mut out).unwrap();
    assert!(!out.is_empty());
    assert_eq!(
        String::from_utf8(out).unwrap(),
        write_dsn(&board, "flush-test")
    );
}

/// `NonConvexPadstackTest.dsnExportPreservesAuxiliaryShapesInRoundTrip`.
#[test]
fn dsn_export_preserves_auxiliary_shapes_in_round_trip() {
    const DSN: &str = r#"
    (pcb "test_non_convex_pad.dsn"
      (parser (string_quote ") (space_in_quoted_tokens on) (host_cad "KiCad's Pcbnew") (host_version "10.0.0"))
      (resolution um 10)
      (unit um)
      (structure
        (layer F.Cu (type signal) (property (index 0)))
        (layer B.Cu (type signal) (property (index 1)))
        (boundary (path pcb 0  0 0  10000 0  10000 10000  0 10000  0 0))
        (via "Via[0-1]_800:400_um")
        (rule (width 200) (clearance 50) (clearance 50 (type default_smd)) (clearance 50 (type smd_smd))))
      (placement (component "U_TEST:TEST_PKG" (place U1 5000 5000 front 0)))
      (library
        (image "U_TEST:TEST_PKG" (pin Custom_L_Pad 1 0 0))
        (padstack Custom_L_Pad
          (shape (polygon F.Cu 0 -100 -100  100 -100  100 0  0 0  0 100  -100 100))
          (attach off))
        (padstack "Via[0-1]_800:400_um" (shape (circle F.Cu 800)) (shape (circle B.Cu 800)) (attach off)))
      (network (net NET1 (pins U1-1))))"#;
    let board = read_text(DSN);
    let reloaded = read_text(&write_dsn(&board, "roundtrip"));
    let padstack = reloaded
        .library
        .padstacks
        .get_by_name("Custom_L_Pad")
        .expect("padstack exists after the round trip");
    assert!(
        padstack.has_non_convex_geometry(),
        "padstack must remain flagged as non-convex"
    );
    assert!(
        padstack.has_auxiliary_shapes(),
        "padstack must retain auxiliary shapes"
    );
    let aux = reloaded
        .items()
        .filter_map(|i| i.as_area())
        .filter(|a| a.name.as_deref().is_some_and(|n| n.ends_with("_aux")))
        .count();
    assert!(
        aux > 0,
        "companion obstacles must be recreated after the round trip"
    );
    let checker = DesignRulesChecker::new(&reloaded);
    assert!(
        checker.all_clearance_violations().is_empty(),
        "no clearance violations after the round trip"
    );
}

/// `Issue955PlacementLifecycleTest.dsnRoundTripPreservesBothContacts`: three round trips keep
/// the pins' centres and contacts.
#[test]
fn dsn_round_trip_preserves_both_contacts() {
    let board = load_fixture("Issue955-rotated-pad-contacts.dsn");
    let reload = {
        let mut b = round_trip(&board, "review");
        b = round_trip(&b, "review");
        round_trip(&b, "review")
    };
    let pins = |b: &BasicBoard| -> Vec<(String, usize, ItemId)> {
        b.items()
            .filter_map(|i| {
                let pin = i.as_pin()?;
                let c = b
                    .components
                    .get(freerouting::board::structure::ComponentId(i.component_id()))?;
                Some((c.name.clone(), pin.pin_index, i.id()))
            })
            .collect()
    };
    let centre = |b: &BasicBoard, id: ItemId| b.item(id).and_then(|i| i.drill()).map(|d| d.center);
    for (component, index, id) in pins(&board) {
        let (_, _, again) = pins(&reload)
            .into_iter()
            .find(|(c, i, _)| *c == component && *i == index)
            .expect("pin after the round trip");
        assert_eq!(
            centre(&board, id),
            centre(&reload, again),
            "{component}-{index}"
        );
        assert_eq!(
            board.normal_contacts(id).len(),
            reload.normal_contacts(again).len(),
            "{component}-{index}"
        );
    }
    let contacts = |name: &str| {
        let id = reload
            .items()
            .find(|i| {
                i.as_pin().is_some()
                    && reload
                        .components
                        .get(freerouting::board::structure::ComponentId(i.component_id()))
                        .is_some_and(|c| c.name == "S21")
                    && reload.pin_name(i).is_some_and(|n| n == name)
            })
            .map(|i| i.id())
            .expect("pin of S21");
        reload.normal_contacts(id).len()
    };
    assert_eq!(contacts("2@1"), 1);
    assert_eq!(contacts("1"), 1);
}

/// C06.8, the engine side: the board is written as it is. After `prepare_job` it carries the
/// classes `board_edge` and `hole_edge`, and they are written; a re-read board has them, the
/// outline's class included, so the default edge override is skipped on the next run (Java's
/// rule for an explicit outline class).
#[test]
fn prepare_job_classes_are_written() {
    use freerouting::job::prepare::{JobInput, prepare_job};
    use freerouting::settings::ResolveContext;
    let bytes = std::fs::read(fixtures_dir().join("Issue413-test.dsn")).unwrap();
    let prepared = prepare_job(
        &JobInput::new(bytes, "Issue413-test"),
        &ResolveContext { cores: 8 },
    )
    .expect("the job prepares");
    let board: &BasicBoard = &prepared.board;
    let names: Vec<String> = (0..board.rules.clearance_matrix.class_count() as i32)
        .filter_map(|i| board.rules.clearance_matrix.get_name(i).map(str::to_string))
        .collect();
    assert!(names.iter().any(|n| n == "board_edge"), "{names:?}");
    let again = round_trip(board, "Issue413-test");
    assert_eq!(clearance(&again), clearance(board));
    assert_eq!(frame(&again), frame(board));
    assert_eq!(areas(&again), areas(board));
    let outline_class = |b: &BasicBoard| {
        b.outline().map(|i| {
            b.rules
                .clearance_matrix
                .get_name(i.clearance_class())
                .map(str::to_string)
        })
    };
    assert_eq!(outline_class(&again), outline_class(board));
}

/// The release-build write time of the largest board of the Java DSN dumps. Run with
/// `cargo test --release --test dsn_round_trip write_time -- --nocapture`.
#[test]
fn write_time_of_issue190_processor_z80() {
    let board = load_fixture("Issue190-processor.Z80.dsn");
    let mut best = std::time::Duration::MAX;
    let mut size = 0;
    for _ in 0..5 {
        let start = std::time::Instant::now();
        let text = write_dsn(&board, "Issue190-processor.Z80");
        best = best.min(start.elapsed());
        size = text.len();
    }
    eprintln!("Issue190-processor.Z80: write_dsn {best:?} for {size} bytes (best of 5)");
    assert!(best < std::time::Duration::from_secs(20));
}
