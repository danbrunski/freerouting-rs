//! `AutorouteControl`: the parameters of one autoroute search (A01.3).
//!
//! Port of `autoroute/maze/AutorouteControl.java` (at `a773a70ec`). It is
//! plain data: one struct with a `Vec` per layer, built once per connection
//! by [`AutorouteControl::for_net`] (Java's five-argument constructor, which
//! the batch router uses) or [`AutorouteControl::for_net_default`] (the
//! three-argument one). The fields keep Java's names in snake case, so the
//! `control` probe maps one to one.
//!
//! What decides results, in Java's order:
//!
//! - the plane-layer rule of the constructor: a non-signal layer that the
//!   settings mark active is forced inactive, with Java's warning;
//! - `init_net`: the net class, or for a missing net the fallback (clearance
//!   class 1 and the first via rule); the trace half widths (net 0 uses
//!   net 1), the compensated widths, and the class's inactive layers;
//! - `rebuild_via_info`: the via radii and the via costs, reset before
//!   rebuilding so a smaller rule cannot retain stale radii (N06.2).
//!
//! **Beyond Java (Q14).** Under `router.via_clearance = net_class` every via
//! the search checks or inserts takes the net class's via clearance class
//! in place of its via info's
//! ([`via_info_clearance_class`](AutorouteControl::via_info_clearance_class),
//! [`via_clearance`](super::via_clearance)); the default is Java's rule.
//!
//! **The fanout's merged via rule** (A12.1). `RoutingBoard.fanout` replaces
//! `viaRule` by a new `ViaRule` that is not on the board: the class's vias
//! followed by those of the board's first via rule that it lacks. Here
//! [`AutorouteControl::merged_via_rule`] holds that list, and every reader
//! of the via rule goes through [`AutorouteControl::via_rule_vias`].
//!
//! **Beyond Java (Q13).** Under `router.power_layer_traces = own_plane`
//! [`AutorouteControl::own_plane_areas`] lists the net's planes per power
//! layer, and the maze search lays a trace there only inside one of them;
//! under `none` (N05) it lays none there and only lands vias in them
//! ([`power_layer_traces`](super::power_layer_traces)); the default is
//! Java's per-layer rule ([`AutorouteControl::own_plane_layers`]).
//!
//! Java's `settings` field and the debug-only `fanoutStartPinName` are not
//! carried; the settings a search reads are copied into the fields here.
//! Where Java throws (a missing net, no via rule at all, a net class
//! without a via rule) the constructors return a [`ControlError`] naming
//! Java's exception.

use std::fmt;

use crate::autoroute::via_clearance::ViaClearanceMode;
use crate::board::BasicBoard;
use crate::board::items::ItemId;
use crate::board::structure::Unit;
use crate::geometry::Point;
use crate::geometry::jmath::max_f64;
use crate::rules::{BoardRules, ItemClass, ViaInfo, ViaInfoId, ViaRuleId};
use crate::settings::board::BoardRouterSettings;

/// The horizontal and vertical trace costs on a layer (Java
/// `AutorouteControl.ExpansionCostFactor`). The settings already define the
/// type; this is the same one.
pub use crate::settings::layers::TraceCostFactor as ExpansionCostFactor;

/// A via type the search may place: its layer range and whether it may sit
/// on an SMD pad (Java `AutorouteControl.ViaMask`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViaMask {
    pub from_layer: i32,
    pub to_layer: i32,
    pub attach_smd_allowed: bool,
}

/// The additional costs of a via from one layer to each other layer (Java
/// `AutorouteControl.ViaCost`). Always 0 at the pin; kept because the maze
/// search adds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViaCost {
    pub to_layer: Vec<i32>,
}

/// Where Java's constructors throw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlError {
    /// `BoardRules.getTraceHalfWidth` on a net that does not exist: Java's
    /// `NullPointerException`.
    NoSuchNet(i32),
    /// `viaRules.firstElement()` on a board without via rules: Java's
    /// `NoSuchElementException`.
    NoViaRules,
    /// The net's class has no via rule: Java's `NullPointerException` in
    /// `rebuildViaInfo`.
    NoViaRule(i32),
    /// A via padstack reaches a layer outside the board: Java's
    /// `ArrayIndexOutOfBoundsException`.
    LayerOutOfRange(i32),
}

impl ControlError {
    /// The simple name of the exception Java throws.
    pub fn java_exception(&self) -> &'static str {
        match self {
            ControlError::NoSuchNet(_) | ControlError::NoViaRule(_) => "NullPointerException",
            ControlError::NoViaRules => "NoSuchElementException",
            ControlError::LayerOutOfRange(_) => "ArrayIndexOutOfBoundsException",
        }
    }
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ControlError::NoSuchNet(net) => write!(f, "net {net} does not exist"),
            ControlError::NoViaRules => f.write_str("the board has no via rule"),
            ControlError::NoViaRule(net) => write!(f, "the net class of net {net} has no via rule"),
            ControlError::LayerOutOfRange(layer) => {
                write!(f, "a via padstack reaches layer {layer} outside the board")
            }
        }
    }
}

impl std::error::Error for ControlError {}

/// The parameters of one autoroute search.
#[derive(Clone, Debug, PartialEq)]
pub struct AutorouteControl {
    /// The horizontal and vertical trace costs on each layer.
    pub trace_costs: Vec<ExpansionCostFactor>,
    pub bend_costs: Vec<f64>,
    pub with_neckdown: bool,
    /// Half of the narrowest trace the router may create
    /// (`router.min_trace_width_um`), in board units; 0 = no limit (Java
    /// `minTraceHalfWidth`, `46bf9e2ef`, after the pin; unset changes
    /// nothing).
    pub min_trace_half_width: i32,
    /// Whether each layer may be used for routing.
    pub layer_active: Vec<bool>,
    pub layer_count: usize,
    /// The trace half width on each layer.
    pub trace_half_width: Vec<i32>,
    /// The trace half width plus the clearance compensation on each layer.
    pub compensated_trace_half_width: Vec<i32>,
    pub via_radii: Vec<f64>,
    /// The additional via costs between layers (all 0 at the pin).
    pub add_via_costs: Vec<ViaCost>,
    pub trace_clearance_class_index: i32,
    pub vias_allowed: bool,
    /// True if some via of the rule may sit on an SMD pad.
    pub attach_smd_allowed: bool,
    pub min_normal_via_cost: f64,
    pub ripup_allowed: bool,
    pub ripup_costs: i32,
    /// Never set by the batch router at the pin: always 1
    /// (`AutorouteControl.java:192`, A00 question 3).
    pub ripup_pass_no: i32,
    /// The search completes after the first drill.
    pub is_fanout: bool,
    /// The fanout connects the pin to its net's plane.
    pub is_plane_fanout: bool,
    pub fanout_start_pin_center: Option<Point>,
    pub fanout_start_pin_layer: i32,
    pub remove_unconnected_vias: bool,
    pub via_rule: ViaRuleId,
    /// The vias of a via rule that is not on the board, in place of
    /// `via_rule`'s (the fanout's merged rule, A12.1); `None` for the
    /// board's rule.
    pub merged_via_rule: Option<Vec<ViaInfoId>>,
    pub net_number: i32,
    /// Per layer: true if the current net has a plane (conduction area) on
    /// that power layer. Power layers are never active, and the maze search
    /// enters one only for the nets of its planes, so a via can land on the
    /// net's own plane but no other net is routed across it (Java
    /// `ownPlaneLayers`, `7f56b062d`, after the pin; Q01.7).
    pub own_plane_layers: Vec<bool>,
    /// Q13 (`router.power_layer_traces = own_plane | none`): per layer, the
    /// current net's conduction areas on that power layer, inside which
    /// alone the maze search may lay its trace there (`own_plane`) or land
    /// its via (`none`, which lays no trace there)
    /// ([`power_layer_traces`](super::power_layer_traces)). `None` under
    /// Java's rule (`layer`).
    pub own_plane_areas: Option<Vec<Vec<ItemId>>>,
    /// `router.power_layer_traces`; [`init_net`](Self::init_net) reads it to
    /// set [`Self::own_plane_areas`].
    pub power_layer_traces: crate::autoroute::power_layer_traces::PowerLayerTraces,
    /// True if the board may have trace obstacles that let vias through
    /// (Q09: a trace-only keepout, or the pour guard's `traces` mode), so a
    /// drill may pass a layer where only those cover its location. False on
    /// the parity path, where the maze search is Java's.
    pub vias_pass_trace_only_areas: bool,
    pub via_clearance_class: i32,
    /// Q14 (`router.via_clearance = net_class`): the via clearance class of
    /// the net's class, which every via the search checks or inserts uses
    /// in place of its via info's ([`Self::via_info_clearance_class`]).
    /// `None` under Java's rule, and for a net without a class.
    pub net_class_via_clearance_class: Option<i32>,
    /// `router.via_clearance` is `net_class`; [`init_net`](Self::init_net)
    /// reads it to set [`Self::net_class_via_clearance_class`].
    pub via_clearance_follows_net_class: bool,
    pub via_infos: Vec<ViaMask>,
    pub via_lower_bound: i32,
    pub via_upper_bound: i32,
    pub max_via_radius: f64,
    pub tidy_region_width: i32,
    pub pull_tight_accuracy: i32,
    pub max_shove_trace_recursion_depth: i32,
    pub max_shove_via_recursion_depth: i32,
    pub max_spring_over_recursion_depth: i32,
    pub min_cheap_via_cost: f64,
    /// Java's `FRLogger.warn` lines of the construction, in order.
    pub warnings: Vec<String>,
    /// When set, only these items end the search; other items of the net
    /// that are not in the start set are ignored as targets. `None` is
    /// Java's rule (every item of the net outside the start set is a
    /// destination), and the only value the Freerouting stages use. For
    /// algorithm stages that route between two chosen sets of one net
    /// (X02's straps between pour islands, past the net's own plane).
    pub destinations_only: Option<std::collections::BTreeSet<ItemId>>,
}

impl AutorouteControl {
    /// Java's five-argument constructor: the control of net `net_number`
    /// with the given via costs and trace costs (what the batch router
    /// passes: `getPlaneViaCosts()` for a plane net, else `getViaCosts()`,
    /// and its own trace costs).
    pub fn for_net(
        board: &BasicBoard,
        net_number: i32,
        settings: &BoardRouterSettings,
        via_costs: i32,
        trace_costs: &[ExpansionCostFactor],
    ) -> Result<AutorouteControl, ControlError> {
        let mut control = Self::new(board, settings, trace_costs.to_vec());
        control.init_net(net_number, board, via_costs)?;
        Ok(control)
    }

    /// Java's three-argument constructor: the settings' trace costs and
    /// `getViaCosts()`.
    pub fn for_net_default(
        board: &BasicBoard,
        net_number: i32,
        settings: &BoardRouterSettings,
    ) -> Result<AutorouteControl, ControlError> {
        Self::for_net(
            board,
            net_number,
            settings,
            settings.via_costs(),
            settings.trace_costs(),
        )
    }

    /// The private constructor (`AutorouteControl.java:140-193`).
    fn new(
        board: &BasicBoard,
        settings: &BoardRouterSettings,
        trace_costs: Vec<ExpansionCostFactor>,
    ) -> AutorouteControl {
        let layer_count = board.layer_count();
        let mut warnings = Vec::new();
        let bend_costs = (0..layer_count).map(|i| settings.bend_cost(i)).collect();
        let layer_active = (0..layer_count)
            .map(|i| {
                let active = settings.layer_active(i);
                let layer = &board.layer_structure.layers[i];
                // PARITY: AutorouteControl.java:162-171, a plane layer the
                // settings mark active is forced inactive, with this warning.
                if !layer.is_signal && active {
                    warnings.push(format!(
                        "Layer '{}' is a dedicated power plane and cannot be routed. \
                         Forcing active state to false.",
                        layer.name
                    ));
                    false
                } else {
                    active
                }
            })
            .collect();
        AutorouteControl {
            trace_costs,
            bend_costs,
            with_neckdown: settings.automatic_neckdown(),
            min_trace_half_width: min_trace_half_width(board, settings),
            layer_active,
            layer_count,
            trace_half_width: vec![0; layer_count],
            compensated_trace_half_width: vec![0; layer_count],
            via_radii: vec![0.0; layer_count],
            add_via_costs: vec![
                ViaCost {
                    to_layer: vec![0; layer_count],
                };
                layer_count
            ],
            trace_clearance_class_index: 0,
            vias_allowed: settings.vias_allowed(),
            attach_smd_allowed: false,
            min_normal_via_cost: 0.0,
            ripup_allowed: false,
            ripup_costs: 1000,
            ripup_pass_no: 1,
            is_fanout: false,
            is_plane_fanout: false,
            fanout_start_pin_center: None,
            fanout_start_pin_layer: -1,
            remove_unconnected_vias: true,
            via_rule: ViaRuleId(0),
            merged_via_rule: None,
            net_number: 0,
            own_plane_layers: vec![false; layer_count],
            own_plane_areas: None,
            power_layer_traces: settings.power_layer_traces(),
            vias_pass_trace_only_areas: false,
            via_clearance_class: 0,
            net_class_via_clearance_class: None,
            via_clearance_follows_net_class: settings.via_clearance() == ViaClearanceMode::NetClass,
            via_infos: Vec::new(),
            via_lower_bound: 0,
            via_upper_bound: layer_count as i32,
            max_via_radius: 0.0,
            tidy_region_width: i32::MAX,
            pull_tight_accuracy: 500,
            max_shove_trace_recursion_depth: 20,
            max_shove_via_recursion_depth: 8,
            max_spring_over_recursion_depth: 8,
            min_cheap_via_cost: 0.0,
            warnings,
            destinations_only: None,
        }
    }

    /// `initNet` (`AutorouteControl.java:210-238`).
    fn init_net(
        &mut self,
        net_number: i32,
        board: &BasicBoard,
        via_costs: i32,
    ) -> Result<(), ControlError> {
        self.net_number = net_number;
        let rules = &board.rules;
        let net_class = match rules.nets.get(net_number) {
            Some(net) => {
                let class = rules.net_classes.get(net.net_class());
                self.trace_clearance_class_index = class.trace_clearance_class();
                self.via_rule = class
                    .via_rule()
                    .ok_or(ControlError::NoViaRule(net_number))?;
                self.net_class_via_clearance_class = self
                    .via_clearance_follows_net_class
                    .then(|| class.default_item_clearance_classes.get(ItemClass::Via));
                Some(class)
            }
            None => {
                // PARITY: AutorouteControl.java:220-222, the fallback for a
                // missing net: class 1 and `viaRules.firstElement()`.
                self.trace_clearance_class_index = 1;
                self.via_rule = rules.via_rules.nth(0).ok_or(ControlError::NoViaRules)?;
                self.net_class_via_clearance_class = None;
                None
            }
        };
        // Net 0 (and below) takes the widths of net 1.
        let width_net = if net_number > 0 { net_number } else { 1 };
        let width_net_exists = rules.nets.get(width_net).is_some();
        for i in 0..self.layer_count {
            if !width_net_exists {
                return Err(ControlError::NoSuchNet(width_net));
            }
            let layer = i as i32;
            self.trace_half_width[i] = rules.trace_half_width(width_net, layer);
            self.compensated_trace_half_width[i] = self.trace_half_width[i]
                + rules
                    .clearance_matrix
                    .clearance_compensation_value(self.trace_clearance_class_index, layer);
            if net_class.is_some_and(|class| !class.is_active_routing_layer(layer)) {
                self.layer_active[i] = false;
            }
        }
        self.rebuild_via_info(board, via_costs, net_number)?;
        self.own_plane_layers = own_plane_layers(board, net_number);
        self.own_plane_areas = self
            .power_layer_traces
            .needs_plane_areas()
            .then(|| super::power_layer_traces::own_plane_areas(board, net_number));
        self.vias_pass_trace_only_areas = board.may_let_vias_through();
        Ok(())
    }

    /// The vias of the control's via rule (Java `viaRule`): the merged
    /// rule's when set, else those of `via_rule` on the board.
    pub fn via_rule_vias<'a>(&'a self, rules: &'a BoardRules) -> &'a [ViaInfoId] {
        match &self.merged_via_rule {
            Some(vias) => vias,
            None => rules.via_rules.get(self.via_rule).vias(),
        }
    }

    /// The clearance class a via of type `info` gets when the search checks
    /// or inserts it: the info's own (Java), or under Q14's `net_class` rule
    /// the net class's via clearance class.
    pub fn via_info_clearance_class(&self, info: &ViaInfo) -> i32 {
        self.net_class_via_clearance_class
            .unwrap_or(info.clearance_class)
    }

    /// `rebuildViaInfo` (`AutorouteControl.java:240-286`): the via masks and
    /// the via costs of the current via rule.
    pub fn rebuild_via_info(
        &mut self,
        board: &BasicBoard,
        via_costs: i32,
        net_number: i32,
    ) -> Result<(), ControlError> {
        let rules = &board.rules;
        let vias = self.via_rule_vias(rules).to_vec();
        self.via_clearance_class = match vias.first() {
            Some(via) => self.via_info_clearance_class(rules.via_infos.get(*via)),
            None => 1,
        };
        // N06.2: a rebuild must also handle shrinking rules.
        self.via_radii.fill(0.0);
        self.max_via_radius = 0.0;
        self.via_infos = Vec::with_capacity(vias.len());
        self.attach_smd_allowed = false;
        for &via_id in &vias {
            let via = rules.via_infos.get(via_id);
            if via.attach_smd_allowed {
                self.attach_smd_allowed = true;
            }
            let padstack = board.library.padstacks.get(via.padstack);
            let (from_layer, to_layer) =
                padstack.map_or((0, -1), |p| (p.from_layer(), p.to_layer()));
            for j in from_layer..=to_layer {
                let radius = padstack
                    .and_then(|p| p.get_shape(j))
                    .map_or(0.0, |shape| 0.5 * shape.max_width());
                let slot = usize::try_from(j)
                    .ok()
                    .and_then(|j| self.via_radii.get_mut(j))
                    .ok_or(ControlError::LayerOutOfRange(j))?;
                // PARITY(quirk): AutorouteControl.java:268 never resets viaRadii;
                // N06.2 fixes that outright by clearing it before this loop.
                *slot = max_f64(*slot, radius);
            }
            self.via_infos.push(ViaMask {
                from_layer,
                to_layer,
                attach_smd_allowed: via.attach_smd_allowed,
            });
        }
        let pure_smd_net = is_pure_smd_net(board, net_number);
        for j in 0..self.layer_count {
            self.via_radii[j] = max_f64(self.via_radii[j], f64::from(self.trace_half_width[j]));
            // PARITY(quirk): AutorouteControl.java:278 never resets maxViaRadius;
            // N06.2 fixes that outright before rebuilding the masks.
            self.max_via_radius = max_f64(self.max_via_radius, self.via_radii[j]);
        }
        // PARITY: AutorouteControl.java:280-287, this operation order.
        let mut via_cost_factor = self.max_via_radius;
        via_cost_factor = max_f64(via_cost_factor, 1.0);
        if pure_smd_net {
            via_cost_factor *= 0.1;
        }
        self.min_normal_via_cost = f64::from(via_costs) * via_cost_factor;
        self.min_cheap_via_cost = 0.8 * self.min_normal_via_cost;
        Ok(())
    }

    /// Java `neckdownHalfWidth(pin, layer)` (`46bf9e2ef`): the half width a
    /// neck-down into `pin` on `layer` uses, the pin's own
    /// `getTraceNeckdownHalfwidth`, but never less than
    /// [`min_trace_half_width`](Self::min_trace_half_width).
    pub fn neckdown_half_width(&self, board: &BasicBoard, pin: ItemId, layer: i32) -> i32 {
        board
            .trace_neckdown_half_width(pin, layer)
            .max(self.min_trace_half_width)
    }
}

/// The power layers that carry a plane (conduction area) of net
/// `net_number` (Java `initNet`'s `ownPlaneLayers` loop, `7f56b062d`).
fn own_plane_layers(board: &BasicBoard, net_number: i32) -> Vec<bool> {
    let mut own = vec![false; board.layer_count()];
    for id in board.conduction_areas() {
        let Some(area) = board.item(id) else {
            continue;
        };
        let index = area.first_layer() as usize;
        if !board.layer_structure.layers[index].is_signal && area.contains_net(net_number) {
            own[index] = true;
        }
    }
    own
}

/// Java `AutorouteControl.minTraceHalfWidth(board, settings)`
/// (`46bf9e2ef`): `router.min_trace_width_um` in board units, halved and
/// rounded up; 0 when the setting is unset or not positive.
pub fn min_trace_half_width(board: &BasicBoard, settings: &BoardRouterSettings) -> i32 {
    let min_width_um = settings.min_trace_width_um();
    if min_width_um <= 0.0 {
        return 0;
    }
    let resolution = board.communication.resolution.max(1);
    let min_width = Unit::scale(
        min_width_um * f64::from(resolution),
        Unit::Um,
        board.communication.unit,
    );
    crate::geometry::jmath::d2i((min_width / 2.0).ceil())
}

/// `isPureSmdNet` (`AutorouteControl.java:195-208`): the net has
/// connectable items and all are single-layer pins.
fn is_pure_smd_net(board: &BasicBoard, net_number: i32) -> bool {
    let items = board.connectable_items(net_number);
    !items.is_empty()
        && items.iter().all(|id| {
            board
                .item(*id)
                .is_some_and(|item| item.is_pin() && item.first_layer() == item.last_layer())
        })
}
