//! Autoroute-probe support (A01.1): the Rust side of
//! `tools/java-reference/probes/autoroute/AutorouteProbe.java`.
//!
//! - [`ladder`] and [`kind_fixtures`] read `fixtures.tsv`;
//! - [`load`] loads a board as the Java probe does (J01's `route` mode:
//!   the default settings and the DSN's, through `prepare_job`, which is
//!   Java's headless load sequence), pinned to `router.profile = legacy`
//!   (N01.2) so a probe never compares `enhanced` with Java;
//! - [`sample`] is the connection sampler every A checklist reuses: nets in
//!   ascending order, the first connectable item (highest id first) with a
//!   non-empty unconnected set and no ignored nets, at most
//!   [`MAX_CONNECTIONS`], with the start and destination sets of
//!   `AutorouteConnectionRouter.route`;
//! - [`compare_probe`] compares the Rust lines with the committed Java
//!   output (or its digest) and reports the first difference with the five
//!   lines before it, the decoded doubles, and the `probe.sh` command that
//!   writes Java's full output.
//!
//! Later A checklists append helpers here; they don't change these.

#![allow(dead_code)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use freerouting::autoroute::AttemptState;
use freerouting::autoroute::control::{AutorouteControl, ControlError};
use freerouting::autoroute::probe::{self, ProbeValue, ProbeWriter};
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
use freerouting::settings::board::BoardRouterSettings;
use freerouting::settings::profile::{legacy_pin, require_legacy};
use freerouting::settings::{ResolveContext, RouterSettings, Source};

/// At most this many connections per board (the sampler, step 6).
pub const MAX_CONNECTIONS: usize = 200;

/// The core count the settings are resolved with. Only thread-count
/// defaults read it; nothing a probe prints does.
const CORES: usize = 8;

/// The repository root.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn tsv_row(first: &str) -> Vec<String> {
    let path = repo_root().join("tools/java-reference/probes/autoroute/fixtures.tsv");
    let text = std::fs::read_to_string(&path).expect("fixtures.tsv");
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .find(|cols| cols[0] == first)
        .map(|cols| cols[1..].iter().map(|c| c.to_string()).collect())
        .unwrap_or_else(|| panic!("no row {first} in fixtures.tsv"))
}

/// The fixture ladder: the DSN paths of the `ladder` row, smallest first.
pub fn ladder() -> Vec<String> {
    tsv_row("ladder")
}

/// The DSN paths of probe `kind` (`@ladder` expanded; `-` kept).
pub fn kind_fixtures(kind: &str) -> Vec<String> {
    let row = tsv_row(kind);
    if row.first().map(String::as_str) == Some("@ladder") {
        ladder()
    } else {
        row
    }
}

/// The board name of a DSN path: below `fixtures/`, without `.dsn`.
pub fn board_name(dsn: &str) -> String {
    dsn.strip_prefix("fixtures/")
        .unwrap_or(dsn)
        .trim_end_matches(".dsn")
        .to_string()
}

/// A board loaded as the Java probe loads it.
pub struct Loaded {
    pub board: RoutingBoard,
    pub settings: RouterSettings,
    pub router: BoardRouterSettings,
}

/// Loads `dsn` (a path below the repository root) with the default
/// settings, the DSN's settings and the adjacent rules file: `prepare_job`,
/// with the `legacy` profile pinned at the testing priority.
pub fn load(dsn: &str) -> Loaded {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("board");
    let mut input = JobInput::new(bytes, JobInput::design_name_of(file_name));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input.extra_sources.push((Source::Testing, legacy_pin()));
    let prepared = prepare_job(&input, &ResolveContext { cores: CORES })
        .unwrap_or_else(|e| panic!("{dsn}: {e}"));
    require_legacy(&prepared.settings).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    Loaded {
        board: prepared.board,
        settings: prepared.settings,
        router: prepared.board_settings,
    }
}

/// One sampled connection: the net, the item and the route sets.
#[derive(Clone, Debug)]
pub struct Conn {
    pub net: i32,
    pub item: ItemId,
    /// The net contains a plane.
    pub plane: bool,
    /// `Some(ConnectedToPlane)` when the router would skip it.
    pub state: Option<AttemptState>,
    pub start: BTreeSet<ItemId>,
    pub dest: BTreeSet<ItemId>,
}

/// The connection sampler (A01 decisions, steps 3 and 4).
pub fn sample(board: &RoutingBoard) -> Vec<Conn> {
    let mut result = Vec::new();
    let max_net = board.rules.nets.max_net_number();
    for net in 1..=max_net {
        if result.len() >= MAX_CONNECTIONS {
            break;
        }
        // PARITY: the first connectable item, highest id first.
        let chosen = board.connectable_items(net).into_iter().find(|id| {
            let item = board.item(*id).expect("connectable item");
            !board.has_ignored_nets(item) && !board.unconnected_set(*id, net).is_empty()
        });
        let Some(item) = chosen else {
            continue;
        };
        result.push(route_sets(board, net, item));
    }
    result
}

/// The start and destination sets of `AutorouteConnectionRouter.route`
/// (`:47-80`).
pub fn route_sets(board: &RoutingBoard, net: i32, item: ItemId) -> Conn {
    let plane = board
        .rules
        .nets
        .get(net)
        .is_some_and(|n| n.contains_plane());
    let unconnected = board.unconnected_set(item, net);
    let connected = board.connected_set(item, net, false);
    let mut conn = Conn {
        net,
        item,
        plane,
        state: None,
        start: BTreeSet::new(),
        dest: BTreeSet::new(),
    };
    if plane {
        let on_plane = connected
            .iter()
            .any(|id| board.item(*id).is_some_and(|i| i.is_conduction_area()));
        if on_plane {
            conn.state = Some(AttemptState::ConnectedToPlane);
            return conn;
        }
        let targets = board.power_plane_areas(unconnected.iter().copied());
        conn.dest = if targets.is_empty() {
            unconnected
        } else {
            targets
        };
        conn.start = connected;
    } else {
        conn.start = unconnected;
        conn.dest = connected;
    }
    conn
}

/// The control of net `net` as `AutorouteConnectionRouter` builds it: the
/// plane via costs for a plane net, the router's trace costs, and with
/// `ripup` the start ripup costs of pass 1.
pub fn router_control(
    loaded: &Loaded,
    net: i32,
    ripup: bool,
) -> Result<AutorouteControl, ControlError> {
    let board = &loaded.board;
    let plane = board
        .rules
        .nets
        .get(net)
        .is_some_and(|n| n.contains_plane());
    let via_costs = if plane {
        loaded.router.plane_via_costs()
    } else {
        loaded.router.via_costs()
    };
    let mut control = AutorouteControl::for_net(
        board,
        net,
        &loaded.router,
        via_costs,
        loaded.router.trace_costs(),
    )?;
    if ripup {
        control.ripup_allowed = true;
        // `getStartRipupCosts() * ripupPassNo` at pass 1.
        control.ripup_costs = loaded.router.start_ripup_costs();
    }
    control.remove_unconnected_vias = !loaded.settings.fanout.enabled;
    Ok(control)
}

/// The `sample` line every connection-based kind starts with.
pub fn sample_line(out: &mut ProbeWriter, board: &RoutingBoard, conns: &[Conn]) {
    out.record(&[
        ("stage", "sample".into()),
        ("maxConnections", MAX_CONNECTIONS.into()),
        ("maxNetNumber", board.rules.nets.max_net_number().into()),
        ("connections", conns.len().into()),
    ]);
}

/// Item ids as a probe list, in set order.
pub fn ids<'a>(items: impl IntoIterator<Item = &'a ItemId>) -> ProbeValue {
    ProbeValue::ints(items.into_iter().map(|id| i64::from(id.0)))
}

/// A probe writer with the Rust header of `kind` on `board`.
pub fn writer(kind: &str, board: &str) -> ProbeWriter {
    let mut out = ProbeWriter::new();
    out.header(kind, board, "rust");
    out
}

/// The `probe.sh` command that writes Java's full output of `kind` on
/// `board` (`dsn` is the fixture path or `-`).
pub fn probe_command(kind: &str, dsn: &str) -> String {
    format!(
        "REFERENCE_WORK_DIR=<work> TMPDIR=<scratch> tools/java-reference/probe.sh <pin-worktree> \
         tools/java-reference/probes/autoroute/AutorouteProbe.java {kind} '{dsn}' <out.jsonl>"
    )
}

fn write_lines(path: &Path, lines: &[String]) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("output directory");
    }
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    std::fs::write(path, text).expect("probe output");
}

/// The committed Java output of `kind` on `board`: the lines and whether
/// they are a digest.
pub fn java_output(kind: &str, board: &str) -> Option<(Vec<String>, bool)> {
    let base = repo_root().join("fixtures/reference/probe").join(kind);
    let read = |ext: &str| std::fs::read_to_string(base.join(format!("{board}.{ext}"))).ok();
    let lines = |text: String| text.lines().map(str::to_string).collect::<Vec<_>>();
    read("jsonl")
        .map(|t| (lines(t), false))
        .or_else(|| read("digest").map(|t| (lines(t), true)))
}

/// Compares the Rust lines of probe `kind` on `dsn` (header first) with the
/// committed Java output and returns a report of the first difference.
pub fn compare_probe(kind: &str, dsn: &str, rust_lines: &[String]) -> Result<(), String> {
    let board = if dsn == "-" {
        kind.to_string()
    } else {
        board_name(dsn)
    };
    let Some((java, is_digest)) = java_output(kind, &board) else {
        return Err(format!(
            "{kind}/{board}: no Java output; generate it with {}",
            probe_command(kind, dsn)
        ));
    };
    compare_lines(kind, &board, dsn, &java, is_digest, rust_lines)
}

/// [`compare_probe`] on given Java lines (`is_digest` if they are a
/// digest). On a difference the full Rust output is written to
/// `target/probe/<kind>/<board>.rust.jsonl`.
pub fn compare_lines(
    kind: &str,
    board: &str,
    dsn: &str,
    java: &[String],
    is_digest: bool,
    rust_lines: &[String],
) -> Result<(), String> {
    let rust_cmp = if is_digest {
        probe::digest(rust_lines)
    } else {
        rust_lines.to_vec()
    };
    let Some(index) = probe::first_difference(java, &rust_cmp) else {
        return Ok(());
    };
    let out = repo_root()
        .join("target/probe")
        .join(kind)
        .join(format!("{board}.rust.jsonl"));
    write_lines(&out, rust_lines);
    let context: Vec<String> = rust_cmp[index.saturating_sub(5)..index.min(rust_cmp.len())]
        .iter()
        .map(|l| format!("    {l}"))
        .collect();
    let java_line = java.get(index).map(String::as_str).unwrap_or("<end>");
    let rust_line = rust_cmp.get(index).map(String::as_str).unwrap_or("<end>");
    let digest_note = if is_digest {
        format!(
            "\n  The Java reference is a digest; write Java's full output with\n    {}\n  and diff it with the Rust output.",
            probe_command(kind, dsn)
        )
    } else {
        String::new()
    };
    Err(format!(
        "{kind}/{board}: line {} differs\n  Java: {java_line}\n        {}\n  Rust: {rust_line}\n        {}\n  before it:\n{}\n  Rust output: {}{digest_note}",
        index + 1,
        probe::explain_bits(java_line),
        probe::explain_bits(rust_line),
        context.join("\n"),
        out.display(),
    ))
}
