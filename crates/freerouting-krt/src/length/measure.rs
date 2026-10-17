// SPDX-License-Identifier: GPL-3.0-only AND MIT
// SPDX-FileCopyrightText: 2026 drandyhaas (KiCadRoutingTools)
// Portions ported from KiCadRoutingTools py_router/net_queries.py
// (calculate_route_length, net_pad_to_die_length) and py_router/kicad_parser.py
// (get_via_barrel_length) at 0486e799.

//! Net length measurement (X07.1): trace length plus via barrel spans plus
//! pad-to-die lengths.
//!
//! - The trace part is [`BasicBoard::net_trace_length`], the sum Java's
//!   `NetIncompletes.calcLengthViolation` compares with the class bounds,
//!   so a net without vias measures exactly what Java checks.
//! - A via's barrel is the stackup thickness from its first to its last
//!   copper layer, both copper layers included (KRT
//!   `get_via_barrel_length`: a through via of a 1.6 mm board measures
//!   1.6 mm). The stackup comes from the KiCad reader (K05.1,
//!   `KicadRead.extras.stackup`); its copper layers map to the board's
//!   layers in physical order, so the stackup must have as many copper
//!   layers as the board.
//! - Pad-to-die is the `die_length` of each pin of the net (K05.1,
//!   `KicadRead.extras.die_lengths`), keyed by component and pin name.
//!
//! Without a stackup (a DSN input) the barrel term is 0 and
//! [`NetLength::stackup_known`] is false; the stage's report then says
//! "stackup unknown". Without the KiCad reader there are no pad-to-die
//! lengths either, so both terms are 0. Every pin of the net counts, as in
//! KiCad 10's length DRC (KRT `net_pad_to_die_length`).
//!
//! All lengths are in board units (`f64`); [`Units`] converts.

use std::collections::BTreeMap;

use freerouting::board::BasicBoard;
use freerouting::board::items::Item;
use freerouting::board::structure::ComponentId;
use freerouting::io::kicad::Extras;
use freerouting::io::kicad::reader::layers::StackupLayer;

/// The board's length unit against micrometres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Units {
    /// Board units per micrometre.
    pub board_per_um: f64,
}

impl Units {
    /// The units of `board` (its DSN unit and resolution).
    pub fn of(board: &BasicBoard) -> Units {
        let c = &board.communication;
        Units {
            board_per_um: c.coordinate_transform.scale_factor() / c.unit.micrometers(),
        }
    }

    /// `um` micrometres in board units.
    pub fn from_um(self, um: f64) -> f64 {
        um * self.board_per_um
    }

    /// `value` board units in micrometres.
    pub fn to_um(self, value: f64) -> f64 {
        value / self.board_per_um
    }

    /// `nm` nanometres in board units.
    pub fn from_nm(self, nm: i64) -> f64 {
        self.from_um(nm as f64 / 1000.0)
    }
}

/// A board stackup from the KiCad reader, top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct Stackup {
    /// Every stackup layer as the file lists it.
    pub layers: Vec<StackupLayer>,
    /// The index in `layers` of each copper layer, top to bottom.
    pub copper: Vec<usize>,
}

impl Stackup {
    /// The stackup of `layers`, `None` if it has no copper layer.
    pub fn new(layers: Vec<StackupLayer>) -> Option<Stackup> {
        let copper: Vec<usize> = layers
            .iter()
            .enumerate()
            .filter(|(_, l)| l.kind == "copper")
            .map(|(i, _)| i)
            .collect();
        if copper.is_empty() {
            return None;
        }
        Some(Stackup { layers, copper })
    }

    /// The number of copper layers.
    pub fn copper_count(&self) -> usize {
        self.copper.len()
    }

    /// The barrel length in nanometres between copper layers `a` and `b`
    /// (0 = top), both included (KRT `get_via_barrel_length`). A layer
    /// without a thickness counts 0.
    pub fn barrel_nm(&self, a: usize, b: usize) -> Option<i64> {
        let (a, b) = (a.min(b), a.max(b));
        let from = *self.copper.get(a)?;
        let to = *self.copper.get(b)?;
        Some(
            self.layers[from..=to]
                .iter()
                .map(|l| l.thickness_nm.unwrap_or(0))
                .sum(),
        )
    }
}

/// What the measurement knows beyond the board: the stackup and the
/// pad-to-die lengths (K05.1).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LengthModel {
    /// The stackup, if the input had one.
    pub stackup: Option<Stackup>,
    /// `die_length` in nanometres by (component, pin name).
    pub die_lengths: BTreeMap<(String, String), i64>,
}

impl LengthModel {
    /// The model of a DSN input: no stackup, no pad-to-die.
    pub fn none() -> LengthModel {
        LengthModel::default()
    }

    /// The model from the KiCad reader's extras.
    pub fn from_kicad(extras: &Extras) -> LengthModel {
        LengthModel {
            stackup: Stackup::new(extras.stackup.clone()),
            die_lengths: extras
                .die_lengths
                .iter()
                .map(|d| ((d.component.clone(), d.pin.clone()), d.nm))
                .collect(),
        }
    }

    /// The stackup if it fits `board` (as many copper layers as board
    /// layers), else `None`.
    pub fn stackup_for(&self, board: &BasicBoard) -> Option<&Stackup> {
        self.stackup
            .as_ref()
            .filter(|s| s.copper_count() == board.layer_count())
    }
}

/// The measured length of one net, in board units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NetLength {
    /// The net number.
    pub net: i32,
    /// The trace length ([`BasicBoard::net_trace_length`]).
    pub trace: f64,
    /// The barrel spans of the net's vias.
    pub via_barrels: f64,
    /// The pad-to-die lengths of the net's pins.
    pub pad_to_die: f64,
    /// The net's vias.
    pub vias: usize,
    /// True if a stackup that fits the board was known.
    pub stackup_known: bool,
}

impl NetLength {
    /// Trace plus via barrels plus pad-to-die.
    pub fn total(&self) -> f64 {
        self.trace + self.via_barrels + self.pad_to_die
    }
}

/// The vias of `net`, in item order.
pub fn net_vias(board: &BasicBoard, net: i32) -> impl Iterator<Item = &Item> + '_ {
    board
        .items()
        .filter(move |i| i.is_via() && i.contains_net(net))
}

/// Measures `net` (module docs).
pub fn measure_net(board: &BasicBoard, net: i32, model: &LengthModel) -> NetLength {
    let units = Units::of(board);
    let trace = board.net_trace_length(net);
    let stackup = model.stackup_for(board);
    let mut via_barrels = 0.0;
    let mut vias = 0;
    for via in net_vias(board, net) {
        vias += 1;
        if let Some(s) = stackup {
            let nm = s
                .barrel_nm(via.first_layer() as usize, via.last_layer() as usize)
                .unwrap_or(0);
            via_barrels += units.from_nm(nm);
        }
    }
    let mut pad_to_die = 0.0;
    if !model.die_lengths.is_empty() {
        for pin in board.items().filter(|i| i.is_pin() && i.contains_net(net)) {
            if let Some(nm) = die_length_of(board, pin, model) {
                pad_to_die += units.from_nm(nm);
            }
        }
    }
    NetLength {
        net,
        trace,
        via_barrels,
        pad_to_die,
        vias,
        stackup_known: stackup.is_some(),
    }
}

/// The `die_length` of pin `pin`, if the model has one.
fn die_length_of(board: &BasicBoard, pin: &Item, model: &LengthModel) -> Option<i64> {
    let component = board.components.get(ComponentId(pin.component_id()))?;
    let name = board.pin_name(pin)?;
    model
        .die_lengths
        .get(&(component.name.clone(), name.to_string()))
        .copied()
}
