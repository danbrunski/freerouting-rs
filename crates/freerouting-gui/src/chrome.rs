//! The look of the menu bar and the toolbar: icons before labels, the green
//! Autoroute and red Cancel buttons, segmented groups and the open board's
//! chip.
//!
//! An icon is a glyph of egui's default fonts, painted into space the button
//! keeps for it ([`Atom::custom`]), so a button's accessible label is still
//! its text alone: the tests and screen readers find "Autoroute", not
//! "▶ Autoroute".

use eframe::egui::{
    self, Align2, Atom, Color32, ColorImage, CornerRadius, FontId, Frame, Id, IntoAtoms as _,
    Margin, Rect, Response, Stroke, Style, TextureHandle, TextureOptions, Ui, Vec2, pos2, vec2,
};

/// Glyphs of egui's default fonts (Ubuntu, Noto Emoji and the emoji icon
/// font), close to Java's toolbar icons where Java has one.
pub mod icon {
    pub const OPEN: &str = "🗁";
    pub const RECENT: &str = "🕘";
    pub const SAVE: &str = "💾";
    pub const RELOAD: &str = "🔃";
    pub const EXPORT: &str = "🖹";
    pub const QUIT: &str = "✖";
    pub const FIT: &str = "⛶";
    pub const ZOOM_IN: &str = "➕";
    pub const ZOOM_OUT: &str = "➖";
    pub const CENTRE: &str = "⌖";
    pub const ZOOM_REGION: &str = "🔍";
    pub const CROSSHAIR: &str = "✚";
    pub const ABOUT: &str = "ℹ";
    pub const AUTOROUTE: &str = "▶";
    pub const CANCEL: &str = "⏹";
    pub const INSPECT: &str = "⬉";
    pub const ROUTE: &str = "✏";
    pub const DRAG: &str = "✋";
    /// Java: `spider-web`.
    pub const RATSNEST: &str = "🕸";
    pub const VIOLATIONS: &str = "⚠";
    pub const UNDO: &str = "⟲";
    pub const REDO: &str = "⟳";
    /// Java: `delete-sweep`.
    pub const DELETE_ALL: &str = "🗑";
    pub const FILE: &str = "🗋";
}

/// The height of a toolbar button, and of a segmented group with its frame.
const BUTTON_HEIGHT: f32 = 24.0;
/// The room kept for an icon.
const ICON_SIDE: f32 = 16.0;
/// The font size an icon is painted at.
const ICON_FONT_SIZE: f32 = 14.0;
/// The corner radius of a segmented group (its buttons: one less).
const RADIUS: u8 = 6;
/// A segmented group's frame: its inner margin and stroke, on each side.
const SEGMENT_INSET: f32 = 3.0;

/// Autoroute's fill: inactive, hovered, pressed.
const GO: [Color32; 3] = [
    Color32::from_rgb(38, 122, 66),
    Color32::from_rgb(46, 142, 78),
    Color32::from_rgb(30, 100, 54),
];
/// Cancel's fill while the autorouter runs: inactive, hovered, pressed.
const STOP: [Color32; 3] = [
    Color32::from_rgb(168, 50, 50),
    Color32::from_rgb(192, 60, 60),
    Color32::from_rgb(138, 40, 40),
];
/// The menu bar's panel; the toolbar keeps egui's panel fill (gray 27).
const MENU_BAR_FILL: Color32 = Color32::from_gray(20);
/// Delete All's fill while hovered or pressed.
const DANGER_HOVER: Color32 = Color32::from_rgb(96, 36, 36);
/// Delete All's icon.
const DANGER_ICON: Color32 = Color32::from_rgb(232, 112, 100);

/// The custom atom an icon is painted into; one per button, so one id does.
fn icon_id() -> Id {
    Id::new("chrome-icon")
}

/// How a toolbar button is coloured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Framed while hovered or selected only.
    Quiet,
    /// Filled green: starts the autorouter.
    Go,
    /// Filled red: stops it.
    Stop,
    /// A red icon and a red hover: an edit that throws work away.
    Danger,
}

/// A toolbar button with an icon before its label.
pub fn button(ui: &mut Ui, icon: &str, label: &str, tone: Tone) -> Response {
    tool_button(ui, Some(icon), label, false, tone, BUTTON_HEIGHT)
}

/// A toolbar toggle, filled while `on`.
pub fn toggle(ui: &mut Ui, icon: &str, label: &str, on: bool) -> Response {
    tool_button(ui, Some(icon), label, on, Tone::Quiet, BUTTON_HEIGHT)
}

/// Buttons that read as one control: a sunken frame around them, the chosen
/// one filled. Add them with [`segment`].
pub fn segmented<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let visuals = ui.visuals();
    Frame::new()
        .fill(visuals.extreme_bg_color)
        .stroke(visuals.widgets.noninteractive.bg_stroke)
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::same((SEGMENT_INSET - 1.0) as i8))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            add(ui)
        })
        .inner
}

/// One choice of a [`segmented`] group.
pub fn segment(ui: &mut Ui, icon: Option<&str>, label: &str, selected: bool) -> Response {
    let height = BUTTON_HEIGHT - 2.0 * SEGMENT_INSET;
    tool_button(ui, icon, label, selected, Tone::Quiet, height)
}

fn tool_button(
    ui: &mut Ui,
    icon: Option<&str>,
    label: &str,
    selected: bool,
    tone: Tone,
    height: f32,
) -> Response {
    ui.scope(|ui| {
        // `style_mut` copies the shared style; a quiet button needs none.
        if tone != Tone::Quiet {
            apply_tone(ui.style_mut(), tone);
        }
        let id = icon_id();
        let atoms = match icon {
            Some(_) => (Atom::custom(id, Vec2::splat(ICON_SIDE)), label).into_atoms(),
            None => label.into_atoms(),
        };
        let filled = selected || matches!(tone, Tone::Go | Tone::Stop);
        let shown = egui::Button::new(atoms)
            .selected(selected)
            .frame_when_inactive(filled)
            .min_size(vec2(0.0, height))
            .corner_radius(CornerRadius::same(RADIUS - 1))
            .atom_ui(ui);
        if let (Some(icon), Some(rect)) = (icon, shown.rect(id)) {
            let color = match tone {
                Tone::Danger => DANGER_ICON,
                _ => ui
                    .style()
                    .interact_selectable(&shown.response, selected)
                    .text_color(),
            };
            paint_icon(ui, rect, icon, color);
        }
        shown.response
    })
    .inner
}

fn apply_tone(style: &mut Style, tone: Tone) {
    let widgets = &mut style.visuals.widgets;
    let filled = match tone {
        Tone::Quiet => return,
        Tone::Danger => {
            widgets.hovered.weak_bg_fill = DANGER_HOVER;
            widgets.active.weak_bg_fill = DANGER_HOVER;
            return;
        }
        Tone::Go => GO,
        Tone::Stop => STOP,
    };
    for (state, fill) in [
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
    ]
    .into_iter()
    .zip(filled)
    {
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::NONE;
        state.fg_stroke.color = Color32::WHITE;
    }
}

/// Paints `icon` centred in `rect`.
fn paint_icon(ui: &Ui, rect: Rect, icon: &str, color: Color32) {
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        icon,
        FontId::proportional(ICON_FONT_SIZE),
        color,
    );
}

/// A menu entry: an icon column (blank without `icon`, so the labels of a
/// menu line up), the label, and the shortcut in weak text on the right.
pub fn menu_item(
    ui: &mut Ui,
    icon: Option<&str>,
    label: &str,
    shortcut: Option<&str>,
    enabled: bool,
) -> Response {
    ui.add_enabled_ui(enabled, |ui| {
        let id = icon_id();
        let mut button = egui::Button::new((Atom::custom(id, Vec2::splat(ICON_SIDE)), label));
        if let Some(shortcut) = shortcut {
            button = button.shortcut_text(shortcut);
        }
        let shown = button.atom_ui(ui);
        if let (Some(icon), Some(rect)) = (icon, shown.rect(id)) {
            let color = ui.style().interact(&shown.response).text_color();
            paint_icon(ui, rect, icon, color);
        }
        shown.response
    })
    .inner
}

/// The icon column of a submenu button, which egui lays out itself: the room
/// [`menu_title`] keeps at the left of `button`. `open`: the submenu shows,
/// and its title has egui's open colours.
pub fn paint_menu_icon(ui: &Ui, button: &Response, icon: &str, open: bool) {
    let left = button.rect.left() + ui.spacing().button_padding.x;
    let rect = Rect::from_min_size(
        pos2(left, button.rect.center().y - ICON_SIDE / 2.0),
        Vec2::splat(ICON_SIDE),
    );
    let visuals = &ui.visuals().widgets;
    let color = if open {
        visuals.open.text_color()
    } else {
        ui.style().interact(button).text_color()
    };
    paint_icon(ui, rect, icon, color);
}

/// The atoms of a submenu title with an icon column, for
/// [`paint_menu_icon`].
pub fn menu_title(label: &str) -> (Atom<'_>, &str) {
    (Atom::custom(icon_id(), Vec2::splat(ICON_SIDE)), label)
}

/// The menu bar's panel: a shade darker than the toolbar under it.
pub fn menu_bar_frame(style: &Style) -> Frame {
    Frame::side_top_panel(style)
        .inner_margin(Margin::symmetric(8, 3))
        .fill(MENU_BAR_FILL)
}

/// The toolbar's panel: room above and below its buttons.
pub fn toolbar_frame(style: &Style) -> Frame {
    Frame::side_top_panel(style).inner_margin(Margin::symmetric(8, 5))
}

/// The menu bar's buttons: roomier than egui's, still frameless until hovered.
pub fn bar_style(style: &mut Style) {
    egui::containers::menu::menu_style(style);
    style.spacing.button_padding = vec2(7.0, 3.0);
}

/// The open menus: taller rows with room between label and shortcut.
pub fn menu_style(style: &mut Style) {
    egui::containers::menu::menu_style(style);
    style.spacing.button_padding = vec2(6.0, 4.0);
    style.spacing.item_spacing = vec2(10.0, 1.0);
    style.spacing.menu_margin = Margin::same(5);
}

/// The open board at the right end of the menu bar: a pill with a file icon
/// at its left. The bar lays it out right to left, so `add` adds its texts
/// from the right end and the icon comes last.
pub fn chip(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let visuals = ui.visuals();
    Frame::new()
        .fill(visuals.widgets.inactive.bg_fill)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(8, 1))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            add(ui);
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(ICON_SIDE), egui::Sense::hover());
            paint_icon(ui, rect, icon::FILE, ui.visuals().weak_text_color());
        });
}

/// The app's icon (`icons/icon-32.png`), uploaded once per context.
pub fn logo(ctx: &egui::Context) -> Option<TextureHandle> {
    let id = Id::new("chrome-logo");
    if let Some(texture) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return Some(texture);
    }
    let image = decode_logo()?;
    let texture = ctx.load_texture("freerouting-logo", image, TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    Some(texture)
}

fn decode_logo() -> Option<ColorImage> {
    const PNG: &[u8] = include_bytes!("../icons/icon-32.png");
    let mut reader = png::Decoder::new(std::io::Cursor::new(PNG))
        .read_info()
        .ok()?;
    let mut pixels = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut pixels).ok()?;
    if frame.color_type != png::ColorType::Rgba || frame.bit_depth != png::BitDepth::Eight {
        return None;
    }
    let size = [frame.width as usize, frame.height as usize];
    Some(ColorImage::from_rgba_unmultiplied(
        size,
        &pixels[..frame.buffer_size()],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_logo_decodes_to_a_32_pixel_square() {
        let image = decode_logo().expect("the bundled icon decodes");
        assert_eq!(image.size, [32, 32]);
        // Not blank: the icon's dark tile is opaque.
        assert!(image.pixels.iter().any(|p| p.a() == 255));
    }

    #[test]
    fn a_filled_tone_sets_every_state_and_quiet_changes_nothing() {
        let quiet = {
            let mut style = Style::default();
            apply_tone(&mut style, Tone::Quiet);
            style
        };
        assert_eq!(quiet.visuals, Style::default().visuals);
        let mut go = Style::default();
        apply_tone(&mut go, Tone::Go);
        let w = &go.visuals.widgets;
        assert_eq!(
            [
                w.inactive.weak_bg_fill,
                w.hovered.weak_bg_fill,
                w.active.weak_bg_fill
            ],
            GO
        );
        assert_eq!(w.inactive.fg_stroke.color, Color32::WHITE);
    }
}
