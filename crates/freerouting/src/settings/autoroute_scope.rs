//! Java's `AutorouteSettings.readScope` and `readLayerRule`, and the
//! `DsnFileSettings` (priority 20) and `RulesFileSettings` (priority 40)
//! sources built on them: an `autoroute_settings` scope to a
//! [`SettingsPatch`].
//!
//! - **Seeding.** The patch starts with `set_layer_count(n)`: every layer
//!   routable with the other layer members unset, both cost arrays
//!   `[1.0; n]`, `costs_initialized = Some(false)`.
//! - `vias` sets `allowed_via_types`; `via_costs`, `plane_via_costs` and
//!   `start_ripup_costs` are clamped to at least 1.
//! - **`layer_rule`.** The layer is looked up by name (Java's
//!   `LayerStructure.getNo`: the exact name, else a name containing `Top`
//!   is layer 0 and one containing `Bottom` the last layer); an unknown
//!   layer aborts the whole scope. `active` sets `routable`,
//!   `preferred_direction` the direction. The two trace costs are clamped to
//!   at least 0.1; each writes the array entry and the layer member and sets
//!   `costs_initialized = Some(true)`.
//! - `autorouter.enabled` comes from `autoroute` and `optimizer.enabled`
//!   from `postroute`, both on when absent.
//! - A DSN *without* the scope still yields the seeded patch, with only the
//!   layer count. A rules file without it yields an empty patch.

use super::fields::SettingsPatch;
use crate::dsn::ast::{AutorouteSettings, Direction, Pcb, RulesFile, RulesItem};

/// `LayerStructure.getNo(name)`.
pub fn layer_number(layer_names: &[String], name: &str) -> Option<usize> {
    if let Some(i) = layer_names.iter().position(|n| n == name) {
        return Some(i);
    }
    if name.contains("Top") {
        return Some(0);
    }
    if name.contains("Bottom") {
        return layer_names.len().checked_sub(1);
    }
    None
}

/// `AutorouteSettings.readScope`: `None` when Java returns `null` (a
/// `layer_rule` naming an unknown layer).
pub fn read_scope(scope: &AutorouteSettings, layer_names: &[String]) -> Option<SettingsPatch> {
    let mut result = SettingsPatch::default();
    result.set_layer_count(layer_names.len());
    if let Some(vias) = scope.vias {
        result.set_vias_allowed(vias);
    }
    if let Some(v) = scope.via_costs {
        result.set_via_costs(v);
    }
    if let Some(v) = scope.plane_via_costs {
        result.set_plane_via_costs(v);
    }
    if let Some(v) = scope.start_ripup_costs {
        result.set_start_ripup_costs(v);
    }
    for rule in &scope.layer_rules {
        let layer = layer_number(layer_names, &rule.layer)?;
        if let Some(active) = rule.active {
            result.set_layer_active(layer, active);
        }
        if let Some(direction) = rule.preferred_direction {
            result.set_preferred_direction_is_horizontal(layer, direction == Direction::Horizontal);
        }
        if let Some(cost) = rule.preferred_direction_trace_costs {
            result.set_preferred_direction_trace_costs(layer, cost);
        }
        if let Some(cost) = rule.against_preferred_direction_trace_costs {
            result.set_against_preferred_direction_trace_costs(layer, cost);
        }
    }
    result.set_run_router(scope.autoroute.unwrap_or(true));
    result.set_run_optimizer(scope.postroute.unwrap_or(true));
    Some(result)
}

/// `DsnFileSettings`: the patch of a DSN file.
///
/// The layer count is the number of `(layer ...)` entries in the structure.
/// It seeds the layer arrays whenever the scope did not.
pub fn patch_from_dsn(pcb: &Pcb) -> SettingsPatch {
    let layer_names: Vec<String> = pcb
        .structure
        .as_ref()
        .map(|s| s.layers.iter().map(|l| l.name.clone()).collect())
        .unwrap_or_default();
    let extracted = pcb
        .structure
        .as_ref()
        .and_then(|s| s.autoroute_settings.as_ref())
        .and_then(|scope| read_scope(scope, &layer_names));
    let mut patch = extracted.unwrap_or_default();
    if !layer_names.is_empty() && patch.layer_count() == 0 {
        patch.set_layer_count(layer_names.len());
    }
    patch
}

/// `RulesReader.discoverLayerStructure`: the distinct names after
/// `(layer_rule` and `(layer` in file order, `F.Cu` and `B.Cu` when there
/// are none.
pub fn rules_layer_names(rules: &RulesFile) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut add = |name: &str| {
        if !name.trim().is_empty() && !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
    };
    for item in &rules.items {
        match item {
            RulesItem::Layer { name, .. } => add(name),
            RulesItem::AutorouteSettings(scope) => {
                for rule in &scope.layer_rules {
                    add(&rule.layer);
                }
            }
            _ => {}
        }
    }
    if names.is_empty() {
        names.push("F.Cu".to_string());
        names.push("B.Cu".to_string());
    }
    names
}

/// `RulesFileSettings`: the patch of a rules file, from its first
/// `autoroute_settings` scope; empty when there is none.
pub fn patch_from_rules(rules: &RulesFile) -> SettingsPatch {
    let names = rules_layer_names(rules);
    rules
        .items
        .iter()
        .find_map(|item| match item {
            RulesItem::AutorouteSettings(scope) => Some(read_scope(scope, &names)),
            _ => None,
        })
        .flatten()
        .unwrap_or_default()
}
