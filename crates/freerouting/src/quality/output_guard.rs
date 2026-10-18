//! The two-axis output guard (Q04.3): a routing run never ends on a board
//! worse than its input. Not Java; on with `router.autorouter.output_guard
//! = true` (unset is off). After the idea of KiCadRoutingTools'
//! `improvement_gate.py` (#600: on spartan6_4layer a run lost 36 nets and
//! gained 43 while the disconnected pads went from 83 to 154), "neither axis
//! may outvote the other"; no code is taken from it.
//!
//! The batch loop keeps the board it was handed (a B01 snapshot and its
//! [`PinGroups`]) and, in its finish, after the controller's best-board
//! restore and before the final tail removal, compares the board it is
//! about to leave with that input on two axes:
//!
//! - **connections:** per net, the pins the input connects and the board
//!   no longer does (lost) against the pins the board connects and the
//!   input did not (gained), counted as spanning connections: a net's
//!   connections are its pins minus its groups of connected pins, and both
//!   counts are measured against the groups the two boards agree on. Worse
//!   when more are lost than gained.
//! - **unconnected pins:** the pins of a net with two or more pins that
//!   touch no other pin of their net through copper (planes included).
//!   Worse when there are more.
//!
//! If either axis is worse, the loop leaves instead the best board of the
//! history (best first, as a restore sorts it) that is worse on neither, or
//! the input board if none is, and logs both axes. The tail removal that
//! follows removes only traces and vias that connect nothing, so it changes
//! neither axis.
//!
//! Connected pins come from [`BasicBoard::connected_set`] (normal contacts,
//! through conduction areas), not from `drc::incompletes`: the guard needs
//! to know which pins are joined, not only how many airlines remain. On a
//! board with no input wiring the input connects only pins that touch each
//! other or a plane, which no routing removes, so the guard never fires.

use std::collections::{BTreeMap, BTreeSet};

use crate::autoroute::batch::snapshot::SnapshotHistory;
use crate::autoroute::history::HistoryEntry;
use crate::board::items::ItemId;
use crate::board::{BasicBoard, BoardSnapshot, RoutingBoard};
use crate::settings::RouterSettings;
use crate::util::HashMap;

/// The settings key.
pub const SETTINGS_KEY: &str = "router.autorouter.output_guard";

/// True if `settings` turn the guard on.
pub fn enabled(settings: &RouterSettings) -> bool {
    settings.autorouter.output_guard == Some(true)
}

/// Which pins of each net are connected: per net with at least two pins,
/// each pin's group (groups numbered from 0 in pin-id order).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PinGroups {
    nets: BTreeMap<i32, BTreeMap<ItemId, usize>>,
}

impl PinGroups {
    /// The pin groups of `board`.
    pub fn of(board: &BasicBoard) -> PinGroups {
        let mut pins_by_net: BTreeMap<i32, BTreeSet<ItemId>> = BTreeMap::new();
        for id in board.pins() {
            let Some(item) = board.item(id) else {
                continue;
            };
            for &net in item.net_numbers() {
                if net > 0 {
                    pins_by_net.entry(net).or_default().insert(id);
                }
            }
        }
        let mut contacts = HashMap::default();
        let mut nets = BTreeMap::new();
        for (net, pins) in pins_by_net {
            if pins.len() < 2 {
                continue;
            }
            let mut groups: BTreeMap<ItemId, usize> = BTreeMap::new();
            let mut next = 0;
            for &pin in &pins {
                if groups.contains_key(&pin) {
                    continue;
                }
                let connected = board.connected_set_cached(pin, net, false, &mut contacts);
                for other in connected.intersection(&pins) {
                    groups.insert(*other, next);
                }
                groups.insert(pin, next);
                next += 1;
            }
            nets.insert(net, groups);
        }
        PinGroups { nets }
    }

    /// The completed connections: per net, its pins minus its groups.
    pub fn completed(&self) -> usize {
        self.nets
            .values()
            .map(|groups| connections(groups.values().copied()))
            .sum()
    }

    /// The pins connected to no other pin of their net.
    pub fn unconnected_pins(&self) -> usize {
        self.nets
            .values()
            .map(|groups| {
                let mut sizes: BTreeMap<usize, usize> = BTreeMap::new();
                for &group in groups.values() {
                    *sizes.entry(group).or_default() += 1;
                }
                sizes.values().filter(|&&size| size == 1).count()
            })
            .sum()
    }
}

/// Pins minus distinct groups, for the groups of a net's pins.
fn connections<G: Ord>(groups: impl Iterator<Item = G>) -> usize {
    let mut pins = 0;
    let mut distinct = BTreeSet::new();
    for group in groups {
        pins += 1;
        distinct.insert(group);
    }
    pins - distinct.len()
}

/// A board against the input on the guard's two axes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Axes {
    /// Connections of the input the board lost.
    pub lost: usize,
    /// Connections the board has and the input had not.
    pub gained: usize,
    /// Unconnected pins of the input.
    pub unconnected_input: usize,
    /// Unconnected pins of the board.
    pub unconnected: usize,
}

impl Axes {
    /// `board` against `input`. A pin that only one of them has counts as
    /// its own group in the other.
    pub fn between(input: &PinGroups, board: &PinGroups) -> Axes {
        let empty = BTreeMap::new();
        let mut axes = Axes {
            unconnected_input: input.unconnected_pins(),
            unconnected: board.unconnected_pins(),
            ..Axes::default()
        };
        let nets: BTreeSet<i32> = input
            .nets
            .keys()
            .chain(board.nets.keys())
            .copied()
            .collect();
        for net in nets {
            let a = input.nets.get(&net).unwrap_or(&empty);
            let b = board.nets.get(&net).unwrap_or(&empty);
            let pins: BTreeSet<ItemId> = a.keys().chain(b.keys()).copied().collect();
            // A pin missing on one side is a group of its own there.
            let group = |groups: &BTreeMap<ItemId, usize>, index: usize, pin: &ItemId| {
                groups.get(pin).map_or((1, index), |&g| (0, g))
            };
            let pairs: Vec<((usize, usize), (usize, usize))> = pins
                .iter()
                .enumerate()
                .map(|(i, pin)| (group(a, i, pin), group(b, i, pin)))
                .collect();
            let in_input = connections(pairs.iter().map(|p| p.0));
            let in_board = connections(pairs.iter().map(|p| p.1));
            let in_both = connections(pairs.iter().copied());
            axes.lost += in_input - in_both;
            axes.gained += in_board - in_both;
        }
        axes
    }

    /// True if the board is worse than the input on either axis.
    pub fn worse(&self) -> bool {
        self.lost > self.gained || self.unconnected > self.unconnected_input
    }

    /// The two axes as the log names them.
    pub fn text(&self) -> String {
        format!(
            "completed connections lost {}, gained {}; unconnected pins {} -> {}",
            self.lost, self.gained, self.unconnected_input, self.unconnected
        )
    }
}

/// The board the guard left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GuardChoice {
    /// The routed board, worse on neither axis.
    Routed,
    /// A history board: its 1-based place best first and its score.
    History {
        /// Its place in the history, best first.
        rank: usize,
        /// Its router score.
        score: f32,
    },
    /// The input board: every history board was worse on an axis.
    Input,
}

/// What the guard did.
#[derive(Clone, Debug, PartialEq)]
pub struct GuardOutcome {
    /// The routed board against the input.
    pub routed: Axes,
    /// The board left.
    pub choice: GuardChoice,
    /// The board left against the input.
    pub saved: Axes,
    /// The job log lines.
    pub log: Vec<String>,
}

impl GuardOutcome {
    /// True if the guard replaced the routed board.
    pub fn fired(&self) -> bool {
        self.choice != GuardChoice::Routed
    }
}

/// The input board, kept for the finish.
#[derive(Clone, Debug)]
pub struct OutputGuard {
    input: BoardSnapshot,
    groups: PinGroups,
}

impl OutputGuard {
    /// The guard of a run on `board`, if `settings` turn it on.
    pub fn start(board: &RoutingBoard, settings: &RouterSettings) -> Option<OutputGuard> {
        enabled(settings).then(|| OutputGuard::new(board))
    }

    /// The guard of a run whose input is `board`.
    pub fn new(board: &RoutingBoard) -> OutputGuard {
        OutputGuard {
            input: board.snapshot(),
            groups: PinGroups::of(board),
        }
    }

    /// The input's pin groups.
    pub fn input_groups(&self) -> &PinGroups {
        &self.groups
    }

    /// Judges `board` against the input; when it is worse on an axis,
    /// also returns the board to leave instead: the first history board,
    /// best first, worse on neither (a board equal to `board` is skipped),
    /// or the input board.
    pub fn check(
        &self,
        board: &RoutingBoard,
        history: &SnapshotHistory,
    ) -> (GuardOutcome, Option<RoutingBoard>) {
        let routed = Axes::between(&self.groups, &PinGroups::of(board));
        let mut log = vec![format!(
            "Output guard: the routed board against the input: {}.",
            routed.text()
        )];
        if !routed.worse() {
            log.push("Output guard: kept the routed board.".to_string());
            let outcome = GuardOutcome {
                routed,
                choice: GuardChoice::Routed,
                saved: routed,
                log,
            };
            return (outcome, None);
        }
        let current = board.state_hash();
        for (index, (entry, score)) in history.best_first().into_iter().enumerate() {
            if entry.state_hash() == current {
                continue;
            }
            let candidate = entry.restore();
            let axes = Axes::between(&self.groups, &PinGroups::of(&candidate));
            if !axes.worse() {
                let rank = index + 1;
                log.push(format!(
                    "Output guard: the routed board is worse than the input; saving history \
                     board #{rank} (score {score}): {}.",
                    axes.text()
                ));
                let outcome = GuardOutcome {
                    routed,
                    choice: GuardChoice::History { rank, score },
                    saved: axes,
                    log,
                };
                return (outcome, Some(candidate));
            }
        }
        let input = self.input.restore();
        let saved = Axes::between(&self.groups, &self.groups);
        log.push(format!(
            "Output guard: the routed board and every history board are worse than the input; \
             saving the input board: {}.",
            saved.text()
        ));
        let outcome = GuardOutcome {
            routed,
            choice: GuardChoice::Input,
            saved,
            log,
        };
        (outcome, Some(input))
    }
}
