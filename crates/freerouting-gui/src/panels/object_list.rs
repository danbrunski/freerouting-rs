//! The generic list window of the Info menu: Java's `WindowObjectList` and
//! `WindowObjectListWithFilter`. Rows, a filter field, and the buttons
//! Select, Invert, Info and Recalculate.
//!
//! The window only shows rows. What a row says and which items it stands for
//! is decided by the list module ([`super::violations`] and the others);
//! Select turns the selected rows into [`Command::Select`] followed by
//! [`Command::ZoomSelection`], as Java's `selectInstances` does.

use std::collections::BTreeSet;

use eframe::egui;
use freerouting::board::items::ItemId;

use crate::analysis::Tag;
use crate::commands::{Command, ListKind};
use crate::text;

/// One line of a list and the board items it stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub text: String,
    pub items: Vec<ItemId>,
    /// More text about the row, shown under the list while the row is
    /// chosen (Java's net window prints `Net.printInfo` there). The lists
    /// of the Info menu have none.
    pub detail: Option<String>,
    /// The text the filter looks at when it is not `text` (Java filters the
    /// net window on `Net.toString()`, which has no class name).
    pub filter_text: Option<String>,
}

/// A list window: open or closed, its filter, the chosen rows and the rows
/// last built.
#[derive(Default)]
pub struct ListWindow {
    pub open: bool,
    filter: String,
    /// Indices into the rows that pass the filter.
    selected: BTreeSet<usize>,
    /// The rows, and the state of the board they were built for.
    built: Option<(Tag, Vec<Row>)>,
}

/// What a list window looks like: Java's `WindowObjectList` lets each
/// subclass hide buttons and the nets window renames Select.
pub struct ListSpec<'a> {
    pub title: &'a str,
    /// Shown when no row passes the filter.
    pub empty: &'a str,
    pub select_label: &'a str,
    pub select_tooltip: &'a str,
    pub show_info: bool,
    pub show_invert: bool,
    pub show_recalculate: bool,
    /// What Recalculate does: the Info lists run the DRC again, the lists
    /// over the board's own data rebuild their rows.
    pub recalculate: Command,
}

impl<'a> ListSpec<'a> {
    /// The window of an Info list (G03): all four buttons, Recalculate runs
    /// the DRC.
    pub fn info(title: &'a str, empty: &'a str) -> ListSpec<'a> {
        ListSpec {
            title,
            empty,
            select_label: text::SELECT,
            select_tooltip: text::SELECT_TOOLTIP,
            show_info: true,
            show_invert: true,
            show_recalculate: true,
            recalculate: Command::RunDrc,
        }
    }
}

impl ListWindow {
    /// True if rows for `tag` are stored.
    pub fn has_rows(&self, tag: Tag) -> bool {
        self.built.as_ref().is_some_and(|(t, _)| *t == tag)
    }

    /// The rows last stored, whatever state of the board they are for.
    pub fn built_rows(&self) -> Option<&[Row]> {
        self.built.as_ref().map(|(_, r)| r.as_slice())
    }

    /// The rows if they are those of `tag`.
    pub fn rows(&self, tag: Tag) -> Option<&[Row]> {
        self.built
            .as_ref()
            .filter(|(t, _)| *t == tag)
            .map(|(_, r)| r.as_slice())
    }

    /// Forgets the rows, so the next frame builds them again (Recalculate of
    /// the lists over the board's own data).
    pub fn invalidate(&mut self) {
        self.built = None;
        self.selected.clear();
    }

    /// Stores freshly built rows for `tag` and clears the choice, as Java's
    /// `recalculate` clears the list model.
    pub fn set_rows(&mut self, tag: Tag, rows: Vec<Row>) {
        self.built = Some((tag, rows));
        self.selected.clear();
    }

    /// The text filter (Java: contained in the row text, ignoring case,
    /// after trimming).
    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn set_filter(&mut self, filter: &str) {
        if self.filter != filter {
            self.filter = filter.to_string();
            self.selected.clear();
        }
    }

    /// The rows that pass the filter, in list order.
    pub fn visible_rows<'a>(&self, rows: &'a [Row]) -> Vec<&'a Row> {
        self.visible(rows)
    }

    fn visible<'a>(&self, rows: &'a [Row]) -> Vec<&'a Row> {
        let filter = self.filter.trim().to_lowercase();
        rows.iter()
            .filter(|r| {
                filter.is_empty()
                    || r.filter_text
                        .as_deref()
                        .unwrap_or(&r.text)
                        .to_lowercase()
                        .contains(&filter)
            })
            .collect()
    }

    /// Chooses visible row `index` alone, or toggles it (Ctrl).
    pub fn click(&mut self, index: usize, toggle: bool) {
        if toggle {
            if !self.selected.remove(&index) {
                self.selected.insert(index);
            }
        } else {
            self.selected = BTreeSet::from([index]);
        }
    }

    /// The chosen visible rows, by index.
    pub fn chosen(&self) -> &BTreeSet<usize> {
        &self.selected
    }

    /// The items of the chosen rows, for the Select button.
    pub fn chosen_items(&self, rows: &[Row]) -> Vec<ItemId> {
        let visible = self.visible(rows);
        let items: BTreeSet<ItemId> = self
            .selected
            .iter()
            .filter_map(|i| visible.get(*i))
            .flat_map(|r| r.items.iter().copied())
            .collect();
        items.into_iter().collect()
    }

    /// Chooses the rows that are not chosen (Invert).
    pub fn invert(&mut self, rows: &[Row]) {
        let count = self.visible(rows).len();
        self.selected = (0..count).filter(|i| !self.selected.contains(i)).collect();
    }

    /// Draws the window if it is open. It shows the rows stored for `tag`,
    /// or "Calculating…" while the analysis the list is built from still
    /// runs.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        kind: ListKind,
        title: &str,
        empty: &str,
        tag: Tag,
    ) -> Vec<Command> {
        let spec = ListSpec::info(title, empty);
        self.show_with(ctx, kind, &spec, tag, |_| Vec::new())
    }

    /// [`show`](Self::show) with a [`ListSpec`] and a header the caller
    /// draws above the list (the nets window's filter check box).
    pub fn show_with(
        &mut self,
        ctx: &egui::Context,
        kind: ListKind,
        spec: &ListSpec<'_>,
        tag: Tag,
        header: impl FnOnce(&mut egui::Ui) -> Vec<Command>,
    ) -> Vec<Command> {
        let mut out = Vec::new();
        if !self.open {
            return out;
        }
        let built = self.built.take();
        let rows = built
            .as_ref()
            .filter(|(t, _)| *t == tag)
            .map(|(_, r)| r.as_slice());
        let mut open = true;
        egui::Window::new(spec.title)
            .id(egui::Id::new(("info-list", kind)))
            .open(&mut open)
            .default_size([640.0, 320.0])
            .show(ctx, |ui| {
                out.extend(header(ui));
                self.body(ui, rows, spec, &mut out);
            });
        self.built = built;
        self.open = open;
        out
    }

    fn body(
        &mut self,
        ui: &mut egui::Ui,
        rows: Option<&[Row]>,
        spec: &ListSpec<'_>,
        out: &mut Vec<Command>,
    ) {
        let Some(rows) = rows else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(text::CALCULATING);
            });
            return;
        };
        let visible = self.visible(rows);
        egui::ScrollArea::vertical()
            .max_height(260.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                if visible.is_empty() {
                    ui.label(spec.empty);
                }
                let mut clicked = None;
                for (i, row) in visible.iter().enumerate() {
                    let response = ui.selectable_label(self.selected.contains(&i), &row.text);
                    if response.clicked() {
                        let toggle = ui.input(|input| input.modifiers.command);
                        clicked = Some((i, toggle));
                    }
                    if response.double_clicked() {
                        out.extend(self.select_command(rows));
                    }
                }
                if let Some((i, toggle)) = clicked {
                    self.click(i, toggle);
                }
            });
        let details: Vec<&str> = self
            .selected
            .iter()
            .filter_map(|i| visible.get(*i))
            .filter_map(|r| r.detail.as_deref())
            .collect();
        if !details.is_empty() {
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("row-details")
                .max_height(80.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for detail in details {
                        ui.label(detail);
                    }
                });
        }
        ui.separator();
        ui.horizontal(|ui| {
            if spec.show_info {
                // Info shows the item info window, which G04.3 adds.
                ui.add_enabled(false, egui::Button::new(text::INFO))
                    .on_disabled_hover_text(text::INFO_TOOLTIP);
            }
            if ui
                .button(spec.select_label)
                .on_hover_text(spec.select_tooltip)
                .clicked()
            {
                out.extend(self.select_command(rows));
            }
            if spec.show_invert
                && ui
                    .button(text::INVERT)
                    .on_hover_text(text::INVERT_TOOLTIP)
                    .clicked()
            {
                self.invert(rows);
            }
            if spec.show_recalculate
                && ui
                    .button(text::RECALCULATE)
                    .on_hover_text(text::RECALCULATE_TOOLTIP)
                    .clicked()
            {
                out.push(spec.recalculate.clone());
            }
        });
        ui.horizontal(|ui| {
            ui.label(text::FILTER);
            let mut filter = self.filter.clone();
            ui.text_edit_singleline(&mut filter);
            self.set_filter(&filter);
        });
    }

    fn select_command(&self, rows: &[Row]) -> Vec<Command> {
        let items = self.chosen_items(rows);
        if items.is_empty() {
            return Vec::new();
        }
        vec![Command::Select(items), Command::ZoomSelection]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<Row> {
        ["Alpha one", "beta two", "Alpha three"]
            .iter()
            .enumerate()
            .map(|(i, t)| Row {
                text: (*t).to_string(),
                items: vec![ItemId(i as i32)],
                detail: None,
                filter_text: None,
            })
            .collect()
    }

    #[test]
    fn filter_ignores_case_and_trims() {
        let mut w = ListWindow::default();
        w.set_filter("  ALPHA ");
        let rows = rows();
        let visible: Vec<_> = w.visible(&rows).iter().map(|r| r.text.clone()).collect();
        assert_eq!(visible, ["Alpha one", "Alpha three"]);
    }

    #[test]
    fn select_invert_and_items() {
        let mut w = ListWindow::default();
        let rows = rows();
        w.click(1, false);
        assert_eq!(w.chosen_items(&rows), [ItemId(1)]);
        w.invert(&rows);
        assert_eq!(w.chosen().iter().copied().collect::<Vec<_>>(), [0, 2]);
        // Ctrl toggles; a plain click replaces.
        w.click(0, true);
        assert_eq!(w.chosen().iter().copied().collect::<Vec<_>>(), [2]);
        w.click(1, false);
        assert_eq!(w.chosen().iter().copied().collect::<Vec<_>>(), [1]);
        // A new filter clears the choice, as Java's recalculate does.
        w.set_filter("alpha");
        assert!(w.chosen().is_empty());
    }
}
