//! The A14 gate's machinery (`tests/autoroute_board_parity.rs`): the route references, the
//! route, the session comparison, the D8 owner-rule check, the per-board records and the
//! summary. Its module docs describe the gate.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use freerouting::autoroute::events::{
    CancelToken, ItemGeometry, NullEvents, Progress, RouterEvents, RunSummary,
};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::items::ItemId;
use freerouting::board::{BasicBoard, BoardSnapshot, RoutingBoard};
use freerouting::drc::json::JsonValue;
use freerouting::dsn::Session;
use freerouting::io::specctra::{SesStyle, import_session, write_ses};
use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
use freerouting::settings::board::BoardRouterSettings;
use freerouting::settings::{ResolveContext, RouterSettings, SettingsPatch, Source};

use crate::ses_diff::{STATISTIC_FIELDS, SesDiff, parse_session};

/// The first differing nets a record keeps.
const MAX_NETS: usize = 10;

/// The ±5 % tolerance of vias and trace length on `cutoff` boards (A00 question 2).
const TOLERANCE: f64 = 0.05;

// ---------------------------------------------------------------------------
// JSON access
// ---------------------------------------------------------------------------

fn field<'a>(value: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    match value {
        JsonValue::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn number(value: Option<&JsonValue>) -> Option<f64> {
    match value {
        Some(JsonValue::Number(text)) => text.parse().ok(),
        _ => None,
    }
}

fn int(value: Option<&JsonValue>) -> Option<i64> {
    number(value).map(|v| v as i64)
}

fn text(value: Option<&JsonValue>) -> Option<String> {
    match value {
        Some(JsonValue::String(s)) => Some(s.clone()),
        _ => None,
    }
}

fn array(value: Option<&JsonValue>) -> &[JsonValue] {
    match value {
        Some(JsonValue::Array(items)) => items,
        _ => &[],
    }
}

fn strings(value: Option<&JsonValue>) -> Vec<String> {
    array(value).iter().filter_map(|v| text(Some(v))).collect()
}

fn string_array(items: &[String]) -> JsonValue {
    JsonValue::Array(items.iter().cloned().map(JsonValue::String).collect())
}

fn float(value: f64) -> JsonValue {
    JsonValue::Number(format!("{value}"))
}

// ---------------------------------------------------------------------------
// References
// ---------------------------------------------------------------------------

/// A board's class (A14's decisions).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// Zero Java work-limit cut-offs: the sessions must match.
    Exact,
    /// One or more cut-offs: aggregates until B02's proof.
    Cutoff,
    /// Java did not complete normally: the outcome is compared.
    JavaFailure,
}

impl Class {
    fn name(self) -> &'static str {
        match self {
            Class::Exact => "exact",
            Class::Cutoff => "cutoff",
            Class::JavaFailure => "java-failure",
        }
    }

    fn parse(text: &str) -> Option<Class> {
        [Class::Exact, Class::Cutoff, Class::JavaFailure]
            .into_iter()
            .find(|c| c.name() == text)
    }
}

/// One Java route reference.
#[derive(Clone, Debug)]
pub struct Reference {
    /// The board name: the file stem, or the path below `fixtures/reference/route/` without
    /// `.json` for a board in a subdirectory (`PARITY_SUBDIRS`).
    pub name: String,
    /// The DSN, relative to the repository root.
    pub dsn: PathBuf,
    /// Java's `result`.
    pub result: String,
    /// `maxPasses`.
    pub max_passes: Option<i32>,
    /// `maxItems`.
    pub max_items: Option<i32>,
    /// `workLimitCutoffs.count`.
    pub cutoffs: usize,
    /// The per-pass score lines.
    pub passes: Vec<String>,
    /// `routeMillis`.
    pub java_ms: Option<i64>,
    /// The class.
    pub class: Class,
}

/// Every route reference, in name order. A board in `java_failures` is a `JavaFailure`.
pub fn references(root: &Path, java_failures: &[(&str, &str, &str)]) -> Vec<Reference> {
    let dir = root.join("fixtures/reference/route");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    // `PARITY_SUBDIRS=1`: the references one directory down too (`canary-pwr/`, the
    // multi-board issues), named by their path below `route/` (Q01.7).
    if std::env::var("PARITY_SUBDIRS").is_ok_and(|v| v == "1") {
        let mut subdirs: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_dir())
            .collect();
        subdirs.sort();
        for sub in subdirs {
            paths.extend(
                std::fs::read_dir(&sub)
                    .unwrap_or_else(|e| panic!("{}: {e}", sub.display()))
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.extension().is_some_and(|x| x == "json")),
            );
        }
    }
    paths.sort();
    paths
        .iter()
        .map(|path| {
            let json = std::fs::read_to_string(path)
                .ok()
                .and_then(|t| JsonValue::parse(&t).ok())
                .unwrap_or_else(|| panic!("{}: not JSON", path.display()));
            let name = path
                .strip_prefix(&dir)
                .unwrap_or(path)
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/");
            let fixture = text(field(&json, "fixture")).unwrap_or_else(|| format!("{name}.dsn"));
            let result = text(field(&json, "result")).unwrap_or_default();
            let cutoffs = field(&json, "workLimitCutoffs")
                .and_then(|c| int(field(c, "count")))
                .unwrap_or(0) as usize;
            let passes = array(field(&json, "passes"))
                .iter()
                .filter_map(|p| text(field(p, "score")))
                .collect();
            let class = if result != "completed" || java_failures.iter().any(|f| f.0 == name) {
                Class::JavaFailure
            } else if cutoffs > 0 {
                Class::Cutoff
            } else {
                Class::Exact
            };
            Reference {
                dsn: PathBuf::from("fixtures").join(fixture),
                result,
                max_passes: int(field(&json, "maxPasses")).map(|v| v as i32),
                max_items: int(field(&json, "maxItems")).map(|v| v as i32),
                cutoffs,
                passes,
                java_ms: int(field(&json, "routeMillis")),
                class,
                name,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Loading and routing
// ---------------------------------------------------------------------------

/// A board loaded in J01's route mode.
pub struct Loaded {
    pub board: RoutingBoard,
    pub settings: RouterSettings,
    pub router: BoardRouterSettings,
    pub design: String,
}

/// J01.3's `route` mode: the defaults, the DSN's settings and the adjacent rules file
/// (`prepare_job`), then the optimizer off and the reference's bounds at the testing
/// priority.
pub fn load(root: &Path, reference: &Reference) -> Result<Loaded, String> {
    let path = root.join(&reference.dsn);
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("board");
    let design = JobInput::design_name_of(file_name);
    let mut input = JobInput::new(bytes, design.clone());
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    let mut patch = SettingsPatch::default();
    patch.optimizer.enabled = Some(false);
    patch.autorouter.max_passes = reference.max_passes;
    patch.autorouter.max_items = reference.max_items;
    // P02.1: `PARITY_AUDIT=1` routes with the incremental search-tree audit.
    if std::env::var("PARITY_AUDIT").is_ok_and(|v| v == "1") {
        patch.debug.audit_incremental = Some(true);
    }
    input.extra_sources.push((Source::Testing, patch));
    let prepared = prepare_job(&input, &ResolveContext { cores: 8 }).map_err(|e| e.to_string())?;
    Ok(Loaded {
        board: prepared.board,
        settings: prepared.settings,
        router: prepared.board_settings,
        design,
    })
}

/// Which event sink the route uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sink {
    /// No listener.
    Noop,
    /// A listening sink that counts the events (the change journal on).
    Counters,
}

/// A listening sink that counts what it gets.
#[derive(Default)]
struct Counters {
    progress: Cell<usize>,
    deltas: Cell<usize>,
}

impl RouterEvents for Counters {
    fn is_listening(&self) -> bool {
        true
    }
    fn progress(&self, _: Progress) -> bool {
        self.progress.set(self.progress.get() + 1);
        true
    }
    fn delta(&self, _: Vec<ItemId>, _: Vec<ItemGeometry>) {
        self.deltas.set(self.deltas.get() + 1);
    }
    fn replaced(&self, _: BoardSnapshot) {}
    fn finished(&self, board: RoutingBoard, _: RunSummary) -> Option<RoutingBoard> {
        Some(board)
    }
}

/// What the route left.
struct Routed {
    board: RoutingBoard,
    passes: Vec<String>,
    last_mile: Vec<usize>,
    restores: usize,
    seconds: f64,
}

fn run(loaded: Loaded, sink: Sink) -> Result<Routed, String> {
    let counters = Counters::default();
    let events: &dyn RouterEvents = match sink {
        Sink::Noop => &NullEvents,
        Sink::Counters => &counters,
    };
    let job = PipelineJob {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel: Arc::new(CancelToken::new()),
        events,
        deadline: None,
        registry: None,
    };
    let started = Instant::now();
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| run_pipeline(loaded.board, &job)));
    let seconds = started.elapsed().as_secs_f64();
    let mut outcome = match result {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(failure)) => return Err(format!("pipeline_error: {}", failure.error)),
        Err(panic) => {
            let message = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            return Err(format!("panic: {message}"));
        }
    };
    let board = outcome.board.take().ok_or("no board came back")?;
    if let Some(audit) = &outcome.audit {
        eprintln!("  {}", audit.summary_line());
        if let Some((point, difference)) = &audit.first {
            return Err(format!("audit_difference: {point}: {difference}"));
        }
    }
    let last_mile = outcome
        .log
        .iter()
        .filter_map(|l| l.text.strip_prefix("Last-mile rip-up removed "))
        .filter_map(|rest| rest.split(' ').next()?.parse().ok())
        .collect();
    Ok(Routed {
        board,
        passes: outcome.passes.iter().map(|p| p.score_text()).collect(),
        last_mile,
        restores: outcome.restores,
        seconds,
    })
}

// ---------------------------------------------------------------------------
// The comparison
// ---------------------------------------------------------------------------

/// The `ses-diff` comparison of Java's session (a) with Rust's (b).
#[derive(Clone, Debug)]
pub struct Comparison {
    /// `SesDiff::equivalent`.
    pub equivalent: bool,
    /// How many nets differ.
    pub net_count: usize,
    /// The first [`MAX_NETS`] differing nets, in name order.
    pub differing_nets: Vec<String>,
    /// Java's statistics, in [`STATISTIC_FIELDS`] order.
    pub java: [f64; 5],
    /// Rust's statistics.
    pub rust: [f64; 5],
}

/// Compares `java` with `rust`, each imported into its own copy of `board`.
pub fn compare_sessions(board: &BasicBoard, java: &Session, rust: &Session) -> Comparison {
    let diff = SesDiff::new(board, java, rust);
    Comparison {
        equivalent: diff.equivalent(),
        net_count: diff.nets.len(),
        differing_nets: diff
            .nets
            .iter()
            .take(MAX_NETS)
            .map(|n| n.net.clone())
            .collect(),
        java: diff.a.statistics.values(),
        rust: diff.b.statistics.values(),
    }
}

// ---------------------------------------------------------------------------
// D8: no via in a pad, plane-net pads with their own via
// ---------------------------------------------------------------------------

/// Items reached from `start` through normal contacts, not through a conduction area.
fn reachable_without_plane(board: &BasicBoard, start: ItemId) -> BTreeSet<ItemId> {
    let mut reached = BTreeSet::new();
    let mut queue = vec![start];
    while let Some(id) = queue.pop() {
        let Some(item) = board.item(id) else {
            continue;
        };
        if item.is_conduction_area() || !reached.insert(id) {
            continue;
        }
        queue.extend(board.normal_contacts(id));
    }
    reached
}

fn smd_pins(board: &BasicBoard) -> Vec<ItemId> {
    board
        .items()
        .filter(|i| i.is_pin() && i.first_layer() == i.last_layer())
        .map(|i| i.id())
        .collect()
}

/// The D8 breaches `board` adds to `input` (the board as loaded): a new via without attach
/// on an SMD pad of its layer (`via x,y on pin #id`), and an SMD pad of a plane net that
/// reaches the plane and is chained by copper to another SMD pad of its net, where the
/// input did not chain them (`pins #a #b`), and a new trace on a power layer of a net that
/// has no plane on that layer (`trace of <net> on power layer <layer>`, Q01.7, Java
/// `7f56b062d`). The text uses positions, net names and the ids of input items, so Rust's
/// and Java's boards give comparable lines.
pub fn d8_breaches(board: &BasicBoard, input: &BasicBoard) -> Vec<String> {
    let input_ids: BTreeSet<ItemId> = input.items().map(|i| i.id()).collect();
    let pins = smd_pins(board);
    let pads: Vec<_> = pins
        .iter()
        .filter_map(|&id| {
            let pin = board.item(id)?;
            Some((id, pin.first_layer(), board.tile_shape(pin, 0)?))
        })
        .collect();
    let mut out = Vec::new();
    for via in board
        .items()
        .filter(|i| i.is_via() && !input_ids.contains(&i.id()))
    {
        if via.as_via().is_some_and(|v| v.attach_allowed) {
            continue;
        }
        let drill = via.drill().expect("a via has a drill");
        for k in 0..board.tile_shape_count(via) {
            let layer = drill.shape_layer(k);
            let Some(shape) = board.tile_shape(via, k) else {
                continue;
            };
            for (pin, _, pad) in pads.iter().filter(|p| p.1 == layer) {
                if shape.intersects_tile(pad) {
                    let c = drill.center.to_float();
                    out.push(format!("via {:.0},{:.0} on pin #{}", c.x, c.y, pin.0));
                }
            }
        }
    }
    for &pin_id in &pins {
        let pin = board.item(pin_id).expect("a pin");
        if pin.net_count() != 1 {
            continue;
        }
        let net = pin.net_number(0);
        if !board
            .rules
            .nets
            .get(net)
            .is_some_and(|n| n.contains_plane())
        {
            continue;
        }
        let on_plane = board
            .connected_set(pin_id, net, false)
            .iter()
            .any(|id| board.item(*id).is_some_and(|i| i.is_conduction_area()));
        if !on_plane {
            continue;
        }
        let reached = reachable_without_plane(board, pin_id);
        let before = reachable_without_plane(input, pin_id);
        for &other in pins.iter().filter(|&&o| o > pin_id) {
            if reached.contains(&other)
                && !before.contains(&other)
                && board.item(other).is_some_and(|o| o.contains_net(net))
            {
                out.push(format!("pins #{} #{}", pin_id.0, other.0));
            }
        }
    }
    out.extend(foreign_traces_on_power_layers(board, &input_ids));
    out
}

/// The traces not in the input that lie on a power layer without a plane of their net there
/// (Q01.7), in item order, one line per (net, layer).
fn foreign_traces_on_power_layers(board: &BasicBoard, input_ids: &BTreeSet<ItemId>) -> Vec<String> {
    let planes: Vec<&freerouting::board::items::Item> = board
        .conduction_areas()
        .into_iter()
        .filter_map(|id| board.item(id))
        .collect();
    let mut seen = BTreeSet::new();
    for trace in board
        .items()
        .filter(|i| i.is_trace() && !input_ids.contains(&i.id()))
    {
        let layer = trace.first_layer();
        if board.layer_structure.layers[layer as usize].is_signal {
            continue;
        }
        let own_plane = planes.iter().any(|plane| {
            plane.first_layer() == layer
                && trace.net_numbers().iter().any(|n| plane.contains_net(*n))
        });
        if own_plane {
            continue;
        }
        let net = trace.net_numbers().first().copied().unwrap_or(0);
        let net_name = board.rules.nets.get(net).map_or("?", |n| n.name.as_str());
        let layer_name = &board.layer_structure.layers[layer as usize].name;
        seen.insert(format!("trace of {net_name} on power layer {layer_name}"));
    }
    seen.into_iter().collect()
}

mod record;
mod summary;

pub use record::*;
pub use summary::*;
