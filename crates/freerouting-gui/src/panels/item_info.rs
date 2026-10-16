//! The item info window (G04.3, wired to the selection by G05.4): Java's
//! `WindowObjectInfo.display(items, ..)`.
//!
//! The values come from the engine's [`ItemInfo`] (B08.2, the port of each
//! class's `Item.printInfo`), in the engine's order. This module adds the
//! words Java puts between the values (`Pin: component U1, pin 3, padstack
//! X at (1 , 2), Net N`), the summary line, and the units: lengths and
//! points are shown in the unit of the toolbar's selector.
//!
//! The text is built when the window opens, as in Java. Java's windows also
//! have buttons that open the info of a component, net or padstack; those
//! are not ported (the values are shown as text).

use eframe::egui;
use freerouting::board::BasicBoard;
use freerouting::board::items::{BoardItemType, ItemId};
use freerouting::board::structure::{FixedState, Unit};
use freerouting::board::{InfoField, ItemInfo};
use freerouting::geometry::FloatPoint;

use crate::overlays::Frame;
use crate::text;

/// One run of text, bold or plain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    pub bold: bool,
    pub text: String,
}

/// One line of the window.
pub type Line = Vec<Part>;

/// The plain text of a line.
pub fn plain(line: &[Part]) -> String {
    line.iter().map(|p| p.text.as_str()).collect()
}

/// An open info window.
#[derive(Clone, Debug)]
pub struct InfoWindow {
    pub id: u64,
    /// The items it was opened for.
    pub items: Vec<ItemId>,
    pub lines: Vec<Line>,
    pub summary: Line,
}

/// The open info windows.
#[derive(Debug, Default)]
pub struct InfoWindows {
    windows: Vec<InfoWindow>,
    next_id: u64,
}

impl InfoWindows {
    /// Opens a window for `items`.
    pub fn open(&mut self, board: &BasicBoard, items: &[ItemId], unit: Unit) {
        self.next_id += 1;
        let (lines, summary) = build(board, items, unit);
        self.windows.push(InfoWindow {
            id: self.next_id,
            items: items.to_vec(),
            lines,
            summary,
        });
    }

    pub fn windows(&self) -> &[InfoWindow] {
        &self.windows
    }

    pub fn clear(&mut self) {
        self.windows.clear();
    }

    /// Draws the windows. Closed ones are dropped.
    pub fn show(&mut self, ctx: &egui::Context) {
        self.windows.retain_mut(|window| {
            let mut open = true;
            egui::Window::new(text::INFO_TITLE)
                .id(egui::Id::new(("item-info", window.id)))
                .open(&mut open)
                .default_size([520.0, 180.0])
                .show(ctx, |ui| {
                    egui::ScrollArea::both()
                        .auto_shrink([false, true])
                        .max_height(420.0)
                        .show(ui, |ui| {
                            for line in &window.lines {
                                ui.horizontal_wrapped(|ui| draw_line(ui, line));
                            }
                            if window.lines.is_empty() {
                                ui.label(text::INFO_EMPTY);
                            }
                            ui.horizontal_wrapped(|ui| draw_line(ui, &window.summary));
                        });
                });
            open
        });
    }
}

fn draw_line(ui: &mut egui::Ui, line: &[Part]) {
    ui.spacing_mut().item_spacing.x = 0.0;
    for part in line {
        if part.bold {
            ui.label(egui::RichText::new(&part.text).strong());
        } else {
            ui.label(&part.text);
        }
    }
}

/// The lines of the items (those with a text; a component outline has
/// none) and the summary line.
pub fn build(board: &BasicBoard, items: &[ItemId], unit: Unit) -> (Vec<Line>, Line) {
    let frame = Frame::new(board);
    let mut lines = Vec::new();
    let (mut pins, mut vias, mut traces, mut length) = (0usize, 0usize, 0usize, 0.0f64);
    for id in items {
        let Some(info) = board.item_info(*id) else {
            continue;
        };
        match info.kind {
            BoardItemType::Pin => pins += 1,
            BoardItemType::Via => vias += 1,
            BoardItemType::Trace => {
                traces += 1;
                length += info
                    .fields
                    .iter()
                    .find_map(|f| match f {
                        InfoField::Length(l) => Some(*l),
                        _ => None,
                    })
                    .unwrap_or(0.0);
            }
            _ => {}
        }
        let line = item_line(&info, &frame, unit);
        if !line.is_empty() {
            lines.push(line);
        }
    }
    (lines, summary(pins, vias, traces, &frame, length, unit))
}

/// Java's summary: `Summary: 2 pins, 1 via, 3 traces, cumulative length 4.5`.
fn summary(
    pins: usize,
    vias: usize,
    traces: usize,
    frame: &Frame,
    length: f64,
    unit: Unit,
) -> Line {
    let mut parts = vec![bold(format!("{} ", text::INFO_SUMMARY))];
    let count = |n: usize, one: &str, many: &str| {
        format!(
            "{} {}",
            text::format_number(n as f64),
            if n == 1 { one } else { many }
        )
    };
    let mut pieces = Vec::new();
    if pins > 0 {
        pieces.push(count(pins, text::INFO_PIN, text::INFO_PINS));
    }
    if vias > 0 {
        pieces.push(count(vias, text::INFO_VIA, text::INFO_VIAS));
    }
    if traces > 0 {
        pieces.push(format!(
            "{} {}",
            count(traces, text::INFO_TRACE, text::INFO_TRACES),
            len(frame, length, unit)
        ));
    }
    parts.push(plain_part(pieces.join(", ")));
    parts
}

fn bold(text: impl Into<String>) -> Part {
    Part {
        bold: true,
        text: text.into(),
    }
}

fn plain_part(text: impl Into<String>) -> Part {
    Part {
        bold: false,
        text: text.into(),
    }
}

/// A board length in `unit` (Java: `float` of the user value, at most four
/// fraction digits).
fn len(frame: &Frame, board_length: f64, unit: Unit) -> String {
    let mm = frame.len(board_length);
    let value = Unit::scale(mm, Unit::Mm, unit);
    text::format_number(f64::from(value as f32))
}

fn point(frame: &Frame, p: FloatPoint, unit: Unit) -> String {
    let mm = frame.pt(p);
    text::format_point(
        Unit::scale(mm.x, Unit::Mm, unit),
        Unit::scale(mm.y, Unit::Mm, unit),
    )
}

fn fixed_text(state: FixedState) -> &'static str {
    match state {
        FixedState::Unfixed => "Unfixed",
        FixedState::ShoveFixed => "Shove fixed",
        FixedState::UserFixed => "User fixed",
        FixedState::SystemFixed => "System fixed",
    }
}

/// The one line `printInfo` prints for an item, empty for a component
/// outline. The heading (the kind) is bold.
pub fn item_line(info: &ItemInfo, frame: &Frame, unit: Unit) -> Line {
    let heading = match info.kind {
        BoardItemType::Pin => "Pin: ",
        BoardItemType::Via => "Via",
        BoardItemType::Trace => "Trace",
        BoardItemType::ConductionArea => "Conduction Area",
        BoardItemType::ObstacleArea => "Keepout",
        BoardItemType::ViaObstacleArea => "Via Keepout",
        BoardItemType::ComponentObstacleArea => "Component Keepout",
        BoardItemType::BoardOutline => "Board Outline",
        BoardItemType::ComponentOutline | BoardItemType::Other => return Vec::new(),
    };
    let mut line = vec![bold(heading)];
    let mut out = String::new();
    let keepout = info.kind == BoardItemType::ObstacleArea;
    for field in &info.fields {
        match field {
            InfoField::Component { name, .. } => {
                if keepout {
                    out += &format!(" of component {name}");
                } else {
                    out += &format!("component {name}");
                }
            }
            InfoField::PinName(name) => out += &format!(", pin {name}"),
            // Java prints `, padstack` and the padstack's button; the
            // button's margin is zero, so a via reads `padstackNAME` there.
            // A space is added.
            InfoField::Padstack { name, .. } => out += &format!(", padstack {name}"),
            InfoField::Location(p) => out += &format!(" at {}", point(frame, *p, unit)),
            InfoField::From(p) => out += &format!(" from {}", point(frame, *p, unit)),
            InfoField::To(p) => out += &format!(" to {}", point(frame, *p, unit)),
            InfoField::Layer(name) => out += &format!(" on layer {name}"),
            InfoField::Width(w) => out += &format!(", width {}", len(frame, *w, unit)),
            InfoField::Length(l) => out += &format!(", length {}", len(frame, *l, unit)),
            InfoField::HoleCount(n) => {
                out += &format!(
                    " with {} {}",
                    text::format_number(*n as f64),
                    if *n == 1 { "hole" } else { "holes" }
                );
            }
            InfoField::ClearanceClass { name, .. } => out += &format!(", clearance class {name}"),
            InfoField::Fixed(state) => out += &format!(", {}", fixed_text(*state)),
            InfoField::Net { name, .. } => out += &format!(", Net {name}"),
            InfoField::Contacts(items) => out += &format!(", contacts {}", items.len()),
            InfoField::ClearanceViolations(violations) => {
                out += &format!(
                    ", {} {}",
                    violations.len(),
                    if violations.len() == 1 {
                        "clearance violation"
                    } else {
                        "clearance violations"
                    }
                );
            }
        }
    }
    line.push(plain_part(out));
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_joins_the_parts() {
        let line = vec![bold("Trace"), plain_part(" from (1 , 2)")];
        assert_eq!(plain(&line), "Trace from (1 , 2)");
    }

    #[test]
    fn fixed_states_read_as_javas_bundle() {
        assert_eq!(fixed_text(FixedState::UserFixed), "User fixed");
        assert_eq!(fixed_text(FixedState::ShoveFixed), "Shove fixed");
    }
}
