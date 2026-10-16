//! Context menus (G05.6): Java's `PopupMenuMain`, `PopupMenuInspectedItems`
//! and `PopupMenuDisplay`, and the construction menus.
//!
//! A right click opens the menu of the mode at the click location:
//! - nothing selected (`PopupMenuMain`): Select item, Start route, Create
//!   keepout (Tile, Circle, Polygon, Hole), Swap pin when the board has
//!   logical parts, then the display rows;
//! - items selected (`PopupMenuInspectedItems`): the "Extend selection to"
//!   header, Nets, Connected Sets, Connections, Components, then the display
//!   rows;
//! - while a keepout is under construction: Done / Insert and Cancel.
//!
//! The display rows (Center display, Zoom in, Zoom out) act on the click
//! location, not on the cursor, which has moved into the menu. Entries
//! whose engine work has not landed (route, pin swap, hole) are shown
//! disabled with a tooltip naming what is missing.

use eframe::egui;

use crate::commands::{Command, ExtendKind, KeepoutKind};
use crate::scene::P;
use crate::text;

/// An open context menu.
#[derive(Clone, Debug, PartialEq)]
pub struct Popup {
    /// Where the right click was, in the scene's millimetres.
    pub at: P,
    /// Where the menu is drawn.
    pub screen: egui::Pos2,
    /// The "Create keepout" submenu is open.
    pub keepout_open: bool,
}

/// What the menu is built for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Nothing selected; `swap` if the board has logical parts.
    Main {
        swap: bool,
    },
    Selection,
    /// A keepout under construction: `circle` ends with a click.
    Construction {
        circle: bool,
    },
}

/// One row of a menu.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    /// A non-interactive heading.
    Header(&'static str),
    Item {
        label: &'static str,
        command: Command,
        /// Why the entry is disabled, if it is.
        disabled: Option<&'static str>,
    },
    /// "Create keepout" and its four shapes.
    Submenu {
        label: &'static str,
        items: Vec<Entry>,
    },
    Separator,
}

fn item(label: &'static str, command: Command) -> Entry {
    Entry::Item {
        label,
        command,
        disabled: None,
    }
}

/// The display rows every menu has but the construction ones
/// (`PopupMenuDisplay`).
fn display_rows(at: P) -> Vec<Entry> {
    vec![
        item(text::POPUP_CENTER_DISPLAY, Command::CenterAt(at)),
        item(text::POPUP_ZOOM_IN, Command::ZoomInAt(at)),
        item(text::POPUP_ZOOM_OUT, Command::ZoomOutAt(at)),
    ]
}

/// The rows of the menu for `mode` opened at `at`, in Java's order.
pub fn entries(mode: Mode, at: P) -> Vec<Entry> {
    match mode {
        Mode::Main { swap } => {
            let mut rows = vec![
                item(text::POPUP_SELECT_ITEM, Command::SelectAt(at)),
                Entry::Item {
                    label: text::POPUP_START_ROUTE,
                    command: Command::ClosePopup,
                    disabled: Some(text::POPUP_NEEDS_ROUTE),
                },
                Entry::Submenu {
                    label: text::POPUP_CREATE_KEEPOUT,
                    items: vec![
                        item(
                            text::POPUP_TILE,
                            Command::StartKeepout {
                                kind: KeepoutKind::Tile,
                                at,
                            },
                        ),
                        item(
                            text::POPUP_CIRCLE,
                            Command::StartKeepout {
                                kind: KeepoutKind::Circle,
                                at,
                            },
                        ),
                        item(
                            text::POPUP_POLYGON,
                            Command::StartKeepout {
                                kind: KeepoutKind::Polygon,
                                at,
                            },
                        ),
                        Entry::Item {
                            label: text::POPUP_HOLE,
                            command: Command::ClosePopup,
                            disabled: Some(text::POPUP_NEEDS_HOLE),
                        },
                    ],
                },
            ];
            if swap {
                rows.push(Entry::Item {
                    label: text::POPUP_SWAP_PIN,
                    command: Command::ClosePopup,
                    disabled: Some(text::POPUP_NEEDS_SWAP),
                });
            }
            rows.extend(display_rows(at));
            rows
        }
        Mode::Selection => {
            let mut rows = vec![
                Entry::Header(text::POPUP_EXTEND_SELECTION),
                item(text::TB_NETS, Command::ExtendSelection(ExtendKind::Nets)),
                item(
                    text::TB_CONN_SETS,
                    Command::ExtendSelection(ExtendKind::ConnectedSets),
                ),
                item(
                    text::TB_CONNECTIONS,
                    Command::ExtendSelection(ExtendKind::Connections),
                ),
                item(
                    text::TB_COMPONENTS,
                    Command::ExtendSelection(ExtendKind::Components),
                ),
                Entry::Separator,
            ];
            rows.extend(display_rows(at));
            rows
        }
        Mode::Construction { circle } => vec![
            if circle {
                item(text::POPUP_INSERT, Command::Complete)
            } else {
                item(text::POPUP_DONE, Command::Complete)
            },
            item(text::POPUP_CANCEL, Command::Cancel),
        ],
    }
}

/// The labels of the rows that can be clicked or read at the top level, in
/// order (a submenu's own label, not its items).
pub fn labels(mode: Mode, at: P) -> Vec<&'static str> {
    entries(mode, at)
        .into_iter()
        .filter_map(|e| match e {
            Entry::Header(label) => Some(label),
            Entry::Item { label, .. } | Entry::Submenu { label, .. } => Some(label),
            Entry::Separator => None,
        })
        .collect()
}

/// Draws the menu and returns the chosen command, if any. The caller closes
/// the popup when a command came back or the user clicked elsewhere.
pub fn ui(ctx: &egui::Context, popup: &mut Popup, mode: Mode) -> (Vec<Command>, bool) {
    let mut out = Vec::new();
    let rows = entries(mode, popup.at);
    let mut keepout_open = popup.keepout_open;
    let area = egui::Area::new(egui::Id::new("canvas-popup"))
        .order(egui::Order::Foreground)
        .fixed_pos(popup.screen)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(150.0);
                for row in &rows {
                    draw_row(ui, row, &mut keepout_open, &mut out);
                }
            });
        });
    popup.keepout_open = keepout_open;
    let clicked_away = ctx.input(|i| i.pointer.any_pressed())
        && !area.response.contains_pointer()
        && out.is_empty();
    let close = !out.is_empty() || clicked_away;
    (out, close)
}

fn draw_row(ui: &mut egui::Ui, row: &Entry, keepout_open: &mut bool, out: &mut Vec<Command>) {
    match row {
        Entry::Header(label) => {
            ui.label(egui::RichText::new(*label).strong().weak());
        }
        Entry::Separator => {
            ui.separator();
        }
        Entry::Item {
            label,
            command,
            disabled,
        } => {
            let response =
                ui.add_enabled(disabled.is_none(), egui::Button::new(*label).frame(false));
            let response = match disabled {
                Some(why) => response.on_disabled_hover_text(*why),
                None => response,
            };
            if response.clicked() {
                out.push(command.clone());
            }
        }
        Entry::Submenu { label, items } => {
            let arrow = if *keepout_open { "v" } else { ">" };
            let clicked = ui
                .horizontal(|ui| {
                    // The arrow is a separate label, so the button's own
                    // name is Java's "Create keepout".
                    let button = ui.add(egui::Button::new(*label).frame(false));
                    ui.label(arrow);
                    button.clicked()
                })
                .inner;
            if clicked {
                *keepout_open = !*keepout_open;
            }
            if *keepout_open {
                ui.indent("keepout-items", |ui| {
                    for child in items {
                        draw_row(ui, child, keepout_open, out);
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: P = P::new(1.0, 2.0);

    #[test]
    fn main_menu_is_javas_in_order() {
        assert_eq!(
            labels(Mode::Main { swap: false }, AT),
            [
                "Select item (I)",
                "Start route (T)",
                "Create keepout",
                "Center display (C)",
                "Zoom in (Z)",
                "Zoom out (O)"
            ]
        );
        assert!(labels(Mode::Main { swap: true }, AT).contains(&"Swap pin (W)"));
    }

    #[test]
    fn selection_menu_has_the_four_extend_items() {
        assert_eq!(
            labels(Mode::Selection, AT),
            [
                "Extend selection to:",
                "Nets",
                "Connected Sets",
                "Connections",
                "Components",
                "Center display (C)",
                "Zoom in (Z)",
                "Zoom out (O)"
            ]
        );
    }

    /// Java `PopupMenuInspectedItemsTest.popupContainsExtendItemsAndDispatchesActions`:
    /// the header first, the four extend items dispatching their actions,
    /// and a separator at index 5. The German-header test is not ported
    /// (English only, G00 question 3).
    #[test]
    fn extend_items_dispatch_and_a_separator_follows() {
        let rows = entries(Mode::Selection, AT);
        assert_eq!(rows[0], Entry::Header("Extend selection to:"));
        let commands: Vec<Command> = rows[1..5]
            .iter()
            .map(|row| match row {
                Entry::Item { command, .. } => command.clone(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            commands,
            [
                Command::ExtendSelection(ExtendKind::Nets),
                Command::ExtendSelection(ExtendKind::ConnectedSets),
                Command::ExtendSelection(ExtendKind::Connections),
                Command::ExtendSelection(ExtendKind::Components),
            ]
        );
        assert_eq!(rows[5], Entry::Separator);
    }

    #[test]
    fn display_rows_act_on_the_click_location() {
        let rows = entries(Mode::Selection, AT);
        assert!(rows.iter().any(|e| matches!(
            e,
            Entry::Item { command: Command::ZoomInAt(p), .. } if *p == AT
        )));
    }
}
