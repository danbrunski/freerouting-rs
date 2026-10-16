//! How the board looks: colours, visibility levels, layer dimming and the
//! virtual layers (G04.4, G04.5, G04.7).
//!
//! [`Look`] is the one struct the canvas reads to colour a draw. It holds
//!
//! - the **palette**: a colour per copper layer for each kind of item
//!   (Java's `ItemColorTableModel`) and the "other" colours
//!   (`OtherColorTableModel`);
//! - the **levels**: 0 to 100 % per copper layer and per kind of object
//!   (`GraphicsContext.layerVisibilityArr` and `ColorIntensityTable`), and the
//!   automatic dimming of the layers that are not active (`autoLayerDimFactor`);
//! - the six **virtual layers** (front and back silkscreen, courtyard and
//!   fabrication) that component outlines are drawn on, each on or off.
//!
//! The levels act on the colours only. A draw whose level is 0 is left out of
//! the plan, so a hidden layer costs nothing, and a level between mixes the
//! colour into the background (opaque shapes) or lowers its alpha
//! (translucent ones), the way the fixed 0.6 dimming always did. Nothing here
//! touches the geometry, so a change needs no tessellation and no upload but
//! the colour table.
//!
//! `docs/DIVERGENCES.md` ("GUI") lists the differences from Java: the object
//! sliders are relative to the default look (100 % is the default alpha), the
//! default dimming stays at 0.6 and the virtual layers are not dimmed.

use std::collections::BTreeMap;

use eframe::egui::Color32;
use freerouting::dsn::ast::Side;

use crate::scene::{Kind, Slot};
use crate::text;

/// The highest level.
pub const MAX_LEVEL: u8 = 100;
/// The sliders snap to multiples of this (Java `WindowVisibility.SLIDER_STEP`).
pub const LEVEL_STEP: u8 = 10;
/// The default of the dimming slider. Java's `autoLayerDimFactor` is 0.7, so
/// its slider starts at 30; this port always dimmed to 0.6, which is 40.
pub const DEFAULT_DIM: u8 = 40;

/// The colour of the canvas background unless the user changes it.
pub const BACKGROUND: Color32 = Color32::from_rgb(12, 14, 18);

/// Java's `ColorIntensityTable.ObjectNames` that the port draws. (Length
/// matching areas and drill holes are not drawn here, so they have no slider.)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Object {
    Traces,
    Vias,
    Pins,
    ConductionAreas,
    Keepouts,
    ViaKeepouts,
    PlaceKeepouts,
    ComponentOutlines,
    Highlight,
    Incompletes,
}

impl Object {
    pub const ALL: [Object; 10] = [
        Object::Traces,
        Object::Vias,
        Object::Pins,
        Object::ConductionAreas,
        Object::Keepouts,
        Object::ViaKeepouts,
        Object::PlaceKeepouts,
        Object::ComponentOutlines,
        Object::Highlight,
        Object::Incompletes,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// `WindowVisibility_en.properties`.
    pub fn label(self) -> &'static str {
        match self {
            Object::Traces => text::OBJ_TRACES,
            Object::Vias => text::OBJ_VIAS,
            Object::Pins => text::OBJ_PINS,
            Object::ConductionAreas => text::OBJ_CONDUCTION_AREAS,
            Object::Keepouts => text::OBJ_KEEPOUTS,
            Object::ViaKeepouts => text::OBJ_VIA_KEEPOUTS,
            Object::PlaceKeepouts => text::OBJ_PLACE_KEEPOUTS,
            Object::ComponentOutlines => text::OBJ_COMPONENT_OUTLINES,
            Object::Highlight => text::OBJ_HIGHLIGHT,
            Object::Incompletes => text::OBJ_INCOMPLETES,
        }
    }

    /// The object a scene kind belongs to; the board outline has none.
    pub fn of_kind(kind: Kind) -> Option<Object> {
        Some(match kind {
            Kind::Plane => Object::ConductionAreas,
            Kind::Keepout => Object::Keepouts,
            Kind::ViaKeepout => Object::ViaKeepouts,
            Kind::PlaceKeepout => Object::PlaceKeepouts,
            Kind::Trace => Object::Traces,
            Kind::Pad => Object::Pins,
            Kind::Via => Object::Vias,
            Kind::ComponentOutline => Object::ComponentOutlines,
            Kind::Outline => return None,
        })
    }
}

/// The six virtual layers, in Java's order (`BoardRenderer.virtualLayerFor`):
/// front silkscreen, back silkscreen, front courtyard, back courtyard, front
/// fabrication, back fabrication.
pub const VIRTUAL_LAYERS: usize = 6;

/// The virtual layer of a slot, if it is one.
pub fn virtual_index(slot: Slot) -> Option<usize> {
    let (family, side) = match slot {
        Slot::Silk(s) => (0, s),
        Slot::Courtyard(s) => (1, s),
        Slot::Fab(s) => (2, s),
        Slot::Layer(_) | Slot::Board => return None,
    };
    Some(family * 2 + usize::from(side == Side::Back))
}

/// The name of virtual layer `i` (`WindowSelectParameter_en.properties`).
pub fn virtual_name(i: usize) -> &'static str {
    [
        text::F_SILKSCREEN,
        text::B_SILKSCREEN,
        text::F_COURTYARD,
        text::B_COURTYARD,
        text::F_FAB,
        text::B_FAB,
    ][i]
}

/// The tooltip of virtual layer `i`.
pub fn virtual_tooltip(i: usize) -> &'static str {
    [
        text::F_SILKSCREEN_TIP,
        text::B_SILKSCREEN_TIP,
        text::F_COURTYARD_TIP,
        text::B_COURTYARD_TIP,
        text::F_FAB_TIP,
        text::B_FAB_TIP,
    ][i]
}

/// The columns of the colour manager's layer table that the port draws
/// (`ItemColorTableModel`; Java's fixed traces and fixed vias have no
/// counterpart, because the scene does not tell fixed items apart).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ItemColor {
    Trace,
    Via,
    Pin,
    Plane,
    Keepout,
    ViaKeepout,
    PlaceKeepout,
}

impl ItemColor {
    pub const ALL: [ItemColor; 7] = [
        ItemColor::Trace,
        ItemColor::Via,
        ItemColor::Pin,
        ItemColor::Plane,
        ItemColor::Keepout,
        ItemColor::ViaKeepout,
        ItemColor::PlaceKeepout,
    ];

    /// `ColorTableModel_en.properties`.
    pub fn label(self) -> &'static str {
        match self {
            ItemColor::Trace => text::OBJ_TRACES,
            ItemColor::Via => text::OBJ_VIAS,
            ItemColor::Pin => text::OBJ_PINS,
            ItemColor::Plane => text::OBJ_CONDUCTION_AREAS,
            ItemColor::Keepout => text::OBJ_KEEPOUTS,
            ItemColor::ViaKeepout => text::OBJ_VIA_KEEPOUTS,
            ItemColor::PlaceKeepout => text::OBJ_PLACE_KEEPOUTS,
        }
    }

    fn key(self) -> &'static str {
        match self {
            ItemColor::Trace => "trace",
            ItemColor::Via => "via",
            ItemColor::Pin => "pin",
            ItemColor::Plane => "plane",
            ItemColor::Keepout => "keepout",
            ItemColor::ViaKeepout => "via-keepout",
            ItemColor::PlaceKeepout => "place-keepout",
        }
    }

    /// The column that colours `kind` on a copper layer.
    pub fn of_kind(kind: Kind) -> Option<ItemColor> {
        Some(match kind {
            Kind::Trace => ItemColor::Trace,
            Kind::Via => ItemColor::Via,
            Kind::Pad => ItemColor::Pin,
            Kind::Plane => ItemColor::Plane,
            Kind::Keepout => ItemColor::Keepout,
            Kind::ViaKeepout => ItemColor::ViaKeepout,
            Kind::PlaceKeepout => ItemColor::PlaceKeepout,
            Kind::Outline | Kind::ComponentOutline => return None,
        })
    }
}

/// The colours of `OtherColorTableModel` that the port draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OtherColor {
    Background,
    Highlight,
    Incompletes,
    Violations,
    Outline,
    SilkscreenFront,
    SilkscreenBack,
    CourtyardFront,
    CourtyardBack,
    FabFront,
    FabBack,
}

impl OtherColor {
    pub const ALL: [OtherColor; 11] = [
        OtherColor::Background,
        OtherColor::Highlight,
        OtherColor::Incompletes,
        OtherColor::Violations,
        OtherColor::Outline,
        OtherColor::SilkscreenFront,
        OtherColor::SilkscreenBack,
        OtherColor::CourtyardFront,
        OtherColor::CourtyardBack,
        OtherColor::FabFront,
        OtherColor::FabBack,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// `ColorTableModel_en.properties`.
    pub fn label(self) -> &'static str {
        match self {
            OtherColor::Background => text::COLOR_BACKGROUND,
            OtherColor::Highlight => text::COLOR_HIGHLIGHT,
            OtherColor::Incompletes => text::COLOR_INCOMPLETES,
            OtherColor::Violations => text::COLOR_VIOLATIONS,
            OtherColor::Outline => text::COLOR_OUTLINE,
            OtherColor::SilkscreenFront => text::COLOR_SILKSCREEN_FRONT,
            OtherColor::SilkscreenBack => text::COLOR_SILKSCREEN_BACK,
            OtherColor::CourtyardFront => text::COLOR_COURTYARD_FRONT,
            OtherColor::CourtyardBack => text::COLOR_COURTYARD_BACK,
            OtherColor::FabFront => text::COLOR_FAB_FRONT,
            OtherColor::FabBack => text::COLOR_FAB_BACK,
        }
    }

    fn key(self) -> &'static str {
        match self {
            OtherColor::Background => "background",
            OtherColor::Highlight => "highlight",
            OtherColor::Incompletes => "incompletes",
            OtherColor::Violations => "violations",
            OtherColor::Outline => "outline",
            OtherColor::SilkscreenFront => "silk-front",
            OtherColor::SilkscreenBack => "silk-back",
            OtherColor::CourtyardFront => "courtyard-front",
            OtherColor::CourtyardBack => "courtyard-back",
            OtherColor::FabFront => "fab-front",
            OtherColor::FabBack => "fab-back",
        }
    }

    /// The colour of the component outlines of a virtual layer.
    fn of_virtual(i: usize) -> OtherColor {
        [
            OtherColor::SilkscreenFront,
            OtherColor::SilkscreenBack,
            OtherColor::CourtyardFront,
            OtherColor::CourtyardBack,
            OtherColor::FabFront,
            OtherColor::FabBack,
        ][i]
    }

    /// Where the default came from: Java's `OtherColorTableModel` for the
    /// courtyard and fabrication colours, the port's first canvas for the rest.
    pub fn default_color(self) -> Color32 {
        match self {
            OtherColor::Background => BACKGROUND,
            OtherColor::Highlight => Color32::from_rgb(0, 235, 255),
            OtherColor::Incompletes => Color32::from_rgb(255, 225, 110),
            OtherColor::Violations => Color32::from_rgb(255, 70, 70),
            OtherColor::Outline => Color32::from_rgb(222, 214, 120),
            OtherColor::SilkscreenFront => Color32::from_rgb(190, 190, 190),
            OtherColor::SilkscreenBack => Color32::from_rgb(110, 125, 160),
            OtherColor::CourtyardFront => Color32::from_rgb(255, 38, 226),
            OtherColor::CourtyardBack => Color32::from_rgb(38, 233, 255),
            OtherColor::FabFront => Color32::from_rgb(175, 175, 175),
            OtherColor::FabBack => Color32::from_rgb(88, 93, 132),
        }
    }
}

/// The default colour of copper layer `index` of `count`: red for the top,
/// blue for the bottom, as most PCB tools draw them.
pub fn layer_color(index: usize, count: usize) -> Color32 {
    const INNER: [Color32; 8] = [
        Color32::from_rgb(127, 200, 127),
        Color32::from_rgb(206, 125, 44),
        Color32::from_rgb(79, 203, 203),
        Color32::from_rgb(219, 98, 139),
        Color32::from_rgb(167, 165, 198),
        Color32::from_rgb(40, 204, 217),
        Color32::from_rgb(232, 178, 167),
        Color32::from_rgb(242, 237, 161),
    ];
    if index == 0 {
        Color32::from_rgb(200, 52, 52)
    } else if index + 1 == count {
        Color32::from_rgb(77, 127, 196)
    } else {
        INNER[(index - 1) % INNER.len()]
    }
}

/// The default colour of `column` on copper layer `index` of `count`.
pub fn default_item_color(column: ItemColor, index: usize, count: usize) -> Color32 {
    let layer = layer_color(index, count);
    match column {
        ItemColor::Trace | ItemColor::Plane => layer,
        ItemColor::Pin => layer.lerp_to_gamma(Color32::WHITE, 0.15),
        ItemColor::Via => Color32::from_rgb(212, 196, 120),
        ItemColor::Keepout => Color32::from_rgb(150, 110, 200),
        ItemColor::ViaKeepout => Color32::from_rgb(150, 150, 150),
        ItemColor::PlaceKeepout => Color32::from_rgb(120, 160, 120),
    }
}

/// How opaque `kind` is drawn at level 100 (planes and keepouts are
/// translucent so that what is under them shows).
pub fn kind_alpha(kind: Kind) -> f32 {
    match kind {
        Kind::Plane => 0.35,
        Kind::Keepout => 0.3,
        Kind::ViaKeepout => 0.25,
        Kind::PlaceKeepout => 0.2,
        _ => 1.0,
    }
}

/// Everything that decides how a (slot, kind) is coloured and whether it is
/// drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    /// `[layer][ItemColor]`
    item_colors: Vec<[Color32; 7]>,
    other_colors: [Color32; 11],
    /// 0 to 100 per copper layer.
    layer_levels: Vec<u8>,
    object_levels: [u8; 10],
    /// The slider of the automatic layer dimming: 0 is no dimming, 100 draws
    /// the layers that are not active as the background. Java's factor is
    /// `1 - dim / 100`.
    dim: u8,
    virtual_on: [bool; VIRTUAL_LAYERS],
}

impl Default for Look {
    fn default() -> Self {
        Look::new(0)
    }
}

impl Look {
    /// The default look of a board with `layers` copper layers.
    pub fn new(layers: usize) -> Look {
        Look {
            item_colors: (0..layers)
                .map(|i| ItemColor::ALL.map(|c| default_item_color(c, i, layers)))
                .collect(),
            other_colors: OtherColor::ALL.map(OtherColor::default_color),
            layer_levels: vec![MAX_LEVEL; layers],
            object_levels: [MAX_LEVEL; 10],
            dim: DEFAULT_DIM,
            virtual_on: [true; VIRTUAL_LAYERS],
        }
    }

    /// A new board: the colours and the layer levels start again, the dimming,
    /// object levels and virtual layers are kept (they are about the user's
    /// taste, not about one board).
    pub fn reset_for_board(&mut self, layers: usize) {
        let kept = Look {
            object_levels: self.object_levels,
            dim: self.dim,
            virtual_on: self.virtual_on,
            other_colors: self.other_colors,
            ..Look::new(layers)
        };
        *self = kept;
    }

    pub fn layer_count(&self) -> usize {
        self.layer_levels.len()
    }

    // ---- levels ----

    pub fn layer_level(&self, layer: usize) -> u8 {
        self.layer_levels.get(layer).copied().unwrap_or(MAX_LEVEL)
    }

    pub fn set_layer_level(&mut self, layer: usize, level: u8) {
        if let Some(l) = self.layer_levels.get_mut(layer) {
            *l = level.min(MAX_LEVEL);
        }
    }

    pub fn layer_levels(&self) -> &[u8] {
        &self.layer_levels
    }

    pub fn object_level(&self, object: Object) -> u8 {
        self.object_levels[object.index()]
    }

    pub fn set_object_level(&mut self, object: Object, level: u8) {
        self.object_levels[object.index()] = level.min(MAX_LEVEL);
    }

    pub fn dim(&self) -> u8 {
        self.dim
    }

    pub fn set_dim(&mut self, dim: u8) {
        self.dim = dim.min(MAX_LEVEL);
    }

    /// Java's `autoLayerDimFactor`.
    pub fn dim_factor(&self) -> f32 {
        f32::from(MAX_LEVEL - self.dim) / f32::from(MAX_LEVEL)
    }

    /// Visibility window > Reset: every level back to 100 and the dimming to
    /// its default.
    pub fn reset_levels(&mut self) {
        self.layer_levels.iter_mut().for_each(|l| *l = MAX_LEVEL);
        self.object_levels = [MAX_LEVEL; 10];
        self.dim = DEFAULT_DIM;
    }

    pub fn virtual_on(&self, i: usize) -> bool {
        self.virtual_on[i]
    }

    pub fn set_virtual_on(&mut self, i: usize, on: bool) {
        self.virtual_on[i] = on;
    }

    pub fn virtual_flags(&self) -> [bool; VIRTUAL_LAYERS] {
        self.virtual_on
    }

    /// True if anything on `slot` can be seen: a copper layer above level 0,
    /// a virtual layer that is on, or the board outline.
    pub fn slot_visible(&self, slot: Slot) -> bool {
        match slot {
            Slot::Layer(i) => self.layer_levels.get(i).is_some_and(|&l| l > 0),
            Slot::Board => true,
            other => virtual_index(other).is_some_and(|i| self.virtual_on[i]),
        }
    }

    /// True if objects of `kind` are not at level 0.
    pub fn kind_visible(&self, kind: Kind) -> bool {
        Object::of_kind(kind).is_none_or(|o| self.object_level(o) > 0)
    }

    // ---- colours ----

    pub fn other_color(&self, color: OtherColor) -> Color32 {
        self.other_colors[color.index()]
    }

    pub fn set_other_color(&mut self, color: OtherColor, value: Color32) {
        self.other_colors[color.index()] = value;
    }

    pub fn background(&self) -> Color32 {
        self.other_color(OtherColor::Background)
    }

    pub fn item_color(&self, layer: usize, column: ItemColor) -> Color32 {
        self.item_colors
            .get(layer)
            .map_or(Color32::GRAY, |row| row[column as usize])
    }

    pub fn set_item_color(&mut self, layer: usize, column: ItemColor, value: Color32) {
        if let Some(row) = self.item_colors.get_mut(layer) {
            row[column as usize] = value;
        }
    }

    /// Colour manager > Reset to defaults.
    pub fn reset_colors(&mut self) {
        let layers = self.item_colors.len();
        *self = Look {
            item_colors: Look::new(layers).item_colors,
            other_colors: Look::new(layers).other_colors,
            ..self.clone()
        };
    }

    /// The opaque colour a (slot, kind) is drawn in at full level.
    pub fn base_color(&self, kind: Kind, slot: Slot) -> Color32 {
        match (slot, kind) {
            (Slot::Layer(i), kind) => {
                ItemColor::of_kind(kind).map_or(Color32::GRAY, |column| self.item_color(i, column))
            }
            (Slot::Board, _) => self.other_color(OtherColor::Outline),
            (slot, _) => virtual_index(slot).map_or(Color32::GRAY, |i| {
                self.other_color(OtherColor::of_virtual(i))
            }),
        }
    }

    /// The colour of a draw: the base colour with the kind's alpha, then the
    /// layer level, the object level and, for a copper layer other than
    /// `active`, the dimming. Opaque colours are mixed into the background
    /// (translucent joints of a trace would show as dark dots), translucent
    /// ones lose alpha.
    pub fn draw_color(&self, kind: Kind, slot: Slot, active: usize) -> Color32 {
        let alpha = kind_alpha(kind);
        let base = self.base_color(kind, slot);
        let mut color = if alpha < 1.0 {
            base.gamma_multiply(alpha)
        } else {
            base
        };
        let mut factor = 1.0f32;
        if let Slot::Layer(i) = slot {
            factor *= f32::from(self.layer_level(i)) / f32::from(MAX_LEVEL);
            if i != active {
                factor *= self.dim_factor();
            }
        }
        if let Some(o) = Object::of_kind(kind) {
            factor *= f32::from(self.object_level(o)) / f32::from(MAX_LEVEL);
        }
        if factor < 1.0 {
            color = if color.is_opaque() {
                self.background().lerp_to_gamma(color, factor)
            } else {
                color.gamma_multiply(factor)
            };
        }
        color
    }

    /// The colour of an overlay (`OtherColor::Highlight` and so on) at the
    /// level of `object`, as an alpha factor on the colour.
    pub fn overlay_color(&self, color: OtherColor, object: Object) -> Color32 {
        let level = f32::from(self.object_level(object)) / f32::from(MAX_LEVEL);
        self.other_color(color).gamma_multiply(level)
    }

    // ---- persistence ----

    /// The colours that differ from the defaults, as `key -> [r, g, b]`. The
    /// layer colours are keyed by layer name, so a stored colour belongs to
    /// the layer with that name on any board.
    pub fn changed_colors(&self, layer_names: &[String]) -> BTreeMap<String, [u8; 3]> {
        let mut out = BTreeMap::new();
        let defaults = Look::new(self.item_colors.len());
        for (i, name) in layer_names.iter().enumerate().take(self.item_colors.len()) {
            for column in ItemColor::ALL {
                let now = self.item_color(i, column);
                if now != defaults.item_color(i, column) {
                    out.insert(
                        format!("layer/{name}/{}", column.key()),
                        [now.r(), now.g(), now.b()],
                    );
                }
            }
        }
        for color in OtherColor::ALL {
            let now = self.other_color(color);
            if now != color.default_color() {
                out.insert(
                    format!("other/{}", color.key()),
                    [now.r(), now.g(), now.b()],
                );
            }
        }
        out
    }

    /// Applies colours stored by [`changed_colors`](Self::changed_colors).
    /// Keys that match nothing on this board are ignored.
    pub fn apply_colors(&mut self, stored: &BTreeMap<String, [u8; 3]>, layer_names: &[String]) {
        for (key, [r, g, b]) in stored {
            let value = Color32::from_rgb(*r, *g, *b);
            if let Some(rest) = key.strip_prefix("other/") {
                if let Some(color) = OtherColor::ALL.iter().find(|c| c.key() == rest) {
                    self.set_other_color(*color, value);
                }
            } else if let Some(rest) = key.strip_prefix("layer/")
                && let Some((name, column)) = rest.rsplit_once('/')
                && let Some(layer) = layer_names.iter().position(|n| n == name)
                && let Some(column) = ItemColor::ALL.iter().find(|c| c.key() == column)
            {
                self.set_item_color(layer, *column, value);
            }
        }
    }
}

/// Snaps a slider value to the step (Java `WindowVisibility.snapToStep`).
pub fn snap_level(value: u8) -> u8 {
    let step = f32::from(LEVEL_STEP);
    let snapped = (f32::from(value) / step).round() * step;
    snapped.clamp(0.0, f32::from(MAX_LEVEL)) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_first_canvas_colours() {
        let look = Look::new(2);
        // The top trace is red, the bottom blue; an inactive layer is dimmed
        // to 0.6 toward the background, as it always was.
        assert_eq!(
            look.draw_color(Kind::Trace, Slot::Layer(0), 0),
            Color32::from_rgb(200, 52, 52)
        );
        assert_eq!(
            look.draw_color(Kind::Trace, Slot::Layer(1), 0),
            BACKGROUND.lerp_to_gamma(Color32::from_rgb(77, 127, 196), 0.6)
        );
        // Planes are translucent.
        assert_eq!(
            look.draw_color(Kind::Plane, Slot::Layer(0), 0),
            Color32::from_rgb(200, 52, 52).gamma_multiply(0.35)
        );
    }

    #[test]
    fn levels_hide_and_mix() {
        let mut look = Look::new(2);
        look.set_layer_level(0, 0);
        assert!(!look.slot_visible(Slot::Layer(0)));
        assert!(look.slot_visible(Slot::Layer(1)));
        look.set_layer_level(1, 50);
        let half = look.draw_color(Kind::Trace, Slot::Layer(1), 1);
        assert_eq!(
            half,
            BACKGROUND.lerp_to_gamma(Color32::from_rgb(77, 127, 196), 0.5)
        );
        look.set_object_level(Object::Traces, 0);
        assert!(!look.kind_visible(Kind::Trace));
        assert!(look.kind_visible(Kind::Pad));
        look.reset_levels();
        assert!(look.slot_visible(Slot::Layer(0)) && look.kind_visible(Kind::Trace));
    }

    #[test]
    fn dimming_slider_replaces_the_fixed_factor() {
        let mut look = Look::new(2);
        assert!((look.dim_factor() - 0.6).abs() < 1e-6);
        look.set_dim(0);
        assert_eq!(
            look.draw_color(Kind::Trace, Slot::Layer(1), 0),
            Color32::from_rgb(77, 127, 196)
        );
    }

    #[test]
    fn virtual_layers_follow_javas_order() {
        use Side::{Back, Front};
        let slots = [
            Slot::Silk(Front),
            Slot::Silk(Back),
            Slot::Courtyard(Front),
            Slot::Courtyard(Back),
            Slot::Fab(Front),
            Slot::Fab(Back),
        ];
        for (i, slot) in slots.into_iter().enumerate() {
            assert_eq!(virtual_index(slot), Some(i));
        }
        let mut look = Look::new(2);
        look.set_virtual_on(2, false);
        assert!(!look.slot_visible(Slot::Courtyard(Front)));
        assert!(look.slot_visible(Slot::Courtyard(Back)));
    }

    #[test]
    fn colour_edits_round_trip_by_layer_name() {
        let names: Vec<String> = ["F.Cu", "B.Cu"].map(String::from).to_vec();
        let mut look = Look::new(2);
        assert!(look.changed_colors(&names).is_empty());
        look.set_item_color(0, ItemColor::Trace, Color32::from_rgb(1, 2, 3));
        look.set_other_color(OtherColor::Background, Color32::from_rgb(9, 9, 9));
        let stored = look.changed_colors(&names);
        assert_eq!(stored.len(), 2);
        let mut fresh = Look::new(2);
        fresh.apply_colors(&stored, &names);
        assert_eq!(fresh, look);
        // A board with another layer order puts the colour on the same name.
        let swapped: Vec<String> = ["B.Cu", "F.Cu"].map(String::from).to_vec();
        let mut other = Look::new(2);
        other.apply_colors(&stored, &swapped);
        assert_eq!(
            other.item_color(1, ItemColor::Trace),
            Color32::from_rgb(1, 2, 3)
        );
        look.reset_colors();
        assert_eq!(look, Look::new(2));
    }

    #[test]
    fn slider_values_snap_to_the_step() {
        assert_eq!(snap_level(44), 40);
        assert_eq!(snap_level(45), 50);
        assert_eq!(snap_level(100), 100);
        assert_eq!(snap_level(0), 0);
    }
}
