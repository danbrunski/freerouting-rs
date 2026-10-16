//! Footprints, pads and footprint graphics as KiCad holds them (K05.5).
//!
//! KiCad keeps every footprint child in board coordinates: `parsePAD` calls
//! `SetFPRelativePosition` (rotate by the footprint orientation, add its
//! position, both in integer nanometres) and `parsePCB_SHAPE` rotates and
//! moves footprint graphics the same way. The DSN exporter then reads
//! footprint-relative values back (`GetFPRelativePosition`: subtract, rotate
//! by minus the orientation), so the integer rounding of both steps shows in
//! the output (a pin at `-0.001`). This module repeats both steps in the same
//! order. For a footprint on the back, `FlipFOOTPRINTs` flips it about its own
//! position (`FOOTPRINT::Flip`, `PAD::Flip`, `EDA_SHAPE::flip`) before the
//! image is built; [`Footprint::flip_to_front`] does the same.
//!
//! Sources, all at tag `10.0.0` (GPL-2.0-or-later): `pcb_io_kicad_sexpr_parser.cpp`
//! (`parseFOOTPRINT_unchecked`, `parsePAD`, `parsePCB_SHAPE`), `footprint.cpp`
//! (`Flip`, `SetOrientation`), `pad.cpp` (`Flip`, `FlipPrimitives`),
//! `eda_shape.cpp` (`rotate`, `flip`, `SetArcGeometry`, `GetArcMid`),
//! `board_item.cpp` (`GetFPRelativePosition`).

use crate::arc::calc_arc_centre;
use crate::error::{ErrorKind, KicadError, Warning};
use crate::layers::{CuMask, copper_of_layer_token};
use crate::legacy::{Angle, Pt, rotate_about, rotate_origin};
use crate::nets::NetTable;
use crate::sexpr::NodeRef;
use crate::units::mm_to_nm;
use crate::zones::{Zone, ZoneLayerMap, parse_zone};

/// `DEFAULT_LINE_WIDTH`, 0.1 mm: KiCad gives an unfilled shape without a
/// stroke width this one.
pub const DEFAULT_LINE_WIDTH: i32 = 100_000;

/// The pad shapes of KiCad (`PAD_SHAPE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadShape {
    Circle,
    Rect,
    Oval,
    Trapezoid,
    RoundRect,
    ChamferedRect,
    Custom,
}

/// The pad types (`PAD_ATTRIB`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadAttr {
    Pth,
    Smd,
    Conn,
    Npth,
}

/// The geometry of a graphic item, in board coordinates (nm).
#[derive(Clone, Debug, PartialEq)]
pub enum GKind {
    Segment {
        start: Pt,
        end: Pt,
    },
    Rect {
        start: Pt,
        end: Pt,
    },
    /// `start` is the centre, `end` a point on the circle (KiCad's layout).
    Circle {
        start: Pt,
        end: Pt,
    },
    /// An arc: `start` and `end` as KiCad keeps them after `SetArcGeometry`
    /// (swapped when the winding differed), the computed `centre`, and the
    /// mid point the file gave while nothing has moved the arc.
    Arc {
        start: Pt,
        end: Pt,
        centre: Pt,
        cached: Option<ArcCache>,
    },
    Poly(Vec<Pt>),
    Bezier {
        start: Pt,
        c1: Pt,
        c2: Pt,
        end: Pt,
    },
}

/// `m_arcMidData`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArcCache {
    pub start: Pt,
    pub end: Pt,
    pub centre: Pt,
    pub mid: Pt,
}

/// A `PCB_SHAPE`.
#[derive(Clone, Debug, PartialEq)]
pub struct Graphic {
    pub kind: GKind,
    /// The layer name as the file spells it (`F.SilkS`, `Edge.Cuts`).
    pub layer: String,
    /// Stroke width, nm.
    pub width: i32,
    pub filled: bool,
    pub line: u32,
}

impl Graphic {
    /// `EDA_SHAPE::GetArcMid`.
    pub fn arc_mid(&self) -> Option<Pt> {
        let GKind::Arc {
            start,
            end,
            centre,
            cached,
        } = &self.kind
        else {
            return None;
        };
        if let Some(c) = cached
            && c.start == *start
            && c.end == *end
            && c.centre == *centre
        {
            return Some(c.mid);
        }
        let angle = arc_angle(*start, *end, *centre);
        Some(rotate_about(*start, *centre, Angle(-angle / 2.0)))
    }

    /// `EDA_SHAPE::GetStart`/`GetEnd` of the shapes the DSN export and the
    /// outline chaining use.
    pub fn ends(&self) -> Option<(Pt, Pt)> {
        match &self.kind {
            GKind::Segment { start, end }
            | GKind::Arc { start, end, .. }
            | GKind::Bezier { start, end, .. } => Some((*start, *end)),
            _ => None,
        }
    }

    /// `EDA_SHAPE::rotate`.
    pub fn rotate(&mut self, centre: Pt, a: Angle) {
        let rp = |p: &mut Pt| *p = rotate_about(*p, centre, a);
        let mut to_poly = None;
        match &mut self.kind {
            GKind::Segment { start, end } | GKind::Circle { start, end } => {
                rp(start);
                rp(end);
            }
            GKind::Arc {
                start,
                end,
                centre: c,
                cached,
            } => {
                rp(start);
                rp(end);
                rp(c);
                if let Some(k) = cached {
                    rp(&mut k.start);
                    rp(&mut k.end);
                    rp(&mut k.mid);
                    rp(&mut k.centre);
                }
            }
            GKind::Rect { start, end } => {
                if is_cardinal(a.0) {
                    rp(start);
                    rp(end);
                } else {
                    let (s, e) = (*start, *end);
                    // ROUNDRECT with radius 0: SHAPE_RECT corners
                    let (w, h) = (e.0 - s.0, e.1 - s.1);
                    let (x0, x1) = if w < 0 {
                        (s.0 + w, s.0)
                    } else {
                        (s.0, s.0 + w)
                    };
                    let (y0, y1) = if h < 0 {
                        (s.1 + h, s.1)
                    } else {
                        (s.1, s.1 + h)
                    };
                    let mut pts = vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
                    for p in &mut pts {
                        rp(p);
                    }
                    to_poly = Some(pts);
                }
            }
            GKind::Poly(pts) => pts.iter_mut().for_each(rp),
            GKind::Bezier { start, c1, c2, end } => {
                rp(start);
                rp(c1);
                rp(c2);
                rp(end);
            }
        }
        if let Some(pts) = to_poly {
            self.kind = GKind::Poly(pts);
        }
    }

    /// `Move`.
    pub fn translate(&mut self, d: Pt) {
        let mv = |p: &mut Pt| *p = (p.0.wrapping_add(d.0), p.1.wrapping_add(d.1));
        match &mut self.kind {
            GKind::Segment { start, end }
            | GKind::Circle { start, end }
            | GKind::Rect { start, end } => {
                mv(start);
                mv(end);
            }
            GKind::Arc {
                start,
                end,
                centre,
                cached,
            } => {
                mv(start);
                mv(end);
                mv(centre);
                if let Some(k) = cached {
                    mv(&mut k.start);
                    mv(&mut k.end);
                    mv(&mut k.mid);
                    mv(&mut k.centre);
                }
            }
            GKind::Poly(pts) => pts.iter_mut().for_each(mv),
            GKind::Bezier { start, c1, c2, end } => {
                mv(start);
                mv(c1);
                mv(c2);
                mv(end);
            }
        }
    }

    /// `EDA_SHAPE::flip( centre, TOP_BOTTOM )`: mirror Y about `centre_y`;
    /// arcs also swap their ends.
    pub fn flip_top_bottom(&mut self, centre_y: i32) {
        let m = |p: &mut Pt| p.1 = centre_y.wrapping_sub(p.1.wrapping_sub(centre_y));
        match &mut self.kind {
            GKind::Segment { start, end }
            | GKind::Rect { start, end }
            | GKind::Circle { start, end } => {
                m(start);
                m(end);
            }
            GKind::Arc {
                start, end, centre, ..
            } => {
                m(start);
                m(end);
                m(centre);
                std::mem::swap(start, end);
            }
            GKind::Poly(pts) => pts.iter_mut().for_each(m),
            GKind::Bezier { start, c1, c2, end } => {
                m(start);
                m(c1);
                m(c2);
                m(end);
            }
        }
    }
}

/// `EDA_ANGLE::IsCardinal`.
fn is_cardinal(deg: f64) -> bool {
    let mut t = deg;
    while t < 0.0 {
        t += 90.0;
    }
    while t >= 90.0 {
        t -= 90.0;
    }
    t == 0.0
}

/// `CalcArcAngles` then `GetArcAngle`: end minus start, degrees, 0 < a <= 360.
fn arc_angle(start: Pt, end: Pt, centre: Pt) -> f64 {
    let sa = Angle::from_vector(f64::from(start.0 - centre.0), f64::from(start.1 - centre.1));
    let mut ea = Angle::from_vector(f64::from(end.0 - centre.0), f64::from(end.1 - centre.1));
    if ea.0 == sa.0 {
        ea = Angle(sa.0 + 360.0);
    }
    while ea.0 < sa.0 {
        ea = Angle(ea.0 + 360.0);
    }
    ea.0 - sa.0
}

/// A pad, in board coordinates; primitives and the shape values stay in the
/// pad's own frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Pad {
    /// The pad number as written (empty for NPTH pads, which have none).
    pub number: String,
    pub attr: PadAttr,
    pub shape: PadShape,
    /// The anchor of a custom pad: `true` for a rectangle, `false` for a circle.
    pub anchor_rect: bool,
    pub pos: Pt,
    /// Absolute orientation in degrees (the file's `(at x y angle)`, flipped
    /// with the footprint).
    pub orient: f64,
    pub size: (i32, i32),
    pub offset: Pt,
    pub delta: Pt,
    pub drill: (i32, i32),
    pub rratio: f64,
    pub chamfer_ratio: f64,
    /// `RECT_CHAMFER_*` bits.
    pub chamfer: i32,
    /// The copper layers of the pad.
    pub layers: CuMask,
    /// Whether the pad lists any layer outside copper (mask, paste).
    pub net: u32,
    pub primitives: Vec<Graphic>,
    /// `(die_length ...)`, nm.
    pub die_length: Option<i64>,
    /// Always 0: counting lines is linear in the file.
    pub line: u32,
}

/// A footprint with everything the image builder reads.
#[derive(Clone, Debug, PartialEq)]
pub struct Footprint {
    /// The footprint id (`lib:name`).
    pub id: String,
    pub reference: String,
    pub value: String,
    pub back: bool,
    pub pos: Pt,
    /// Orientation in degrees, normalised to (-180, 180] like `SetOrientation`.
    pub orient: f64,
    pub pads: Vec<Pad>,
    pub graphics: Vec<Graphic>,
    /// The footprint's zones, in board coordinates like the file stores them
    /// (K06.3); mirrored by [`Footprint::flip_to_front`].
    pub zones: Vec<Zone>,
    /// The same zones as the file has them, never flipped: a copper zone of a
    /// back-side footprint is a plane in board coordinates.
    pub zones_as_filed: Vec<Zone>,
    /// Arena index of the footprint's node in the tree (its line is computed
    /// on demand: counting lines is linear in the file).
    pub line: u32,
    /// True after [`Footprint::flip_to_front`].
    pub flipped: bool,
}

/// `EDA_ANGLE::Normalize180`.
pub fn normalize_180(mut v: f64) -> f64 {
    while v <= -180.0 {
        v += 360.0;
    }
    while v > 180.0 {
        v -= 360.0;
    }
    v
}

/// `EDA_ANGLE::Normalize` into [0, 360).
pub fn normalize_360(mut v: f64) -> f64 {
    while v < -0.0 {
        v += 360.0;
    }
    while v >= 360.0 {
        v -= 360.0;
    }
    v
}

fn pt_of(node: NodeRef<'_>) -> Option<Pt> {
    Some((
        mm_to_nm(node.atom(1)?.parse().ok()?),
        mm_to_nm(node.atom(2)?.parse().ok()?),
    ))
}

fn bad(node: NodeRef<'_>, what: &str) -> KicadError {
    node.error(ErrorKind::Semantic, what.to_owned())
}

/// A `(layer "X")` name of a shape.
fn layer_name(node: NodeRef<'_>) -> String {
    node.child("layer")
        .and_then(|l| l.str_value(1))
        .map(|s| s.into_owned())
        .unwrap_or_default()
}

/// `parsePCB_SHAPE` for the shapes the reader uses. `fp` is the parent
/// footprint's position and orientation (degrees), or `None` for a board
/// shape or a pad primitive.
pub fn parse_shape(
    node: NodeRef<'_>,
    fp: Option<(Pt, f64)>,
    warnings: &mut Vec<Warning>,
) -> Result<Option<Graphic>, KicadError> {
    let head = node.head().unwrap_or("");
    let kind_name = head.split('_').nth(1).unwrap_or("");
    let p = |name: &str| -> Result<Pt, KicadError> {
        node.child(name)
            .and_then(pt_of)
            .ok_or_else(|| bad(node, &format!("`{head}` without a valid ({name} x y)")))
    };
    let kind = match kind_name {
        "line" | "vector" => GKind::Segment {
            start: p("start")?,
            end: p("end")?,
        },
        "rect" | "bbox" => GKind::Rect {
            start: p("start")?,
            end: p("end")?,
        },
        "circle" => GKind::Circle {
            start: p("center")?,
            end: p("end")?,
        },
        "arc" => {
            let (s, m, e) = (p("start")?, p("mid")?, p("end")?);
            // SetArcGeometry
            let centre = calc_arc_centre(s, m, e);
            let g = Graphic {
                kind: GKind::Arc {
                    start: s,
                    end: e,
                    centre,
                    cached: None,
                },
                layer: String::new(),
                width: 0,
                filled: false,
                line: 0,
            };
            let new_mid = g.arc_mid().unwrap_or(m);
            let d1 = (
                f64::from(new_mid.0) - f64::from(m.0),
                f64::from(new_mid.1) - f64::from(m.1),
            );
            let d2 = (
                f64::from(new_mid.0) - f64::from(centre.0),
                f64::from(new_mid.1) - f64::from(centre.1),
            );
            let cache = ArcCache {
                start: s,
                end: e,
                centre,
                mid: m,
            };
            let (mut st, mut en) = (s, e);
            if d1.0 * d1.0 + d1.1 * d1.1 > d2.0 * d2.0 + d2.1 * d2.1 {
                std::mem::swap(&mut st, &mut en);
            }
            GKind::Arc {
                start: st,
                end: en,
                centre,
                cached: Some(cache),
            }
        }
        "poly" => {
            let pts: Vec<Pt> = node
                .child("pts")
                .map(|l| l.children("xy").filter_map(pt_of).collect())
                .unwrap_or_default();
            GKind::Poly(pts)
        }
        "curve" => {
            let pts: Vec<Pt> = node
                .child("pts")
                .map(|l| l.children("xy").filter_map(pt_of).collect())
                .unwrap_or_default();
            if pts.len() < 4 {
                warnings.push(Warning {
                    line: 0,
                    message: format!("`{head}` has fewer than four control points; ignored"),
                });
                return Ok(None);
            }
            GKind::Bezier {
                start: pts[0],
                c1: pts[1],
                c2: pts[2],
                end: pts[3],
            }
        }
        _ => return Ok(None),
    };
    // stroke width: (stroke (width w)) or the legacy (width w)
    let width = node
        .child("stroke")
        .and_then(|s| s.child("width"))
        .or_else(|| node.child("width"))
        .and_then(|w| w.atom(1))
        .and_then(crate::units::parse_board_units)
        .unwrap_or(0);
    let fill = node
        .child("fill")
        .and_then(|f| f.atom(1).map(str::to_owned));
    let layer = layer_name(node);
    let mut filled = match fill.as_deref() {
        Some("yes") | Some("solid") => true,
        Some(_) => false,
        None => false,
    };
    if fill.is_none() {
        // legacy files: a zero stroke fills rectangles and circles, and
        // polygons off Edge.Cuts were always filled
        if (width == 0 && matches!(kind_name, "rect" | "circle"))
            || (kind_name == "poly" && layer != "Edge.Cuts")
        {
            filled = true;
        }
    }
    let width = if width <= 0 && !filled {
        DEFAULT_LINE_WIDTH
    } else {
        width
    };
    let mut g = Graphic {
        kind,
        layer,
        width,
        filled,
        line: 0,
    };
    if let Some((pos, orient)) = fp {
        g.rotate((0, 0), Angle(orient));
        g.translate(pos);
    }
    Ok(Some(g))
}

const RECT_CHAMFER_TOP_LEFT: i32 = 1;
const RECT_CHAMFER_TOP_RIGHT: i32 = 2;
const RECT_CHAMFER_BOTTOM_LEFT: i32 = 4;
const RECT_CHAMFER_BOTTOM_RIGHT: i32 = 8;

fn parse_pad(
    node: NodeRef<'_>,
    fp_pos: Pt,
    fp_orient: f64,
    nets: &NetTable,
    warnings: &mut Vec<Warning>,
) -> Result<Pad, KicadError> {
    let number = node
        .str_value(1)
        .map(|s| s.into_owned())
        .unwrap_or_default();
    let attr = match node.atom(2) {
        Some("thru_hole") => PadAttr::Pth,
        Some("smd") => PadAttr::Smd,
        Some("connect") => PadAttr::Conn,
        Some("np_thru_hole") => PadAttr::Npth,
        other => {
            return Err(bad(
                node,
                &format!(
                    "pad type `{}` is not thru_hole, smd, connect or np_thru_hole",
                    other.unwrap_or("")
                ),
            ));
        }
    };
    let mut shape = match node.atom(3) {
        Some("circle") => PadShape::Circle,
        Some("rect") => PadShape::Rect,
        Some("oval") => PadShape::Oval,
        Some("trapezoid") => PadShape::Trapezoid,
        Some("roundrect") => PadShape::RoundRect,
        Some("custom") => PadShape::Custom,
        other => {
            return Err(bad(
                node,
                &format!("unknown pad shape `{}`", other.unwrap_or("")),
            ));
        }
    };
    let mut pad = Pad {
        number,
        attr,
        shape,
        anchor_rect: false,
        pos: fp_pos,
        orient: 0.0,
        size: (0, 0),
        offset: (0, 0),
        delta: (0, 0),
        drill: match attr {
            PadAttr::Pth => (1, 1),
            _ => (0, 0),
        },
        rratio: 0.25,
        chamfer_ratio: 0.2,
        chamfer: 0,
        layers: CuMask::ALL,
        net: 0,
        primitives: Vec::new(),
        die_length: None,
        line: 0,
    };
    for child in node.items().skip(4).filter(|c| c.is_list()) {
        match child.head().unwrap_or("") {
            "size" => {
                pad.size = (
                    child
                        .atom(1)
                        .and_then(crate::units::parse_board_units)
                        .unwrap_or(0),
                    child
                        .atom(2)
                        .and_then(crate::units::parse_board_units)
                        .unwrap_or(0),
                );
            }
            "at" => {
                if let Some(local) = pt_of(child) {
                    let r = rotate_origin(local.0, local.1, Angle(fp_orient));
                    pad.pos = (r.0.wrapping_add(fp_pos.0), r.1.wrapping_add(fp_pos.1));
                }
                if let Some(a) = child.f64(3) {
                    pad.orient = a;
                }
            }
            "rect_delta" => {
                pad.delta = pt_of(child).unwrap_or((0, 0));
            }
            "drill" => {
                let mut have_width = false;
                let mut d = pad.drill;
                for it in child.items().skip(1) {
                    if it.is_list() {
                        if it.head() == Some("offset") {
                            pad.offset = pt_of(it).unwrap_or((0, 0));
                        }
                    } else if it.text() == "oval" {
                        // oblong drill: shape only matters for the pad's own keepout test
                    } else if let Ok(v) = it.text().parse::<f64>() {
                        let nm = mm_to_nm(v);
                        if !have_width {
                            d = (nm, nm);
                            have_width = true;
                        } else {
                            d.1 = nm;
                        }
                    }
                }
                pad.drill = if matches!(attr, PadAttr::Smd | PadAttr::Conn) {
                    (0, 0)
                } else {
                    d
                };
            }
            "layers" => {
                let mut m = CuMask::NONE;
                for t in child.items().skip(1) {
                    m = m.or(copper_of_layer_token(t.text()));
                }
                pad.layers = m;
            }
            "roundrect_rratio" => {
                pad.rratio = child.f64(1).unwrap_or(0.25).clamp(0.0, 0.5);
            }
            "chamfer_ratio" => {
                pad.chamfer_ratio = child.f64(1).unwrap_or(0.2);
                if pad.chamfer_ratio > 0.0 {
                    shape = PadShape::ChamferedRect;
                }
            }
            "chamfer" => {
                let mut bits = 0;
                for t in child.items().skip(1) {
                    bits |= match t.text() {
                        "top_left" => RECT_CHAMFER_TOP_LEFT,
                        "top_right" => RECT_CHAMFER_TOP_RIGHT,
                        "bottom_left" => RECT_CHAMFER_BOTTOM_LEFT,
                        "bottom_right" => RECT_CHAMFER_BOTTOM_RIGHT,
                        _ => 0,
                    };
                }
                pad.chamfer = bits;
                if bits != 0 {
                    shape = PadShape::ChamferedRect;
                }
            }
            "die_length" => {
                pad.die_length = child.atom(1).and_then(crate::units::nm);
            }
            "options" => {
                if let Some(a) = child.child("anchor") {
                    pad.anchor_rect = a.atom(1) == Some("rect");
                }
            }
            "primitives" => {
                for prim in child.items().skip(1).filter(|p| p.is_list()) {
                    if matches!(prim.head(), Some("gr_bbox") | Some("gr_vector")) {
                        continue; // proxy items
                    }
                    if let Some(g) = parse_shape(prim, None, warnings)? {
                        pad.primitives.push(g);
                    }
                }
            }
            "padstack" => {
                let mode = child.child("mode").and_then(|m| m.atom(1)).unwrap_or("");
                if !mode.is_empty() && mode != "normal" {
                    warnings.push(Warning {
                        line: 0,
                        message: format!(
                            "pad `{}` has a `{mode}` padstack; only the pad's main shape is used",
                            pad.number
                        ),
                    });
                }
            }
            "net" => {
                pad.net = nets.net_of(node).unwrap_or(0);
            }
            _ => {}
        }
    }
    pad.shape = shape;
    if pad.attr == PadAttr::Npth {
        pad.number.clear();
    }
    if pad.size.0 <= 0 || pad.size.1 <= 0 {
        pad.size = (1_000, 1_000);
        warnings.push(Warning {
            line: 0,
            message: "invalid zero-sized pad pinned to 1 µm".to_owned(),
        });
    }
    if pad.attr == PadAttr::Pth && node.child("layers").is_none() {
        pad.layers = CuMask::ALL;
    }
    Ok(pad)
}

/// Reads every footprint of `board`, in file order, in board coordinates and
/// as the file has them (not flipped).
pub fn read_footprints(
    board: NodeRef<'_>,
    nets: &NetTable,
    warnings: &mut Vec<Warning>,
) -> Result<Vec<Footprint>, KicadError> {
    let mut out = Vec::new();
    let mut zone_map: Option<ZoneLayerMap> = None;
    for node in board
        .items()
        .filter(|c| c.is_list() && matches!(c.head(), Some("footprint") | Some("module")))
    {
        out.push(read_footprint(node, nets, &mut zone_map, board, warnings)?);
    }
    Ok(out)
}

fn read_footprint(
    node: NodeRef<'_>,
    nets: &NetTable,
    zone_map: &mut Option<ZoneLayerMap>,
    board: NodeRef<'_>,
    warnings: &mut Vec<Warning>,
) -> Result<Footprint, KicadError> {
    let id = node
        .str_value(1)
        .map(|s| s.into_owned())
        .unwrap_or_default();
    let (mut pos, mut orient) = ((0, 0), 0.0);
    if let Some(at) = node.child("at") {
        pos = pt_of(at).ok_or_else(|| bad(at, "footprint `(at)` without coordinates"))?;
        orient = normalize_180(at.f64(3).unwrap_or(0.0));
    }
    let back = node
        .child("layer")
        .and_then(|l| l.str_value(1))
        .is_some_and(|l| l == "B.Cu");
    let mut reference = String::new();
    let mut value = String::new();
    for p in node.children("property") {
        match p.str_value(1).as_deref() {
            Some("Reference") => {
                reference = p.str_value(2).map(|s| s.into_owned()).unwrap_or_default()
            }
            Some("Value") => value = p.str_value(2).map(|s| s.into_owned()).unwrap_or_default(),
            _ => {}
        }
    }
    for t in node.children("fp_text") {
        // KiCad 6 can write `(fp_text reference locked "U4" ...)`
        let text_index = if t.atom(2) == Some("locked") { 3 } else { 2 };
        let text = t
            .str_value(text_index)
            .map(|s| s.into_owned())
            .unwrap_or_default();
        match t.atom(1) {
            Some("reference") => reference = text,
            Some("value") => value = text,
            _ => {}
        }
    }
    let mut pads = Vec::new();
    let mut graphics = Vec::new();
    let mut zones = Vec::new();
    for child in node.items().filter(|c| c.is_list()) {
        match child.head().unwrap_or("") {
            "zone" => {
                let map = zone_map.get_or_insert_with(|| ZoneLayerMap::from_board(board));
                if let Some(z) = parse_zone(child, map, nets, true, warnings) {
                    zones.push(z);
                }
            }
            "pad" => pads.push(parse_pad(child, pos, orient, nets, warnings)?),
            "fp_line" | "fp_rect" | "fp_circle" | "fp_arc" | "fp_poly" | "fp_curve" => {
                if let Some(g) = parse_shape(child, Some((pos, orient)), warnings)? {
                    graphics.push(g);
                }
            }
            _ => {}
        }
    }
    Ok(Footprint {
        id,
        reference,
        value,
        back,
        pos,
        orient,
        pads,
        graphics,
        zones_as_filed: zones.clone(),
        zones,
        line: node.index(),
        flipped: false,
    })
}

impl Footprint {
    /// The point `pt` relative to the footprint (`GetFPRelativePosition`).
    pub fn relative(&self, pt: Pt) -> Pt {
        let d = (pt.0.wrapping_sub(self.pos.0), pt.1.wrapping_sub(self.pos.1));
        rotate_origin(d.0, d.1, Angle(-self.orient))
    }

    /// `FlipFOOTPRINTs` for one footprint: `FOOTPRINT::Flip( position,
    /// TOP_BOTTOM )` on a footprint on the back.
    pub fn flip_to_front(&mut self, copper_count: usize) {
        let cy = self.pos.1;
        let new_orient = normalize_180(-self.orient);
        for pad in &mut self.pads {
            // MIRROR( m_pos, centre, flip )
            pad.pos.1 = cy.wrapping_sub(pad.pos.1.wrapping_sub(cy));
            pad.offset.1 = pad.offset.1.wrapping_neg();
            pad.delta.1 = pad.delta.1.wrapping_neg();
            // SetFPRelativeOrientation( -GetFPRelativeOrientation() ) with the
            // footprint orientation temporarily zero
            pad.orient = -pad.orient;
            let c = pad.chamfer;
            let swap = |a: i32, b: i32, bits: i32| -> i32 {
                let (ha, hb) = (bits & a != 0, bits & b != 0);
                let mut r = bits & !(a | b);
                if ha {
                    r |= b;
                }
                if hb {
                    r |= a;
                }
                r
            };
            let c = swap(RECT_CHAMFER_TOP_LEFT, RECT_CHAMFER_BOTTOM_LEFT, c);
            pad.chamfer = swap(RECT_CHAMFER_TOP_RIGHT, RECT_CHAMFER_BOTTOM_RIGHT, c);
            pad.layers = pad.layers.flipped(copper_count);
            for prim in &mut pad.primitives {
                prim.flip_top_bottom(0);
            }
        }
        self.orient = new_orient;
        for z in &mut self.zones {
            z.flip_top_bottom(cy, copper_count);
        }
        for g in &mut self.graphics {
            g.flip_top_bottom(cy);
            g.layer = flip_layer_name(&g.layer);
        }
        self.flipped = true;
    }
}

/// Maps `F.X` to `B.X` and back for the layer names that come in pairs.
pub fn flip_layer_name(name: &str) -> String {
    if let Some(rest) = name.strip_prefix("F.") {
        format!("B.{rest}")
    } else if let Some(rest) = name.strip_prefix("B.") {
        format!("F.{rest}")
    } else {
        name.to_owned()
    }
}
