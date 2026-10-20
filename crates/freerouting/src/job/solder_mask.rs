//! Q18.7: other nets' copper keeps out of a pad's solder-mask aperture.
//!
//! KiCad's DRC (`solder_mask_bridge`) flags copper of another net inside a
//! pad's mask aperture: the pad grown by its solder-mask expansion (the pad's
//! `solder_mask_margin`, else its footprint's, else the board's
//! `pad_to_mask_clearance`). On the outer layer of the aperture, a trace or
//! via of another net must therefore keep max(copper clearance, expansion)
//! from the pad's copper. A DSN carries no mask layers, so the expansions come
//! as a setting, `router.solder_mask_expansions`: entries
//! `<component>:<pin>=<front µm>,<back µm>` separated by `;` (0 for a side
//! without an aperture; the component is split off at the first `:`). The
//! KiCad plugin fills it in from pcbnew (`kicad_board.mask_expansions`) and a
//! `.kicad_pcb` input from the reader (`freerouting_kicad::read::Extras`),
//! each listing only the pads whose expansion exceeds their own clearance.
//!
//! [`apply_solder_mask_expansions`] gives each listed pin a clearance class of
//! its own, `<class>+mask<front>/<back>` (board units): a copy of the pin's
//! class with every clearance on the top layer raised to at least the front
//! expansion and on the bottom layer to the back one. Pins with the same
//! class and expansions share it. Unset (the default), nothing changes.
//! `docs/DIVERGENCES.md` lists it (Java has no such rule).

use std::collections::BTreeMap;

use super::overrides::um_to_board_units;
use super::{LogLevel, LogLine};
use crate::board::BasicBoard;
use crate::settings::RouterSettings;

/// One pad's solder-mask expansion per outer side, µm.
#[derive(Clone, Debug, PartialEq)]
pub struct MaskExpansion {
    pub component: String,
    pub pin: String,
    pub front_um: f64,
    pub back_um: f64,
}

/// Parses the setting's text (module docs); an empty text is no entry.
pub fn parse_expansions(text: &str) -> Result<Vec<MaskExpansion>, String> {
    let mut out = Vec::new();
    for entry in text.split(';').map(str::trim).filter(|e| !e.is_empty()) {
        let bad = || format!("'{entry}' is not <component>:<pin>=<front um>,<back um>");
        let (pin, values) = entry.rsplit_once('=').ok_or_else(bad)?;
        let (component, pin) = pin.split_once(':').ok_or_else(bad)?;
        let (front, back) = values.split_once(',').ok_or_else(bad)?;
        let um = |v: &str| -> Result<f64, String> {
            match v.trim().parse::<f64>() {
                Ok(v) if v.is_finite() && v >= 0.0 => Ok(v),
                _ => Err(bad()),
            }
        };
        out.push(MaskExpansion {
            component: component.to_owned(),
            pin: pin.to_owned(),
            front_um: um(front)?,
            back_um: um(back)?,
        });
    }
    Ok(out)
}

/// The setting's text of `list` (Java's shortest `double` text per value).
pub fn format_expansions(list: &[MaskExpansion]) -> String {
    let v = |x: f64| {
        let s = format!("{x}");
        s.strip_suffix(".0").map(str::to_owned).unwrap_or(s)
    };
    list.iter()
        .map(|e| {
            format!(
                "{}:{}={},{}",
                e.component,
                e.pin,
                v(e.front_um),
                v(e.back_um)
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// The clearance class of `base` raised to `front` on layer 0 and `back` on
/// the last layer, created on first use.
fn raised_class(board: &mut BasicBoard, base: i32, front: i32, back: i32) -> i32 {
    let matrix = &mut board.rules.clearance_matrix;
    let base_name = matrix.get_name(base).unwrap_or("default").to_owned();
    let name = format!("{base_name}+mask{front}/{back}");
    let found = matrix.get_no(&name);
    if found >= 0 {
        return found;
    }
    matrix.append_class(&name);
    let new = matrix.get_no(&name);
    let (count, layers) = (matrix.class_count(), matrix.layer_count());
    for layer in 0..layers {
        let floor = match layer {
            0 => front,
            l if l + 1 == layers => back,
            _ => 0,
        };
        for class in 1..count {
            let from = if class as i32 == new {
                base
            } else {
                class as i32
            };
            let value = matrix.get_value(base, from, layer as i32, false).max(floor);
            matrix.set_value_on_layer(new as usize, class, layer, value);
            matrix.set_value_on_layer(class, new as usize, layer, value);
        }
    }
    new
}

/// Applies `router.solder_mask_expansions` (module docs); returns the log.
pub fn apply_solder_mask_expansions(
    board: &mut BasicBoard,
    settings: &RouterSettings,
) -> Vec<LogLine> {
    let Some(text) = settings.solder_mask_expansions.as_deref() else {
        return Vec::new();
    };
    let list = match parse_expansions(text) {
        Ok(list) => list,
        Err(e) => {
            return vec![LogLine {
                level: LogLevel::Warn,
                text: format!("Ignoring router.solder_mask_expansions: {e}"),
            }];
        }
    };
    let mut pins: BTreeMap<(String, String), Vec<_>> = BTreeMap::new();
    for id in board.pins() {
        let Some(item) = board.item(id) else { continue };
        let component = board
            .components
            .get(crate::board::structure::ComponentId(item.component_id()))
            .map(|c| c.name.clone());
        if let (Some(component), Some(pin)) = (component, board.pin_name(item)) {
            pins.entry((component, pin.to_owned()))
                .or_default()
                .push(id);
        }
    }
    let mut unknown = Vec::new();
    let mut changed = Vec::new();
    for e in &list {
        let (front, back) = (
            um_to_board_units(board, e.front_um),
            um_to_board_units(board, e.back_um),
        );
        let Some(ids) = pins.get(&(e.component.clone(), e.pin.clone())) else {
            unknown.push(format!("{}:{}", e.component, e.pin));
            continue;
        };
        if front <= 0 && back <= 0 {
            continue;
        }
        for &id in ids {
            let base = board.item(id).map_or(1, |i| i.clearance_class());
            let class = raised_class(board, base, front, back);
            changed.push((id, class));
        }
    }
    let mut lines = Vec::new();
    if !unknown.is_empty() {
        lines.push(LogLine {
            level: LogLevel::Warn,
            text: format!(
                "router.solder_mask_expansions: {} unknown pin(s): {}",
                unknown.len(),
                unknown.join(", ")
            ),
        });
    }
    if changed.is_empty() {
        return lines;
    }
    for &(id, class) in &changed {
        if let Some(item) = board.items.get_mut(&id) {
            item.clearance_class = class;
        }
    }
    // Tree shapes carry their clearance class's compensation.
    board.reinsert_tree_items();
    let classes: std::collections::BTreeSet<i32> = changed.iter().map(|c| c.1).collect();
    lines.push(LogLine {
        level: LogLevel::Info,
        text: format!(
            "Solder-mask apertures: {} pad(s) keep other nets' copper out of their mask \
             expansion on the outer layers ({} clearance class(es)).",
            changed.len(),
            classes.len()
        ),
    });
    lines
}
