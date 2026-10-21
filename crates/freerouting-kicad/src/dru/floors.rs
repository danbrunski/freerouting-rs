//! Board-wide `.kicad_dru` rules (K15.3): minimums and `disallow`.
//!
//! # What KiCad does with a rule that has no condition
//!
//! KiCad's custom-rules language (the "Custom Rules" page of the PCB editor's
//! documentation) applies a rule to every pair of items it can constrain when
//! it has no `(condition ...)` clause: the condition is "always true". A
//! `(layer ...)` clause restricts it to items on those layers, `outer` and
//! `inner` meaning the outer and the inner copper layers. For a `min` of
//! `clearance`, `track_width`, `hole_size`, `annular_width` and
//! `edge_clearance` the value is a lower bound the DRC checks, so a rule
//! below Board Setup's value (or below another rule's) changes nothing the
//! DRC reports, and one above it raises the bound. The reader takes a `min`
//! as exactly that: **a floor that only raises**.
//!
//! # The floors
//!
//! A constraint whose condition holds for the item it is about (a track for
//! `track_width`, `clearance` and `edge_clearance`; a via for `hole_size` and
//! `annular_width`) in every net class is a board-wide floor, carried in
//! [`BoardFloors`] for the caller to raise the matching setting to, as the
//! `hole_clearance` and `hole_to_hole` floors are (Q22.5, Q22.14):
//!
//! | constraint (`min`) | setting |
//! |---|---|
//! | `track_width` | `router.board_minimums.track_width_um` |
//! | `clearance` (no condition) | `router.board_minimums.clearance_um`, under every class pair |
//! | `hole_size` | `router.board_minimums.via_drill_um` (the router places vias, not pads) |
//! | `annular_width` | `router.board_minimums.via_annular_um` (vias; a rule about pads is not placed) |
//! | `edge_clearance` | `router.copper_to_edge_clearance_um` |
//!
//! The largest `min` of the matching rules wins, and the caller never lowers
//! a value Board Setup, the command line or another rule set. A rule that
//! asks for less than the DSN or Board Setup gives is still APPLIED; the log
//! line of the raise says "no less".
//!
//! **Narrower than KiCad.**
//!
//! - A `(layer ...)` clause or layer term that does not cover every copper
//!   layer is applied to every layer, with a warning naming the rule (the pcu
//!   file's `edge_clearance` on `outer` 0.254 mm and on `inner` 0.381 mm: **one
//!   edge clearance for all layers, the largest**, 0.381 mm).
//! - A `min` whose condition holds for some net classes only is **never a
//!   board-wide floor**: that would over-constrain every net (a 0.5 mm
//!   `track_width` for class `Power` would stop fine-pitch pins routing).
//!   `track_width` raises the width rule of each selected class to at least the
//!   `min` (the class path an `opt` width uses; a class is never made
//!   narrower, the Default class's structure width follows it, and the status
//!   is APPLIED with the classes raised, [`super::DruReport::track_widths`]
//!   naming them). The other kinds (`hole_size`, `annular_width`,
//!   `edge_clearance`) are UNSUPPORTED, "limited to some net classes; the
//!   router keeps one board-wide value". A `clearance` with a condition keeps
//!   its class-pair path.
//!
//! # `disallow`
//!
//! `(constraint disallow blind_via)` (and `buried_via`, `micro_via`) is met
//! when none of the board's via padstacks is of that kind; the rule is then
//! reported as ignored, "met". Otherwise the padstacks of that kind are
//! removed from the router's choices (the structure's `via` list, the classes'
//! `use_via`, the network's via infos). The kind is read from the padstack's
//! name `Via[a-b]_...` (KiCad's, and the reader's): a **blind** via has one
//! end on an outer layer and the other on an inner one, a **buried** via has
//! both ends inside, and a **micro** via goes from an outer layer to its
//! neighbour (the DSN cannot tell it from a blind via of the same span, so
//! `micro_via` also removes that blind via, with a warning). Any other
//! `disallow` argument (`track`, `via`, `pad`, `zone`, ...) is reported
//! UNSUPPORTED by name.
//!
//! # Left out, and reported by name
//!
//! `hole_size` with only a `max` (the pcu file's plated-slot rule, whose
//! condition also asks about a pad's holes) is a limit on the footprints; a
//! pad annular ring (`A.Hole_Size_X > 0.4mm`) is the footprint's. Their
//! reasons say so.

use std::collections::BTreeSet;

use freerouting_dsn::ast::{Network, Rule, Structure};

use super::item_types::{holds, kicad_classes};
use super::{
    ApplyCtx, Bounds, Cond, DSN_DEFAULT, DruRule, HoleClearance, ItemType, Probe, Status,
    kicad_name, set_width,
};

/// The board-wide minimums of the applied `.kicad_dru` rules (K15.3), each
/// the largest `min` of its rules with their names, in file order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BoardFloors {
    /// `track_width`: raises `router.board_minimums.track_width_um`.
    pub track_width: Option<HoleClearance>,
    /// `clearance` without a condition: raises
    /// `router.board_minimums.clearance_um`.
    pub clearance: Option<HoleClearance>,
    /// `hole_size`: raises `router.board_minimums.via_drill_um`.
    pub via_drill: Option<HoleClearance>,
    /// `annular_width` of vias: raises `router.board_minimums.via_annular_um`.
    pub via_annular: Option<HoleClearance>,
    /// `edge_clearance`, of every layer: raises
    /// `router.copper_to_edge_clearance_um`.
    pub edge_clearance: Option<HoleClearance>,
}

impl BoardFloors {
    /// Whether no rule gave a floor.
    pub fn is_empty(&self) -> bool {
        *self == BoardFloors::default()
    }
}

fn mm(nm: i64) -> String {
    let text = format!("{:.6}", nm as f64 / 1e6);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{text} mm")
}

fn raise(slot: &mut Option<HoleClearance>, nm: i64, rule: &str) {
    let floor = slot.get_or_insert(HoleClearance {
        nm,
        rules: Vec::new(),
    });
    floor.nm = floor.nm.max(nm);
    floor.rules.push(rule.to_owned());
}

/// Whether `cond` holds for `item` (a pair of the same item type) across the
/// net classes: (some pair, some pair not).
fn evaluate(
    cond: Option<&Cond>,
    item: ItemType,
    classes: &[String],
) -> Result<(bool, bool), String> {
    let names = kicad_classes(classes);
    let (mut hit, mut miss) = (false, false);
    for (i, a) in names.iter().enumerate() {
        for b in &names[i..] {
            let pa = Probe {
                class: a,
                item: Some(item),
            };
            let pb = Probe {
                class: b,
                item: Some(item),
            };
            if holds(cond, pa, pb)? {
                hit = true;
            } else {
                miss = true;
            }
        }
    }
    Ok((hit, miss))
}

/// A `min` constraint of `kind` as a board-wide floor (module docs).
#[allow(clippy::too_many_arguments)]
pub(super) fn floor(
    rule: &DruRule,
    kind: &str,
    bounds: &Bounds,
    cond: Option<&Cond>,
    layer_names: &[String],
    classes: &[String],
    ctx: &mut ApplyCtx<'_>,
    network: &mut Network,
    structure: &mut Structure,
) -> Status {
    let Some(min) = bounds.min else {
        let why = match (kind, bounds.max) {
            ("hole_size", Some(_)) => {
                "hole_size with only a max value limits the footprints' holes (a plated slot's), \
                 which the router does not place"
                    .to_owned()
            }
            _ => format!("{kind} without a min value"),
        };
        return Status::Unsupported(why);
    };
    let (item, what, setting) = match kind {
        "track_width" => (
            ItemType::Track,
            "track",
            "router.board_minimums.track_width_um",
        ),
        "clearance" => (
            ItemType::Track,
            "track",
            "router.board_minimums.clearance_um",
        ),
        "hole_size" => (ItemType::Via, "via", "router.board_minimums.via_drill_um"),
        "annular_width" => (ItemType::Via, "via", "router.board_minimums.via_annular_um"),
        _ => (
            ItemType::Track,
            "track",
            "router.copper_to_edge_clearance_um",
        ),
    };
    let (hit, miss) = match evaluate(cond, item, classes) {
        Ok(x) => x,
        Err(e) => return Status::Unsupported(e),
    };
    if !hit {
        return Status::Ignored(format!("matches no {what} the router places"));
    }
    // a `min` for some net classes only is never a board-wide floor
    if miss && kind != "track_width" {
        return Status::Unsupported(
            "limited to some net classes; the router keeps one board-wide value".to_owned(),
        );
    }
    if !ctx.apply {
        return Status::Ignored("apply_design_rules is off".to_owned());
    }
    if miss {
        return class_track_width(
            rule,
            min,
            cond,
            layer_names,
            classes,
            ctx,
            network,
            structure,
        );
    }
    let layers_all = layer_names.is_empty() || layer_names.len() >= ctx.layers.len();
    if !layers_all {
        ctx.warnings.push(format!(
            "design rule '{}' (line {}): {kind} {} becomes a floor of {setting} for every layer \
             and net class; its layers ({}) limit it in KiCad",
            rule.name,
            rule.line,
            mm(min),
            layer_names.join(", ")
        ));
    }
    if kind == "annular_width"
        && let Ok((true, _)) = evaluate(cond, ItemType::PthPad, classes)
    {
        ctx.warnings.push(format!(
            "design rule '{}' (line {}): annular_width {} is kept for the router's vias; a pad's \
             ring is the footprint's",
            rule.name,
            rule.line,
            mm(min)
        ));
    }
    let slot = match kind {
        "track_width" => &mut ctx.floors.track_width,
        "clearance" => &mut ctx.floors.clearance,
        "hole_size" => &mut ctx.floors.via_drill,
        "annular_width" => &mut ctx.floors.via_annular,
        _ => &mut ctx.floors.edge_clearance,
    };
    raise(slot, min, &rule.name);
    Status::Applied { effects: 1 }
}

/// A `track_width` `min` for some net classes only: each selected class's
/// width rule is raised to at least `min` (never lowered; a class with no
/// width rule of its own takes the structure's, as the DSN reader does), and no
/// board-wide floor is made.
#[allow(clippy::too_many_arguments)]
fn class_track_width(
    rule: &DruRule,
    min: i64,
    cond: Option<&Cond>,
    layer_names: &[String],
    classes: &[String],
    ctx: &mut ApplyCtx<'_>,
    network: &mut Network,
    structure: &mut Structure,
) -> Status {
    if !layer_names.is_empty() && layer_names.len() < ctx.layers.len() {
        return Status::Unsupported(
            "track_width limited to net classes and layers has no DSN form".to_owned(),
        );
    }
    let want = min as f64 / ctx.nm_per_unit;
    let width_of = |rules: &[Rule]| {
        rules.iter().find_map(|r| match r {
            Rule::Width(w) => Some(*w),
            _ => None,
        })
    };
    let structure_width = width_of(&structure.rules);
    let mut raised = Vec::new();
    for name in classes {
        let probe = Probe {
            class: kicad_name(name),
            item: Some(ItemType::Track),
        };
        match cond.map_or(Ok(true), |c| c.eval(probe, None)) {
            Ok(false) => continue,
            Ok(true) => {}
            Err(e) => return Status::Unsupported(e),
        }
        let Some(class) = network.classes.iter_mut().find(|c| c.name == *name) else {
            continue;
        };
        let current = width_of(&class.rules).or(structure_width).unwrap_or(0.0);
        if current < want {
            set_width(&mut class.rules, want);
            if name == DSN_DEFAULT {
                set_width(&mut structure.rules, want);
            }
            raised.push(name.clone());
        }
    }
    if raised.is_empty() {
        return Status::Ignored("asks for no more than the selected classes' widths".to_owned());
    }
    ctx.track_widths
        .extend(raised.iter().map(|c| (c.clone(), rule.name.clone())));
    Status::Applied {
        effects: raised.len(),
    }
}

/// `(first, last)` copper layer positions of a `Via[a-b]_...` padstack name.
fn via_span(name: &str) -> Option<(usize, usize)> {
    let inner = name.strip_prefix("Via[")?.split_once(']')?.0;
    let (a, b) = inner.split_once('-')?;
    let (a, b): (usize, usize) = (a.parse().ok()?, b.parse().ok()?);
    Some((a.min(b), a.max(b)))
}

/// Whether the via padstack `name` is of `kind` (`blind_via`, `buried_via`,
/// `micro_via`) on a board of `layers` copper layers; a name that gives no
/// span is a through via.
fn is_kind(name: &str, kind: &str, layers: usize) -> bool {
    let Some((lo, hi)) = via_span(name) else {
        return false;
    };
    if layers < 2 || (lo == 0 && hi + 1 >= layers) {
        return false;
    }
    let outer_end = lo == 0 || hi + 1 >= layers;
    match kind {
        "blind_via" => outer_end,
        "buried_via" => !outer_end,
        _ => outer_end && hi - lo == 1,
    }
}

/// A `disallow` constraint on via kinds (module docs).
pub(super) fn disallow(
    rule: &DruRule,
    args: &str,
    cond: Option<&Cond>,
    classes: &[String],
    ctx: &mut ApplyCtx<'_>,
    network: &mut Network,
    structure: &mut Structure,
) -> Status {
    let kinds: Vec<&str> = args.split_whitespace().collect();
    if let Some(other) = kinds
        .iter()
        .find(|k| !matches!(**k, "blind_via" | "buried_via" | "micro_via"))
    {
        return Status::Unsupported(format!(
            "disallow {other} has no form: the router has no setting to forbid it"
        ));
    }
    let (hit, miss) = match evaluate(cond, ItemType::Via, classes) {
        Ok(x) => x,
        Err(e) => return Status::Unsupported(e),
    };
    if !hit {
        return Status::Ignored("matches no via the router places".to_owned());
    }
    if hit && miss {
        return Status::Unsupported(
            "disallow limited to some net classes has no form: vias are chosen board-wide"
                .to_owned(),
        );
    }
    if !ctx.apply {
        return Status::Ignored("apply_design_rules is off".to_owned());
    }
    let layers = ctx.layers.len();
    let listed: BTreeSet<String> = structure
        .via
        .iter()
        .flat_map(|v| v.padstacks.iter().chain(&v.spare))
        .chain(network.vias.iter().map(|v| &v.padstack))
        .chain(
            network
                .classes
                .iter()
                .flat_map(|c| c.circuit.iter().flat_map(|c| &c.use_via)),
        )
        .cloned()
        .collect();
    let offending: BTreeSet<String> = listed
        .into_iter()
        .filter(|n| kinds.iter().any(|k| is_kind(n, k, layers)))
        .collect();
    if offending.is_empty() {
        return Status::Ignored(format!(
            "met: none of the board's via padstacks is a {}",
            kinds.join(" or ").replace('_', " ")
        ));
    }
    // via infos of the network that use an offending padstack, and their names
    let removed_infos: BTreeSet<String> = network
        .vias
        .iter()
        .filter(|v| offending.contains(&v.padstack))
        .map(|v| v.name.clone())
        .collect();
    if let Some(v) = structure.via.as_mut() {
        v.padstacks.retain(|n| !offending.contains(n));
        v.spare.retain(|n| !offending.contains(n));
    }
    network.vias.retain(|v| !offending.contains(&v.padstack));
    for r in &mut network.via_rules {
        r.vias
            .retain(|n| !removed_infos.contains(n) && !offending.contains(n));
    }
    for c in &mut network.classes {
        if let Some(circuit) = c.circuit.as_mut() {
            circuit
                .use_via
                .retain(|n| !removed_infos.contains(n) && !offending.contains(n));
        }
    }
    if kinds.contains(&"micro_via") {
        ctx.warnings.push(format!(
            "design rule '{}' (line {}): the DSN names no micro via, so disallow micro_via removes \
             every via from an outer layer to its neighbour (a blind via of that span too)",
            rule.name, rule.line
        ));
    }
    Status::Applied {
        effects: offending.len(),
    }
}
