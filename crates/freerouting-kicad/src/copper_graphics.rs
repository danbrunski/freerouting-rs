//! Q22.4: copper graphics and copper text, which KiCad's DSN export leaves
//! out, as obstacles (decided 2026-10-20: routing never breaks the board's
//! manufacturing rules, graded under the board's own rules).
//!
//! KiCad's DRC keeps other copper its clearance from every shape and every
//! visible text on a copper layer: footprint lines, rectangles, polygons,
//! arcs, circles and texts, and the board's own. The export writes none of
//! them (a footprint's copper lines only as image outline, which is drawing
//! only, [`crate::image_outlines`]), so the router laid 127 tracks of
//! pcietest over its connectors' B.Cu `fp_line`s and one of sonde-xilinx over
//! a `gr_text "GND"` on F.Cu.
//!
//! [`read_copper_graphics`] collects them, in board order then footprint
//! order; [`keepouts`] makes them obstacles:
//!
//! - A shape with no net, of a net-tie footprint (`net_tie_pad_groups`; its
//!   ties stay ties), or of a net without pins (not in the DSN's network)
//!   becomes keepouts named [`KEEPOUT_NAME`] on its layer: the body of a
//!   filled polygon, rectangle or circle and the rectangles within half the
//!   stroke of each piece (`edge_keepouts::shape_polygons`).
//! - A visible text (`gr_text`, `gr_text_box`, `fp_text`, a footprint
//!   `property` on copper) becomes one keepout named [`TEXT_KEEPOUT_NAME`]:
//!   a box that holds every glyph. The reader has no font engine, so the box
//!   is an over-estimate of KiCad's stroke font: `1.25 × width` per character
//!   of the longest line plus the stroke, by `1.8 × height` plus `1.62 ×
//!   height` per further line plus the stroke, placed by the text's
//!   justification (top or bottom justification covers both sides), mirrored
//!   and rotated about its anchor. A footprint's text is the square of half
//!   side `hypot(width, height)` about its anchor, which holds the box at any
//!   angle and justification. The KiCad plugin uses KiCad's own glyph strokes
//!   (`TransformTextToPolySet`).
//! - A shape of a net with pins is left out, and counted in a warning
//!   ([`left_out_warning`]: "N shape(s) of a net left out (Q22.4
//!   remainder)"). Decided 2026-10-20: fixed wires of that net covering it
//!   (boundary wires plus 50 µm strips) cut vfo-controller's route at 120 s
//!   with 7 unrouted, against 1.4 s and 0 without them. The remainder is one
//!   conduction polygon of the net plus a keepout of its outline that holds
//!   only for other nets.
//!
//! The router keeps a keepout its default clearance from a net (the larger of
//! the net's class clearance and the Default class's, as KiCad does with a
//! shape of no net), so the shapes go in at their copper size. The KiCad
//! plugin does the same to
//! KiCad's export (`kicad-plugin/freerouting_rs/copper_graphics.py`), corners
//! rounded to 1 µm. `docs/DIVERGENCES.md` lists the difference from the
//! export.

use freerouting_dsn::ast::Keepout;

use crate::edge_keepouts::{F, keepout, shape_polygons};
use crate::error::{KicadError, Warning};
use crate::footprints::{Graphic, normalize_180, parse_shape};
use crate::layers::Layers;
use crate::legacy::{Angle, Pt, rotate_about};
use crate::nets::NetTable;
use crate::sexpr::NodeRef;
use crate::units::{ki_round, mm_to_nm};

/// The name of the keepouts of copper shapes.
pub const KEEPOUT_NAME: &str = "copper_graphic";
/// The name of the keepouts of copper texts.
pub const TEXT_KEEPOUT_NAME: &str = "copper_text";
/// A shape on a copper layer.
#[derive(Clone, Debug, PartialEq)]
pub struct CopperShape {
    pub graphic: Graphic,
    /// Copper position (0 = `F.Cu`).
    pub layer: usize,
    /// KiCad net code; 0 for none and in a net-tie footprint.
    pub net: u32,
}

/// A visible text on a copper layer.
#[derive(Clone, Debug, PartialEq)]
pub struct CopperText {
    pub layer: usize,
    /// The box that holds it (KiCad's frame, nm), four corners.
    pub polygon: Vec<F>,
}

/// What [`read_copper_graphics`] found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CopperGraphics {
    pub shapes: Vec<CopperShape>,
    pub texts: Vec<CopperText>,
    /// Shapes of net-tie footprints (kept out with no net).
    pub net_tie_shapes: usize,
}

fn copper_layer(layers: &Layers, name: &str) -> Option<usize> {
    layers.copper.iter().position(|l| l.std_name == name)
}

fn layer_of(node: NodeRef<'_>) -> String {
    node.child("layer")
        .and_then(|l| l.str_value(1))
        .map(|s| s.into_owned())
        .unwrap_or_default()
}

fn point(node: NodeRef<'_>) -> Option<Pt> {
    Some((
        mm_to_nm(node.atom(1)?.parse().ok()?),
        mm_to_nm(node.atom(2)?.parse().ok()?),
    ))
}

/// `hide` as a bare atom (KiCad 6, 7) or `(hide yes)` (KiCad 8+), on the
/// text or in its `effects`.
fn hidden(node: NodeRef<'_>) -> bool {
    let has = |n: NodeRef<'_>| {
        n.items()
            .skip(1)
            .any(|c| !c.is_list() && c.text() == "hide")
            || n.child("hide").is_some_and(|h| h.atom(1) != Some("no"))
    };
    has(node) || node.child("effects").is_some_and(has)
}

/// The box (text frame, nm, x right and y down from the anchor) of a text.
fn text_box(node: NodeRef<'_>, text: &str) -> (f64, f64, f64, f64) {
    let effects = node.child("effects");
    let font = effects.and_then(|e| e.child("font"));
    let size = font.and_then(|f| f.child("size"));
    let h = size.and_then(|s| s.f64(1)).unwrap_or(1.0) * 1e6;
    let w = size.and_then(|s| s.f64(2)).unwrap_or(1.0) * 1e6;
    let t = font
        .and_then(|f| f.child("thickness"))
        .and_then(|t| t.f64(1))
        .map_or(h * 0.15, |t| t * 1e6);
    let lines: Vec<&str> = text.split('\n').flat_map(|l| l.split("\\n")).collect();
    let chars = lines
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0)
        .max(1);
    let width = chars as f64 * 1.25 * w + t;
    let height = 1.8 * h + 1.62 * h * (lines.len().max(1) - 1) as f64 + t;
    let justify: Vec<&str> = effects
        .and_then(|e| e.child("justify"))
        .map(|j| j.items().skip(1).map(|c| c.text()).collect())
        .unwrap_or_default();
    let mirror = justify.contains(&"mirror");
    let (mut left, mut right) = if justify.contains(&"left") {
        (-t, width)
    } else if justify.contains(&"right") {
        (-width, t)
    } else {
        (-width / 2.0, width / 2.0)
    };
    if mirror {
        (left, right) = (-right, -left);
    }
    let (top, bottom) = if justify.contains(&"top") || justify.contains(&"bottom") {
        (-height, height)
    } else {
        (-height / 2.0, height / 2.0)
    };
    (left, top, right, bottom)
}

fn text_string(node: NodeRef<'_>) -> String {
    let index = match node.head() {
        Some("fp_text") if node.atom(2) == Some("locked") => 3,
        Some("fp_text") | Some("property") => 2,
        _ => 1,
    };
    node.str_value(index)
        .map(|s| s.into_owned())
        .unwrap_or_default()
}

/// The keepout box of a board text: its box rotated about its anchor.
fn board_text_polygon(node: NodeRef<'_>) -> Option<Vec<F>> {
    if node.head() == Some("gr_text_box") {
        let pts: Vec<Pt> = node
            .child("pts")
            .map(|p| p.children("xy").filter_map(point).collect())
            .unwrap_or_else(|| {
                ["start", "end"]
                    .iter()
                    .filter_map(|n| node.child(n).and_then(point))
                    .collect()
            });
        let xs = pts.iter().map(|p| f64::from(p.0));
        let ys = pts.iter().map(|p| f64::from(p.1));
        let (x0, x1) = (
            xs.clone().fold(f64::MAX, f64::min),
            xs.fold(f64::MIN, f64::max),
        );
        let (y0, y1) = (
            ys.clone().fold(f64::MAX, f64::min),
            ys.fold(f64::MIN, f64::max),
        );
        return (pts.len() >= 2).then(|| vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)]);
    }
    let at = node.child("at")?;
    let anchor = point(at)?;
    let angle = at.f64(3).unwrap_or(0.0);
    let (l, t, r, b) = text_box(node, &text_string(node));
    Some(
        [(l, t), (r, t), (r, b), (l, b)]
            .iter()
            .map(|&(x, y)| {
                let p = (
                    anchor.0.saturating_add(ki_round(x)),
                    anchor.1.saturating_add(ki_round(y)),
                );
                let q = rotate_about(p, anchor, Angle(angle));
                (f64::from(q.0), f64::from(q.1))
            })
            .collect(),
    )
}

/// The keepout square of a footprint text (module docs).
fn footprint_text_polygon(node: NodeRef<'_>, fp_pos: Pt, fp_orient: f64) -> Option<Vec<F>> {
    let rel = point(node.child("at")?)?;
    let c = rotate_about(rel, (0, 0), Angle(fp_orient));
    let (cx, cy) = (
        f64::from(c.0) + f64::from(fp_pos.0),
        f64::from(c.1) + f64::from(fp_pos.1),
    );
    let (l, t, r, b) = text_box(node, &text_string(node));
    let half = (r - l).hypot(b - t);
    Some(vec![
        (cx - half, cy - half),
        (cx + half, cy - half),
        (cx + half, cy + half),
        (cx - half, cy + half),
    ])
}

const BOARD_SHAPES: [&str; 6] = [
    "gr_line",
    "gr_arc",
    "gr_circle",
    "gr_rect",
    "gr_poly",
    "gr_curve",
];
const FOOTPRINT_SHAPES: [&str; 6] = [
    "fp_line",
    "fp_arc",
    "fp_circle",
    "fp_rect",
    "fp_poly",
    "fp_curve",
];

/// Every shape and visible text on a copper layer of `board` and its
/// footprints, in board then footprint order (module docs).
pub fn read_copper_graphics(
    board: NodeRef<'_>,
    layers: &Layers,
    nets: &NetTable,
    warnings: &mut Vec<Warning>,
) -> Result<CopperGraphics, KicadError> {
    let mut out = CopperGraphics::default();
    for c in board.items().filter(|c| c.is_list()) {
        let head = c.head().unwrap_or("");
        let Some(layer) = copper_layer(layers, &layer_of(c)) else {
            continue;
        };
        if BOARD_SHAPES.contains(&head) {
            if let Some(graphic) = parse_shape(c, None, warnings)? {
                let net = nets.net_of(c).unwrap_or(0);
                out.shapes.push(CopperShape {
                    graphic,
                    layer,
                    net,
                });
            }
        } else if matches!(head, "gr_text" | "gr_text_box")
            && !hidden(c)
            && let Some(polygon) = board_text_polygon(c)
        {
            out.texts.push(CopperText { layer, polygon });
        }
    }
    for fp in board
        .items()
        .filter(|c| c.is_list() && matches!(c.head(), Some("footprint") | Some("module")))
    {
        let (pos, orient) = fp
            .child("at")
            .and_then(|at| Some((point(at)?, normalize_180(at.f64(3).unwrap_or(0.0)))))
            .unwrap_or(((0, 0), 0.0));
        let tie = fp
            .child("net_tie_pad_groups")
            .is_some_and(|g| g.str_value(1).is_some());
        for c in fp.items().filter(|c| c.is_list()) {
            let head = c.head().unwrap_or("");
            let Some(layer) = copper_layer(layers, &layer_of(c)) else {
                continue;
            };
            if FOOTPRINT_SHAPES.contains(&head) {
                if let Some(graphic) = parse_shape(c, Some((pos, orient)), warnings)? {
                    let net = if tie { 0 } else { nets.net_of(c).unwrap_or(0) };
                    out.net_tie_shapes += usize::from(tie);
                    out.shapes.push(CopperShape {
                        graphic,
                        layer,
                        net,
                    });
                }
            } else if matches!(head, "fp_text" | "property")
                && !hidden(c)
                && let Some(polygon) = footprint_text_polygon(c, pos, orient)
            {
                out.texts.push(CopperText { layer, polygon });
            }
        }
    }
    Ok(out)
}

/// The keepouts of the shapes `has_net` says have no routable net, and of
/// the texts, in DSN order (shapes, then texts).
pub fn keepouts(
    graphics: &CopperGraphics,
    layers: &Layers,
    has_net: impl Fn(u32) -> bool,
    max_error: i32,
) -> Vec<Keepout> {
    let mut out = Vec::new();
    for s in graphics.shapes.iter().filter(|s| !has_net(s.net)) {
        let (body, strokes) = shape_polygons(&s.graphic, max_error);
        out.extend(
            body.iter()
                .chain(strokes.iter())
                .map(|p| keepout(KEEPOUT_NAME, layers.name(s.layer), p)),
        );
    }
    out.extend(
        graphics
            .texts
            .iter()
            .map(|t| keepout(TEXT_KEEPOUT_NAME, layers.name(t.layer), &t.polygon)),
    );
    out
}

/// The warning that counts the shapes `has_net` says have a routable net,
/// which are left out (module docs); `None` with none.
pub fn left_out_warning(
    graphics: &CopperGraphics,
    has_net: impl Fn(u32) -> bool,
) -> Option<Warning> {
    let n = graphics.shapes.iter().filter(|s| has_net(s.net)).count();
    (n > 0).then(|| Warning {
        line: 0,
        message: format!("copper graphics: {n} shape(s) of a net left out (Q22.4 remainder)"),
    })
}
