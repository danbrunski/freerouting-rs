//! Copper islands compared with Java (B07.3).
//!
//! `tools/java-reference/probes/board/IslandsProbe.java` loads every
//! `fixtures.tsv` board that has conduction areas, imports its session and
//! writes to `fixtures/reference/probe/islands/<board>.jsonl`: Java's
//! islands and `getZoneIslandViolations()` findings, with the evidence of
//! Java's hole pairing per island, and the same check with the hole pairing
//! fixed (`fixedIsland` and `fixed` lines: every hole subtracted from the
//! smallest outer contour around it, islands in the port's order). This test
//! loads the same boards and runs `DesignRulesChecker::zone_island_violations`
//! (islands on) and `fill_islands`. Three comparisons per board:
//!
//! 1. Islands: Rust's islands of each conduction area pair with Java's fixed
//!    islands. A Rust island that pairs with none must be a sliver below
//!    1e5 board units² (docs/DIVERGENCES.md, "Design rule check"). A Java
//!    island that pairs with none must be below [`SPECK`] (pieces of thermal
//!    spokes that AWT fails to clip), or lie inside a paired Rust island
//!    (merged into its neighbour), or be listed in
//!    [`EXPECTED_ISLAND_MISMATCHES`].
//! 2. Fixed findings: Rust's findings equal Java's findings on the fixed
//!    islands.
//! 3. Java's findings: Rust's findings equal Java's, except on the boards of
//!    [`ALLOWLIST`]. Each entry names its evidence, which the test re-checks:
//!    a Java island whose probe line says `holeOutside` (a hole contour
//!    outside the island's own outer boundary, or an island reaching outside
//!    the fill), or a tie for the primary island that Java's island order
//!    breaks the other way. The test fails when the evidence disappears or the
//!    board's findings start to match. On an allowlisted board each finding
//!    only in Java must lie on an island with `holeOutside`, or be judged
//!    against a primary island with one, or pair with a finding only in Rust
//!    of the same type and item count (the tie); each finding only in Rust
//!    must be a fixed finding. The difference is proven finding by finding.
//!
//! Findings are multisets keyed by (conduction area id, type, sorted item
//! ids). Within a key, and for islands, a Java entry pairs with a Rust entry
//! whose bounds are within 1% of the side of the fill's bounding box (per
//! axis) and whose area is within 1%, the closest such entry first. Two
//! floors cover small islands: the area may always differ by a sliver
//! (1e5 board units²), and bounds and area by what the port's 16-step
//! flattening of Bézier circles moves an island's border ([`FLATTENING_UM`]
//! along its perimeter). Rust's island area is the sum of its trapezoids;
//! Java's is the shoelace area of the AWT path flattened to 1 board unit.
//!
//! `ISLANDS_BOARD=<board>[,<board>...]` restricts the run to those fixtures.

mod board_probe;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use board_probe::Row;
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::structure::Unit;
use freerouting::drc::json::JsonValue;
use freerouting::drc::{DesignRulesChecker, Island, fill_islands};
use freerouting::geometry::FloatPoint;

/// The relative tolerance of bounds (of the area's bounding-box side) and
/// of island areas.
const TOLERANCE: f64 = 0.01;

/// The largest island one side may have and the other not, and the
/// largest difference of island areas that is always accepted
/// (docs/DIVERGENCES.md: the boundaries differ by slivers below 1e5 board
/// units²).
const SLIVER: f64 = 1e5;

/// The largest Java island the port may lack. Where a thermal spoke ends on
/// its clearance circle (a round pad: the spoke is as long as the circle's
/// radius), AWT's `Area.intersect` leaves a piece of the spoke outside the
/// circle (on Issue756-tomu-fpga: 1457 x 296 units, 1.9e5 units²); seen up
/// to 6.5e5 board units². Such pieces hold no items (comparison 2 would show
/// them) and are far below the dead copper threshold of 1 mm².
const SPECK: f64 = 1e6;

/// The farthest a flattened circle of the port lies from Java's Bézier
/// circle, in µm: a 16-step quarter arc of radius r deviates by
/// r (1 - cos(π/64)) ≈ 0.0012 r, 5 µm for clearance circles up to 4 mm.
/// An island bounded by circles may grow or shrink by that along its
/// border, so its area may differ by its perimeter times this.
const FLATTENING_UM: f64 = 5.0;

/// A board whose findings differ from Java's, with the evidence the test
/// re-checks.
struct Allowed {
    board: &'static str,
    evidence: Evidence,
}

enum Evidence {
    /// Java's hole pairing: the probe line of island `island` of conduction
    /// area `area` says `holeOutside`.
    MisplacedHole { area: i64, island: i64 },
    /// The island order: in conduction area `area` two islands tie for the
    /// most items, and Java's AWT path order picks the other one as the
    /// primary, so each side reports the island the other takes for the
    /// primary (a finding only in Java pairs with one only in Rust of the
    /// same type and item count).
    IslandOrder { area: i64 },
}

use Evidence::{IslandOrder, MisplacedHole};

const ALLOWLIST: &[Allowed] = &[
    Allowed {
        board: "Issue015-StackOverflow",
        evidence: MisplacedHole {
            area: 13,
            island: 1,
        },
    },
    Allowed {
        board: "Issue022-AutoRouter_interrupted",
        evidence: MisplacedHole {
            area: 23,
            island: 1,
        },
    },
    Allowed {
        board: "Issue027-zMRETestFixture",
        evidence: MisplacedHole {
            area: 19,
            island: 4,
        },
    },
    Allowed {
        board: "Issue029-hw48na",
        evidence: MisplacedHole {
            area: 13,
            island: 1,
        },
    },
    Allowed {
        board: "Issue054-tairakb",
        evidence: MisplacedHole { area: 3, island: 1 },
    },
    Allowed {
        board: "Issue093-interf_u",
        evidence: MisplacedHole {
            area: 2,
            island: 21,
        },
    },
    Allowed {
        board: "Issue113-Protein",
        evidence: MisplacedHole { area: 3, island: 1 },
    },
    Allowed {
        board: "Issue145-smoothieboard",
        evidence: MisplacedHole { area: 2, island: 1 },
    },
    Allowed {
        board: "Issue153-wavefolder",
        evidence: MisplacedHole { area: 2, island: 4 },
    },
    Allowed {
        board: "Issue163-pic_programmer",
        evidence: MisplacedHole { area: 2, island: 1 },
    },
    Allowed {
        board: "Issue214-freerouting",
        evidence: MisplacedHole {
            area: 8,
            island: 31,
        },
    },
    Allowed {
        board: "Issue219-LogicBoard_smt",
        evidence: MisplacedHole { area: 3, island: 4 },
    },
    Allowed {
        board: "Issue230-CNH_Functional_Tester/CNH_Functional_Tester_1",
        evidence: MisplacedHole { area: 3, island: 1 },
    },
    Allowed {
        board: "Issue230-CNH_Functional_Tester_1",
        evidence: MisplacedHole { area: 3, island: 1 },
    },
    Allowed {
        board: "Issue269-min_fr_test/min_fr_test",
        evidence: MisplacedHole { area: 2, island: 4 },
    },
    Allowed {
        board: "Issue269-min_fr_test/min_fr_test_no_quotes",
        evidence: MisplacedHole { area: 2, island: 4 },
    },
    Allowed {
        board: "Issue269-z10_module",
        evidence: MisplacedHole {
            area: 20,
            island: 1,
        },
    },
    Allowed {
        board: "Issue367-Charger",
        evidence: MisplacedHole {
            area: 16,
            island: 2,
        },
    },
    Allowed {
        board: "Issue555-CNH_Functional_Tester_1",
        evidence: MisplacedHole { area: 3, island: 1 },
    },
    Allowed {
        board: "Issue649-kicad_ecc83-pp_input_board_v1",
        evidence: MisplacedHole { area: 2, island: 4 },
    },
    Allowed {
        board: "Issue649-kicad_ecc83-pp_input_board_v2",
        evidence: MisplacedHole { area: 2, island: 4 },
    },
    Allowed {
        board: "Issue676-ch32v-tx118s",
        evidence: MisplacedHole { area: 2, island: 6 },
    },
    Allowed {
        board: "Issue690-ecc83",
        evidence: MisplacedHole { area: 2, island: 4 },
    },
    Allowed {
        board: "Issue690-kit-dev-coldfire-xilinx_5213",
        evidence: MisplacedHole { area: 4, island: 1 },
    },
    Allowed {
        board: "Issue690-sonde_xilinx",
        evidence: MisplacedHole {
            area: 2,
            island: 12,
        },
    },
    Allowed {
        board: "Issue732-CM5_MINIMA_3",
        evidence: MisplacedHole {
            area: 23,
            island: 2,
        },
    },
    Allowed {
        board: "Issue732-RoyalBlue54L-Feather",
        evidence: MisplacedHole {
            area: 11,
            island: 4,
        },
    },
    Allowed {
        board: "Issue733-kicad_interf_u_input_design",
        evidence: MisplacedHole {
            area: 2,
            island: 19,
        },
    },
    Allowed {
        board: "Issue753-CPU-85_r104",
        evidence: MisplacedHole { area: 4, island: 2 },
    },
    Allowed {
        board: "Issue754-avionics_hub",
        evidence: IslandOrder { area: 3 },
    },
    Allowed {
        board: "Issue756-tomu-fpga",
        evidence: MisplacedHole {
            area: 10,
            island: 1,
        },
    },
    Allowed {
        board: "Issue756-tomu-fpga11",
        evidence: MisplacedHole {
            area: 14,
            island: 3,
        },
    },
    Allowed {
        board: "Issue756-tomu-fpga7",
        evidence: MisplacedHole {
            area: 665,
            island: 3,
        },
    },
    Allowed {
        board: "Issue756-tomu-fpga8",
        evidence: MisplacedHole {
            area: 665,
            island: 3,
        },
    },
    Allowed {
        board: "Issue756-tomu-fpga9",
        evidence: MisplacedHole {
            area: 664,
            island: 3,
        },
    },
    Allowed {
        board: "Issue957-smoke-ldo-led",
        evidence: MisplacedHole {
            area: 3,
            island: 36,
        },
    },
    Allowed {
        board: "canary-pwr/canary-pwr-2026-10-15-1747-unpoured",
        evidence: MisplacedHole { area: 4, island: 3 },
    },
    Allowed {
        board: "canary-pwr/canary-pwr-pours-as-planes",
        evidence: MisplacedHole {
            area: 28,
            island: 3,
        },
    },
    Allowed {
        board: "canary-pwr/canary-pwr-pours1",
        evidence: MisplacedHole { area: 5, island: 5 },
    },
    Allowed {
        board: "canary-pwr/canary-pwr-pours2",
        evidence: MisplacedHole {
            area: 19,
            island: 3,
        },
    },
    Allowed {
        board: "canary-pwr/canary-pwr-pours3",
        evidence: MisplacedHole {
            area: 19,
            island: 3,
        },
    },
];

/// An island difference that is not the flattening, with its reason
/// (docs/DIVERGENCES.md, "Design rule check").
struct ExpectedMismatch {
    board: &'static str,
    area: i64,
    #[allow(dead_code)]
    why: &'static str,
}

const EXPECTED_ISLAND_MISMATCHES: &[ExpectedMismatch] = &[ExpectedMismatch {
    board: "Issue367-Charger",
    area: 19,
    why: "Java's Area.intersect leaves the lower left end of one thermal spoke of pad 764 \
        unclipped (the spoke ends on the clearance circle, so it is nearly tangent there): \
        Java's island reaches (492704, -283863), the spoke rectangle's corner, the port's \
        stops at the circle, 297 units in",
}];

/// A finding, or an island (kind and items empty).
#[derive(Clone, Debug)]
struct Entry {
    area: i64,
    kind: String,
    items: Vec<i64>,
    /// The Java island index (`-1` for Rust entries).
    island: i64,
    /// `[min x, min y, max x, max y]`.
    bounds: [f64; 4],
    size: f64,
    /// A point well inside a Java fixed island.
    inside: Option<FloatPoint>,
}

impl Entry {
    fn key(&self) -> (i64, &str, &[i64]) {
        (self.area, &self.kind, &self.items)
    }
}

#[derive(Debug)]
struct JavaIsland {
    area: i64,
    island: i64,
    hole_outside: bool,
    line: String,
}

#[derive(Debug, Default)]
struct JavaBoard {
    areas: Vec<i64>,
    islands: Vec<JavaIsland>,
    findings: Vec<Entry>,
    fixed_islands: Vec<Entry>,
    fixed: Vec<Entry>,
    /// The island each area's connectivity is judged against, by area.
    primaries: BTreeMap<i64, i64>,
    exception: Option<String>,
}

fn field<'a>(value: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    match value {
        JsonValue::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn int(value: &JsonValue, key: &str) -> i64 {
    match field(value, key) {
        Some(JsonValue::Number(n)) => n.parse().expect("an integer"),
        other => panic!("{key}: not a number: {other:?}"),
    }
}

fn text(value: &JsonValue, key: &str) -> String {
    match field(value, key) {
        Some(JsonValue::String(s)) => s.clone(),
        other => panic!("{key}: not a string: {other:?}"),
    }
}

fn double(value: &JsonValue, key: &str) -> f64 {
    let bits = text(value, key);
    let hex = bits.strip_prefix("0x").expect("0x + raw bits");
    f64::from_bits(u64::from_str_radix(hex, 16).expect("hex bits"))
}

fn flag(value: &JsonValue, key: &str) -> bool {
    match field(value, key) {
        Some(JsonValue::Bool(b)) => *b,
        other => panic!("{key}: not a bool: {other:?}"),
    }
}

fn java_entry(value: &JsonValue, with_finding: bool) -> Entry {
    let (kind, items) = if with_finding {
        let items = match field(value, "items") {
            Some(JsonValue::Array(ids)) => ids
                .iter()
                .map(|id| match id {
                    JsonValue::Number(n) => n.parse().expect("an item id"),
                    other => panic!("item id {other:?}"),
                })
                .collect(),
            other => panic!("items: {other:?}"),
        };
        (text(value, "type"), items)
    } else {
        (String::new(), Vec::new())
    };
    Entry {
        area: int(value, "area"),
        kind,
        items,
        island: if with_finding {
            int(value, "island")
        } else {
            int(value, "fixedIsland")
        },
        bounds: [
            double(value, "minX"),
            double(value, "minY"),
            double(value, "maxX"),
            double(value, "maxY"),
        ],
        size: double(value, "awtArea"),
        inside: (!with_finding)
            .then(|| FloatPoint::new(double(value, "insideX"), double(value, "insideY"))),
    }
}

fn java_path(row: &Row) -> PathBuf {
    board_probe::repo_root()
        .join("fixtures/reference/probe/islands")
        .join(format!("{}.jsonl", row.board))
}

/// The committed Java output of `row`, `None` when the probe skipped it
/// (no conduction areas).
fn java_board(row: &Row) -> Option<JavaBoard> {
    let path = java_path(row);
    assert!(
        !path.with_extension("digest").exists(),
        "{}: the Java output grew above 1 MiB and generate.sh digested it; the island \
         comparison needs the lines (shorten IslandsProbe.java's output)",
        row.board
    );
    let all = std::fs::read_to_string(path).ok()?;
    let mut board = JavaBoard::default();
    for line in all.lines().skip(1) {
        let value = JsonValue::parse(line).unwrap_or_else(|e| panic!("{}: {e:?}", row.board));
        if field(&value, "exception").is_some() {
            board.exception = Some(line.to_string());
        } else if field(&value, "finding").is_some() {
            board.findings.push(java_entry(&value, true));
        } else if field(&value, "fixed").is_some() {
            board.fixed.push(java_entry(&value, true));
        } else if field(&value, "fixedIsland").is_some() {
            board.fixed_islands.push(java_entry(&value, false));
        } else if field(&value, "island").is_some() {
            board.islands.push(JavaIsland {
                area: int(&value, "area"),
                island: int(&value, "island"),
                hole_outside: flag(&value, "holeOutside"),
                line: line.to_string(),
            });
        } else {
            board.areas.push(int(&value, "area"));
            board
                .primaries
                .insert(int(&value, "area"), int(&value, "primary"));
        }
    }
    Some(board)
}

fn island_size(island: &Island) -> f64 {
    island
        .trapezoids
        .iter()
        .map(|t| {
            (t.x_right - t.x_left) * ((t.left[1] - t.left[0]) + (t.right[1] - t.right[0])) / 2.0
        })
        .sum()
}

fn rust_entry(area: i64, kind: &str, items: &[ItemId], island: &Island) -> Entry {
    let mut items: Vec<i64> = items.iter().map(|ItemId(id)| i64::from(*id)).collect();
    items.sort_unstable();
    Entry {
        area,
        kind: kind.to_string(),
        items,
        island: -1,
        bounds: island.bounds,
        size: island_size(island),
        inside: None,
    }
}

/// Rust's side of one board.
struct RustBoard {
    /// [`FLATTENING_UM`] in board units.
    flattening: f64,
    /// The bounding box of each conduction area, by id.
    boxes: BTreeMap<i64, [f64; 4]>,
    islands: Vec<Entry>,
    /// The islands of [`islands`](Self::islands), for point tests.
    shapes: Vec<Island>,
    findings: Vec<Entry>,
}

/// Rust's islands and findings of `row`, `None` when the board has no
/// conduction areas.
fn rust_board(row: &Row) -> Option<RustBoard> {
    let mut board: RoutingBoard = board_probe::load(row);
    if board.conduction_areas().is_empty() {
        return None;
    }
    board_probe::import_session(&mut board, row);
    let mut boxes = BTreeMap::new();
    let mut islands = Vec::new();
    let mut shapes = Vec::new();
    for id in board.conduction_areas() {
        let item = board.item(id).expect("a conduction area");
        let b = item.bounding_box();
        let area = i64::from(id.0);
        boxes.insert(
            area,
            [
                f64::from(b.ll.x),
                f64::from(b.ll.y),
                f64::from(b.ur.x),
                f64::from(b.ur.y),
            ],
        );
        for island in fill_islands(&board, item) {
            islands.push(rust_entry(area, "", &[], &island));
            shapes.push(island);
        }
    }
    let findings = DesignRulesChecker::new(&board)
        .zone_island_violations()
        .iter()
        .map(|v| {
            rust_entry(
                i64::from(v.conduction_area.0),
                v.kind.type_name(),
                &v.items_in_island,
                &v.island,
            )
        })
        .collect();
    Some(RustBoard {
        flattening: FLATTENING_UM * board.communication.get_resolution(Unit::Um),
        boxes,
        islands,
        shapes,
        findings,
    })
}

/// True when Rust merged the Java island `java` into a neighbour: a Rust
/// island of the same area that paired with a Java island contains a point
/// well inside `java` (the port's flattened circles are smaller than AWT's,
/// which can open the neck between a sliver and its neighbour).
fn merged(java: &Entry, rust: &RustBoard, paired: &[bool]) -> bool {
    let Some(inside) = java.inside else {
        return false;
    };
    rust.islands
        .iter()
        .zip(&rust.shapes)
        .zip(paired)
        .any(|((entry, shape), paired)| {
            *paired && entry.area == java.area && shape.contains(inside)
        })
}

/// The extent of each conduction area's fill: the union of Java's fixed
/// islands (some fills reach outside the area's own bounding box, which is
/// the fallback).
fn fill_boxes(java: &JavaBoard, rust: &RustBoard) -> BTreeMap<i64, [f64; 4]> {
    let mut boxes: BTreeMap<i64, [f64; 4]> = BTreeMap::new();
    for island in &java.fixed_islands {
        let b = boxes
            .entry(island.area)
            .or_insert([f64::MAX, f64::MAX, f64::MIN, f64::MIN]);
        b[0] = b[0].min(island.bounds[0]);
        b[1] = b[1].min(island.bounds[1]);
        b[2] = b[2].max(island.bounds[2]);
        b[3] = b[3].max(island.bounds[3]);
    }
    for (area, b) in &rust.boxes {
        boxes.entry(*area).or_insert(*b);
    }
    boxes
}

/// The tolerances of one board.
#[derive(Clone, Copy)]
struct Limits<'a> {
    /// The extent of each conduction area's fill.
    boxes: &'a BTreeMap<i64, [f64; 4]>,
    /// [`FLATTENING_UM`] in board units.
    flattening: f64,
}

/// True when `rust` is within the tolerances of `java`.
fn close(java: &Entry, rust: &Entry, limits: Limits) -> bool {
    let side = limits.boxes.get(&java.area).copied().unwrap_or(java.bounds);
    let width = (side[2] - side[0]).max(1.0);
    let height = (side[3] - side[1]).max(1.0);
    let sides = [width, height, width, height];
    let bounds_ok = (0..4).all(|i| {
        (java.bounds[i] - rust.bounds[i]).abs() <= (TOLERANCE * sides[i]).max(limits.flattening)
    });
    let b = java.bounds;
    let perimeter = 2.0 * ((b[2] - b[0]) + (b[3] - b[1]));
    let size_limit = (TOLERANCE * java.size.max(rust.size))
        .max(SLIVER)
        .max(perimeter * limits.flattening);
    bounds_ok && (java.size - rust.size).abs() <= size_limit
}

/// How far `rust`'s bounds are from `java`'s.
fn distance(java: &Entry, rust: &Entry) -> f64 {
    (0..4)
        .map(|i| (java.bounds[i] - rust.bounds[i]).abs())
        .sum()
}

/// The entries of a pairing that found no partner.
#[derive(Debug, Default)]
struct Pairing {
    matched: usize,
    /// Per Rust entry: paired with a Java entry.
    rust_paired: Vec<bool>,
    java_only: Vec<Entry>,
    rust_only: Vec<Entry>,
}

impl Pairing {
    fn differs(&self) -> bool {
        !self.java_only.is_empty() || !self.rust_only.is_empty()
    }
}

/// Pairs equal keys within the tolerances, in Java order, each Java entry
/// with the free Rust entry whose bounds are closest (the first on ties).
fn pair(java: &[Entry], rust: &[Entry], limits: Limits) -> Pairing {
    let mut pairing = Pairing::default();
    let mut free: Vec<bool> = vec![true; rust.len()];
    for j in java {
        let pick = (0..rust.len())
            .filter(|&i| free[i] && rust[i].key() == j.key() && close(j, &rust[i], limits))
            .min_by(|&a, &b| distance(j, &rust[a]).total_cmp(&distance(j, &rust[b])));
        match pick {
            Some(i) => {
                free[i] = false;
                pairing.matched += 1;
            }
            None => pairing.java_only.push(j.clone()),
        }
    }
    pairing.rust_only = rust
        .iter()
        .zip(&free)
        .filter(|(_, f)| **f)
        .map(|(r, _)| r.clone())
        .collect();
    pairing.rust_paired = free.iter().map(|f| !f).collect();
    pairing
}

fn describe(e: &Entry) -> String {
    format!(
        "area {} {} items {:?} island {} bounds [{:.0}, {:.0}, {:.0}, {:.0}] area {:.0}",
        e.area,
        e.kind,
        e.items,
        e.island,
        e.bounds[0],
        e.bounds[1],
        e.bounds[2],
        e.bounds[3],
        e.size
    )
}

fn report(board: &str, what: &str, pairing: &Pairing) -> String {
    let mut text = format!(
        "{board}: {what}: {} matched, {} only in Java, {} only in Rust",
        pairing.matched,
        pairing.java_only.len(),
        pairing.rust_only.len()
    );
    for e in pairing.java_only.iter().take(6) {
        text.push_str(&format!("\n  Java: {}", describe(e)));
    }
    for e in pairing.rust_only.iter().take(6) {
        text.push_str(&format!("\n  Rust: {}", describe(e)));
    }
    text
}

/// What one fixture contributes to the totals.
enum Verdict {
    /// No conduction areas on either side.
    Skipped,
    Compared {
        islands: usize,
        /// Java-only islands below SLIVER, below SPECK, merged.
        exempt: [usize; 3],
        fixed: usize,
        java: usize,
        allowlisted: bool,
    },
    Failed(Vec<String>),
}

/// Comparison 3 on an allowlisted board: the evidence holds and every
/// difference is the hole pairing.
fn check_allowed(
    allowed: &Allowed,
    java: &JavaBoard,
    limits: Limits,
    pairing: &Pairing,
    board: &str,
) -> Vec<String> {
    let mut failures = Vec::new();
    // A Java-only finding whose island ties with a Rust-only finding's.
    let swapped = |j: &Entry| {
        pairing
            .rust_only
            .iter()
            .any(|r| r.area == j.area && r.kind == j.kind && r.items.len() == j.items.len())
    };
    match allowed.evidence {
        MisplacedHole { area, island } => {
            match java
                .islands
                .iter()
                .find(|i| i.area == area && i.island == island)
            {
                Some(line) if line.hole_outside => {}
                Some(line) => failures.push(format!(
                    "{board}: allowlisted, but its evidence shows no misplaced hole: {}",
                    line.line
                )),
                None => failures.push(format!(
                    "{board}: allowlisted, but Java has no island {island} of area {area}"
                )),
            }
        }
        IslandOrder { area } => {
            if !pairing
                .java_only
                .iter()
                .any(|j| j.area == area && swapped(j))
            {
                failures.push(format!(
                    "{board}: allowlisted for the island order of area {area}, but no finding \
                     there is swapped"
                ));
            }
        }
    }
    if !pairing.differs() {
        failures.push(format!(
            "{board}: allowlisted, but its {} findings match Java's; remove it from ALLOWLIST",
            pairing.matched
        ));
    }
    let misplaced = |area: i64, island: i64| {
        java.islands
            .iter()
            .any(|i| i.area == area && i.island == island && i.hole_outside)
    };
    for j in &pairing.java_only {
        let primary = java.primaries.get(&j.area).copied().unwrap_or(-1);
        if !misplaced(j.area, j.island) && !misplaced(j.area, primary) && !swapped(j) {
            failures.push(format!(
                "{board}: only in Java, on an island without a misplaced hole, judged against \
                 a primary island without one, and not swapped by the island order: {}",
                describe(j)
            ));
        }
    }
    let in_fixed = pair(&java.fixed, &pairing.rust_only, limits);
    for r in &in_fixed.rust_only {
        failures.push(format!(
            "{board}: only in Rust, and not a fixed finding either: {}",
            describe(r)
        ));
    }
    failures
}

/// For a board that is not allowlisted: the entry that would allowlist it,
/// if the finding-by-finding proof of [`check_allowed`] holds.
fn suggestion(board: &str, java: &JavaBoard, limits: Limits, pairing: &Pairing) -> String {
    let evidence = pairing
        .java_only
        .iter()
        .find_map(|j| {
            java.islands
                .iter()
                .find(|i| i.area == j.area && i.island == j.island && i.hole_outside)
        })
        .or_else(|| java.islands.iter().find(|i| i.hole_outside));
    let entry = match (evidence, pairing.java_only.first()) {
        (Some(e), _) => Allowed {
            board: "",
            evidence: MisplacedHole {
                area: e.area,
                island: e.island,
            },
        },
        (None, Some(j)) => Allowed {
            board: "",
            evidence: IslandOrder { area: j.area },
        },
        (None, None) => return "\n  no finding only in Java".to_string(),
    };
    let proof = check_allowed(&entry, java, limits, pairing, board);
    let text = match entry.evidence {
        MisplacedHole { area, island } => {
            format!("MisplacedHole {{ area: {area}, island: {island} }}")
        }
        IslandOrder { area } => format!("IslandOrder {{ area: {area} }}"),
    };
    if proof.is_empty() {
        format!(
            "\n  explained; allowlist it with SUGGEST(\"{board}\", {text}){}",
            evidence
                .map(|e| format!(" ({})", e.line))
                .unwrap_or_default()
        )
    } else {
        format!("\n  not explained:\n  {}", proof.join("\n  "))
    }
}

fn check_row(row: &Row) -> Verdict {
    let board = row.board.as_str();
    let (java, rust) = match (java_board(row), rust_board(row)) {
        (None, None) => return Verdict::Skipped,
        (Some(j), Some(r)) => (j, r),
        (Some(_), None) => {
            return Verdict::Failed(vec![format!(
                "{board}: Java has conduction areas, Rust none"
            )]);
        }
        (None, Some(_)) => {
            return Verdict::Failed(vec![format!(
                "{board}: Rust has conduction areas but there is no Java output; generate it with {}",
                board_probe::probe_command("islands", row)
            )]);
        }
    };
    if let Some(exception) = &java.exception {
        return Verdict::Failed(vec![format!("{board}: Java threw {exception}")]);
    }
    let mut java_areas = java.areas.clone();
    java_areas.sort_unstable();
    let rust_areas: Vec<i64> = rust.boxes.keys().copied().collect();
    if java_areas != rust_areas {
        return Verdict::Failed(vec![format!(
            "{board}: conduction areas differ: Java {java_areas:?}, Rust {rust_areas:?}"
        )]);
    }
    let boxes = fill_boxes(&java, &rust);
    let limits = Limits {
        boxes: &boxes,
        flattening: rust.flattening,
    };
    let mut failures = Vec::new();
    // 1. Islands, up to slivers.
    let mut islands = pair(&java.fixed_islands, &rust.islands, limits);
    let paired = std::mem::take(&mut islands.rust_paired);
    let mut exempt = [0; 3];
    islands.java_only.retain(|j| {
        let bucket = if j.size < SLIVER {
            0
        } else if j.size < SPECK {
            1
        } else if merged(j, &rust, &paired) {
            2
        } else {
            return true;
        };
        exempt[bucket] += 1;
        false
    });
    islands.rust_only.retain(|e| e.size >= SLIVER);
    let expected: Vec<&ExpectedMismatch> = EXPECTED_ISLAND_MISMATCHES
        .iter()
        .filter(|e| e.board == board)
        .collect();
    if let Some(stale) = expected.iter().find(|x| {
        !islands
            .java_only
            .iter()
            .chain(&islands.rust_only)
            .any(|e| e.area == x.area)
    }) {
        failures.push(format!(
            "{board}: area {} no longer differs; remove it from EXPECTED_ISLAND_MISMATCHES",
            stale.area
        ));
    }
    let unexpected = |e: &Entry| !expected.iter().any(|x| x.area == e.area);
    islands.java_only.retain(unexpected);
    islands.rust_only.retain(unexpected);
    if islands.differs() {
        failures.push(report(board, "islands", &islands));
    }
    // 2. Findings on the fixed islands.
    let fixed = pair(&java.fixed, &rust.findings, limits);
    if fixed.differs() {
        failures.push(report(board, "fixed findings", &fixed));
    }
    // 3. Java's findings.
    let actual = pair(&java.findings, &rust.findings, limits);
    let allowed = ALLOWLIST.iter().find(|a| a.board == board);
    match allowed {
        Some(allowed) => failures.extend(check_allowed(allowed, &java, limits, &actual, board)),
        None if actual.differs() => {
            let mut text = report(board, "Java findings", &actual);
            text.push_str(&suggestion(board, &java, limits, &actual));
            failures.push(text);
        }
        None => {}
    }
    if !failures.is_empty() {
        return Verdict::Failed(failures);
    }
    Verdict::Compared {
        islands: islands.matched,
        exempt,
        fixed: fixed.matched,
        java: actual.matched,
        allowlisted: allowed.is_some(),
    }
}

#[test]
fn zone_islands_match_java() {
    let only = std::env::var("ISLANDS_BOARD").ok();
    let rows: Vec<Row> = board_probe::fixtures()
        .into_iter()
        .filter(|r| {
            only.as_ref()
                .is_none_or(|b| b.split(',').any(|b| b == r.board))
        })
        .collect();
    let next = AtomicUsize::new(0);
    let verdicts: Mutex<Vec<(usize, Verdict)>> = Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(row) = rows.get(index) else {
                        break;
                    };
                    let verdict = check_row(row);
                    verdicts.lock().expect("verdicts").push((index, verdict));
                }
            });
        }
    });
    let mut verdicts = verdicts.into_inner().expect("verdicts");
    verdicts.sort_by_key(|(index, _)| *index);
    let (mut compared, mut islands, mut fixed, mut java, mut allowlisted) = (0, 0, 0, 0, 0);
    let mut exempt = [0; 3];
    let mut failures = Vec::new();
    for (_, verdict) in verdicts {
        match verdict {
            Verdict::Skipped => {}
            Verdict::Compared {
                islands: i,
                exempt: e,
                fixed: f,
                java: j,
                allowlisted: a,
            } => {
                compared += 1;
                islands += i;
                for (total, count) in exempt.iter_mut().zip(e) {
                    *total += count;
                }
                fixed += f;
                java += j;
                allowlisted += usize::from(a);
            }
            Verdict::Failed(texts) => failures.extend(texts),
        }
    }
    println!(
        "zone islands: {compared} fixtures compared; matched {islands} islands, {fixed} fixed \
         findings, {java} Java findings; {allowlisted} boards allowlisted; Java islands only in \
         Java: {} below SLIVER, {} below SPECK, {} merged into a neighbour",
        exempt[0], exempt[1], exempt[2]
    );
    assert!(
        failures.is_empty(),
        "{} differences:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(compared > 0, "no fixture with conduction areas");
}

#[test]
fn allowlist_is_sorted_and_names_fixtures() {
    let rows = board_probe::fixtures();
    for allowed in ALLOWLIST {
        assert!(
            rows.iter().any(|r| r.board == allowed.board),
            "{} is not in fixtures.tsv",
            allowed.board
        );
    }
    let names: Vec<&str> = ALLOWLIST.iter().map(|a| a.board).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(names, sorted, "ALLOWLIST is sorted, one entry per board");
}
