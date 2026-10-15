//! The board-building half of the Java DSN reader: the `readScope`
//! methods of `io.specctra.parser`, applied to the syntax tree that
//! `freerouting-dsn` read.

pub(crate) mod dsn_file;
pub(crate) mod layer;
pub(crate) mod library;
pub(crate) mod network;
pub(crate) mod shape;
pub(crate) mod structure;
pub(crate) mod wiring;

use std::collections::{BTreeMap, BTreeSet};

use crate::board::RoutingBoard;
use crate::board::state::WriteResolution;
use crate::board::structure::{AngleRestriction, Unit};
use crate::io::CoordinateTransform;
use crate::util::JavaString;

use layer::DsnLayerStructure;
use shape::DsnArea;

/// The id of a net in the DSN net list: name and subnet number.
///
/// Port of `io.specctra.parser.Net.Id`, ordered as Java orders it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct NetKey {
    pub name: JavaString,
    pub subnet_number: i32,
}

impl NetKey {
    pub fn new(name: &str, subnet_number: i32) -> Self {
        NetKey {
            name: JavaString(name.to_string()),
            subnet_number,
        }
    }
}

/// A component pin in a net: component name and pin name.
///
/// Port of `io.specctra.parser.Net.Pin`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct PinKey {
    pub component: JavaString,
    pub pin: JavaString,
}

impl PinKey {
    pub fn new(component: &str, pin: &str) -> Self {
        PinKey {
            component: JavaString(component.to_string()),
            pin: JavaString(pin.to_string()),
        }
    }
}

/// The nets read from the network scope, with their pins.
///
/// Port of `io.specctra.parser.NetList`.
#[derive(Clone, Debug, Default)]
pub(crate) struct NetList {
    nets: BTreeMap<NetKey, Option<BTreeSet<PinKey>>>,
}

impl NetList {
    /// Adds a net without pins. Returns false if it exists.
    pub fn add_net(&mut self, key: NetKey) -> bool {
        if self.nets.contains_key(&key) {
            return false;
        }
        self.nets.insert(key, None);
        true
    }

    pub fn set_pins(&mut self, key: &NetKey, pins: impl IntoIterator<Item = PinKey>) {
        if let Some(entry) = self.nets.get_mut(key) {
            *entry = Some(pins.into_iter().collect());
        }
    }

    /// For each pin, the nets containing it, in net list order (Java
    /// `getNets` for every pin at once).
    pub fn nets_by_pin(&self) -> BTreeMap<&PinKey, Vec<&NetKey>> {
        let mut result: BTreeMap<&PinKey, Vec<&NetKey>> = BTreeMap::new();
        for (key, pins) in &self.nets {
            for pin in pins.iter().flatten() {
                result.entry(pin).or_default().push(key);
            }
        }
        result
    }
}

/// A plane read in the structure scope.
///
/// Port of `ReadScopeParameter.PlaneInfo`.
#[derive(Clone, Debug)]
pub(crate) struct PlaneInfo {
    pub area: DsnArea,
    pub net_name: String,
}

/// The state shared by the scope readers.
///
/// Port of `io.specctra.parser.ReadScopeParameter`.
pub(crate) struct ReadContext {
    pub board: Option<RoutingBoard>,
    /// Warnings returned to the caller (Java `ReadScopeParameter.warnings`).
    pub warnings: Vec<String>,
    /// Messages Java only logs. They go to the board log once there is a
    /// board.
    pub log: Vec<String>,
    pub netlist: NetList,
    pub plane_list: Vec<PlaneInfo>,
    pub via_padstack_names: Option<Vec<String>>,
    pub via_at_smd_allowed: bool,
    pub snap_angle: AngleRestriction,
    pub string_quote: String,
    pub host_cad: Option<String>,
    pub host_version: Option<String>,
    pub constants: Vec<(String, String)>,
    pub write_resolution: Option<WriteResolution>,
    pub dsn_file_generated_by_host: bool,
    pub board_outline_ok: bool,
    pub coordinate_transform: CoordinateTransform,
    pub layer_structure: DsnLayerStructure,
    pub autoroute_settings_read: bool,
    pub unit: Unit,
    pub resolution: i32,
    /// Java's `scanner.getScopeIdentifier()`: the last name the reader
    /// noted, used in messages.
    pub scope_identifier: String,
}

impl ReadContext {
    pub fn new() -> Self {
        ReadContext {
            board: None,
            warnings: Vec::new(),
            log: Vec::new(),
            netlist: NetList::default(),
            plane_list: Vec::new(),
            via_padstack_names: None,
            via_at_smd_allowed: false,
            snap_angle: AngleRestriction::FortyfiveDegree,
            string_quote: "\"".into(),
            host_cad: None,
            host_version: None,
            constants: Vec::new(),
            write_resolution: None,
            dsn_file_generated_by_host: true,
            board_outline_ok: true,
            coordinate_transform: CoordinateTransform::new(1.0, 0.0, 0.0),
            layer_structure: DsnLayerStructure::default(),
            autoroute_settings_read: false,
            unit: Unit::Mil,
            resolution: 100,
            scope_identifier: String::new(),
        }
    }

    /// Logs a message (Java `FRLogger.warn`/`info`).
    pub fn log(&mut self, message: impl Into<String>) {
        let message = message.into();
        match &mut self.board {
            Some(board) => board.log.push(message),
            None => self.log.push(message),
        }
    }

    /// Adds a warning for the caller; Java also logs it.
    pub fn warn(&mut self, message: String) {
        self.log(message.clone());
        self.warnings.push(message);
    }

    pub fn board(&mut self) -> &mut RoutingBoard {
        self.board
            .as_mut()
            .expect("board created by the structure scope")
    }
}
