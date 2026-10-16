//! Background analysis of the open board: the ratsnest (airlines and the
//! incomplete count), the design rules check and the lists built from them.
//!
//! The engine's `DesignRulesChecker` borrows the board, so a worker thread
//! owns an `Arc` clone of the session board for one run (G01.2). A run is
//! tagged with the [`Tag`] it was started for. A result whose tag is no
//! longer the session's is kept out of the UI ([`Analysis::ratsnest`] and
//! [`Analysis::drc`] return `None`), and a new run starts as soon as the
//! worker is free. There is at most one worker at a time.
//!
//! A run sends two messages: the ratsnest first (cheap, so the count and the
//! airlines appear quickly), then the rest of the check.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use eframe::egui;
use freerouting::board::RoutingBoard;
use freerouting::drc::{
    AirLine, ClearanceViolation, DesignRulesChecker, UnconnectedItems, ZoneIslandViolation,
};

use crate::session::Session;

/// The state of the session a result was computed for.
///
/// `revision` counts inserts and removes of the board, `edit_generation`
/// every command that mutated the session (the board revision misses rule
/// edits and `delete_all_traces_and_vias`). `load` tells boards apart that
/// share both counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tag {
    pub load: u64,
    pub revision: u64,
    pub edit_generation: u64,
}

/// The ratsnest of a board: Java's `RatsNest` data.
#[derive(Clone, Debug, Default)]
pub struct Ratsnest {
    /// Every airline of every net, in `all_airlines()` order.
    pub airlines: Vec<AirLine>,
    /// `incomplete_count()`.
    pub incomplete_count: usize,
    /// The nets with a length violation, ascending by net number, with the
    /// signed violation (positive: too long, negative: too short).
    pub length_violations: Vec<(i32, f64)>,
}

/// The rest of the design rules check.
#[derive(Clone, Debug, Default)]
pub struct Drc {
    pub clearance: Vec<ClearanceViolation>,
    pub unconnected: Vec<UnconnectedItems>,
    pub islands: Vec<ZoneIslandViolation>,
    /// How long the worker took, for the log.
    pub elapsed: Duration,
}

enum Message {
    Ratsnest(Tag, Ratsnest),
    Drc(Tag, Drc),
}

struct Worker {
    tag: Tag,
    rx: Receiver<Message>,
}

/// The results and the worker that produces them.
#[derive(Default)]
pub struct Analysis {
    load: u64,
    ratsnest: Option<(Tag, Arc<Ratsnest>)>,
    drc: Option<(Tag, Arc<Drc>)>,
    worker: Option<Worker>,
    /// A run was asked for (Info > Run DRC) although the results are current.
    forced: bool,
    /// Workers started since the app began, for tests.
    runs_started: u64,
}

impl Analysis {
    /// The tag of the session's current state.
    pub fn tag_of(&self, session: &Session) -> Tag {
        Tag {
            load: self.load,
            revision: session.board.revision(),
            edit_generation: session.edit_generation,
        }
    }

    /// A new board was loaded: drop every result.
    pub fn reset(&mut self) {
        self.load += 1;
        self.ratsnest = None;
        self.drc = None;
        self.worker = None;
        self.forced = false;
    }

    /// Asks for a fresh run (Info > Run DRC, the lists' Recalculate).
    pub fn force(&mut self) {
        self.forced = true;
    }

    /// Workers started since the app began.
    pub fn runs_started(&self) -> u64 {
        self.runs_started
    }

    /// The ratsnest, if it is the one of the session's current state.
    pub fn ratsnest(&self, session: &Session) -> Option<&Arc<Ratsnest>> {
        let tag = self.tag_of(session);
        self.ratsnest
            .as_ref()
            .filter(|(t, _)| *t == tag)
            .map(|(_, r)| r)
    }

    /// The design rules check, if it is the one of the current state.
    pub fn drc(&self, session: &Session) -> Option<&Arc<Drc>> {
        let tag = self.tag_of(session);
        self.drc.as_ref().filter(|(t, _)| *t == tag).map(|(_, d)| d)
    }

    /// True while a worker runs.
    pub fn is_running(&self) -> bool {
        self.worker.is_some()
    }

    /// True when the results are those of the current state and nothing
    /// runs.
    pub fn is_idle(&self, session: &Session) -> bool {
        self.worker.is_none() && self.drc(session).is_some() && !self.forced
    }

    /// Takes the worker's messages and starts a run when the results are
    /// stale. Called once per frame.
    pub fn poll(&mut self, ctx: &egui::Context, session: Option<&Session>) {
        let Some(session) = session else { return };
        self.receive();
        let tag = self.tag_of(session);
        if self.worker.is_some() {
            return;
        }
        let current = self.drc.as_ref().is_some_and(|(t, _)| *t == tag);
        if current && !self.forced {
            return;
        }
        self.forced = false;
        self.start(ctx, tag, Arc::clone(&session.board));
    }

    fn receive(&mut self) {
        let Some(worker) = &self.worker else { return };
        let mut finished = false;
        loop {
            match worker.rx.try_recv() {
                Ok(Message::Ratsnest(tag, r)) => self.ratsnest = Some((tag, Arc::new(r))),
                Ok(Message::Drc(tag, d)) => {
                    self.drc = Some((tag, Arc::new(d)));
                    finished = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    finished = true;
                    break;
                }
            }
        }
        if finished {
            self.worker = None;
        }
    }

    fn start(&mut self, ctx: &egui::Context, tag: Tag, board: Arc<RoutingBoard>) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let started = Instant::now();
            let mut checker = DesignRulesChecker::new(board.basic());
            let ratsnest = Ratsnest {
                airlines: checker.all_airlines(),
                incomplete_count: checker.incomplete_count(),
                length_violations: checker
                    .net_incompletes()
                    .iter()
                    .filter(|n| n.length_violation() != 0.0)
                    .map(|n| (n.net_number, n.length_violation()))
                    .collect(),
            };
            if tx.send(Message::Ratsnest(tag, ratsnest)).is_err() {
                return;
            }
            ctx.request_repaint();
            let drc = Drc {
                clearance: checker.all_clearance_violations(),
                unconnected: checker.all_unconnected_items(),
                islands: checker.zone_island_violations(),
                elapsed: started.elapsed(),
            };
            let _ = tx.send(Message::Drc(tag, drc));
            ctx.request_repaint();
        });
        self.runs_started += 1;
        self.worker = Some(Worker { tag, rx });
    }

    /// The tag the running worker was started for.
    pub fn running_tag(&self) -> Option<Tag> {
        self.worker.as_ref().map(|w| w.tag)
    }
}
