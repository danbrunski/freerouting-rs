//! K07.3: the board built from a `.kicad_pcb` against the board built from
//! the DSN that KiCad 10 exports from the same file.
//!
//! For each committed board and each synthetic board the test builds
//! `build_board(read_kicad(board))` ("reader") and
//! `build_board(parse(<board>.kicad10.dsn))` ("export") and compares:
//!
//! - item counts by kind (pins, traces, vias, each area kind, component
//!   outlines, board outline), and per kind the sequence of items;
//! - nets: names, pin sets, net classes and their rules, the clearance matrix;
//! - the `BoardStatistics` fields except `host`;
//! - the DRC findings by type and the incomplete connections;
//! - geometry: items matched by kind, layers, nets and name, nearest
//!   position, within the tolerance of KiCad's `%.6g` formatting.
//!
//! Every difference is a `(board, key)` pair that [`EXPECTED_DIFFERENCES`]
//! lists with its reason. A difference that is not listed fails the test as a
//! reader bug; a listed one that no longer occurs fails it too, so the table
//! cannot go stale (`removing_an_expected_difference_fails` runs that check
//! on a copy of the table). Item order is compared and printed
//! (`--nocapture`); a difference in it is a row of the table like any other.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use freerouting::board::RoutingBoard;
use freerouting::board::items::{AreaKind, Item, ItemKind};
use freerouting::drc::DesignRulesChecker;
use freerouting::dsn::ast::Pcb;
use freerouting::io::kicad::{ReadOptions, read_kicad_input};
use freerouting::io::specctra::{BoardReadResult, build_board};
use freerouting::scoring::BoardStatistics;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/kicad")
}

fn boards() -> Vec<PathBuf> {
    fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                collect(&p, out);
            } else if p.extension().is_some_and(|x| x == "kicad_pcb") {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    collect(&fixtures(), &mut out);
    out.sort();
    out
}

fn board_name(path: &Path) -> String {
    path.file_stem().unwrap().to_string_lossy().into_owned()
}

fn unwrap_board(r: BoardReadResult, what: &str) -> RoutingBoard {
    match r {
        BoardReadResult::Success { board, .. } => *board,
        other => panic!("{what}: {other:?}"),
    }
}

/// The reader board and the export board of one `.kicad_pcb`.
fn both(path: &Path) -> (RoutingBoard, RoutingBoard) {
    let input = read_kicad_input(path, ReadOptions::default()).unwrap();
    let reader = unwrap_board(build_board(&input.pcb), "reader");
    let export_path = path.with_extension("kicad10.dsn");
    let export = Pcb::parse(&std::fs::read_to_string(&export_path).unwrap()).unwrap();
    let export = unwrap_board(build_board(&export), "export");
    (reader, export)
}

// ---- item signatures --------------------------------------------------------

#[derive(Clone, Debug)]
struct Sig {
    kind: String,
    layers: (i32, i32),
    nets: Vec<String>,
    name: String,
    bbox: [i32; 4],
}

impl Sig {
    fn key(&self) -> String {
        format!(
            "{}|{}-{}|{}|{}",
            self.kind,
            self.layers.0,
            self.layers.1,
            self.nets.join(","),
            self.name
        )
    }

    fn deviation(&self, other: &Sig) -> i32 {
        (0..4)
            .map(|i| (self.bbox[i] - other.bbox[i]).abs())
            .max()
            .unwrap()
    }
}

fn kind_label(item: &Item) -> String {
    match &item.kind {
        ItemKind::Pin(_) => "pin".into(),
        ItemKind::Via(_) => "via".into(),
        ItemKind::Trace(_) => "trace".into(),
        ItemKind::Area(a) => match a.kind {
            AreaKind::Obstacle => "area:keepout".into(),
            AreaKind::ViaObstacle => "area:via_keepout".into(),
            AreaKind::ComponentObstacle => "area:place_keepout".into(),
            AreaKind::Conduction { .. } => "area:plane".into(),
        },
        ItemKind::ComponentOutline(_) => "component_outline".into(),
        ItemKind::BoardOutline(_) => "board_outline".into(),
    }
}

fn signatures(board: &RoutingBoard) -> Vec<Sig> {
    let mut items: Vec<&Item> = board.items().collect();
    items.sort_by_key(|i| i.id());
    items
        .into_iter()
        .map(|item| {
            let mut nets: Vec<String> = item
                .net_numbers()
                .iter()
                .map(|n| {
                    board
                        .rules
                        .nets
                        .get(*n)
                        .map_or("?".into(), |n| n.name.clone())
                })
                .collect();
            nets.sort();
            let name = match &item.kind {
                ItemKind::Pin(_) => format!(
                    "{}-{}",
                    board
                        .components
                        .get(freerouting::board::structure::ComponentId(
                            item.component_id()
                        ))
                        .map_or("?", |c| c.name.as_str()),
                    board.pin_name(item).unwrap_or("?")
                ),
                ItemKind::ComponentOutline(_) => board
                    .components
                    .get(freerouting::board::structure::ComponentId(
                        item.component_id(),
                    ))
                    .map_or("?".into(), |c| c.name.clone()),
                _ => String::new(),
            };
            let b = item.bounding_box();
            Sig {
                kind: kind_label(item),
                layers: (item.first_layer(), item.last_layer()),
                nets,
                name,
                bbox: [b.ll.x, b.ll.y, b.ur.x, b.ur.y],
            }
        })
        .collect()
}

/// Half a unit of the sixth significant digit of the largest coordinate, in
/// board units (0.1 um), twice (a box edge is a coordinate plus a width), plus
/// one unit.
fn tolerance(sigs: &[Sig]) -> i32 {
    let max_units = sigs
        .iter()
        .flat_map(|s| s.bbox)
        .map(|v| v.unsigned_abs())
        .max()
        .unwrap_or(0) as f64;
    let um = max_units / 10.0;
    let digits = if um < 1.0 {
        1
    } else {
        um.log10().floor() as i32 + 1
    };
    let half_step_um = 0.5 * 10f64.powi(digits - 6);
    (2.0 * (half_step_um * 10.0).ceil()) as i32 + 1
}

// ---- the comparison -----------------------------------------------------------

type Diffs = BTreeMap<String, Vec<String>>;

fn note(d: &mut Diffs, key: &str, detail: String) {
    d.entry(key.to_owned()).or_default().push(detail);
}

fn compare_items(
    reader: &RoutingBoard,
    export: &RoutingBoard,
    d: &mut Diffs,
    order: &mut Vec<String>,
) {
    let (r, e) = (signatures(reader), signatures(export));
    let tol = tolerance(&r).max(tolerance(&e));
    let kinds: std::collections::BTreeSet<&str> =
        r.iter().chain(e.iter()).map(|s| s.kind.as_str()).collect();
    for kind in kinds {
        let rk: Vec<&Sig> = r.iter().filter(|s| s.kind == kind).collect();
        let ek: Vec<&Sig> = e.iter().filter(|s| s.kind == kind).collect();
        if rk.len() != ek.len() {
            note(
                d,
                &format!("count:{kind}"),
                format!("reader {} export {}", rk.len(), ek.len()),
            );
        }
        // sequence
        let first = rk
            .iter()
            .zip(&ek)
            .position(|(a, b)| a.key() != b.key() || a.deviation(b) > tol);
        if let Some(i) = first {
            note(
                d,
                &format!("order:{kind}"),
                format!("first differs at {i} of {}/{}", rk.len(), ek.len()),
            );
        }
        order.push(match (first, rk.len() == ek.len()) {
            (None, true) => format!("{kind}: same order ({})", rk.len()),
            (None, false) => format!(
                "{kind}: common prefix equal, counts {}/{}",
                rk.len(),
                ek.len()
            ),
            (Some(i), _) => format!(
                "{kind}: first differs at {i} of {}/{}: reader {} {:?}, export {} {:?}",
                rk.len(),
                ek.len(),
                rk[i].key(),
                rk[i].bbox,
                ek.get(i).map_or("-".into(), |e| e.key()),
                ek.get(i).map(|e| e.bbox)
            ),
        });
        // geometry: nearest match within (kind, layers, nets, name)
        let mut buckets: BTreeMap<String, Vec<(&Sig, bool)>> = BTreeMap::new();
        for s in &ek {
            buckets.entry(s.key()).or_default().push((s, false));
        }
        let mut worst = 0;
        let mut unmatched = Vec::new();
        for s in &rk {
            let Some(bucket) = buckets.get_mut(&s.key()) else {
                unmatched.push(format!("{} has no export item", s.key()));
                continue;
            };
            let best = bucket
                .iter()
                .enumerate()
                .filter(|(_, (_, used))| !used)
                .min_by_key(|(_, (o, _))| s.deviation(o))
                .map(|(i, _)| i);
            match best {
                Some(i) => {
                    let dev = s.deviation(bucket[i].0);
                    bucket[i].1 = true;
                    worst = worst.max(dev);
                    if dev > tol {
                        note(
                            d,
                            &format!("geometry:{kind}"),
                            format!(
                                "{}: nearest export item is {dev} units away (tolerance {tol})",
                                s.key()
                            ),
                        );
                    }
                }
                None => unmatched.push(format!("{} has no unused export item", s.key())),
            }
        }
        for (key, bucket) in &buckets {
            for (_, used) in bucket {
                if !used {
                    unmatched.push(format!("export item {key} has no reader item"));
                }
            }
        }
        for u in unmatched.into_iter().take(3) {
            note(d, &format!("unmatched:{kind}"), u);
        }
        println!(
            "    {kind}: {} items, worst deviation {worst} units (tolerance {tol})",
            rk.len()
        );
    }
}

fn net_summary(board: &RoutingBoard) -> BTreeMap<String, (String, Vec<String>)> {
    let mut pins: BTreeMap<i32, Vec<String>> = BTreeMap::new();
    for s in board.items() {
        if let ItemKind::Pin(_) = s.kind {
            let name = format!(
                "{}-{}",
                board
                    .components
                    .get(freerouting::board::structure::ComponentId(s.component_id()))
                    .map_or("?", |c| c.name.as_str()),
                board.pin_name(s).unwrap_or("?")
            );
            for n in s.net_numbers() {
                pins.entry(*n).or_default().push(name.clone());
            }
        }
    }
    board
        .rules
        .nets
        .iter()
        .map(|n| {
            let class = board.rules.net_classes.get(n.net_class()).name().to_owned();
            let mut p = pins.remove(&n.net_number).unwrap_or_default();
            p.sort();
            (format!("{}#{}", n.name, n.subnet_number), (class, p))
        })
        .collect()
}

fn class_summary(board: &RoutingBoard) -> BTreeMap<String, String> {
    let layers = board.layer_count();
    let matrix = &board.rules.clearance_matrix;
    board
        .rules
        .net_classes
        .iter()
        .map(|(_, c)| {
            let widths: Vec<i32> = (0..layers as i32).map(|l| c.trace_half_width(l)).collect();
            let clearance = matrix
                .get_name(c.trace_clearance_class())
                .unwrap_or("?")
                .to_owned();
            let via_rule = c
                .via_rule()
                .map(|id| {
                    let rule = board.rules.via_rules.get(id);
                    let vias: Vec<String> = rule
                        .vias()
                        .iter()
                        .map(|v| board.rules.via_infos.get(*v).name.clone())
                        .collect();
                    format!("{vias:?}")
                })
                .unwrap_or_default();
            (
                c.name().to_owned(),
                format!("half widths {widths:?}, clearance class {clearance}, via rule {via_rule}"),
            )
        })
        .collect()
}

fn matrix_summary(board: &RoutingBoard) -> BTreeMap<String, Vec<i32>> {
    let m = &board.rules.clearance_matrix;
    let layers = board.layer_count() as i32;
    let n = m.class_count() as i32;
    let mut out = BTreeMap::new();
    for i in 0..n {
        for j in 0..n {
            let key = format!("{} x {}", m.get_name(i).unwrap(), m.get_name(j).unwrap());
            out.insert(
                key,
                (0..layers).map(|l| m.get_value(i, j, l, false)).collect(),
            );
        }
    }
    out
}

fn compare_nets(
    reader: &RoutingBoard,
    export: &RoutingBoard,
    d: &mut Diffs,
    order: &mut Vec<String>,
) {
    let (rn, en) = (net_summary(reader), net_summary(export));
    for (name, (class, pins)) in &rn {
        match en.get(name) {
            None => note(d, "nets:names", format!("net {name} only in the reader")),
            Some((eclass, epins)) => {
                if class != eclass {
                    note(
                        d,
                        "nets:class",
                        format!("{name}: reader {class}, export {eclass}"),
                    );
                }
                if pins != epins {
                    note(d, "nets:pins", format!("{name}: {pins:?} vs {epins:?}"));
                }
            }
        }
    }
    for name in en.keys().filter(|n| !rn.contains_key(*n)) {
        note(d, "nets:names", format!("net {name} only in the export"));
    }
    let names =
        |b: &RoutingBoard| -> Vec<String> { b.rules.nets.iter().map(|n| n.name.clone()).collect() };
    if names(reader) != names(export) {
        note(d, "order:nets", "net order differs".to_owned());
    }
    order.push(if names(reader) == names(export) {
        format!("nets: same order ({})", rn.len())
    } else {
        "nets: order differs".to_owned()
    });
    // classes and their rules
    let (rc, ec) = (class_summary(reader), class_summary(export));
    for (name, v) in &rc {
        match ec.get(name) {
            None => note(
                d,
                "classes:names",
                format!("class {name} only in the reader"),
            ),
            Some(e) if e != v => note(d, "classes:rules", format!("{name}: {v} vs {e}")),
            _ => {}
        }
    }
    for name in ec.keys().filter(|n| !rc.contains_key(*n)) {
        note(
            d,
            "classes:names",
            format!("class {name} only in the export"),
        );
    }
    let class_order = |b: &RoutingBoard| -> Vec<String> {
        b.rules
            .net_classes
            .iter()
            .map(|(_, c)| c.name().to_owned())
            .collect()
    };
    if class_order(reader) != class_order(export) {
        note(d, "order:net classes", "net class order differs".to_owned());
    }
    order.push(if class_order(reader) == class_order(export) {
        format!("net classes: same order {:?}", class_order(reader))
    } else {
        format!(
            "net classes: order differs, reader {:?} export {:?}",
            class_order(reader),
            class_order(export)
        )
    });
    if matrix_summary(reader) != matrix_summary(export) {
        note(
            d,
            "classes:clearance_matrix",
            "matrix values differ by class name".to_owned(),
        );
    }
}

/// `BoardStatistics` as Debug lines without the host.
fn stat_lines(board: &RoutingBoard) -> Vec<String> {
    let mut s = BoardStatistics::new(board);
    s.host = String::new();
    format!("{s:#?}").lines().map(str::to_owned).collect()
}

fn compare_statistics(reader: &RoutingBoard, export: &RoutingBoard, d: &mut Diffs) {
    let (r, e) = (stat_lines(reader), stat_lines(export));
    assert_eq!(r.len(), e.len());
    for (a, b) in r.iter().zip(&e) {
        if a != b {
            let field = a.trim().split(':').next().unwrap();
            let number = |l: &str| {
                l.split(':')
                    .nth(1)?
                    .trim()
                    .trim_end_matches(',')
                    .parse::<f64>()
                    .ok()
            };
            // a float that differs by less than 2e-4 of itself is the rounding of
            // KiCad's `%.6g`; anything else is a difference of its own
            let rounding = match (number(a), number(b)) {
                (Some(x), Some(y)) => (x - y).abs() <= 2e-4 * x.abs().max(y.abs()),
                _ => false,
            };
            let key = if rounding {
                "stats~rounding".to_owned()
            } else {
                format!("stats:{field}")
            };
            note(
                d,
                &key,
                format!("reader {} / export {}", a.trim(), b.trim()),
            );
        }
    }
}

fn pin_kind(board: &RoutingBoard, id: freerouting::board::items::ItemId) -> String {
    board.item(id).map_or("?".into(), kind_label)
}

fn compare_drc(reader: &RoutingBoard, export: &RoutingBoard, d: &mut Diffs) {
    let summarize = |b: &RoutingBoard| -> BTreeMap<String, usize> {
        let findings = DesignRulesChecker::new(b).check();
        let mut out: BTreeMap<String, usize> = BTreeMap::new();
        for v in &findings.clearance_violations {
            let mut pair = [pin_kind(b, v.first_item), pin_kind(b, v.second_item)];
            pair.sort();
            *out.entry(format!("clearance {}/{}", pair[0], pair[1]))
                .or_default() += 1;
        }
        for u in &findings.unconnected_items {
            *out.entry(format!("unconnected {:?}", u.kind)).or_default() += 1;
        }
        out.insert("zone islands".into(), findings.zone_islands.len());
        out.insert(
            "incomplete connections".into(),
            findings.incompletes.iter().map(|n| n.count()).sum(),
        );
        out
    };
    let (r, e) = (summarize(reader), summarize(export));
    let keys: std::collections::BTreeSet<&String> = r.keys().chain(e.keys()).collect();
    for k in keys {
        let (a, b) = (
            r.get(k).copied().unwrap_or(0),
            e.get(k).copied().unwrap_or(0),
        );
        if a != b {
            note(d, &format!("drc:{k}"), format!("reader {a} export {b}"));
        }
    }
    println!("    drc: reader {r:?}");
}

/// Every difference of one board, and the item-order lines.
fn differences(path: &Path) -> (Diffs, Vec<String>) {
    differences_of(path, true)
}

/// [`differences`]; without `checks` the statistics and the DRC are left out
/// (the corpus run does that for the two boards of 70 MB and more).
fn differences_of(path: &Path, checks: bool) -> (Diffs, Vec<String>) {
    let (reader, export) = both(path);
    let mut d = Diffs::new();
    let mut order = Vec::new();
    compare_items(&reader, &export, &mut d, &mut order);
    compare_nets(&reader, &export, &mut d, &mut order);
    if checks {
        compare_statistics(&reader, &export, &mut d);
        compare_drc(&reader, &export, &mut d);
    }
    (d, order)
}

/// Each known difference: board file stem (`*` for every board), difference
/// key (a prefix match), and the reason. K07.3's done note repeats them.
const EXPECTED_DIFFERENCES: &[(&str, &str, &str)] = &[
    // rule-area kinds (K06.2); `rule_area_and_plane_differences_are_exactly_the_documented_ones`
    // checks the arithmetic behind them on every board
    (
        "*",
        "count:area:place_keepout",
        "K06.2: a rule area that bans footprints also gets a place_keepout; KiCad's export has none",
    ),
    (
        "*",
        "unmatched:area:place_keepout",
        "K06.2: the place_keepouts, seen as unmatched",
    ),
    (
        "rule-areas",
        "count:area:keepout",
        "K06.2: areas that ban neither tracks nor vias are full keepouts in KiCad's export, nothing or a place_keepout in the reader",
    ),
    (
        "rule-areas",
        "unmatched:area:keepout",
        "K06.2: the same full keepouts, seen as unmatched",
    ),
    (
        "*",
        "stats:other_count",
        "K06.2: the place_keepouts are items of the reader's board only",
    ),
    (
        "*",
        "stats:total_count",
        "K06.2: the place_keepouts are items of the reader's board only",
    ),
    // copper zones of footprints (K06.4)
    (
        "rule-areas",
        "count:area:plane",
        "K06.4: the reader keeps the copper zone a footprint owns as a plane; KiCad's export drops it",
    ),
    (
        "rule-areas",
        "unmatched:area:plane",
        "K06.4: that plane, seen as unmatched",
    ),
    (
        "rule-areas",
        "geometry:area:plane",
        "K06.4: that plane, nearest to another plane",
    ),
    (
        "rule-areas",
        "stats:conduction_area_count",
        "K06.4: the kept plane",
    ),
    (
        "rule-areas",
        "stats:maximum_count",
        "K06.4: the kept plane belongs to a net, one more connection",
    ),
    (
        "rule-areas",
        "stats:escaped_count",
        "K06.4: the kept plane lets one more SMD pin escape",
    ),
    (
        "rule-areas",
        "drc:zone islands",
        "K06.4: the kept plane has a copper island, the export's six planes have none",
    ),
    // track arcs (K06.5)
    (
        "arcs",
        "geometry:trace",
        "K06.5: the reader follows an arc as a polyline, KiCad's export writes one chord from start to end",
    ),
    (
        "arcs",
        "stats:",
        "K06.5: more segments and bends of the arcs followed (763 bends against none), so every trace statistic changes",
    ),
    // the length statistic of Line.a/Line.b (Java quirk), made visible by the export's rounding
    (
        "Test",
        "stats:total_angled_length",
        "BoardStatistics sums |Line.a - Line.b| and the first and last Line of a trace are a corner plus a direction vector; the export's %.6g corners make 45-degree segments inexact, so the vector is not (1,1) but about 290000 units long (3 traces)",
    ),
    // item order (compared, not forced)
    (
        "classes",
        "order:net classes",
        "K05.3: KiCad writes the non-default classes in std::unordered_map order, the reader in project order (K00 question 6)",
    ),
    (
        "tastexx-pcb",
        "order:area:keepout",
        "K05.4: the board outline's holes (5 signal keepouts) come in another order and with another start vertex than Clipper's in KiCad; the set of holes is equal (outlines_equal_the_export). Pins, component outlines, nets and net classes are in the same order",
    ),
    (
        "rule-areas",
        "order:area:",
        "K06.2/K06.4: the area counts differ, so the sequences shift",
    ),
    (
        "arcs",
        "order:trace",
        "K06.5: the first trace is an arc followed as a polyline",
    ),
    // statistics that are float sums of coordinates
    (
        "*",
        "stats~rounding",
        "K00 question 2: KiCad's export prints shapes and wires with %.6g, the reader keeps nanometres; areas, lengths and extents differ by less than 2e-4 of themselves",
    ),
];

/// The differences of every board, computed once.
fn all_differences() -> &'static Vec<(String, Diffs, Vec<String>)> {
    static ALL: std::sync::OnceLock<Vec<(String, Diffs, Vec<String>)>> = std::sync::OnceLock::new();
    ALL.get_or_init(|| {
        boards()
            .into_iter()
            .map(|path| {
                let name = board_name(&path);
                println!("{name}:");
                let (diffs, order) = differences(&path);
                (name, diffs, order)
            })
            .collect()
    })
}

/// The differences the table does not cover (`board: key: first detail`), and
/// the table rows nothing used.
fn unexplained(table: &[(&str, &str, &str)]) -> (Vec<String>, Vec<String>) {
    let mut bad = Vec::new();
    let mut used = vec![false; table.len()];
    for (name, diffs, order) in all_differences() {
        println!("{name}:");
        for line in order {
            println!("    order: {line}");
        }
        for (key, details) in diffs {
            let row = table
                .iter()
                .position(|(b, k, _)| (*b == "*" || b == name) && key.starts_with(k));
            match row {
                Some(i) => used[i] = true,
                None => bad.push(format!("{name}: {key}: {}", details[0])),
            }
            println!(
                "    DIFF {key} ({}): {}{}",
                row.map_or("UNEXPLAINED", |_| "expected"),
                details[0],
                if details.len() > 1 {
                    format!(" (+{} more)", details.len() - 1)
                } else {
                    String::new()
                }
            );
        }
    }
    let stale = table
        .iter()
        .zip(&used)
        .filter(|(_, u)| !**u)
        .map(|((b, k, _), _)| format!("{b}: {k}"))
        .collect();
    (bad, stale)
}

#[test]
fn reader_board_equals_the_exports_board() {
    let (bad, stale) = unexplained(EXPECTED_DIFFERENCES);
    assert!(
        bad.is_empty(),
        "unexplained differences:\n{}",
        bad.join("\n")
    );
    assert!(
        stale.is_empty(),
        "expected differences that no longer occur:\n{}",
        stale.join("\n")
    );
}

/// K07.3's negative check, kept as a test: with any one row of the table
/// removed the comparison finds unexplained differences.
#[test]
fn removing_an_expected_difference_fails() {
    for (skip, row) in EXPECTED_DIFFERENCES.iter().enumerate() {
        let table: Vec<_> = EXPECTED_DIFFERENCES
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != skip)
            .map(|(_, r)| *r)
            .collect();
        let (bad, _) = unexplained(&table);
        assert!(!bad.is_empty(), "row {skip} ({}) is not needed", row.1);
    }
}

/// The area differences are exactly what K06.2 and K06.4 say, not just
/// "some difference": on every board the export has as many full keepouts as
/// the reader plus the area-layers that ban neither tracks nor vias, the
/// reader's place_keepouts are the area-layers that ban footprints, the
/// export has none, and the reader has the footprint copper zones it reports
/// as kept as extra planes.
#[test]
fn rule_area_and_plane_differences_are_exactly_the_documented_ones() {
    for path in boards() {
        let name = board_name(&path);
        let input = read_kicad_input(&path, ReadOptions::default()).unwrap();
        let (reader, export) = both(&path);
        let count =
            |b: &RoutingBoard, kind: &str| b.items().filter(|i| kind_label(i) == kind).count();
        let mut neither = 0;
        let mut place = 0;
        for area in &input.report.rule_areas {
            let layers = area.layers.len();
            if area.emitted.contains(&"place_keepout") {
                place += layers;
            }
            if !area.flags.tracks && !area.flags.vias {
                neither += layers;
            }
        }
        assert_eq!(
            count(&export, "area:keepout"),
            count(&reader, "area:keepout") + neither,
            "{name}: full keepouts"
        );
        assert_eq!(
            count(&reader, "area:place_keepout"),
            place,
            "{name}: place keepouts"
        );
        assert_eq!(count(&export, "area:place_keepout"), 0, "{name}");
        assert_eq!(
            count(&reader, "area:plane"),
            count(&export, "area:plane") + input.report.footprint_zones_kept.len(),
            "{name}: planes"
        );
        assert_eq!(
            count(&reader, "area:via_keepout"),
            count(&export, "area:via_keepout"),
            "{name}: via keepouts"
        );
    }
}

/// A trace that differs from the export's is a track arc the reader followed.
#[test]
fn trace_differences_come_from_arcs() {
    for (name, diffs, _) in all_differences() {
        if diffs.keys().any(|k| k.starts_with("geometry:trace")) {
            let path = boards()
                .into_iter()
                .find(|p| &board_name(p) == name)
                .unwrap();
            let input = read_kicad_input(&path, ReadOptions::default()).unwrap();
            assert!(input.report.track_arcs_flattened > 0, "{name}");
        }
    }
}

// ---- K07.6: the corpus (`cargo test --release --test kicad_parity -- --ignored --nocapture`) --------

fn corpus_boards() -> Vec<PathBuf> {
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
    let dir = std::env::var_os("FR_KICAD_CORPUS")
        .map(PathBuf::from)
        .unwrap_or_else(|| fixtures().join("../../target/kicad-corpus"));
    let mut out = Vec::new();
    for sub in ["java", "demos", "canary-pwr"] {
        collect(&dir.join(sub), &mut out);
    }
    // F03.3's corpus, when present
    if let Some(extra) = std::env::var_os("FR_CORPUS_DIR") {
        collect(Path::new(&extra), &mut out);
    }
    out.sort();
    out.retain(|p| p.with_extension("kicad10.dsn").exists());
    // FR_KICAD_ONLY=stem,stem limits a run to some boards
    if let Ok(only) = std::env::var("FR_KICAD_ONLY") {
        out.retain(|p| only.split(',').any(|s| s == board_name(p)));
    }
    out
}

/// One summary row per corpus board: the K07.3 comparison with the generic
/// expected differences, and the unexplained ones listed. Prints; fails only
/// on a panic (a board that does not build).
#[test]
#[ignore = "needs the corpus: tools/kicad-reader/fetch-corpus.sh target/kicad-corpus"]
fn corpus_summary() {
    let boards = corpus_boards();
    if boards.is_empty() {
        println!("no corpus found; set FR_KICAD_CORPUS");
        return;
    }
    println!("board | bytes | items reader/export | order | unexplained differences");
    let generic: Vec<_> = EXPECTED_DIFFERENCES
        .iter()
        .filter(|(b, ..)| *b == "*")
        .collect();
    for path in boards {
        let name = board_name(&path);
        let bytes = std::fs::metadata(&path).map_or(0, |m| m.len());
        let checks = bytes < 20_000_000;
        let started = std::time::Instant::now();
        let result = std::panic::catch_unwind(|| {
            let (reader, export) = both(&path);
            let counts = (reader.item_count(), export.item_count());
            let (d, order) = differences_of(&path, checks);
            (counts, d, order)
        });
        match result {
            Err(_) => println!("{name} | {bytes} | - | - | REFUSED or panicked (see above)"),
            Ok((counts, d, order)) => {
                let unexplained: Vec<String> = d
                    .iter()
                    .filter(|(k, _)| !generic.iter().any(|(_, g, _)| k.starts_with(g)))
                    .map(|(k, v)| format!("{k} ({}; {})", v.len(), v[0]))
                    .collect();
                let order_diffs = order.iter().filter(|l| l.contains("differs")).count();
                println!(
                    "{name} | {bytes} | {}/{} | {} differing kinds | {} [{:.1}s{}]",
                    counts.0,
                    counts.1,
                    order_diffs,
                    if unexplained.is_empty() {
                        "none".to_owned()
                    } else {
                        unexplained.join(" ; ")
                    },
                    started.elapsed().as_secs_f64(),
                    if checks {
                        ""
                    } else {
                        ", statistics and DRC skipped"
                    },
                );
            }
        }
    }
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

/// Read only, and read plus `build_board`, against parse plus `build_board`
/// of the export: the median of three runs, in milliseconds. Boards named in
/// `FR_KICAD_TIME` (comma separated stems; default the two largest demos).
#[test]
#[ignore = "needs the corpus; run in release on a quiet machine"]
fn corpus_timing() {
    let wanted = std::env::var("FR_KICAD_TIME")
        .unwrap_or_else(|_| "jetson-agx-thor-baseboard,vme-wren".to_owned());
    let all = corpus_boards();
    println!(
        "board | reader read | reader read+build | DSN parse | DSN parse+build  (ms, median of 3)"
    );
    for stem in wanted.split(',') {
        let Some(path) = all.iter().find(|p| board_name(p) == stem) else {
            println!("{stem}: not in the corpus");
            continue;
        };
        let export_path = path.with_extension("kicad10.dsn");
        let ms = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1000.0;
        let (mut r, mut rb, mut p, mut pb) = (vec![], vec![], vec![], vec![]);
        for _ in 0..3 {
            let t = std::time::Instant::now();
            let input = read_kicad_input(path, ReadOptions::default()).unwrap();
            r.push(ms(t));
            let t2 = std::time::Instant::now();
            let board = build_board(&input.pcb);
            rb.push(r.last().unwrap() + ms(t2));
            drop(board);
            let t = std::time::Instant::now();
            let text = std::fs::read_to_string(&export_path).unwrap();
            let pcb = Pcb::parse(&text).unwrap();
            p.push(ms(t));
            let t2 = std::time::Instant::now();
            let board = build_board(&pcb);
            pb.push(p.last().unwrap() + ms(t2));
            drop(board);
        }
        println!(
            "{stem} | {:.0} | {:.0} | {:.0} | {:.0}",
            median(r),
            median(rb),
            median(p),
            median(pb)
        );
    }
}

/// The connectivity differences of routed corpus boards come from the export's
/// `%.6g` rounding: round the reader's wires and vias the way the export
/// prints them and the incomplete connections and dangling items equal the
/// export's (boards without track arcs; `FR_KICAD_ROUNDED` names the stems).
#[test]
#[ignore = "needs the corpus"]
fn export_rounding_explains_connectivity() {
    use freerouting::dsn::ast::{Shape, WiringItem};
    use freerouting_kicad::fmt6g::c_fmt_6g;
    let round = |v: f64| c_fmt_6g(v).parse::<f64>().unwrap();
    let wanted = std::env::var("FR_KICAD_ROUNDED")
        .unwrap_or_else(|_| "bm1.routed,bm4.routed,bm10.routed".to_owned());
    for stem in wanted.split(',') {
        let Some(path) = corpus_boards().into_iter().find(|p| board_name(p) == stem) else {
            println!("{stem}: not in the corpus");
            continue;
        };
        let mut input = read_kicad_input(&path, ReadOptions::default()).unwrap();
        assert_eq!(input.report.track_arcs_flattened, 0, "{stem} has arcs");
        for item in &mut input.pcb.wiring.as_mut().unwrap().items {
            match item {
                WiringItem::Wire(w) => {
                    for shape in &mut w.shapes {
                        if let Shape::Path { points, width, .. } = shape {
                            *width = round(*width);
                            for p in points {
                                p.x = round(p.x);
                                p.y = round(p.y);
                            }
                        }
                    }
                }
                WiringItem::Via(v) => {
                    v.at.x = round(v.at.x);
                    v.at.y = round(v.at.y);
                }
            }
        }
        let rounded = unwrap_board(build_board(&input.pcb), "rounded");
        let (_, export) = both(&path);
        let count = |b: &RoutingBoard| {
            let f = DesignRulesChecker::new(b).check();
            (
                f.incompletes.iter().map(|n| n.count()).sum::<usize>(),
                f.unconnected_items.len(),
            )
        };
        println!(
            "{stem}: rounded reader {:?}, export {:?}",
            count(&rounded),
            count(&export)
        );
        assert_eq!(count(&rounded), count(&export), "{stem}");
    }
}
