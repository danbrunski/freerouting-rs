//! Q18: via types below the board's minimum via rules are made at the floor
//! size before routing.
//!
//! `router.board_minimums.{via_diameter_um, via_drill_um, via_annular_um}`
//! (KiCad's `m_ViasMinSize`, `m_MinThroughDrill`, `m_ViasMinAnnularWidth`)
//! are hard floors. A via type whose padstack is smaller (a net class's via
//! of 0.46 mm on a board whose minimum is 0.5 mm) gets a new round padstack
//! at the floor: drill `max(drill, min drill)`, diameter
//! `max(diameter, min diameter, drill + 2 · min annular)`, on the same
//! layers, named KiCad's way (`Via[<from>-<to>]_<diameter>:<drill>_um`, so
//! its drill reads back from the name, `Padstack::get_drill_radius`, and the
//! KiCad plugin's `ses_vias.py` sets it after the import). The via type keeps
//! its name, clearance class and rules; only its padstack changes, and the
//! session's `library_out` carries the new padstack. One warning per net
//! class names the class and both sizes. Vias already on the board are not
//! touched. A via's drill is its name's (KiCad style) or 45 % of its pad, as
//! the router estimates it elsewhere.
//!
//! Unset (every minimum 0, profile `legacy` with nothing supplied), nothing
//! changes (`tests/board_minimums.rs`).

use freerouting_geometry::{Circle, ConvexShape, IntPoint};

use super::{LogLevel, LogLine};
use crate::board::BasicBoard;
use crate::rules::ViaInfoId;
use crate::settings::RouterSettings;
use crate::settings::board::BoardMinimums;

/// Board units per micrometre of `board`.
fn units_per_um(board: &BasicBoard) -> f64 {
    let resolution = board.communication.resolution.max(1);
    crate::board::structure::Unit::scale(
        f64::from(resolution),
        crate::board::structure::Unit::Um,
        board.communication.unit,
    )
}

/// The diameter of a padstack: its narrowest shape on any layer, board units
/// (`None` without a shape).
pub(crate) fn padstack_diameter(
    board: &BasicBoard,
    padstack: crate::library::PadstackId,
) -> Option<f64> {
    let p = board.library.padstacks.get(padstack)?;
    let d = (p.from_layer()..=p.to_layer())
        .filter_map(|layer| p.get_shape(layer))
        .map(|shape| {
            let b = shape.to_shape().bounding_box();
            f64::from(b.width().min(b.height()))
        })
        .fold(f64::INFINITY, f64::min);
    d.is_finite().then_some(d)
}

/// Makes every via type below the minimums of `settings` at the floor size
/// (module docs); returns the warnings.
pub fn apply_via_minimums(board: &mut BasicBoard, settings: &RouterSettings) -> Vec<LogLine> {
    let m = BoardMinimums::of(settings);
    if m.via_diameter_um <= 0.0 && m.via_drill_um <= 0.0 && m.via_annular_um <= 0.0 {
        return Vec::new();
    }
    let scale = units_per_um(board);
    let ids: Vec<ViaInfoId> = board.rules.via_infos.iter().map(|(id, _)| id).collect();
    let mut lines = Vec::new();
    for id in ids {
        let info = board.rules.via_infos.get(id).clone();
        let Some(d) = padstack_diameter(board, info.padstack) else {
            continue;
        };
        let Some(padstack) = board.library.padstacks.get(info.padstack) else {
            continue;
        };
        let (from, to) = (padstack.from_layer(), padstack.to_layer());
        let drill = 2.0 * padstack.get_drill_radius();
        let new_drill = drill.max(m.via_drill_um * scale);
        let new_d = d
            .max(m.via_diameter_um * scale)
            .max(new_drill + 2.0 * m.via_annular_um * scale);
        // A rounding unit of slack: a via at the floor already is not remade.
        if new_d <= d + 0.5 && new_drill <= drill + 0.5 {
            continue;
        }
        let d_um = (new_d / scale).ceil();
        let drill_um = (new_drill / scale).ceil();
        let radius = (d_um * scale / 2.0).ceil() as i32;
        let name = format!("Via[{from}-{to}]_{d_um}:{drill_um}_um");
        let shape = ConvexShape::Circle(Circle::new(IntPoint::new(0, 0), radius));
        let layer_count = board.layer_count();
        let mut shapes = vec![None; layer_count];
        for layer in from.max(0)..=to.min(layer_count as i32 - 1) {
            shapes[layer as usize] = Some(shape.clone());
        }
        let attach = padstack.attach_allowed;
        let existing = board.library.padstacks.get_by_name(&name).map(|p| p.id);
        let new_id = existing.unwrap_or_else(|| {
            let new_id =
                board
                    .library
                    .padstacks
                    .add(name.clone(), shapes, Vec::new(), attach, false);
            board.library.add_via_padstack(new_id);
            new_id
        });
        board.rules.via_infos.get_mut(id).padstack = new_id;
        let mut classes = classes_using(board, id);
        // The DSN reader's class-named copy `<padstack>-<class>` of a via
        // type (`router.via_clearance`) may be in no class's via rule.
        if classes.is_empty()
            && let Some((_, class)) = info.name.rsplit_once('-')
            && board.rules.net_classes.get_by_name(class).is_some()
        {
            classes.push(class.to_string());
        }
        let old_d = d / scale;
        let old_drill = drill / scale;
        let floors = known_minimums(&m);
        for class in if classes.is_empty() {
            vec![String::from("(no net class)")]
        } else {
            classes
        } {
            lines.push(LogLine {
                level: LogLevel::Warn,
                text: format!(
                    "Net class '{class}': via '{}' ({old_d:.0} um, drill {old_drill:.0} um) is \
                     below the board minimums ({floors}); routed vias of it are made as '{name}'.",
                    info.name
                ),
            });
        }
    }
    lines
}

/// `via 500 um, drill 300 um, annular ring 100 um`, the known via minimums.
fn known_minimums(m: &BoardMinimums) -> String {
    [
        ("via", m.via_diameter_um),
        ("drill", m.via_drill_um),
        ("annular ring", m.via_annular_um),
    ]
    .iter()
    .filter(|(_, um)| *um > 0.0)
    .map(|(what, um)| format!("{what} {um} um"))
    .collect::<Vec<_>>()
    .join(", ")
}

/// The names of the net classes whose via rule holds via type `via`, in
/// class order.
fn classes_using(board: &BasicBoard, via: ViaInfoId) -> Vec<String> {
    let rules = &board.rules;
    (0..rules.net_classes.count())
        .filter_map(|i| rules.net_classes.nth(i))
        .map(|id| rules.net_classes.get(id))
        .filter(|class| {
            class
                .via_rule()
                .is_some_and(|rule| rules.via_rules.get(rule).vias().contains(&via))
        })
        .map(|class| class.name().to_string())
        .collect()
}
