//! Shared by `dsn_writer.rs` and `dsn_round_trip.rs`: the fixture list, the round trip through
//! the DSN writer and a snapshot of everything a board keeps from its design file, in sections
//! that the C06 items compare one by one.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use freerouting::board::BasicBoard;
use freerouting::board::items::{AreaKind, ItemKind};
use freerouting::board::structure::ComponentId;
use freerouting::dsn::Pcb;
use freerouting::geometry::jmath::round_to_i32;
use freerouting::io::specctra::build_board;
use freerouting::io::specctra::dsn_writer::write_dsn;
use freerouting::io::{BoardReadResult, read_board};
use freerouting::rules::ItemClass;

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path == fixtures_dir().join("reference") {
            continue;
        }
        if path.is_dir() {
            collect(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("dsn"))
        {
            out.push(path);
        }
    }
}

/// Every fixture DSN file (outside `fixtures/reference`) as `(path relative to fixtures,
/// absolute path)`.
pub fn fixture_files() -> Vec<(String, PathBuf)> {
    let mut paths = Vec::new();
    collect(&fixtures_dir(), &mut paths);
    let only = std::env::var("DSN_ONLY").ok();
    paths
        .into_iter()
        .filter(|p| {
            only.as_ref()
                .is_none_or(|o| p.to_string_lossy().contains(o.as_str()))
        })
        .map(|p| {
            let rel = p
                .strip_prefix(fixtures_dir())
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (rel, p)
        })
        .collect()
}

/// Every fixture that loads (Java's `success` set), with its board.
pub fn loaded_boards() -> Vec<(String, BasicBoard)> {
    fixture_files()
        .into_iter()
        .filter_map(
            |(name, path)| match read_board(&std::fs::read(path).unwrap()) {
                BoardReadResult::Success { board, .. } => Some((name, (**board).clone())),
                _ => None,
            },
        )
        .collect()
}

/// Every fixture that loads, as the routing board the reader makes (for tests that route or
/// normalize it).
pub fn loaded_routing_boards() -> Vec<(String, freerouting::board::RoutingBoard)> {
    fixture_files()
        .into_iter()
        .filter_map(
            |(name, path)| match read_board(&std::fs::read(path).unwrap()) {
                BoardReadResult::Success { board, .. } => Some((name, *board)),
                _ => None,
            },
        )
        .collect()
}

/// The board read back from its own DSN text.
pub fn round_trip(board: &BasicBoard, name: &str) -> BasicBoard {
    let text = write_dsn(board, name);
    let pcb =
        Pcb::parse(&text).unwrap_or_else(|e| panic!("{name}: written DSN does not parse: {e}"));
    match build_board(&pcb) {
        BoardReadResult::Success { board, .. } => (**board).clone(),
        other => panic!(
            "{name}: written DSN does not load: {:?}",
            other.board().is_some()
        ),
    }
}

/// Differences between a board and its re-read board that are expected, as `(fixture, section,
/// reason)`. A test fails if one of them stops happening.
pub const EXPECTED: &[(&str, &str, &str)] = &[
    (
        "Issue179-Autorouter_PCB1_2023-3-24.dsn",
        "library",
        "padstack p7 has a degenerate shape (two parallel lines, a sliver 2 units wide) that has \
         no corners to write; the written polygon is empty. Java's load of this board ends in a \
         NullPointerException anyway (reference_load.rs)",
    ),
    (
        "canary-pwr/canary-pwr-pours-as-planes.dsn",
        "network",
        "the reader adds the net of every plane scope to the net list, in file order, before it \
         ignores the copper pour of a net that has a plane layer. The ignored pours are no items, \
         so the writer has nothing to write them from, and the nets +12V_BUS and +5V_PI get their \
         numbers later (the net lists are equal otherwise)",
    ),
];

fn expected(name: &str, section: &str) -> Option<&'static str> {
    EXPECTED
        .iter()
        .find(|(f, s, _)| *f == name && *s == section)
        .map(|(_, _, why)| *why)
}

/// Runs `check` on the original and the re-read board of every fixture and collects the
/// failures that are not in [`EXPECTED`] for `section`.
pub fn for_every_board(
    section: &str,
    check: impl Fn(&str, &BasicBoard, &BasicBoard) -> Vec<String>,
) {
    let mut failures = Vec::new();
    let mut count = 0;
    let mut seen = Vec::new();
    let only = std::env::var("DSN_ONLY").ok();
    for (name, board) in loaded_boards() {
        let again = round_trip(&board, &name);
        let found = check(&name, &board, &again);
        count += 1;
        if found.is_empty() {
            continue;
        }
        match expected(&name, section) {
            Some(_) => seen.push(name),
            None => failures.extend(found.into_iter().map(|f| format!("{name}: {f}"))),
        }
    }
    assert!(
        only.is_some() || count > 100,
        "only {count} fixtures loaded"
    );
    if only.is_none() {
        for (name, sec, why) in EXPECTED {
            assert!(
                *sec != section || seen.iter().any(|n| n == name),
                "{name} no longer differs in {section}: {why}"
            );
        }
    }
    assert!(
        failures.is_empty(),
        "{} differences:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The lines that only one of two snapshots has (at most 8 of each).
pub fn diff_lines(what: &str, a: &[String], b: &[String]) -> Vec<String> {
    if a == b {
        return Vec::new();
    }
    let only = |x: &[String], y: &[String]| -> Vec<String> {
        x.iter()
            .filter(|l| !y.contains(l))
            .take(8)
            .cloned()
            .collect()
    };
    let mut out = vec![format!("{what}: {} lines vs {}", a.len(), b.len())];
    let (oa, ob) = (only(a, b), only(b, a));
    for (x, y) in oa.iter().zip(&ob) {
        let at = x.chars().zip(y.chars()).take_while(|(p, q)| p == q).count();
        let window = |l: &str| {
            l.chars()
                .skip(at.saturating_sub(40))
                .take(160)
                .collect::<String>()
        };
        out.push(format!(
            "   line starts {:?}\n   - ...{}\n   + ...{}",
            x.chars().take(60).collect::<String>(),
            window(x),
            window(y)
        ));
    }
    for x in oa.iter().skip(ob.len()) {
        out.push(format!("   - {}", x.chars().take(300).collect::<String>()));
    }
    for y in ob.iter().skip(oa.len()) {
        out.push(format!("   + {}", y.chars().take(300).collect::<String>()));
    }
    if out.len() == 1 {
        out.push("   same lines, different order".into());
    }
    vec![out.join("\n")]
}

/// The differences between two snapshots of the same kind, as lines.
pub fn diff<T: std::fmt::Debug + PartialEq>(what: &str, a: &T, b: &T) -> Vec<String> {
    if a == b {
        return Vec::new();
    }
    let (a, b) = (format!("{a:?}"), format!("{b:?}"));
    let at = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
    let from = at.saturating_sub(60);
    let cut = |s: &str| s.chars().skip(from).take(200).collect::<String>();
    vec![format!(
        "{what}: first difference at char {at}\n   original: ...{}\n   re-read:  ...{}",
        cut(&a),
        cut(&b)
    )]
}

// ---------------------------------------------------------------------------
// Snapshots

fn class(board: &BasicBoard, index: i32) -> String {
    board
        .rules
        .clearance_matrix
        .get_name(index)
        .unwrap_or("?")
        .to_string()
}

fn layer(board: &BasicBoard, index: i32) -> String {
    board.layer_structure.layers[index as usize].name.clone()
}

fn net_names(board: &BasicBoard, numbers: &[i32]) -> Vec<String> {
    numbers
        .iter()
        .map(|n| {
            board.rules.nets.get(*n).map_or(format!("#{n}"), |net| {
                format!("{}/{}", net.name, net.subnet_number)
            })
        })
        .collect()
}

/// C06.1: layers, outline, resolution, unit, via padstack list and snap angle.
pub fn frame(board: &BasicBoard) -> Vec<String> {
    let mut out = Vec::new();
    for l in &board.layer_structure.layers {
        out.push(format!("layer {} signal={}", l.name, l.is_signal));
    }
    if let Some(outline) = board.outline().and_then(|i| i.as_board_outline()) {
        for shape in outline.shapes() {
            out.push(format!("outline {shape:?}"));
        }
    }
    let c = &board.communication;
    out.push(format!("resolution {:?} {}", c.unit, c.resolution));
    out.push(format!("snap {:?}", board.rules.trace_angle_restriction()));
    out.push(format!("bounds {:?}", board.bounding_box()));
    let outline_class = board.outline().map(|i| class(board, i.clearance_class()));
    out.push(format!("outline class {outline_class:?}"));
    let names: Vec<_> = (0..board.library.via_padstack_count())
        .filter_map(|i| board.library.get_via_padstack(i))
        .map(|p| p.name.clone())
        .collect();
    out.push(format!("via padstacks {names:?}"));
    out.push(format!(
        "via_at_smd {}",
        board
            .rules
            .via_infos
            .iter()
            .any(|(_, v)| v.attach_smd_allowed)
    ));
    out
}

/// C06.2: the clearance matrix and the default trace widths.
pub fn clearance(board: &BasicBoard) -> Vec<String> {
    let m = &board.rules.clearance_matrix;
    let n = m.class_count() as i32;
    let names: Vec<String> = (0..n).map(|i| class(board, i)).collect();
    let mut out = vec![format!("classes {names:?} layers {}", m.layer_count())];
    for i in 0..n {
        for j in 0..n {
            let values: Vec<i32> = (0..m.layer_count() as i32)
                .map(|l| m.get_value(i, j, l, false))
                .collect();
            out.push(format!("  {i},{j}: {values:?}"));
        }
    }
    if let Some(id) = board.rules.default_net_class_id() {
        let c = board.rules.net_classes.get(id);
        let widths: Vec<i32> = (0..board.layer_count() as i32)
            .map(|l| c.trace_half_width(l))
            .collect();
        out.push(format!("default half widths {widths:?}"));
    }
    out.push(format!(
        "smd_to_turn_gap {}",
        board.rules.pin_edge_to_turn_dist()
    ));
    out
}

/// C06.2: obstacle areas and conduction areas, in item list order.
pub fn areas(board: &BasicBoard) -> Vec<String> {
    let mut out = Vec::new();
    for item in board.items() {
        let ItemKind::Area(area) = &item.kind else {
            continue;
        };
        if item.component_id() != 0 || area.name.as_deref().is_some_and(|n| n.ends_with("_aux")) {
            continue;
        }
        match area.kind {
            AreaKind::Conduction { is_obstacle, .. } => out.push(format!(
                "conduction nets={:?} fixed={:?} class={} layer={} area={:?} is_obstacle={is_obstacle}",
                net_names(board, item.net_numbers()),
                item.fixed_state(),
                class(board, item.clearance_class()),
                layer(board, area.layer()),
                area.area(),
            )),
            kind => out.push(format!(
                "{kind:?} layer={} class={} area={:?}",
                layer(board, area.layer()),
                class(board, item.clearance_class()),
                area.area(),
            )),
        }
    }
    out
}

/// C06.3: components with their pin and keepout clearance classes. A placed component that
/// owns no item (a logo without pads) is left out: Java's writer cannot tell it from a
/// component whose items were all deleted, and drops it.
pub fn placement(board: &BasicBoard) -> BTreeMap<String, String> {
    let owners: BTreeSet<i32> = board.items().map(|i| i.component_id()).collect();
    let mut pins: BTreeMap<(i32, usize), i32> = BTreeMap::new();
    let mut keepouts: BTreeMap<(i32, String), i32> = BTreeMap::new();
    for item in board.items() {
        if item.component_id() == 0 {
            continue;
        }
        match &item.kind {
            ItemKind::Pin(pin) => {
                pins.insert((item.component_id(), pin.pin_index), item.clearance_class());
            }
            ItemKind::Area(area) => {
                if let Some(name) = &area.name {
                    keepouts
                        .entry((item.component_id(), name.clone()))
                        .or_insert(item.clearance_class());
                }
            }
            _ => {}
        }
    }
    let mut out = BTreeMap::new();
    for component in board.components.iter() {
        if component.is_placed() && !owners.contains(&component.id.0) {
            continue;
        }
        let package = board
            .library
            .packages
            .get(component.package())
            .map(|p| p.name.clone());
        let pin_classes: Vec<String> = pins
            .range((component.id.0, 0)..(component.id.0 + 1, 0))
            .map(|((_, i), c)| format!("{i}={}", class(board, *c)))
            .collect();
        let keepout_classes: Vec<String> = keepouts
            .range((component.id.0, String::new())..(component.id.0 + 1, String::new()))
            .map(|((_, n), c)| format!("{n}={}", class(board, *c)))
            .collect();
        out.insert(
            component.name.clone(),
            format!(
                "package={package:?} front={} placed={} at={:?} rot={} fixed={} logical={:?} pins={pin_classes:?} keepouts={keepout_classes:?}",
                component.placed_on_front(),
                component.is_placed(),
                component.exact_location(),
                component.rotation_in_degree(),
                component.position_fixed,
                component.logical_part(),
            ),
        );
    }
    out
}

/// A shape by its geometry: the sorted corners of a polyline shape (a path outline that was a
/// bounding octagon reads back as the polygon of the same corners) or the circle.
fn geometry(shape: &freerouting::geometry::Shape) -> String {
    match shape {
        freerouting::geometry::Shape::Polyline(p) => {
            use freerouting::geometry::PolylineShapeOps;
            let mut corners: Vec<(i64, i64)> = p
                .corner_approx_arr()
                .iter()
                .map(|c| (i64::from(round_to_i32(c.x)), i64::from(round_to_i32(c.y))))
                .collect();
            corners.sort();
            corners.dedup();
            format!("polyline {corners:?}")
        }
        other => format!("{other:?}"),
    }
}

/// C06.4: packages, padstacks and logical parts.
pub fn library(board: &BasicBoard) -> Vec<String> {
    let mut out = Vec::new();
    for package in board.library.packages.iter() {
        let pins: Vec<String> = package
            .pins()
            .iter()
            .map(|p| {
                format!(
                    "{}:{}@{:?}r{}",
                    p.name,
                    board
                        .library
                        .padstacks
                        .get(p.padstack_id)
                        .map_or("?", |s| s.name.as_str()),
                    p.exact_relative_location(),
                    p.rotation_in_degree
                )
            })
            .collect();
        let keepout = |ks: &[freerouting::library::PackageKeepout]| -> Vec<String> {
            ks.iter()
                .map(|k| format!("{}@{}:{:?}", k.name, k.layer, k.area))
                .collect()
        };
        out.push(format!(
            "package {} front={} pins={pins:?} outline={:?} keepouts={:?} via_keepouts={:?} place_keepouts={:?}",
            package.name,
            package.is_front,
            package.outline.as_ref().map(|o| o.iter().map(|s| s.as_ref().map(geometry)).collect::<Vec<_>>()),
            keepout(&package.keepouts),
            keepout(&package.via_keepouts),
            keepout(&package.place_keepouts),
        ));
    }
    for padstack in board.library.padstacks.iter() {
        let shapes: Vec<String> = (0..board.layer_count() as i32)
            .map(|l| {
                let aux: Vec<String> = padstack
                    .get_auxiliary_shapes(l)
                    .unwrap_or_default()
                    .iter()
                    .map(|s| format!("{:?}", s.to_shape()))
                    .collect();
                format!("{:?}+{aux:?}", padstack.get_shape(l).map(|s| s.to_shape()))
            })
            .collect();
        out.push(format!(
            "padstack {} attach={} absolute={} {shapes:?}",
            padstack.name, padstack.attach_allowed, padstack.placed_absolute
        ));
    }
    let parts = &board.library.logical_parts;
    for i in 1..=parts.count() {
        let part = parts.get(freerouting::library::LogicalPartId(i)).unwrap();
        let pins: Vec<_> = (0..part.pin_count())
            .filter_map(|k| part.get_pin(k))
            .collect();
        let comps: Vec<&str> = board
            .components
            .iter()
            .filter(|c| c.logical_part() == Some(part.id))
            .map(|c| c.name.as_str())
            .collect();
        out.push(format!(
            "part {} pins={pins:?} components={comps:?}",
            part.name
        ));
    }
    out
}

/// C06.5: nets, net classes, via infos and via rules.
pub fn network(board: &BasicBoard) -> Vec<String> {
    let mut out = Vec::new();
    let mut pins: BTreeMap<i32, BTreeSet<String>> = BTreeMap::new();
    for item in board.items() {
        let Some(pin) = item.as_pin() else { continue };
        let Some(component) = board.components.get(ComponentId(item.component_id())) else {
            continue;
        };
        let pin_name = board
            .library
            .packages
            .get(component.package())
            .and_then(|p| p.get_pin(pin.pin_index))
            .map(|p| p.name.clone())
            .unwrap_or_default();
        for n in item.net_numbers() {
            pins.entry(*n)
                .or_default()
                .insert(format!("{}-{}", component.name, pin_name));
        }
    }
    for net in board.rules.nets.iter() {
        let class_name = board
            .rules
            .net_classes
            .get(net.net_class())
            .name()
            .to_string();
        out.push(format!(
            "net #{} {} {} class={class_name} plane={} length={:?} pins={:?}",
            net.net_number,
            net.name,
            net.subnet_number,
            net.contains_plane(),
            net.explicit_length_constraint(),
            pins.get(&net.net_number)
        ));
    }
    for (_, c) in board.rules.net_classes.iter() {
        let widths: Vec<i32> = (0..board.layer_count() as i32)
            .map(|l| c.trace_half_width(l))
            .collect();
        let active: Vec<bool> = (0..board.layer_count() as i32)
            .map(|l| c.is_active_routing_layer(l))
            .collect();
        let items: Vec<String> = [
            ItemClass::Trace,
            ItemClass::Via,
            ItemClass::Pin,
            ItemClass::Smd,
            ItemClass::Area,
        ]
        .iter()
        .map(|k| class(board, c.default_item_clearance_classes.get(*k)))
        .collect();
        out.push(format!(
            "class {} widths={widths:?} active={active:?} clearance={} items={items:?} via_rule={:?} shove={} pull={} ignored={} min={} max={}",
            c.name(),
            class(board, c.trace_clearance_class()),
            c.via_rule().map(|r| board.rules.via_rules.get(r).name.clone()),
            c.is_shove_fixed(),
            c.pull_tight(),
            c.is_ignored_by_autorouter,
            c.minimum_trace_length(),
            c.maximum_trace_length(),
        ));
    }
    for (_, v) in board.rules.via_infos.iter() {
        out.push(format!(
            "via {} {} {} attach={}",
            v.name,
            board
                .library
                .padstacks
                .get(v.padstack)
                .map_or("?", |p| p.name.as_str()),
            class(board, v.clearance_class),
            v.attach_smd_allowed
        ));
    }
    // Two via rules of one name collapse to the last when read (Java's `addViaRule`).
    let mut rules: Vec<(String, Vec<String>)> = Vec::new();
    for (_, r) in board.rules.via_rules.iter() {
        let vias: Vec<String> = r
            .vias()
            .iter()
            .map(|id| board.rules.via_infos.get(*id).name.clone())
            .collect();
        rules.retain(|(name, _)| *name != r.name);
        rules.push((r.name.clone(), vias));
    }
    out.extend(
        rules
            .into_iter()
            .map(|(name, vias)| format!("via_rule {name} {vias:?}")),
    );
    out
}

/// C06.6: traces and vias as sorted lists keyed by geometry (item ids are renumbered).
pub fn wiring(board: &BasicBoard) -> (Vec<String>, Vec<String>) {
    let mut traces = Vec::new();
    let mut vias = Vec::new();
    for item in board.items() {
        match &item.kind {
            ItemKind::Trace(t) => traces.push(format!(
                "{} w={} {:?} nets={:?} fixed={:?} class={}",
                layer(board, t.layer),
                t.half_width * 2,
                t.polyline.corners(),
                net_names(board, item.net_numbers()),
                item.fixed_state(),
                class(board, item.clearance_class()),
            )),
            ItemKind::Via(v) => vias.push(format!(
                "{} {:?} nets={:?} fixed={:?} class={}",
                board
                    .library
                    .padstacks
                    .get(v.drill.padstack)
                    .map_or("?", |p| p.name.as_str()),
                v.drill.center,
                net_names(board, item.net_numbers()),
                item.fixed_state(),
                class(board, item.clearance_class()),
            )),
            _ => {}
        }
    }
    traces.sort();
    vias.sort();
    (traces, vias)
}
