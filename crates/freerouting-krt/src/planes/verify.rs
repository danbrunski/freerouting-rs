//! Plane-tap verification (X02.2): a report that changes nothing.
//!
//! Under D8 a plane-net pin counts as connected to its plane only through
//! its own via (or, for a pin on the plane's layer, directly). Freerouting's
//! connectivity treats a plane as one conductor, so a pin whose via lands on
//! a floating island of the fill still looks connected. [`TapReport`] checks
//! each pin of each plane net against [`PlaneComponents`]:
//! - its taps: the pins and vias connected to it without passing through a
//!   plane that anchor an island (normally the pin's own via);
//! - for each island a tap reaches: the island's layer, plane and index,
//!   whether it is the main component of the net on that layer (`on_main`),
//!   and whether it is connected to it (`joined`, through other copper);
//! - its status: `main` if some reached island is joined to the main
//!   component of its layer, `floating_tap` if it reaches islands but none is,
//!   `no_tap` if it reaches none (an incomplete the DRC already reports).
//!
//! Via reuse and via-in-pad, the rungs of KiCadRoutingTools' tap ladder
//! (`plane_pad_tap.py`), are not ported (D8). The main-component check is
//! the idea of its `plane_component_oracle.py` (at `0486e799`); no code is
//! ported.
//!
//! [`TapReportStage`] (`krt-plane-tap-report`) runs the report after the
//! optimizer and logs it as one JSON line; the board is not touched, so the
//! session is the same with and without it.
//!
//! Orders (D2): nets ascending, pins ascending by id within a net, taps and
//! islands ascending.

use std::collections::BTreeSet;

use freerouting::autoroute::stages::{BoardStage, StageCtx, StagePhase, StageReport};
use freerouting::board::items::ItemId;
use freerouting::board::structure::ComponentId;
use freerouting::board::{BasicBoard, RoutingBoard};

use super::components::PlaneComponents;

/// The registry id of [`TapReportStage`].
pub const TAP_REPORT_ID: &str = "krt-plane-tap-report";

/// How a plane-net pin reaches its plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapStatus {
    /// A tap reaches the main component of some plane layer.
    Main,
    /// Taps reach only islands not connected to the main component.
    FloatingTap,
    /// No tap reaches any island.
    NoTap,
}

impl TapStatus {
    /// The report's name of the status.
    pub fn name(self) -> &'static str {
        match self {
            TapStatus::Main => "main",
            TapStatus::FloatingTap => "floating_tap",
            TapStatus::NoTap => "no_tap",
        }
    }
}

/// One island a pin's tap reaches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerTap {
    /// The plane's layer.
    pub layer: i32,
    /// Its name.
    pub layer_name: String,
    /// The plane.
    pub area: ItemId,
    /// The island's index in the plane's fill order.
    pub island: usize,
    /// True if the island is the net's main component on the layer.
    pub on_main: bool,
    /// True if the island is connected to that main component.
    pub joined: bool,
}

/// The report of one plane-net pin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinTap {
    /// The pin.
    pub pin: ItemId,
    /// `component-pin`, or `#id`.
    pub label: String,
    /// The plane net.
    pub net: i32,
    /// Its name.
    pub net_name: String,
    /// The vias among the pin's taps, ascending.
    pub vias: Vec<ItemId>,
    /// True if the pin itself anchors an island (a pin on the plane layer).
    pub direct: bool,
    /// The islands its taps reach, ascending island order.
    pub layers: Vec<LayerTap>,
    /// The verdict.
    pub status: TapStatus,
    /// The pin's electrical group ([`PlaneComponents::group_of_item`]):
    /// equal for pins that are connected.
    pub group: Option<usize>,
}

/// The plane-tap report of a board.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TapReport {
    /// Every pin of every plane net, nets ascending, pins ascending by id.
    pub pins: Vec<PinTap>,
}

impl TapReport {
    /// The report of `board`.
    pub fn build(board: &BasicBoard) -> TapReport {
        TapReport::from_components(board, &PlaneComponents::build(board))
    }

    /// The report of `board` with its plane components `pc`.
    pub fn from_components(board: &BasicBoard, pc: &PlaneComponents) -> TapReport {
        let mut pins = Vec::new();
        let mut board_pins = board.pins();
        board_pins.sort_by_key(|i| i.0);
        for net in pc.nets() {
            for &pin in &board_pins {
                if board.item(pin).is_some_and(|i| i.contains_net(net)) {
                    pins.push(pin_tap(board, pc, net, pin));
                }
            }
        }
        TapReport { pins }
    }

    /// The pins reported as `floating_tap`.
    pub fn floating(&self) -> impl Iterator<Item = &PinTap> {
        self.pins
            .iter()
            .filter(|p| p.status == TapStatus::FloatingTap)
    }

    /// The connections that would join net `net`'s floating pins to its
    /// plane: one per electrical group among them.
    pub fn floating_connections(&self, net: i32) -> usize {
        self.floating()
            .filter(|p| p.net == net)
            .map(|p| p.group)
            .collect::<BTreeSet<_>>()
            .len()
    }

    /// The report as one JSON object, keys in a fixed order:
    /// `{"floating_tap":N,"pins":[...]}`.
    pub fn to_json(&self) -> String {
        let pins: Vec<String> = self.pins.iter().map(PinTap::to_json).collect();
        format!(
            "{{\"floating_tap\":{},\"pins\":[{}]}}",
            self.floating().count(),
            pins.join(",")
        )
    }
}

impl PinTap {
    fn to_json(&self) -> String {
        let vias: Vec<String> = self.vias.iter().map(|v| v.0.to_string()).collect();
        let layers: Vec<String> = self
            .layers
            .iter()
            .map(|l| {
                format!(
                    "{{\"layer\":{},\"area\":{},\"island\":{},\"on_main\":{},\"joined\":{}}}",
                    json_string(&l.layer_name),
                    l.area.0,
                    l.island,
                    l.on_main,
                    l.joined
                )
            })
            .collect();
        format!(
            "{{\"pin\":{},\"item\":{},\"net\":{},\"status\":\"{}\",\"direct\":{},\"vias\":[{}],\"layers\":[{}]}}",
            json_string(&self.label),
            self.pin.0,
            json_string(&self.net_name),
            self.status.name(),
            self.direct,
            vias.join(","),
            layers.join(",")
        )
    }
}

fn pin_tap(board: &BasicBoard, pc: &PlaneComponents, net: i32, pin: ItemId) -> PinTap {
    let mut taps: Vec<ItemId> = pc
        .component_items(pin)
        .unwrap_or(&[])
        .iter()
        .copied()
        .filter(|i| !pc.islands_of_anchor(*i).is_empty())
        .collect();
    taps.sort_by_key(|i| i.0);
    let islands: BTreeSet<usize> = taps
        .iter()
        .flat_map(|t| pc.islands_of_anchor(*t))
        .filter(|k| pc.islands()[*k].net == net)
        .collect();
    let layers: Vec<LayerTap> = islands
        .into_iter()
        .map(|k| {
            let island = &pc.islands()[k];
            let main = pc.main_of(net, island.layer);
            LayerTap {
                layer: island.layer,
                layer_name: layer_name(board, island.layer),
                area: island.area,
                island: island.index_in_area,
                on_main: main == Some(k),
                joined: main.is_some_and(|m| pc.joined(k, m)),
            }
        })
        .collect();
    let status = if layers.is_empty() {
        TapStatus::NoTap
    } else if layers.iter().any(|l| l.joined) {
        TapStatus::Main
    } else {
        TapStatus::FloatingTap
    };
    PinTap {
        pin,
        label: pin_label(board, pin),
        net,
        net_name: board
            .rules
            .nets
            .get(net)
            .map_or_else(|| format!("net {net}"), |n| n.name.clone()),
        vias: taps
            .iter()
            .copied()
            .filter(|t| board.item(*t).is_some_and(|i| i.is_via()))
            .collect(),
        direct: taps.contains(&pin),
        layers,
        status,
        group: pc.group_of_item(pin),
    }
}

/// Reports the plane taps after the optimizer and changes nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct TapReportStage;

impl BoardStage for TapReportStage {
    fn id(&self) -> &'static str {
        TAP_REPORT_ID
    }

    fn phase(&self) -> StagePhase {
        StagePhase::AfterOptimizer
    }

    fn run(&mut self, board: &mut RoutingBoard, _: &mut StageCtx) -> StageReport {
        let report = TapReport::build(board);
        StageReport {
            log: vec![format!("{TAP_REPORT_ID}: {}", report.to_json())],
        }
    }
}

/// `component-pin` for a pin, `#id` otherwise.
pub(crate) fn pin_label(board: &BasicBoard, id: ItemId) -> String {
    let Some(item) = board.item(id) else {
        return format!("#{}", id.0);
    };
    let component = board
        .components
        .get(ComponentId(item.component_id()))
        .map(|c| c.name.as_str());
    match (component, board.pin_name(item)) {
        (Some(component), Some(pin)) => format!("{component}-{pin}"),
        _ => format!("#{}", id.0),
    }
}

pub(crate) fn layer_name(board: &BasicBoard, layer: i32) -> String {
    usize::try_from(layer)
        .ok()
        .and_then(|l| board.layer_structure.layers.get(l))
        .map_or_else(|| format!("layer {layer}"), |l| l.name.clone())
}

pub(crate) fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planes::test_boards::{Pad, board};

    #[test]
    fn the_pin_on_the_cut_off_half_is_a_floating_tap() {
        let pads = [
            Pad::gnd("G1", 13000, 10000),
            Pad::gnd("G2", 13000, 20000),
            Pad::gnd("G3", 27000, 15000),
        ];
        let b = board(&pads, Some((20000, 2000, 20000, 28000)));
        let report = TapReport::build(&b);
        let gnd: Vec<&PinTap> = report.pins.iter().filter(|p| p.net_name == "GND").collect();
        assert_eq!(gnd.len(), 3);
        let floating: Vec<&str> = report.floating().map(|p| p.label.as_str()).collect();
        assert_eq!(floating, vec!["G3-1"]);
        let g3 = report.floating().next().unwrap();
        assert!(g3.direct, "a through-hole pin anchors the island itself");
        assert_eq!(g3.layers.len(), 1);
        assert!(!g3.layers[0].on_main && !g3.layers[0].joined);
        assert_eq!(report.floating_connections(g3.net), 1);
        assert!(
            report
                .to_json()
                .starts_with("{\"floating_tap\":1,\"pins\":[")
        );
    }

    #[test]
    fn an_uncut_pour_has_no_floating_tap() {
        let pads = [Pad::gnd("G1", 13000, 15000), Pad::gnd("G2", 27000, 15000)];
        let report = TapReport::build(&board(&pads, None));
        assert_eq!(report.floating().count(), 0);
        assert!(report.pins.iter().all(|p| p.status == TapStatus::Main));
    }
}
