//! Q01: pours of listed net classes block only the nets that pass through.
//!
//! Java can make conduction areas obstacles only all at once
//! (`router.plane_as_obstacle`), which also blocks a net whose own pads sit
//! inside a pour: the ordinary GND-pour case on a two-layer board. Setting
//! `router.plane_obstacle_net_classes` to a list of net class names makes a
//! signal-layer pour of a listed class an obstacle for net M only if M has
//! no pin, and no via in the input, inside the pour. The idea is
//! KiCadRoutingTools' pour guard (`py_router/pour_guard.py`,
//! `compute_pour_barriers`, at `0486e799`); no code is ported.
//!
//! - **The exemption table** ([`ExemptionTable::build`]) is computed once,
//!   from the board as loaded. For each conduction area on a signal layer
//!   whose net's class is listed, in ascending item id: the nets other than
//!   the area's own that have a pin, or a via, on the area's layer whose
//!   shape centre lies inside the area's outer boundary (holes count as
//!   inside). Vias the router adds later don't change it, so the routing
//!   order cannot (D2).
//! - **The predicate** (the [`AreaObstaclePolicy`] impl): a listed area is an
//!   obstacle for every foreign net that is not exempt. Java's rule still
//!   applies on top (the board's [`AreaObstaclePolicy`] hook is an "or"), and
//!   items of the area's own net connect to it as before.
//! - **Power layers are left alone.** Only areas on signal layers are
//!   listed: a plane on a power layer is crossed by the vias of every net
//!   (Java's planes are never obstacles there), and no foreign net routes
//!   on a power layer any more (Q01.7).
//! - **D8.** A signal-layer pour of a net that has a power-layer plane is
//!   dropped by the DSN reader (it would make the net's pads look
//!   connected), so it is never listed.
//! - **Off.** With the key unset or empty, or with `router.plane_as_obstacle`
//!   set (Java's rule then wins and the key changes nothing), no policy is
//!   installed and routing is Java's.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use freerouting_geometry::FloatPoint;

use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId};
use crate::job::{LogLevel, LogLine};
use crate::rules::{AreaObstaclePolicy, NetClassId};
use crate::settings::RouterSettings;

/// The settings key, as the CLI and the JSON file name it.
pub const SETTINGS_KEY: &str = "router.plane_obstacle_net_classes";

/// A settings error of the pour guard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PourGuardError {
    /// A listed class name is not a net class of the board; with the
    /// board's class names, in board order.
    UnknownNetClass { name: String, known: Vec<String> },
}

impl fmt::Display for PourGuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PourGuardError::UnknownNetClass { name, known } => write!(
                f,
                "{SETTINGS_KEY}: the board has no net class '{name}' (its classes: {})",
                known.join(", ")
            ),
        }
    }
}

impl std::error::Error for PourGuardError {}

/// The listed pours and, per pour, the foreign nets it does not block.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExemptionTable {
    /// Per listed conduction area (in `ItemId` order): the exempt nets,
    /// ascending.
    areas: BTreeMap<ItemId, Vec<i32>>,
}

impl ExemptionTable {
    /// The table of `board` for the net classes named `classes`. Fails on a
    /// name that is not a net class of the board, naming it.
    pub fn build(board: &BasicBoard, classes: &[String]) -> Result<ExemptionTable, PourGuardError> {
        let listed = listed_classes(board, classes)?;
        let mut drills: Vec<&Item> = board
            .items()
            .filter(|item| item.is_pin() || item.is_via())
            .collect();
        drills.sort_by_key(|item| item.id().0);
        let mut area_ids = board.conduction_areas();
        area_ids.sort_by_key(|id| id.0);
        let mut areas = BTreeMap::new();
        for id in area_ids {
            let Some(item) = board.item(id) else {
                continue;
            };
            let Some(area) = item.as_conduction_area() else {
                continue;
            };
            let layer = area.layer();
            let on_signal_layer = board
                .layer_structure
                .layers
                .get(layer as usize)
                .is_some_and(|l| l.is_signal);
            if !on_signal_layer || !has_listed_class(board, item, &listed) {
                continue;
            }
            let border = area.area().get_border();
            let mut exempt = BTreeSet::new();
            for drill in &drills {
                let Some(centre) = shape_centre(drill, layer) else {
                    continue;
                };
                if !border.contains_float(centre) {
                    continue;
                }
                exempt.extend(
                    drill
                        .net_numbers()
                        .iter()
                        .copied()
                        .filter(|n| *n > 0 && !item.contains_net(*n)),
                );
            }
            areas.insert(id, exempt.into_iter().collect());
        }
        Ok(ExemptionTable { areas })
    }

    /// The number of listed pours.
    pub fn len(&self) -> usize {
        self.areas.len()
    }

    /// True if no pour is listed.
    pub fn is_empty(&self) -> bool {
        self.areas.is_empty()
    }

    /// The listed pours, in `ItemId` order (highest id first, as Java's
    /// item sets).
    pub fn listed_areas(&self) -> impl Iterator<Item = ItemId> + '_ {
        self.areas.keys().copied()
    }

    /// The exempt nets of pour `area`, ascending; `None` if it is not listed.
    pub fn exempt_nets(&self, area: ItemId) -> Option<&[i32]> {
        self.areas.get(&area).map(Vec::as_slice)
    }

    /// The number of (pour, net) exemptions.
    pub fn exemption_count(&self) -> usize {
        self.areas.values().map(Vec::len).sum()
    }
}

impl AreaObstaclePolicy for ExemptionTable {
    fn blocks(&self, area: ItemId, net: i32) -> bool {
        self.areas
            .get(&area)
            .is_some_and(|exempt| exempt.binary_search(&net).is_err())
    }
}

/// The class ids named by `classes`, or the first unknown name.
fn listed_classes(
    board: &BasicBoard,
    classes: &[String],
) -> Result<BTreeSet<NetClassId>, PourGuardError> {
    classes
        .iter()
        .map(|name| {
            board.rules.net_classes.get_by_name(name).ok_or_else(|| {
                PourGuardError::UnknownNetClass {
                    name: name.clone(),
                    known: class_names(board),
                }
            })
        })
        .collect()
}

/// The board's net class names, in board order.
fn class_names(board: &BasicBoard) -> Vec<String> {
    board
        .rules
        .net_classes
        .iter()
        .map(|(_, class)| class.name().to_string())
        .collect()
}

/// True if one of the area's nets is in a listed class.
fn has_listed_class(board: &BasicBoard, area: &Item, listed: &BTreeSet<NetClassId>) -> bool {
    area.net_numbers().iter().any(|n| {
        board
            .rules
            .nets
            .get(*n)
            .is_some_and(|net| listed.contains(&net.net_class()))
    })
}

/// The centre of a pin's or via's pad shape on `layer` (the drill centre
/// when the padstack has no shape there); `None` if it does not reach the
/// layer.
fn shape_centre(drill_item: &Item, layer: i32) -> Option<FloatPoint> {
    let drill = drill_item.drill()?;
    if layer < drill.first_layer || layer > drill.last_layer {
        return None;
    }
    Some(match drill.shape_on_layer(layer) {
        Some(shape) => shape.centre_of_gravity(),
        None => drill.center.to_float(),
    })
}

/// Installs the pour guard on `board` as `settings` ask (the C03 apply
/// step): the exemption table as the board's area policy when
/// `plane_obstacle_net_classes` is non-empty and `plane_as_obstacle` is
/// false, otherwise no policy. Returns the log line, if any.
pub fn install(
    board: &mut BasicBoard,
    settings: &RouterSettings,
) -> Result<Option<LogLine>, PourGuardError> {
    let classes = settings
        .plane_obstacle_net_classes
        .as_deref()
        .unwrap_or_default();
    if classes.is_empty() {
        board.rules.area_obstacle_policy = None;
        return Ok(None);
    }
    if settings.plane_as_obstacle {
        board.rules.area_obstacle_policy = None;
        return Ok(Some(LogLine {
            level: LogLevel::Info,
            text: format!(
                "{SETTINGS_KEY} changes nothing: router.plane_as_obstacle makes every plane an \
                 obstacle."
            ),
        }));
    }
    let table = ExemptionTable::build(board, classes)?;
    let text = format!(
        "Pour guard: {} pour(s) of net classes {} block the nets with no pin or via inside \
         them ({} exemptions).",
        table.len(),
        classes.join(", "),
        table.exemption_count()
    );
    board.rules.area_obstacle_policy = Some(Arc::new(table));
    Ok(Some(LogLine {
        level: LogLevel::Info,
        text,
    }))
}
