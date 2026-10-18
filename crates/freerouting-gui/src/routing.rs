//! The autorouter job (G07.2) and the live redraw from its events (G07.4).
//!
//! **The job owns its board.** Start gives the router thread an `Arc` clone
//! of the session board; the thread takes it by move when nobody else holds
//! it and copies it otherwise (the session keeps its own `Arc`, so it copies
//! in practice), applies the settings to the copy
//! ([`prepare_board_for_routing`], the board half of the CLI's load
//! sequence) and runs [`run_pipeline`] with the settings of the dialog and
//! nothing else. A GUI run and a CLI run with equal settings write the same
//! SES (`tests/ui_routing.rs::gui_and_cli_write_identical_ses`): the
//! pipeline does not see the sink it sends to.
//!
//! **Events.** The router sends [`RouterEvent`]s over a bounded channel.
//! Each frame the GUI drains it without blocking ([`FrameEvents::drain`]),
//! merges the frame's deltas into one removed set and one added list (an
//! item added and removed in the same frame is dropped), patches the chunks
//! once ([`apply_frame_events`], `Loaded::patch_scene`), and keeps the newest
//! `Progress`. This is the only place progress is rate-limited (the frame
//! rate; a repaint is asked for every 33 ms while a job runs). A
//! `Replaced` event (a history restore, or the best board at the end) is
//! compared with the drawn scene item by item and only the differences are
//! patched ([`reconcile`]): the scene is never rebuilt in full. The
//! `Finished` board is compared the same way, so a delta the router did not
//! send could not leave the picture wrong ([`RoutingStats::reconcile_fixes`]
//! counts the differences it found; a correct run finds none).
//!
//! **What a delta draws.** A delta carries the geometry of its added items
//! ([`ItemGeometry`]). The router adds and changes traces and vias; those are
//! turned into scene items by [`GeometryMapper`], through the same shapes
//! and the same coordinate conversion as `Flattener`, so a patched chunk is
//! the one a fresh build would make. Any other kind of item is left to
//! [`reconcile`].
//!
//! **Cancel** is a full stop of the engine's cancel token (`stop_all`): the
//! pipeline finishes the current step and hands back the board it keeps (the
//! best one, D8). Dropping the job (the app closing) asks for the same and
//! does not wait.
//!
//! While a job runs, edits are refused (`Command::edits_board`) and the
//! session's board is the one the job started from; `Finished` replaces it.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::dialogs::autoroute_settings::Draft;
use eframe::egui;
use freerouting::autoroute::events::{
    CancelToken, EventSink, ItemGeometry, Phase, Progress, RouterEvent, RouterEvents, RunSummary,
};
use freerouting::autoroute::pipeline::{PipelineJob, PipelineOutcome, run_pipeline};
use freerouting::board::items::{BoardItemType, ItemId};
use freerouting::board::{BoardSnapshot, GeometryShape, RoutingBoard};
use freerouting::geometry::{PolylineShape, Shape, TileShape};
use freerouting::io::CoordinateTransform;
use freerouting::job::prepare::prepare_board_for_routing;
use freerouting::scoring::BoardStatistics;
use freerouting::settings::{ResolveContext, SettingsPatch};

use crate::scene::{Flattener, Geom, Item, P, Slot};
use crate::session::Loaded;

/// How many events the channel holds before the router waits for the GUI
/// (a `Progress` is dropped instead).
pub const EVENT_BOUND: usize = 512;

/// The stack of the router thread: the search recurses.
const STACK_BYTES: usize = 64 * 1024 * 1024;

/// How often a running job asks for a repaint (about 30 Hz).
pub const REPAINT_EVERY: Duration = Duration::from_millis(33);

/// The most events one frame reads, so a router that sends as fast as the GUI
/// reads can never hold the frame (and with it Cancel and the close button).
pub const EVENTS_PER_FRAME: usize = EVENT_BOUND;

/// How long a stop may take before closing the window again quits without
/// waiting for the job.
pub const STOP_GRACE: Duration = Duration::from_secs(3);

/// How a job thread ended.
pub enum JobEnd {
    /// `run_pipeline` returned. The board went over the channel with
    /// `Finished`; the statistics are those of that board.
    Finished {
        outcome: Box<PipelineOutcome>,
        statistics: Option<Box<BoardStatistics>>,
    },
    /// The job could not run (for example every layer is disabled); the
    /// board comes back as it was.
    Failed {
        message: String,
        board: Box<RoutingBoard>,
    },
}

/// A running job: its event channel, its stop switch and its thread.
pub struct RoutingJobHandle {
    pub events: Receiver<RouterEvent>,
    pub cancel: Arc<CancelToken>,
    pub thread: Option<JoinHandle<JobEnd>>,
}

/// A sink that takes the statistics of the final board before it goes to the
/// GUI, so the GUI thread never computes them.
struct StatisticsSink<'a> {
    inner: &'a EventSink,
    statistics: std::sync::Mutex<Option<Box<BoardStatistics>>>,
}

impl RouterEvents for StatisticsSink<'_> {
    fn is_listening(&self) -> bool {
        self.inner.is_listening()
    }

    fn progress(&self, progress: Progress) -> bool {
        self.inner.progress(progress)
    }

    fn delta(&self, removed: Vec<ItemId>, added: Vec<ItemGeometry>) {
        self.inner.delta(removed, added);
    }

    fn replaced(&self, snapshot: BoardSnapshot) {
        self.inner.replaced(snapshot);
    }

    fn finished(&self, board: RoutingBoard, summary: RunSummary) -> Option<RoutingBoard> {
        let statistics = Box::new(BoardStatistics::new(&board));
        *self
            .statistics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(statistics);
        self.inner.finished(board, summary)
    }
}

impl RoutingJobHandle {
    /// Starts the job on its own thread. `patch` is the dialog's settings
    /// (see `dialogs::autoroute_settings`).
    pub fn spawn(board: Arc<RoutingBoard>, patch: SettingsPatch, ctx: egui::Context) -> Self {
        let (sink, events) = EventSink::channel(EVENT_BOUND);
        let cancel = Arc::new(CancelToken::new());
        let token = Arc::clone(&cancel);
        let thread = std::thread::Builder::new()
            .name("autoroute".into())
            .stack_size(STACK_BYTES)
            .spawn(move || run_job_thread(board, &patch, &sink, token, &ctx))
            .expect("the router thread starts");
        RoutingJobHandle {
            events,
            cancel,
            thread: Some(thread),
        }
    }

    /// Asks the job to stop (the Cancel button, Esc, a click on the canvas).
    pub fn stop(&self) {
        self.cancel.stop_all();
    }

    /// Waits for the thread and returns how it ended; `None` if it
    /// panicked or was joined before.
    pub fn join(&mut self) -> Option<JobEnd> {
        self.thread.take()?.join().ok()
    }
}

impl Drop for RoutingJobHandle {
    fn drop(&mut self) {
        self.cancel.stop_all();
    }
}

fn run_job_thread(
    board: Arc<RoutingBoard>,
    patch: &SettingsPatch,
    sink: &EventSink,
    cancel: Arc<CancelToken>,
    ctx: &egui::Context,
) -> JobEnd {
    // The session keeps its own `Arc`, so this copies; a lone `Arc` moves.
    let mut board = Arc::try_unwrap(board).unwrap_or_else(|shared| (*shared).clone());
    let resolve = ResolveContext::from_machine();
    let ready = prepare_board_for_routing(&mut board, patch, &resolve);
    let statistics = StatisticsSink {
        inner: sink,
        statistics: std::sync::Mutex::new(None),
    };
    let job = PipelineJob {
        settings: &ready.settings,
        board_settings: &ready.board_settings,
        cancel,
        events: &statistics,
        // A job limit only requests a full stop (D2).
        deadline: ready.settings.deadlines().job,
        registry: None,
    };
    let end = match run_pipeline(board, &job) {
        Ok(outcome) => JobEnd::Finished {
            outcome: Box::new(outcome),
            statistics: statistics
                .statistics
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take(),
        },
        Err(failure) => JobEnd::Failed {
            message: failure.error.to_string(),
            board: failure.board,
        },
    };
    ctx.request_repaint();
    end
}

// ---------------------------------------------------------------------------
// Geometry of a delta
// ---------------------------------------------------------------------------

/// Turns the [`ItemGeometry`] of an added trace or via into scene items, the
/// way [`Flattener`] turns the engine item into them.
pub struct GeometryMapper {
    mm: f64,
    transform: CoordinateTransform,
    layer_count: usize,
    net_index: HashMap<i32, usize>,
}

impl GeometryMapper {
    pub fn new(board: &RoutingBoard) -> GeometryMapper {
        let b = board.basic();
        GeometryMapper {
            mm: b.communication.unit.micrometers() / 1000.0,
            transform: b.communication.coordinate_transform,
            layer_count: b.layer_structure.layers.len(),
            net_index: b
                .rules
                .nets
                .iter()
                .enumerate()
                .map(|(i, net)| (net.net_number, i))
                .collect(),
        }
    }

    fn pt(&self, p: freerouting::geometry::FloatPoint) -> P {
        let [x, y] = self.transform.board_to_dsn_point(p);
        P::new(x * self.mm, y * self.mm)
    }

    fn len(&self, v: f64) -> f64 {
        self.transform.board_to_dsn(v) * self.mm
    }

    fn slot(&self, layer: i32) -> Option<Slot> {
        usize::try_from(layer)
            .ok()
            .filter(|&l| l < self.layer_count)
            .map(Slot::Layer)
    }

    /// The scene geometry of a via or pad shape (`Flattener::shape_geom`).
    fn shape(&self, shape: &GeometryShape) -> Option<Geom> {
        let ring = |shape: Shape| {
            let outer: Vec<P> = shape
                .corner_approx_arr()
                .into_iter()
                .map(|p| self.pt(p))
                .collect();
            (outer.len() >= 3).then_some(Geom::Polygon {
                outer,
                holes: Vec::new(),
            })
        };
        match shape {
            GeometryShape::Circle { center, radius } => (*radius > 0).then(|| Geom::Circle {
                center: self.pt(center.to_float()),
                radius: self.len(f64::from(*radius)),
            }),
            GeometryShape::Box(b) => {
                ring(Shape::Polyline(PolylineShape::Tile(TileShape::IntBox(*b))))
            }
            GeometryShape::Octagon(o) => ring(Shape::Polyline(PolylineShape::Tile(
                TileShape::IntOctagon(*o),
            ))),
            GeometryShape::Polygon { outer, .. } => {
                let outer: Vec<P> = outer.iter().map(|p| self.pt(*p)).collect();
                (outer.len() >= 3).then_some(Geom::Polygon {
                    outer,
                    holes: Vec::new(),
                })
            }
            GeometryShape::Polyline { .. } => None,
        }
    }

    /// The scene items of `geometry`, in `Flattener`'s push order; `None` for
    /// a kind of item the router does not add (those are left to
    /// [`reconcile`]).
    pub fn items(&self, geometry: &ItemGeometry) -> Option<Vec<Item>> {
        let net = geometry
            .nets
            .first()
            .and_then(|n| self.net_index.get(n).copied());
        let id = geometry.id;
        let mut out = Vec::new();
        match geometry.kind {
            BoardItemType::Trace => {
                for layer_shape in &geometry.shapes {
                    let GeometryShape::Polyline {
                        corners,
                        half_width,
                    } = &layer_shape.shape
                    else {
                        continue;
                    };
                    let Some(slot) = self.slot(layer_shape.layer) else {
                        continue;
                    };
                    out.push(Item {
                        id,
                        kind: crate::scene::Kind::Trace,
                        slot,
                        geom: Geom::Path {
                            points: corners.iter().map(|p| self.pt(*p)).collect(),
                            width: self.len(2.0 * f64::from(*half_width)),
                        },
                        net,
                        owner: None,
                    });
                }
            }
            BoardItemType::Via => {
                for layer_shape in &geometry.shapes {
                    let (Some(slot), Some(geom)) =
                        (self.slot(layer_shape.layer), self.shape(&layer_shape.shape))
                    else {
                        continue;
                    };
                    out.push(Item {
                        id,
                        kind: crate::scene::Kind::Via,
                        slot,
                        geom,
                        net,
                        owner: None,
                    });
                }
            }
            _ => return None,
        }
        Some(out)
    }
}

// ---------------------------------------------------------------------------
// One frame's events
// ---------------------------------------------------------------------------

/// What the router said since the last frame, merged.
#[derive(Default)]
pub struct FrameEvents {
    /// The newest progress.
    pub progress: Option<Progress>,
    /// Ids to take out of the scene (an item added and removed within the
    /// frame is not among them).
    pub removed: BTreeSet<i32>,
    /// The geometry to put in, by id (the newest of an item that changed
    /// several times).
    pub added: BTreeMap<i32, ItemGeometry>,
    /// A board the router swapped in; deltas after it are relative to it.
    pub replaced: Option<Box<BoardSnapshot>>,
    /// The end of the run.
    pub finished: Option<(Box<RoutingBoard>, RunSummary)>,
    /// True once the sender is gone.
    pub disconnected: bool,
    /// True if the frame stopped reading at [`EVENTS_PER_FRAME`] with events
    /// still waiting.
    pub capped: bool,
    /// How many events were read.
    pub events: usize,
    pub progress_events: usize,
    pub delta_events: usize,
}

impl FrameEvents {
    /// Reads the events that are waiting, up to [`EVENTS_PER_FRAME`],
    /// without blocking.
    pub fn drain(events: &Receiver<RouterEvent>) -> FrameEvents {
        FrameEvents::drain_at_most(events, EVENTS_PER_FRAME)
    }

    /// [`drain`](Self::drain) with its limit.
    pub fn drain_at_most(events: &Receiver<RouterEvent>, limit: usize) -> FrameEvents {
        let mut frame = FrameEvents::default();
        loop {
            if frame.events >= limit {
                frame.capped = true;
                break;
            }
            match events.try_recv() {
                Ok(event) => frame.push(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    frame.disconnected = true;
                    break;
                }
            }
        }
        frame
    }

    /// Merges one event.
    pub fn push(&mut self, event: RouterEvent) {
        self.events += 1;
        match event {
            RouterEvent::Progress(progress) => {
                self.progress_events += 1;
                self.progress = Some(progress);
            }
            RouterEvent::Delta { removed, added } => {
                self.delta_events += 1;
                for id in removed {
                    // Added earlier in the frame and removed now: gone before
                    // it was ever drawn.
                    self.added.remove(&id.0);
                    self.removed.insert(id.0);
                }
                for geometry in added {
                    self.added.insert(geometry.id.0, geometry);
                }
            }
            RouterEvent::Replaced { snapshot } => {
                // The new board supersedes what came before it.
                self.removed.clear();
                self.added.clear();
                self.replaced = Some(snapshot);
            }
            RouterEvent::Finished { board, summary } => {
                self.finished = Some((board, summary));
            }
        }
    }

    /// True if there is nothing to draw.
    pub fn no_scene_change(&self) -> bool {
        self.removed.is_empty() && self.added.is_empty() && self.replaced.is_none()
    }
}

/// Counters of a job, for the tests and the status line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoutingStats {
    pub frames_with_events: u64,
    pub progress_events: u64,
    pub delta_events: u64,
    /// Frames that patched the chunks.
    pub patches: u64,
    /// Engine items taken out of the scene by deltas, and put in.
    pub items_removed: u64,
    pub items_added: u64,
    /// `Replaced` events handled.
    pub replaced: u64,
    /// Items the last `Finished` board differed in from the drawn scene
    /// (a correct run needs none).
    pub reconcile_fixes: u64,
    /// Items a `Replaced` board differed in from the drawn scene.
    pub replaced_fixes: u64,
    /// The slowest frame's event handling, in milliseconds.
    pub max_apply_ms: f64,
}

/// Applies a frame's merged events to the drawn board: a `Replaced` board
/// first, then the deltas, in one patch of the chunks.
pub fn apply_frame_events(
    loaded: &mut Loaded,
    mapper: &GeometryMapper,
    events: &mut FrameEvents,
    stats: &mut RoutingStats,
) {
    let started = Instant::now();
    stats.progress_events += events.progress_events as u64;
    stats.delta_events += events.delta_events as u64;
    if events.events > 0 {
        stats.frames_with_events += 1;
    }
    if let Some(snapshot) = events.replaced.take() {
        let board = snapshot.restore();
        stats.replaced_fixes += reconcile(loaded, &board) as u64;
        stats.replaced += 1;
    }
    if !(events.removed.is_empty() && events.added.is_empty()) {
        // A kind the mapper does not draw stays as it is.
        let mut unmapped = BTreeSet::new();
        let mut items = Vec::new();
        for (id, geometry) in &events.added {
            match mapper.items(geometry) {
                Some(mut mapped) => items.append(&mut mapped),
                None => {
                    unmapped.insert(*id);
                }
            }
        }
        let gone: Vec<ItemId> = events
            .removed
            .iter()
            .filter(|id| !unmapped.contains(id))
            .map(|&id| ItemId(id))
            .collect();
        stats.items_removed += gone.len() as u64;
        stats.items_added += (events.added.len() - unmapped.len()) as u64;
        if !(gone.is_empty() && items.is_empty()) {
            loaded.patch_scene(&gone, items);
            stats.patches += 1;
        }
        events.removed.clear();
        events.added.clear();
    }
    stats.max_apply_ms = stats
        .max_apply_ms
        .max(started.elapsed().as_secs_f64() * 1000.0);
}

/// Patches the scene to equal what `board` flattens to, touching only the
/// engine items whose scene items differ. Returns how many.
pub fn reconcile(loaded: &mut Loaded, board: &RoutingBoard) -> usize {
    let flattener = Flattener::new(board);
    let mut fresh: BTreeMap<i32, Vec<Item>> = BTreeMap::new();
    for item in board.basic().items() {
        let mut flat = Vec::new();
        flattener.flatten(item, &mut flat);
        if !flat.is_empty() {
            fresh.insert(item.id().0, flat);
        }
    }
    let mut current: HashMap<i32, Vec<&Item>> = HashMap::new();
    for item in &loaded.scene.items {
        current.entry(item.id.0).or_default().push(item);
    }
    let same =
        |a: &[&Item], b: &[Item]| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| *x == y);
    let mut gone: Vec<ItemId> = Vec::new();
    for (id, now) in &current {
        match fresh.get(id) {
            Some(want) if same(now, want) => {}
            _ => gone.push(ItemId(*id)),
        }
    }
    let mut added: Vec<Item> = Vec::new();
    for (id, want) in &fresh {
        if !current.get(id).is_some_and(|now| same(now, want)) {
            added.extend(want.iter().cloned());
        }
    }
    if gone.is_empty() && added.is_empty() {
        return 0;
    }
    let touched: BTreeSet<i32> = gone
        .iter()
        .map(|id| id.0)
        .chain(added.iter().map(|item| item.id.0))
        .collect();
    let fixes = touched.len();
    gone.sort_by_key(|id| id.0);
    loaded.patch_scene(&gone, added);
    fixes
}

// ---------------------------------------------------------------------------
// The running job
// ---------------------------------------------------------------------------

/// A running job and what the GUI shows of it.
pub struct RoutingRun {
    pub handle: RoutingJobHandle,
    mapper: GeometryMapper,
    pub started: Instant,
    /// The newest progress.
    pub progress: Option<Progress>,
    /// When the user asked to stop.
    pub stop_requested: Option<Instant>,
    pub stats: RoutingStats,
    /// The default trace half width on layer 0 in board units, for the
    /// optimizer's cross.
    pub half_width: i32,
    /// The board frame, for the overlay.
    pub frame: crate::overlays::Frame,
}

impl RoutingRun {
    pub fn new(handle: RoutingJobHandle, board: &RoutingBoard) -> RoutingRun {
        RoutingRun {
            handle,
            mapper: GeometryMapper::new(board),
            started: Instant::now(),
            progress: None,
            stop_requested: None,
            stats: RoutingStats::default(),
            half_width: {
                let rules = &board.basic().rules;
                rules
                    .default_net_class_id()
                    .map_or(0, |id| rules.net_classes.get(id).trace_half_width(0))
            },
            frame: crate::overlays::Frame::new(board.basic()),
        }
    }

    /// Asks the job to stop (once).
    pub fn stop(&mut self) {
        if self.stop_requested.is_none() {
            self.stop_requested = Some(Instant::now());
            self.handle.stop();
        }
    }

    /// True if a stop was asked for more than [`STOP_GRACE`] ago and the job
    /// has not ended.
    pub fn stop_is_slow(&self) -> bool {
        self.stop_requested
            .is_some_and(|asked| asked.elapsed() >= STOP_GRACE)
    }

    /// Reads this frame's events and patches the scene. Returns the merged
    /// events, whose `finished` the caller handles.
    pub fn poll(&mut self, loaded: &mut Loaded) -> FrameEvents {
        let mut events = FrameEvents::drain(&self.handle.events);
        if let Some(progress) = &mut events.progress {
            carry_figures(progress, self.progress.as_ref());
            self.progress = Some(progress.clone());
        }
        apply_frame_events(loaded, &self.mapper, &mut events, &mut self.stats);
        events
    }
}

/// The fanout computes the board's figures at the start and the end of a
/// pass only; the events between carry zeros. Keeps the last figures on show
/// (a real score is never 0.00).
pub fn carry_figures(progress: &mut Progress, last: Option<&Progress>) {
    if progress.phase == Phase::Fanout
        && progress.score == 0.0
        && progress.incomplete == 0
        && progress.violations == 0
        && let Some(last) = last
    {
        progress.score = last.score;
        progress.incomplete = last.incomplete;
        progress.violations = last.violations;
    }
}

/// What a finished job leaves for the summary window.
#[derive(Clone, Debug, PartialEq)]
pub struct RoutingSummaryData {
    pub total_nets: i32,
    pub incomplete: usize,
    pub violations: usize,
    /// The largest clearance violation, in millimetres.
    pub max_violation_mm: f64,
    pub vias: usize,
    /// The total trace length of the board, in millimetres.
    pub trace_length_mm: f64,
    pub duration_seconds: f64,
    pub score: f32,
    pub interrupted: bool,
}

impl RoutingSummaryData {
    /// From the statistics of the returned board and the run's summary.
    pub fn new(
        statistics: &BoardStatistics,
        summary: &RunSummary,
        duration: Duration,
    ) -> RoutingSummaryData {
        use freerouting::autoroute::events::CompletionStatus;
        RoutingSummaryData {
            total_nets: statistics.nets.total_count,
            incomplete: statistics.connections.incomplete_count,
            violations: statistics.clearance_violations.total_count,
            max_violation_mm: statistics.clearance_violations.max_violation_um / 1000.0,
            vias: statistics.items.via_count,
            trace_length_mm: f64::from(statistics.traces.total_length_mm),
            duration_seconds: duration.as_secs_f64(),
            score: summary.score,
            interrupted: summary.status != CompletionStatus::Completed,
        }
    }
}

// ---------------------------------------------------------------------------
// The app side
// ---------------------------------------------------------------------------

/// The autorouter's state in the app, apart from the running job.
#[derive(Default)]
pub struct Autoroute {
    /// What the CLI would start with for the open board, before the user's
    /// changes: the baseline of [`Draft::overrides_against`].
    pub baseline: Option<SettingsPatch>,
    /// The settings the next job starts with.
    pub applied: Option<SettingsPatch>,
    /// The open dialog's draft.
    pub dialog: Option<Draft>,
    /// The last job's summary and whether its window is open.
    pub summary: Option<RoutingSummaryData>,
    pub summary_open: bool,
    /// Counters of the last job that ended.
    pub last_stats: Option<RoutingStats>,
    /// The last job's end, for the tests.
    pub last_run: Option<RunSummary>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_reads_at_most_its_limit() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(16);
        for _ in 0..10 {
            sender
                .send(RouterEvent::Progress(Progress::default()))
                .unwrap();
        }
        let first = FrameEvents::drain_at_most(&receiver, 4);
        assert_eq!((first.events, first.capped), (4, true));
        let rest = FrameEvents::drain_at_most(&receiver, 4);
        assert_eq!((rest.events, rest.capped), (4, true));
        let last = FrameEvents::drain_at_most(&receiver, 4);
        assert_eq!((last.events, last.capped), (2, false));
        drop(sender);
        assert!(FrameEvents::drain(&receiver).disconnected);
    }

    #[test]
    fn fanout_events_without_figures_keep_the_last_ones() {
        let last = Progress {
            phase: Phase::Fanout,
            score: 812.5,
            incomplete: 4,
            violations: 1,
            ..Progress::default()
        };
        let mut between = Progress {
            phase: Phase::Fanout,
            routed: 3,
            ..Progress::default()
        };
        carry_figures(&mut between, Some(&last));
        assert_eq!(
            (between.score, between.incomplete, between.violations),
            (812.5, 4, 1)
        );
        assert_eq!(between.routed, 3);
        // An event with figures, or of another phase, is left alone.
        let mut with = Progress {
            phase: Phase::Fanout,
            score: 900.0,
            ..Progress::default()
        };
        carry_figures(&mut with, Some(&last));
        assert_eq!(with.score, 900.0);
        let mut routing = Progress::default();
        carry_figures(&mut routing, Some(&last));
        assert_eq!(routing.score, 0.0);
    }
}
