//! Reports and small checks of the batch pipeline (A10.7), at `a773a70ec`:
//!
//! - [`unrouted_report`]: `AutorouteUnroutedReport.build`, the text the
//!   batch loop logs when it stops on stagnation, and
//!   [`UnroutedReportStage`], the diagnostics [`BoardStage`] that logs it
//!   (`freerouting-unrouted-report`, opt-in through
//!   `router.special.stages`);
//! - [`corridor`] and [`is_removable_blocker`]: the pure parts of
//!   `LastMileBlockerRipup` (the ripup itself is A13.3);
//! - [`check_routable_layers`]: the start-up check of
//!   `AutorouteBatchLoop.run` that refuses a job with no active signal
//!   layer.

use std::fmt;

use freerouting_geometry::jmath::round_to_i32;
use freerouting_geometry::{FloatPoint, IntBox};

use crate::autoroute::stages::{BoardStage, StageCtx, StagePhase, StageReport};
use crate::board::basic_board::BasicBoard;
use crate::board::items::{Item, ItemId};
use crate::board::routing_board::RoutingBoard;
use crate::board::structure::ComponentId;
use crate::drc::DesignRulesChecker;
use crate::settings::board::BoardRouterSettings;

/// The report when there is no airline.
pub const NO_UNROUTED: &str = "  (no unrouted connections found)";

/// Java `AutorouteUnroutedReport.build`: the board's airlines after
/// `calculateAllIncompletes`, grouped by net name in first-seen order, one
/// line per airline, trailing whitespace stripped.
pub fn unrouted_report(board: &BasicBoard) -> String {
    let mut drc = DesignRulesChecker::new(board);
    drc.calculate_all_incompletes();
    let airlines = drc.all_airlines();
    if airlines.is_empty() {
        return NO_UNROUTED.to_string();
    }
    // PARITY: AutorouteUnroutedReport.java:29-37, a LinkedHashMap by net name.
    let mut by_net: Vec<(String, Vec<String>)> = Vec::new();
    for airline in &airlines {
        let net_name = board
            .rules
            .nets
            .get(airline.net_number)
            .map_or_else(|| "(unknown net)".to_string(), |net| net.name.clone());
        let line = format!(
            "    - {}  ->  {}",
            describe_item(board, airline.from_item),
            describe_item(board, airline.to_item)
        );
        match by_net.iter_mut().find(|(name, _)| *name == net_name) {
            Some((_, lines)) => lines.push(line),
            None => by_net.push((net_name, vec![line])),
        }
    }
    let mut result = String::new();
    for (net_name, lines) in &by_net {
        let count = lines.len();
        let plural = if count == 1 { "" } else { "s" };
        result.push_str(&format!(
            "  Net '{net_name}' ({count} unrouted connection{plural}):\n"
        ));
        for line in lines {
            result.push_str(line);
            result.push('\n');
        }
    }
    result.trim_end().to_string()
}

/// Java `describeItem`: `Component-Pin` for a pin of a placed component,
/// else Java's `Item.toString` (the lower-case class name, `" #<index>"`
/// for a pin with a positive index, `" of component #<id>"`).
pub fn describe_item(board: &BasicBoard, id: ItemId) -> String {
    let Some(item) = board.item(id) else {
        return "(unknown)".to_string();
    };
    if let Some(pin) = item.as_pin()
        && let Some(component) = board.components.get(ComponentId(item.component_id()))
    {
        let package_pin = board
            .library
            .packages
            .get(component.package())
            .and_then(|package| package.get_pin(pin.pin_index));
        return match package_pin {
            Some(package_pin) => format!("{}-{}", component.name, package_pin.name),
            None => format!("{} (pin #{})", component.name, pin.pin_index),
        };
    }
    java_item_string(item)
}

/// Java `Item.toString` and `Pin.toString`.
fn java_item_string(item: &Item) -> String {
    let mut out = item.type_name().to_string();
    if let Some(pin) = item.as_pin()
        && pin.pin_index > 0
    {
        out.push_str(&format!(" #{}", pin.pin_index));
    }
    if item.component_id() > 0 {
        out.push_str(&format!(" of component #{}", item.component_id()));
    }
    out
}

/// The diagnostics stage that logs [`unrouted_report`] after routing
/// (`freerouting-unrouted-report`). It changes nothing on the board.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnroutedReportStage;

impl BoardStage for UnroutedReportStage {
    fn id(&self) -> &'static str {
        "freerouting-unrouted-report"
    }

    fn phase(&self) -> StagePhase {
        StagePhase::AfterRouting
    }

    fn run(&mut self, board: &mut RoutingBoard, _: &mut StageCtx) -> StageReport {
        StageReport {
            log: vec![format!(
                "Unrouted connections after routing:\n{}",
                unrouted_report(board)
            )],
        }
    }
}

/// Java `LastMileBlockerRipup.corridor`: the box around an airline from
/// `from` to `to`, rounded with `(int) Math.round`, grown on every side by
/// `max(span / 4, 1)` where `span` is the larger side. The int arithmetic
/// wraps as Java's does.
pub fn corridor(from: FloatPoint, to: FloatPoint) -> IntBox {
    let (x1, y1) = (round_to_i32(from.x), round_to_i32(from.y));
    let (x2, y2) = (round_to_i32(to.x), round_to_i32(to.y));
    let (min_x, max_x) = (x1.min(x2), x1.max(x2));
    let (min_y, max_y) = (y1.min(y2), y1.max(y2));
    let span = max_x.wrapping_sub(min_x).max(max_y.wrapping_sub(min_y));
    let margin = (span / 4).max(1);
    IntBox::new(
        min_x.wrapping_sub(margin),
        min_y.wrapping_sub(margin),
        max_x.wrapping_add(margin),
        max_y.wrapping_add(margin),
    )
}

/// Java `LastMileBlockerRipup.isRemovableBlocker`: a trace or via that is
/// neither deletion-forbidden nor user-fixed, not on the airline's net
/// `airline_net`, with at least one net.
pub fn is_removable_blocker(board: &BasicBoard, item: &Item, airline_net: i32) -> bool {
    if !item.is_trace() && !item.is_via() {
        return false;
    }
    if board.is_deletion_forbidden(item) || item.is_user_fixed() {
        return false;
    }
    if item.net_numbers().contains(&airline_net) {
        return false;
    }
    item.net_count() > 0
}

/// The autorouter refuses to start: no layer is both active and a signal
/// layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoRoutableLayer;

impl fmt::Display for NoRoutableLayer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // PARITY: AutorouteBatchLoop.java:61, Java's message.
        f.write_str("Cannot start autorouter: all layers are disabled.")
    }
}

impl std::error::Error for NoRoutableLayer {}

/// The routable-layer check of `AutorouteBatchLoop.run`
/// (`AutorouteBatchLoop.java:50-62`): some layer of the settings must be
/// active and a signal layer of the board. Java throws
/// `IllegalArgumentException`; the port returns [`NoRoutableLayer`].
pub fn check_routable_layers(
    board: &BasicBoard,
    settings: &BoardRouterSettings,
) -> Result<(), NoRoutableLayer> {
    let layers = &board.layer_structure.layers;
    let any = (0..settings.layer_count())
        .any(|i| settings.layer_active(i) && layers.get(i).is_some_and(|layer| layer.is_signal));
    if any { Ok(()) } else { Err(NoRoutableLayer) }
}
