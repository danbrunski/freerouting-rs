//! The editor's settings and the rules a route uses (I01.1).
//!
//! [`EditSettings`] is the engine subset of Java's
//! `gui.workspace.WorkspaceSettings` (the fields interactive routing reads),
//! with Java's defaults. [`resolve_route_params`] is
//! `GuiBoardRoutingSettings.getTraceHalfwidth`, `isActiveRoutingLayer`,
//! `getTraceClearanceClass` and `getViaRule`
//! (`gui/workspace/controllers/GuiBoardRoutingSettings.java` at `a773a70ec`)
//! as `RouteState.getInstance` combines them: the half width per layer and
//! the clearance class and via rule of the first route net, a layer active if
//! any route net may use it.
//!
//! Two lookups end in a Java exception and have a defined result here
//! (DIVERGENCES, "Interactive routing"): a net number without a net (Java's
//! `NullPointerException` in `getTraceHalfWidth`, `getTraceClearanceClass`
//! and `getViaRule`) has half width 0, the default clearance class and no via
//! rule; a manual via-rule index out of range (Java's
//! `ArrayIndexOutOfBoundsException` from `Vector.get`) counts as a missing
//! manual rule and falls back to the net class's rule, as Java's `null`
//! check does.

use crate::board::BasicBoard;
use crate::rules::{BoardRules, ViaRuleId};

/// The layers the GUI shows (Java: a positive
/// `graphicsContext.getLayerVisibility(i)`). Route start skips invisible
/// layers when it looks for an item on the other layers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerMask {
    visible: Vec<bool>,
}

impl LayerMask {
    /// Every one of `layer_count` layers visible.
    pub fn all_visible(layer_count: usize) -> Self {
        LayerMask {
            visible: vec![true; layer_count],
        }
    }

    /// The mask with the given visibility per layer.
    pub fn from_visibility(visible: Vec<bool>) -> Self {
        LayerMask { visible }
    }

    /// True if `layer` is visible; a layer the mask doesn't cover counts as
    /// visible.
    pub fn is_visible(&self, layer: i32) -> bool {
        usize::try_from(layer)
            .ok()
            .and_then(|i| self.visible.get(i).copied())
            .unwrap_or(true)
    }

    /// Sets the visibility of `layer` (ignored out of range).
    pub fn set_visible(&mut self, layer: i32, visible: bool) {
        if let Some(v) = usize::try_from(layer)
            .ok()
            .and_then(|i| self.visible.get_mut(i))
        {
            *v = visible;
        }
    }
}

/// The manual rule selection of Java's route parameter window
/// (`WorkspaceSettings.manualTraceHalfWidthArr`, `manualTraceClearanceClass`,
/// `manualViaRuleIndex`). Used only while
/// [`EditSettings::manual_rule_selection`] is on; the values persist when it
/// is off, as in Java.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManualRules {
    /// The trace half width per layer (Java's default 1000 on every layer).
    pub trace_half_widths: Vec<i32>,
    /// The trace clearance class (Java's default 1).
    pub clearance_class: i32,
    /// The index of the via rule in the board's via rules (Java's default 0).
    pub via_rule_index: i32,
}

impl ManualRules {
    /// Java's defaults for a board with `layer_count` layers.
    pub fn new(layer_count: usize) -> Self {
        ManualRules {
            trace_half_widths: vec![1000; layer_count],
            clearance_class: 1,
            via_rule_index: 0,
        }
    }

    /// Java `WorkspaceSettings.getManualTraceHalfWidth`: 0 out of range.
    pub fn trace_half_width(&self, layer: i32) -> i32 {
        usize::try_from(layer)
            .ok()
            .and_then(|i| self.trace_half_widths.get(i).copied())
            .unwrap_or(0)
    }
}

/// The editor settings interactive routing reads: the engine subset of
/// Java's `WorkspaceSettings`, with its defaults ([`EditSettings::new`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditSettings {
    /// The current layer (Java `layer`, default 0).
    pub layer: i32,
    /// Push and shove obstacles while routing (Java `pushEnabled`, default
    /// on). A route reads it when it starts; cancel reads it again.
    pub push_enabled: bool,
    /// Route in stitch mode, one corner per click (Java `isStitchRoute`,
    /// default off).
    pub stitch_route: bool,
    /// The pull-tight region around a new corner (Java
    /// `tracePullTightRegionWidth`, default `i32::MAX`: no clip; 0 skips the
    /// pull-tight).
    pub trace_tidy_width: i32,
    /// The pull-tight accuracy (Java `tracePullTightAccuracy`, default 500).
    pub pull_tight_accuracy: i32,
    /// Neck down at pins that can't take the full width (Java
    /// `automaticNeckdown`, default on).
    pub automatic_neckdown: bool,
    /// Snap a layer-change via to the centre of an own-net SMD pin (Java
    /// `viaSnapToSmdCenter`, default on).
    pub via_snap_to_smd_center: bool,
    /// Show the obstacle a shove failed at (Java `highlightRoutingObstacle`,
    /// default off).
    pub highlight_routing_obstacle: bool,
    /// Look for a start item on the other visible layers when there is none
    /// on the current one (Java `selectOnAllVisibleLayers`, default on).
    pub select_on_all_visible_layers: bool,
    /// Use [`manual_rules`](Self::manual_rules) instead of the net classes
    /// (Java `manualRuleSelection`, default off).
    pub manual_rule_selection: bool,
    pub manual_rules: ManualRules,
    /// The layers the GUI shows.
    pub visible_layers: LayerMask,
}

impl EditSettings {
    /// Java's `WorkspaceSettings(board)` defaults for a board with
    /// `layer_count` layers.
    pub fn new(layer_count: usize) -> Self {
        EditSettings {
            layer: 0,
            push_enabled: true,
            stitch_route: false,
            trace_tidy_width: i32::MAX,
            pull_tight_accuracy: 500,
            automatic_neckdown: true,
            via_snap_to_smd_center: true,
            highlight_routing_obstacle: false,
            select_on_all_visible_layers: true,
            manual_rule_selection: false,
            manual_rules: ManualRules::new(layer_count),
            visible_layers: LayerMask::all_visible(layer_count),
        }
    }

    /// [`new`](Self::new) for `board`.
    pub fn for_board(board: &BasicBoard) -> Self {
        Self::new(board.layer_count())
    }
}

/// The rules a route uses: Java `Route`'s `penHalfWidthArr`, `layerActive`,
/// `clearanceClass` and `viaRule` as `RouteState.getInstance` computes them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteParams {
    /// The trace half width per layer.
    pub half_widths: Vec<i32>,
    /// The layers the route may use.
    pub layer_active: Vec<bool>,
    pub clearance_class: i32,
    /// `None` if the route can't change layers.
    pub via_rule: Option<ViaRuleId>,
}

impl RouteParams {
    /// True if `layer` is in range and active (Java `Route.isLayerActive`).
    pub fn is_layer_active(&self, layer: i32) -> bool {
        usize::try_from(layer)
            .ok()
            .and_then(|i| self.layer_active.get(i).copied())
            .unwrap_or(false)
    }
}

/// The route rules for the nets `nets` (the first one decides the half
/// widths, clearance class and via rule) on `board`.
pub fn resolve_route_params(
    board: &BasicBoard,
    settings: &EditSettings,
    nets: &[i32],
) -> RouteParams {
    resolve_with_rules(&board.rules, board.layer_count(), settings, nets)
}

/// [`resolve_route_params`] on the rules alone.
pub fn resolve_with_rules(
    rules: &BoardRules,
    layer_count: usize,
    settings: &EditSettings,
    nets: &[i32],
) -> RouteParams {
    let first = nets.first().copied().unwrap_or(0);
    let layers = 0..layer_count as i32;
    RouteParams {
        half_widths: layers
            .clone()
            .map(|layer| trace_half_width(rules, settings, first, layer))
            .collect(),
        layer_active: layers
            .map(|layer| {
                nets.iter()
                    .any(|&net| is_active_routing_layer(rules, settings, net, layer))
            })
            .collect(),
        clearance_class: trace_clearance_class(rules, settings, first),
        via_rule: via_rule(rules, settings, first),
    }
}

/// Java `GuiBoardRoutingSettings.getTraceHalfwidth`.
pub fn trace_half_width(rules: &BoardRules, settings: &EditSettings, net: i32, layer: i32) -> i32 {
    if settings.manual_rule_selection {
        settings.manual_rules.trace_half_width(layer)
    } else {
        rules.trace_half_width(net, layer)
    }
}

/// Java `GuiBoardRoutingSettings.isActiveRoutingLayer`: always with manual
/// rules or for a net without a net class.
pub fn is_active_routing_layer(
    rules: &BoardRules,
    settings: &EditSettings,
    net: i32,
    layer: i32,
) -> bool {
    if settings.manual_rule_selection {
        return true;
    }
    rules
        .net_class_of(net)
        .is_none_or(|class| class.is_active_routing_layer(layer))
}

/// Java `GuiBoardRoutingSettings.getTraceClearanceClass`.
pub fn trace_clearance_class(rules: &BoardRules, settings: &EditSettings, net: i32) -> i32 {
    if settings.manual_rule_selection {
        return settings.manual_rules.clearance_class;
    }
    rules
        .net_class_of(net)
        .map_or(BoardRules::default_clearance_class(), |class| {
            class.trace_clearance_class()
        })
}

/// Java `GuiBoardRoutingSettings.getViaRule`: the manual rule, else (also
/// for a manual index out of range) the net class's.
pub fn via_rule(rules: &BoardRules, settings: &EditSettings, net: i32) -> Option<ViaRuleId> {
    let manual = if settings.manual_rule_selection {
        usize::try_from(settings.manual_rules.via_rule_index)
            .ok()
            .and_then(|i| rules.via_rules.nth(i))
    } else {
        None
    };
    manual.or_else(|| rules.net_class_of(net).and_then(|class| class.via_rule()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::structure::{Layer, LayerStructure};
    use crate::rules::{ClearanceMatrix, ViaRule};

    /// Three layers (the middle one a plane), the default class (half width
    /// 1500, clearance class 1, via rule "default") and a class "power"
    /// (half widths 400/0/600, clearance class 2, via rule "power", not on
    /// layer 2); net 1 in the default class, nets 2 and 3 in "power".
    fn rules() -> BoardRules {
        let layers = LayerStructure::new(vec![
            Layer::new("Top", true),
            Layer::new("GND", false),
            Layer::new("Bottom", true),
        ]);
        let mut matrix = ClearanceMatrix::default_instance(3, 100);
        matrix.append_class("power");
        let mut rules = BoardRules::new(layers, matrix);
        let default_rule = rules.via_rules.add(ViaRule::new("default"));
        let power_rule = rules.via_rules.add(ViaRule::new("power"));
        let default = rules.default_net_class();
        rules
            .net_classes
            .get_mut(default)
            .set_via_rule(Some(default_rule));
        rules.add_net("A", 1, false);
        let power = rules.append_named_net_class("power");
        let class = rules.net_classes.get_mut(power);
        class.set_trace_half_width_on_layer(0, 400);
        class.set_trace_half_width_on_layer(2, 600);
        class.set_trace_clearance_class(2);
        class.set_via_rule(Some(power_rule));
        class.set_active_routing_layer(2, false);
        for name in ["B", "C"] {
            let net = rules.add_net(name, 1, false);
            rules.nets.get_mut(net).expect("net").set_class(power);
        }
        rules
    }

    fn rule_name(rules: &BoardRules, id: Option<ViaRuleId>) -> Option<String> {
        id.map(|id| rules.via_rules.get(id).name.clone())
    }

    #[test]
    fn net_class_rules_of_the_first_net() {
        let rules = rules();
        let settings = EditSettings::new(3);
        let p = resolve_with_rules(&rules, 3, &settings, &[2]);
        assert_eq!(p.half_widths[0], 400);
        assert_eq!(p.half_widths[2], 600);
        assert_eq!(p.clearance_class, 2);
        assert_eq!(rule_name(&rules, p.via_rule).as_deref(), Some("power"));
        // the plane layer is no routing layer of either class
        assert_eq!(p.layer_active, vec![true, false, false]);
        let p = resolve_with_rules(&rules, 3, &settings, &[1]);
        assert_eq!(p.half_widths, vec![1500, 1500, 1500]);
        assert_eq!(p.clearance_class, 1);
        assert_eq!(rule_name(&rules, p.via_rule).as_deref(), Some("default"));
        assert_eq!(p.layer_active, vec![true, false, true]);
    }

    #[test]
    fn a_layer_is_active_if_any_route_net_may_use_it() {
        let rules = rules();
        let settings = EditSettings::new(3);
        // net 2 (power) may not use layer 2, net 1 (default) may
        let p = resolve_with_rules(&rules, 3, &settings, &[2, 1]);
        assert_eq!(p.layer_active, vec![true, false, true]);
        // widths, class and rule still come from the first net
        assert_eq!(p.half_widths[0], 400);
        assert_eq!(p.clearance_class, 2);
        assert!(!p.is_layer_active(3));
        assert!(!p.is_layer_active(-1));
    }

    #[test]
    fn manual_rules_override_the_net_class() {
        let rules = rules();
        let mut settings = EditSettings::new(3);
        settings.manual_rule_selection = true;
        settings.manual_rules.trace_half_widths = vec![111, 222, 333];
        settings.manual_rules.clearance_class = 3;
        settings.manual_rules.via_rule_index = 0;
        let p = resolve_with_rules(&rules, 3, &settings, &[2]);
        assert_eq!(p.half_widths, vec![111, 222, 333]);
        assert_eq!(p.clearance_class, 3);
        assert_eq!(rule_name(&rules, p.via_rule).as_deref(), Some("default"));
        // every layer is active with manual rules
        assert_eq!(p.layer_active, vec![true, true, true]);
        // off again: the manual values stay but are not used
        settings.manual_rule_selection = false;
        let p = resolve_with_rules(&rules, 3, &settings, &[2]);
        assert_eq!(p.clearance_class, 2);
        // a short manual width array gives 0 (Java's warning branch)
        settings.manual_rule_selection = true;
        settings.manual_rules.trace_half_widths = vec![5];
        let p = resolve_with_rules(&rules, 3, &settings, &[2]);
        assert_eq!(p.half_widths, vec![5, 0, 0]);
    }

    #[test]
    fn manual_via_rule_index_out_of_range_falls_back_to_the_net_class() {
        let rules = rules();
        let mut settings = EditSettings::new(3);
        settings.manual_rule_selection = true;
        for index in [2, 99, -1] {
            settings.manual_rules.via_rule_index = index;
            let p = resolve_with_rules(&rules, 3, &settings, &[2]);
            assert_eq!(
                rule_name(&rules, p.via_rule).as_deref(),
                Some("power"),
                "index {index}"
            );
        }
        settings.manual_rules.via_rule_index = 1;
        let p = resolve_with_rules(&rules, 3, &settings, &[1]);
        assert_eq!(rule_name(&rules, p.via_rule).as_deref(), Some("power"));
    }

    /// Every net of the port has a net class; a net number without a net
    /// has none: every layer active (Java's `null` class branch), half width
    /// 0, the default clearance class and no via rule (where Java throws).
    #[test]
    fn a_net_without_a_net_class() {
        let rules = rules();
        let settings = EditSettings::new(3);
        let p = resolve_with_rules(&rules, 3, &settings, &[42]);
        assert_eq!(p.half_widths, vec![0, 0, 0]);
        assert_eq!(p.layer_active, vec![true, true, true]);
        assert_eq!(p.clearance_class, 1);
        assert_eq!(p.via_rule, None);
        let p = resolve_with_rules(&rules, 3, &settings, &[]);
        assert_eq!(p.layer_active, vec![false, false, false]);
    }

    #[test]
    fn layer_mask_and_defaults() {
        let mut mask = LayerMask::all_visible(2);
        mask.set_visible(1, false);
        assert!(mask.is_visible(0) && !mask.is_visible(1) && mask.is_visible(5));
        let s = EditSettings::new(2);
        assert!(s.push_enabled && s.select_on_all_visible_layers && s.automatic_neckdown);
        assert!(!s.stitch_route && !s.manual_rule_selection && !s.highlight_routing_obstacle);
        assert_eq!((s.trace_tidy_width, s.pull_tight_accuracy), (i32::MAX, 500));
        assert_eq!(s.manual_rules, ManualRules::new(2));
        assert_eq!(s.manual_rules.trace_half_widths, vec![1000, 1000]);
    }
}
