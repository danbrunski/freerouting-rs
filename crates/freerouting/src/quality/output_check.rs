//! Q20: the router checks its own result against the board's rules before
//! it is saved (`router.output_check = off | report | repair`).
//!
//! Not Java. Decided 2026-10-20: routing never breaks the board's
//! manufacturing rules, so routed copper that breaks one is never saved
//! silently. `repair` is profile `enhanced`'s value; `legacy` leaves the
//! setting unset, which is `off`, and nothing here runs.
//!
//! **The check** is [`check_board_rules`] with the limits the router routed
//! under ([`BoardRuleLimits::of`]): clearance, copper-to-edge clearance,
//! keepouts, track width, via diameter, drill and annular ring, hole
//! clearance, hole to hole (Q22.11), vias in pads, dangling traces and vias (see
//! [`crate::drc::board_rules`]).
//!
//! **When.** [`run_pipeline`](crate::autoroute::pipeline::run_pipeline)
//! starts an [`OutputChecker`] on the board as loaded, before the fanout:
//! it takes the input copper ([`InputCopper`]) and logs, once, the breaches
//! the input already has ("the board already has N violations: ..."; Q20.4).
//! It finishes after the optimizer, the board stages of that phase and Q16's
//! stub cleanup, also after a stop, and before the board is handed over to
//! be written.
//!
//! **Modes.**
//! - `report` logs and counts the routed breaches and changes nothing.
//! - `repair`, in rounds that are all deterministic:
//!   1. removes the traces and vias the router made of every routed breach
//!      ([`BoardRuleViolation::routed_items`], ascending ids), then the
//!      router's stubs that removal leaves ([`remove_stubs`], keeping the
//!      input copper);
//!   2. retries, once, the incomplete connections of the nets it removed
//!      copper of: one pass of the job's own stages over those nets'
//!      items, in ascending (net, item) order, with rip-up off (it only adds
//!      copper), the pass's work budget per connection and the same floors
//!      (the job's settings). No retry after a full stop of the job
//!      (deadline, cancel);
//!   3. checks again and removes what still breaches, round after round
//!      until a round finds no routed breach (each round removes at least
//!      one item, so the rounds end); those connections stay unrouted.
//!
//! **Input copper is never changed** (Q20.4): a breach whose traces and vias
//! are all input copper is counted as input and never removed, and the
//! stub removal keeps every piece of input copper.
//!
//! **Nothing found, nothing changed.** When the first check finds no routed
//! breach the board is untouched, so the session is the one `off` writes.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use crate::autoroute::batch::batch_loop::PassExecutor;
use crate::autoroute::batch::pass::PassStages;
use crate::autoroute::events::{CancelToken, Phase};
use crate::autoroute::stages::{
    ConnectionOutcome, ConnectionParams, ConnectionRouter, NetOrdering, PassReport, Pipeline,
    RipupPolicy, RouteTask, StageCtx, UnknownStage,
};
use crate::board::items::{Item, ItemId};
use crate::board::{BasicBoard, RoutingBoard};
use crate::drc::board_rules::{
    BoardRule, BoardRuleCounts, BoardRuleLimits, BoardRuleViolation, InputCopper, Origin,
    check_board_rules,
};
use crate::drc::json::JsonValue;
use crate::quality::stubs::remove_stubs;
use crate::settings::RouterSettings;
use crate::settings::board::BoardRouterSettings;

/// The settings key.
pub const SETTINGS_KEY: &str = "router.output_check";

/// The values, in the order the error message names them.
pub const MODES: [&str; 3] = ["off", "report", "repair"];

/// What the check does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Nothing (Java).
    #[default]
    Off,
    /// Logs and counts.
    Report,
    /// Removes, retries once, removes what still breaches.
    Repair,
}

impl Mode {
    /// The mode of a setting value; unset is [`Mode::Off`]. Fails on any
    /// other value, naming it.
    pub fn parse(value: Option<&str>) -> Result<Mode, String> {
        match value.map(str::trim) {
            None | Some("") | Some("off") => Ok(Mode::Off),
            Some("report") => Ok(Mode::Report),
            Some("repair") => Ok(Mode::Repair),
            Some(other) => Err(format!(
                "{SETTINGS_KEY}: unknown value '{other}' (expected 'off', 'report' or 'repair')"
            )),
        }
    }

    /// The setting value.
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Report => "report",
            Mode::Repair => "repair",
        }
    }

    /// The mode of `settings`, [`Mode::Off`] for a bad value (which
    /// [`check_settings`] refuses before routing).
    pub fn of(settings: &RouterSettings) -> Mode {
        Mode::parse(settings.output_check.as_deref()).unwrap_or_default()
    }
}

/// Refuses an unknown `router.output_check` value before routing.
pub fn check_settings(settings: &RouterSettings) -> Result<(), UnknownStage> {
    match Mode::parse(settings.output_check.as_deref()) {
        Ok(_) => Ok(()),
        Err(_) => Err(UnknownStage {
            key: SETTINGS_KEY,
            id: settings.output_check.clone().unwrap_or_default(),
            known: MODES.to_vec(),
        }),
    }
}

/// What the check did. Equality leaves out the two times, so two equal runs
/// give equal `OutputCheck`s (and `RunSummary`s).
#[derive(Clone, Debug, Default)]
pub struct OutputCheck {
    /// The mode.
    pub mode: Mode,
    /// Routed traces and vias on the board the check was handed.
    pub checked: usize,
    /// Routed breaches the first check found.
    pub found: usize,
    /// Routed traces and vias removed, in all rounds.
    pub removed: usize,
    /// Incomplete connections of the affected nets that the retry pass
    /// tried again.
    pub retried: usize,
    /// Of them, the ones complete after the retry pass.
    pub repaired: usize,
    /// Incomplete connections of the affected nets at the end.
    pub left_open: usize,
    /// Routed breaches per rule, at the first check.
    pub by_rule: BTreeMap<BoardRule, usize>,
    /// Routed breaches at the end (0 after a repair).
    pub remaining: usize,
    /// Breaches of the input copper per rule, at load (Q20.4).
    pub input: BTreeMap<BoardRule, usize>,
    /// Seconds the checks took (only reported; it decides nothing).
    pub check_seconds: f64,
    /// Seconds the whole finish took, retry included.
    pub seconds: f64,
    /// The job log lines.
    pub log: Vec<String>,
}

impl PartialEq for OutputCheck {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            mode,
            checked,
            found,
            removed,
            retried,
            repaired,
            left_open,
            by_rule,
            remaining,
            input,
            check_seconds: _,
            seconds: _,
            log,
        } = self;
        (
            mode, checked, found, removed, retried, repaired, left_open, by_rule, remaining, input,
            log,
        ) == (
            &other.mode,
            &other.checked,
            &other.found,
            &other.removed,
            &other.retried,
            &other.repaired,
            &other.left_open,
            &other.by_rule,
            &other.remaining,
            &other.input,
            &other.log,
        )
    }
}

impl OutputCheck {
    /// True if the check changed the board.
    pub fn changed(&self) -> bool {
        self.removed > 0
    }

    /// The `by_rule` counts keyed by rule name.
    pub fn by_rule_names(&self) -> BTreeMap<&'static str, usize> {
        self.by_rule.iter().map(|(r, n)| (r.name(), *n)).collect()
    }

    /// The result JSON's `output_check` object: `mode`, `checked`, `found`,
    /// `removed`, `retried`, `repaired`, `left_open`, `remaining`,
    /// `by_rule` (routed breaches per rule name at the first check) and
    /// `input` (the input's breaches per rule name). No time, here or in the
    /// run log, so two equal runs write equal logs and manifests (but for the
    /// manifest's durations); the times are `check_seconds` and `seconds`.
    pub fn to_json(&self) -> JsonValue {
        let n = |v: usize| JsonValue::int(i64::try_from(v).unwrap_or(i64::MAX));
        let rules = |map: &BTreeMap<BoardRule, usize>| {
            JsonValue::Object(
                map.iter()
                    .map(|(rule, count)| (rule.name().to_string(), n(*count)))
                    .collect(),
            )
        };
        JsonValue::Object(vec![
            ("mode".into(), JsonValue::string(self.mode.as_str())),
            ("checked".into(), n(self.checked)),
            ("found".into(), n(self.found)),
            ("removed".into(), n(self.removed)),
            ("retried".into(), n(self.retried)),
            ("repaired".into(), n(self.repaired)),
            ("left_open".into(), n(self.left_open)),
            ("remaining".into(), n(self.remaining)),
            ("by_rule".into(), rules(&self.by_rule)),
            ("input".into(), rules(&self.input)),
        ])
    }
}

/// The check of one job, started on the board as loaded.
#[derive(Clone, Debug)]
pub struct OutputChecker {
    mode: Mode,
    input: InputCopper,
    limits: BoardRuleLimits,
    input_counts: BTreeMap<BoardRule, usize>,
    load_log: Vec<String>,
}

/// The retry `OutputChecker::finish` runs: it routes the incomplete
/// connections of the given nets once.
pub type Retry<'a> = &'a mut dyn FnMut(&mut RoutingBoard, &BTreeSet<i32>);

impl OutputChecker {
    /// The checker of a job on `board` (as loaded), if `settings` turn the
    /// check on; checks the input copper once and keeps its log line.
    pub fn start(
        board: &BasicBoard,
        settings: &RouterSettings,
        board_settings: &BoardRouterSettings,
    ) -> Option<OutputChecker> {
        let mode = Mode::of(settings);
        if mode == Mode::Off {
            return None;
        }
        let input = InputCopper::of(board);
        let limits =
            BoardRuleLimits::of(board, board_settings).with_mask_expansions(board, settings);
        let found = check_board_rules(board, &limits, &input);
        let counts = BoardRuleCounts::of(&found);
        let total = counts.input_total();
        let load_log = vec![if total == 0 {
            "Output check: the board as loaded breaks no board rule.".to_string()
        } else {
            format!(
                "Output check: the board already has {total} violation(s): {}; input copper is \
                 reported, never changed.",
                BoardRuleCounts::text(&counts.input)
            )
        }];
        Some(OutputChecker {
            mode,
            input,
            limits,
            input_counts: counts.input,
            load_log,
        })
    }

    /// The mode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The log line of the input check.
    pub fn load_log(&self) -> &[String] {
        &self.load_log
    }

    /// The limits the check holds copper to.
    pub fn limits(&self) -> &BoardRuleLimits {
        &self.limits
    }

    /// The routed breaches of `board`.
    pub fn routed_breaches(&self, board: &BasicBoard) -> Vec<BoardRuleViolation> {
        check_board_rules(board, &self.limits, &self.input)
            .into_iter()
            .filter(|v| v.origin == Origin::Routed)
            .collect()
    }

    /// Checks `board` and, in `repair`, repairs it (module docs). `retry`
    /// routes the incomplete connections of the nets it is given once, or
    /// is `None` when the job was stopped.
    pub fn finish(&self, board: &mut RoutingBoard, retry: Option<Retry<'_>>) -> OutputCheck {
        let start = Instant::now();
        let mut check_seconds = 0.0;
        let mut timed = |board: &RoutingBoard| {
            let t = Instant::now();
            let found = self.routed_breaches(board);
            check_seconds += t.elapsed().as_secs_f64();
            found
        };
        let checked = board
            .items()
            .filter(|i| (i.is_trace() || i.is_via()) && !self.input.contains(i))
            .count();
        let found = timed(board);
        let mut outcome = OutputCheck {
            mode: self.mode,
            checked,
            found: found.len(),
            by_rule: BoardRuleCounts::of(&found).routed,
            input: self.input_counts.clone(),
            ..OutputCheck::default()
        };
        let mut remaining = found;
        if self.mode == Mode::Repair && !remaining.is_empty() {
            let (removed, nets) = self.remove(board, &remaining);
            outcome.removed += removed;
            let open_before = incomplete_of(board, &nets);
            if let Some(retry) = retry {
                outcome.retried = open_before;
                retry(board, &nets);
                let open_after = incomplete_of(board, &nets);
                outcome.repaired = open_before.saturating_sub(open_after);
            }
            loop {
                remaining = timed(board);
                if remaining.is_empty() {
                    break;
                }
                let (removed, _) = self.remove(board, &remaining);
                if removed == 0 {
                    break;
                }
                outcome.removed += removed;
            }
            outcome.left_open = incomplete_of(board, &nets);
        }
        outcome.remaining = remaining.len();
        outcome.check_seconds = check_seconds;
        outcome.seconds = start.elapsed().as_secs_f64();
        outcome.log = log_lines(&outcome);
        outcome
    }

    /// Removes the routed items of `breaches` and the router's stubs that
    /// leaves; returns the count and the nets of the removed items.
    fn remove(
        &self,
        board: &mut RoutingBoard,
        breaches: &[BoardRuleViolation],
    ) -> (usize, BTreeSet<i32>) {
        let ids: BTreeSet<ItemId> = breaches
            .iter()
            .flat_map(|v| v.routed_items.iter().copied())
            .collect();
        let mut nets = BTreeSet::new();
        let mut removed = 0;
        for id in ids {
            let Some(item) = board.item(id) else {
                continue;
            };
            if !item.is_routable() || board.is_deletion_forbidden(item) || self.input.contains(item)
            {
                continue;
            }
            let item_nets = item.net_numbers().to_vec();
            if board.remove_item(id) {
                removed += 1;
                nets.extend(item_nets);
            }
        }
        if removed > 0 {
            let keep: BTreeSet<ItemId> = board
                .items()
                .filter(|i| self.input.contains(i))
                .map(Item::id)
                .collect();
            removed += remove_stubs(board, &keep).removed();
        }
        (removed, nets)
    }
}

/// The incomplete connections of `nets` on `board`.
fn incomplete_of(board: &BasicBoard, nets: &BTreeSet<i32>) -> usize {
    let ledger = board.routing_ledger();
    nets.iter()
        .map(|net| ledger.net_incomplete_count(board, *net))
        .sum()
}

/// The job log: the totals, one line per rule, and what the repair did.
fn log_lines(o: &OutputCheck) -> Vec<String> {
    let mut lines = vec![format!(
        "Output check ({}): {} routed trace(s) and via(s) checked, {} breach(es) of the board's \
         rules found.",
        o.mode.as_str(),
        o.checked,
        o.found
    )];
    for (rule, n) in &o.by_rule {
        lines.push(format!("Output check: {} {n}", rule.name()));
    }
    let tail = match o.mode {
        Mode::Repair => format!(
            "Output check: removed {} trace(s) and via(s), retried {} connection(s), repaired {}, \
             left open {}; {} routed breach(es) remain.",
            o.removed, o.retried, o.repaired, o.left_open, o.remaining
        ),
        _ => format!(
            "Output check: {} routed breach(es) kept (report only).",
            o.remaining
        ),
    };
    lines.push(tail);
    lines
}

/// The ordering of the retry pass: the job's own items, only those of the
/// affected nets, in ascending (net, item) order.
struct NetFilter<'a> {
    inner: &'a mut dyn NetOrdering,
    nets: &'a BTreeSet<i32>,
}

impl NetOrdering for NetFilter<'_> {
    fn id(&self) -> &'static str {
        self.inner.id()
    }

    fn select(&mut self, board: &RoutingBoard) -> Vec<RouteTask> {
        let mut tasks: Vec<RouteTask> = self
            .inner
            .select(board)
            .into_iter()
            .filter(|t| {
                board
                    .item(t.item)
                    .is_some_and(|item| item.net_numbers().iter().any(|n| self.nets.contains(n)))
            })
            .collect();
        tasks.sort_by_key(|t| (t.net, t.item));
        tasks
    }

    fn order(
        &mut self,
        tasks: Vec<RouteTask>,
        _board: &RoutingBoard,
        _pass: u32,
    ) -> Vec<RouteTask> {
        tasks
    }

    fn on_pass_end(&mut self, report: &PassReport) {
        self.inner.on_pass_end(report);
    }

    fn reset(&mut self) {
        self.inner.reset();
    }
}

/// The connection router of the retry: the job's, with rip-up off.
struct AddOnlyRouter<'a> {
    inner: &'a mut dyn ConnectionRouter,
    job: &'a CancelToken,
}

impl ConnectionRouter for AddOnlyRouter<'_> {
    fn id(&self) -> &'static str {
        self.inner.id()
    }

    fn route(
        &mut self,
        board: &mut RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        ctx: &mut StageCtx,
    ) -> ConnectionOutcome {
        // A full stop of the job (deadline, cancel) only stops work.
        if self.job.is_stop_requested() {
            ctx.cancel.stop_all();
        }
        let params = ConnectionParams {
            ripup_allowed: false,
            ..p.clone()
        };
        self.inner.route(board, task, &params, ctx)
    }
}

/// The ripup policy of the retry: the job's costs and strict check, no
/// repeated-failure ripup.
struct AddOnlyRipup<'a> {
    inner: &'a mut dyn RipupPolicy,
}

impl RipupPolicy for AddOnlyRipup<'_> {
    fn id(&self) -> &'static str {
        self.inner.id()
    }

    fn ripup_cost(&self, phase: Phase, pass: u32, start_ripup_costs: i32) -> i32 {
        self.inner.ripup_cost(phase, pass, start_ripup_costs)
    }

    fn strict_check(&self, pass: u32, strict_drc: bool) -> bool {
        self.inner.strict_check(pass, strict_drc)
    }

    fn after_failure(&mut self, _board: &mut RoutingBoard, _task: &RouteTask, _failures: u32) {}
}

/// The retry pass: pass 1 of `executor` with `pipeline`'s ordering (only
/// `nets`' items), connection router (rip-up off) and ripup policy (no
/// repeated-failure ripup), on a stop token of its own (the loop has asked
/// for a router stop when it ended) that a full stop of `ctx`'s job reaches
/// before each connection. Returns the connections it routed.
pub fn retry_pass(
    board: &mut RoutingBoard,
    nets: &BTreeSet<i32>,
    pipeline: &mut Pipeline,
    executor: &mut dyn PassExecutor,
    ctx: &StageCtx,
) -> usize {
    let local = CancelToken::new();
    let mut local_ctx = StageCtx {
        settings: ctx.settings,
        board_settings: ctx.board_settings,
        cancel: &local,
        events: ctx.events,
    };
    pipeline.ordering.reset();
    let mut ordering = NetFilter {
        inner: pipeline.ordering.as_mut(),
        nets,
    };
    let mut router = AddOnlyRouter {
        inner: pipeline.connection_router.as_mut(),
        job: ctx.cancel,
    };
    let mut ripup = AddOnlyRipup {
        inner: pipeline.ripup_policy.as_mut(),
    };
    let mut stages = PassStages {
        ordering: &mut ordering,
        router: &mut router,
        ripup: &mut ripup,
    };
    executor
        .run_pass(board, 1, &mut stages, &mut local_ctx)
        .routed
}
