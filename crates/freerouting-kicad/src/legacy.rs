//! Conversions KiCad applies when it loads an older file, done on the tree so
//! that later stages see one format.
//!
//! Ported from KiCad 10.0.0 (GPL-2.0-or-later):
//!
//! - **Old arcs.** `PCB_IO_KICAD_SEXPR_PARSER::parsePCB_SHAPE`
//!   (`pcb_io_kicad_sexpr_parser.cpp`) for `version <= LEGACY_ARC_FORMATTING`
//!   (`20210925`, `pcb_io_kicad_sexpr.h`): `(start CENTER) (end ARC_START)
//!   (angle SWEEP)` becomes `(start) (mid) (end)` through
//!   `EDA_SHAPE::SetArcAngleAndEnd` and `GetArcMid` (`common/eda_shape.cpp`),
//!   `RotatePoint` (`libs/kimath/src/trigo.cpp`) and `EDA_ANGLE`
//!   (`libs/kimath/include/geometry/eda_angle.h`). Applies to `gr_arc` and
//!   `fp_arc` (graphic arcs, also inside custom pad primitives). Track arcs
//!   (`(arc (start) (mid) (end))`) have always been start/mid/end.
//! - **Old overbars.** `ConvertToNewOverbarNotation` (`common/string_utils.cpp`)
//!   for `version < 20210606`: `~TEXT~` becomes `~{TEXT}`. KiCad applies it
//!   to the table `(net N "name")` (top level and in pads) and to
//!   `(add_net "name")` in net classes; so does this module. It does not apply
//!   it to a zone's `(net_name ...)`, and neither does this module. Net names
//!   reach the DSN and the SES, so this decides whether KiCad's SES import
//!   finds the net.
//! - **`tstamp`.** KiCad reads `(tstamp X)` as the item's UUID wherever
//!   `(uuid X)` is also accepted; [`item_uuid`] does the same, with `uuid`
//!   winning.
//!
//! Converted items are appended to the tree (see [`crate::sexpr::tree`]); the
//! parsed nodes are not renumbered.

use std::borrow::Cow;

use crate::error::Warning;
use crate::sexpr::{NodeKind, NodeRef, Tree};
use crate::units::{ki_round, mm_to_nm, nm_to_mm_text};

/// Last file version with the old arc syntax (`LEGACY_ARC_FORMATTING`).
pub const LEGACY_ARC_FORMATTING: u32 = 20210925;
/// First file version with `~{...}` overbars (`m_requiredVersion < 20210606`
/// is converted).
pub const NEW_OVERBAR_NOTATION: u32 = 20210606;

/// What [`normalise`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Conversions {
    /// `gr_arc` / `fp_arc` items rewritten to start/mid/end.
    pub arcs: usize,
    /// Net names whose overbars were rewritten.
    pub overbars: usize,
    /// Items that looked old but could not be converted (left as they were).
    pub warnings: Vec<Warning>,
}

/// Applies the conversions that `version` calls for.
pub fn normalise(tree: &mut Tree<'_>, version: u32) -> Conversions {
    let mut out = Conversions::default();
    let n = tree.len() as u32;
    let arcs = version <= LEGACY_ARC_FORMATTING;
    let bars = version < NEW_OVERBAR_NOTATION;
    if !arcs && !bars {
        return out;
    }
    for idx in 0..n {
        let node = tree.node(idx);
        if !node.is_list() {
            continue;
        }
        match node.head() {
            Some("gr_arc") | Some("fp_arc") if arcs => match convert_arc(tree, idx) {
                Ok(()) => out.arcs += 1,
                Err(message) => out.warnings.push(Warning {
                    line: tree.node(idx).line(),
                    message,
                }),
            },
            Some("net") if bars => {
                if convert_name_item(tree, idx, 2) {
                    out.overbars += 1;
                }
            }
            Some("add_net") if bars => {
                if convert_name_item(tree, idx, 1) {
                    out.overbars += 1;
                }
            }
            _ => {}
        }
    }
    out
}

/// The UUID of an item: its `(uuid X)`, else its `(tstamp X)`.
pub fn item_uuid<'a>(item: NodeRef<'a>) -> Option<Cow<'a, str>> {
    item.child("uuid")
        .or_else(|| item.child("tstamp"))
        .and_then(|u| u.str_value(1))
}

fn convert_name_item(tree: &mut Tree<'_>, list: u32, at: usize) -> bool {
    let node = tree.node(list);
    let Some(name) = node.item(at) else {
        return false;
    };
    if name.kind() == NodeKind::List {
        return false;
    }
    let was_str = name.kind() == NodeKind::Str;
    let old = node.str_value(at).unwrap_or_default().into_owned();
    let new = convert_overbar(&old);
    if new == old {
        return false;
    }
    let new_node = if was_str {
        tree.add_str(&new)
    } else {
        tree.add_atom(&new)
    };
    let mut items = tree.item_indices(list);
    items[at] = new_node;
    tree.set_items(list, &items);
    true
}

/// `ConvertToNewOverbarNotation` (`common/string_utils.cpp`): `~A~` to `~{A}`.
///
/// A doubled tilde is a literal tilde; `~~{` and a tilde followed by `{`
/// are left alone (already converted); a space, `}` or `)` ends an open
/// overbar; an overbar still open at the end is closed.
pub fn convert_overbar(old: &str) -> String {
    if old == "~" {
        return old.to_owned();
    }
    let chars: Vec<char> = old.chars().collect();
    let mut out = String::with_capacity(old.len() + 4);
    let mut in_overbar = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '~' {
            let next = chars.get(i + 1).copied();
            if next == Some('~') {
                if chars.get(i + 2).copied() == Some('{') {
                    // "~~{": the brace must not start an overbar
                    out.push_str("~~{}");
                    i += 1;
                    continue;
                }
                // two tildes mean one tilde
                out.push('~');
                i += 2;
                continue;
            } else if next == Some('{') {
                // already in the new notation: bail out
                return old.to_owned();
            }
            if in_overbar {
                out.push('}');
                in_overbar = false;
            } else {
                out.push_str("~{");
                in_overbar = true;
            }
            i += 1;
            continue;
        } else if (c == ' ' || c == '}' || c == ')') && in_overbar {
            out.push('}');
            in_overbar = false;
        }
        out.push(c);
        i += 1;
    }
    if in_overbar {
        out.push('}');
    }
    out
}

// ---- arcs ---------------------------------------------------------------

/// An `EDA_ANGLE`: degrees as an `f64`, with KiCad's exact special cases.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Angle(pub f64);

const DEGREES_TO_RADIANS: f64 = std::f64::consts::PI / 180.0;

impl Angle {
    pub fn from_radians(r: f64) -> Angle {
        Angle(r / DEGREES_TO_RADIANS)
    }

    /// `EDA_ANGLE(VECTOR2D)`.
    pub fn from_vector(x: f64, y: f64) -> Angle {
        if x == 0.0 && y == 0.0 {
            Angle(0.0)
        } else if y == 0.0 {
            Angle(if x >= 0.0 { 0.0 } else { -180.0 })
        } else if x == 0.0 {
            Angle(if y >= 0.0 { 90.0 } else { -90.0 })
        } else if x == y {
            Angle(if x >= 0.0 { 45.0 } else { -180.0 + 45.0 })
        } else if x == -y {
            Angle(if x >= 0.0 { -45.0 } else { 180.0 - 45.0 })
        } else {
            Angle::from_radians(y.atan2(x))
        }
    }

    pub fn normalize(self) -> Angle {
        let mut v = self.0;
        while v < -0.0 {
            v += 360.0;
        }
        while v >= 360.0 {
            v -= 360.0;
        }
        Angle(v)
    }

    pub fn normalize720(self) -> Angle {
        let mut v = self.0;
        while v < -360.0 {
            v += 360.0;
        }
        while v >= 360.0 {
            v -= 360.0;
        }
        Angle(v)
    }

    pub fn radians(self) -> f64 {
        self.0 * DEGREES_TO_RADIANS
    }

    pub fn sin(self) -> f64 {
        let t = self.normalize().0;
        if t == 0.0 || t == 180.0 {
            0.0
        } else if t == 45.0 || t == 135.0 {
            std::f64::consts::FRAC_1_SQRT_2
        } else if t == 225.0 || t == 315.0 {
            -std::f64::consts::FRAC_1_SQRT_2
        } else if t == 90.0 {
            1.0
        } else if t == 270.0 {
            -1.0
        } else {
            self.radians().sin()
        }
    }

    pub fn cos(self) -> f64 {
        let t = self.normalize().0;
        if t == 0.0 {
            1.0
        } else if t == 180.0 {
            -1.0
        } else if t == 90.0 || t == 270.0 {
            0.0
        } else if t == 45.0 || t == 315.0 {
            std::f64::consts::FRAC_1_SQRT_2
        } else if t == 135.0 || t == 225.0 {
            -std::f64::consts::FRAC_1_SQRT_2
        } else {
            self.radians().cos()
        }
    }
}

pub type Pt = (i32, i32);

/// `RotatePoint( int* pX, int* pY, const EDA_ANGLE& )` (`trigo.cpp`).
pub fn rotate_origin(x: i32, y: i32, angle: Angle) -> Pt {
    let a = angle.normalize();
    if a.0 == 0.0 {
        (x, y)
    } else if a.0 == 90.0 {
        (y, x.wrapping_neg())
    } else if a.0 == 180.0 {
        (x.wrapping_neg(), y.wrapping_neg())
    } else if a.0 == 270.0 {
        (y.wrapping_neg(), x)
    } else {
        let (s, c) = (a.sin(), a.cos());
        let (xf, yf) = (x as f64, y as f64);
        (ki_round(yf * s + xf * c), ki_round(yf * c - xf * s))
    }
}

/// `RotatePoint( VECTOR2I&, const VECTOR2I& centre, const EDA_ANGLE& )`.
pub fn rotate_about(p: Pt, centre: Pt, angle: Angle) -> Pt {
    let (ox, oy) = (p.0.wrapping_sub(centre.0), p.1.wrapping_sub(centre.1));
    let (rx, ry) = rotate_origin(ox, oy, angle);
    (rx.wrapping_add(centre.0), ry.wrapping_add(centre.1))
}

/// `EDA_SHAPE::CalcArcAngles`, then `GetArcAngle`: end minus start, in degrees.
fn arc_angle(start: Pt, end: Pt, centre: Pt) -> f64 {
    let sa = Angle::from_vector((start.0 - centre.0) as f64, (start.1 - centre.1) as f64);
    let mut ea = Angle::from_vector((end.0 - centre.0) as f64, (end.1 - centre.1) as f64);
    if ea.0 == sa.0 {
        ea = Angle(sa.0 + 360.0); // a ring, not a null arc
    }
    while ea.0 < sa.0 {
        ea = Angle(ea.0 + 360.0);
    }
    ea.0 - sa.0
}

/// The start, mid and end points KiCad 10 keeps for an arc read from the old
/// syntax. `centre` is the old `(start)`, `arc_start` the old `(end)`, `sweep`
/// the `(angle)` in degrees. Points are in nanometres.
pub fn legacy_arc_points(centre: Pt, arc_start: Pt, sweep: f64) -> (Pt, Pt, Pt) {
    // EDA_SHAPE::SetArcAngleAndEnd( angle, true )
    let angle = Angle(sweep);
    let mut start = arc_start;
    let mut end = rotate_about(start, centre, Angle(-angle.normalize720().0));
    if sweep < 0.0 {
        std::mem::swap(&mut start, &mut end);
    }
    // EDA_SHAPE::GetArcMid(), computed variant
    let half = Angle(-arc_angle(start, end, centre) / 2.0);
    let mid = rotate_about(start, centre, half);
    (start, mid, end)
}

fn point_of(node: NodeRef<'_>) -> Option<Pt> {
    Some((
        mm_to_nm(node.atom(1)?.parse().ok()?),
        mm_to_nm(node.atom(2)?.parse().ok()?),
    ))
}

fn convert_arc(tree: &mut Tree<'_>, idx: u32) -> Result<(), String> {
    let node = tree.node(idx);
    let (Some(c), Some(s), Some(a)) = (node.child("start"), node.child("end"), node.child("angle"))
    else {
        return Err("old-format arc without (start), (end) and (angle); left as it is".into());
    };
    let (Some(centre), Some(arc_start), Some(sweep)) = (
        point_of(c),
        point_of(s),
        a.atom(1).and_then(|t| t.parse::<f64>().ok()),
    ) else {
        return Err("old-format arc with a value that is not a number; left as it is".into());
    };
    let (start, mid, end) = legacy_arc_points(centre, arc_start, sweep);
    let mut make = |head: &str, p: Pt| {
        let h = tree.add_atom(head);
        let x = tree.add_atom(&nm_to_mm_text(p.0));
        let y = tree.add_atom(&nm_to_mm_text(p.1));
        tree.add_list(&[h, x, y])
    };
    let (ns, nm, ne) = (make("start", start), make("mid", mid), make("end", end));
    let mut items = Vec::new();
    for child in tree.item_indices(idx) {
        match tree.node(child).head() {
            Some("start") if tree.node(child).is_list() => items.extend([ns, nm, ne]),
            Some("end") | Some("angle") if tree.node(child).is_list() => {}
            _ => items.push(child),
        }
    }
    tree.set_items(idx, &items);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overbar_cases_from_string_utils() {
        // ConvertToNewOverbarNotation, traced by hand through string_utils.cpp:85-148.
        assert_eq!(convert_overbar("~"), "~");
        assert_eq!(convert_overbar("~RESET~"), "~{RESET}");
        assert_eq!(convert_overbar("A~B~C"), "A~{B}C");
        // an unterminated overbar is closed at the end of the string
        assert_eq!(convert_overbar("~OE"), "~{OE}");
        // a space ends the overbar
        assert_eq!(convert_overbar("~OE GND"), "~{OE} GND");
        // "~~" is a literal tilde
        assert_eq!(convert_overbar("A~~B"), "A~B");
        // a '~' followed by '{' means the name is already converted
        assert_eq!(convert_overbar("~{OE}"), "~{OE}");
        // "~~{x": the first '~' appends "~~{}" and steps one character; the second '~' is then
        // followed by '{', so the function bails out and returns the input unchanged.
        assert_eq!(convert_overbar("~~{x"), "~~{x");
        assert_eq!(convert_overbar("plain"), "plain");
        // non-ASCII characters pass through
        assert_eq!(convert_overbar("~Δ~"), "~{Δ}");
    }

    #[test]
    fn arc_90_degrees_counter_example_by_hand() {
        // centre (0,0), old end = start point (10 mm, 0), sweep +90.
        // SetArcAngleAndEnd: end = RotatePoint(start, -90): Normalize -> 270 -> (-y, x) = (0, 10e6).
        // CalcArcAngles: start 0 deg, end 90 deg (y>=0, x==0) -> 90 deg.
        // mid = RotatePoint(start, -45): 315 deg, sin = -1/sqrt(2), cos = 1/sqrt(2):
        //   x = round(0*sin + 10e6*cos) = 7071068, y = round(0*cos - 10e6*sin) = 7071068.
        let (s, m, e) = legacy_arc_points((0, 0), (10_000_000, 0), 90.0);
        assert_eq!(
            (s, m, e),
            ((10_000_000, 0), (7_071_068, 7_071_068), (0, 10_000_000))
        );
    }

    #[test]
    fn arc_negative_sweep_swaps_the_ends() {
        // sweep -90: end = RotatePoint(start, +90) = (y, -x) = (0, -10e6); sweep < 0 swaps
        // start and end: start (0,-10e6), end (10e6, 0).
        // CalcArcAngles: start (0,-1e7) is -90 deg, end is 0 deg -> 90 deg.
        // mid = RotatePoint((0,-1e7), -45) = 315 deg: x = round(y*sin + x*cos)
        //   = round(-1e7 * -0.70710678) = 7071068, y = round(y*cos - x*sin) = -7071068.
        let (s, m, e) = legacy_arc_points((0, 0), (10_000_000, 0), -90.0);
        assert_eq!(
            (s, m, e),
            ((0, -10_000_000), (7_071_068, -7_071_068), (10_000_000, 0))
        );
    }

    #[test]
    fn arc_with_an_off_origin_centre() {
        // centre (5,5) mm, old end (5,10) mm, sweep 180.
        // relative start (0, 5e6); angle -180 -> Normalize 180 -> (-x,-y) = (0,-5e6) -> end (5e6, 0)+c.
        // end = (5e6, 0); start = (5e6, 10e6)... c + (0,5e6) = (5e6,10e6)? No: start is absolute
        // (5e6,10e6) and end = c + (0,-5e6) = (5e6, 0).
        // angles: start rel (0,5e6) = 90, end rel (0,-5e6) = -90 -> end < start -> +360 = 270 -> sweep 180.
        // mid = RotatePoint(start, -90): 270 -> (-y, x) of rel (0,5e6) = (-5e6, 0) -> abs (0, 5e6).
        let (s, m, e) = legacy_arc_points((5_000_000, 5_000_000), (5_000_000, 10_000_000), 180.0);
        assert_eq!(
            (s, m, e),
            ((5_000_000, 10_000_000), (0, 5_000_000), (5_000_000, 0))
        );
    }

    #[test]
    fn arc_with_a_non_cardinal_angle_rounds_like_kicad() {
        // sweep 30 about the origin from (10 mm, 0): end = rotate by -30:
        // 330 deg: sin = sin(330 deg) = -0.5000000000000001 (libm), cos = 0.8660254037844387.
        // x = round(0*s + 1e7*c) = 8660254, y = round(0*c - 1e7*s) = 5000000.
        let (_, _, e) = legacy_arc_points((0, 0), (10_000_000, 0), 30.0);
        assert_eq!(e, (8_660_254, 5_000_000));
    }

    fn converted(src: &str, version: u32) -> (String, Conversions) {
        let mut tree = Tree::parse(src).unwrap();
        let c = normalise(&mut tree, version);
        let mut out = String::new();
        dump(tree.root(), &mut out);
        (out, c)
    }

    fn dump(n: NodeRef<'_>, out: &mut String) {
        match n.kind() {
            NodeKind::List => {
                out.push('(');
                for (i, c) in n.items().enumerate() {
                    if i > 0 {
                        out.push(' ');
                    }
                    dump(c, out);
                }
                out.push(')');
            }
            NodeKind::Atom => out.push_str(n.text()),
            NodeKind::Str => {
                out.push('"');
                out.push_str(n.text());
                out.push('"');
            }
        }
    }

    #[test]
    fn gr_arc_is_rewritten_in_the_tree() {
        let src =
            "(kicad_pcb (gr_arc (start 0 0) (end 10 0) (angle 90) (layer Edge.Cuts) (width 0.1)))";
        let (out, c) = converted(src, 20210925);
        assert_eq!(c.arcs, 1);
        assert_eq!(
            out,
            "((kicad_pcb (gr_arc (start 10 0) (mid 7.071068 7.071068) (end 0 10) (layer Edge.Cuts) (width 0.1))))"
        );
        // 20211014 and later keep the new syntax untouched
        let new = "(kicad_pcb (gr_arc (start 10 0) (mid 1 1) (end 0 10) (layer Edge.Cuts)))";
        let (out, c) = converted(new, 20211014);
        assert_eq!(c.arcs, 0);
        assert!(out.contains("(mid 1 1)"));
    }

    #[test]
    fn fp_arc_with_locked_keeps_its_other_items() {
        let src = "(kicad_pcb (footprint \"X\" (fp_arc locked (start 1 1) (end 1 3) (angle -90) (layer F.SilkS) (tstamp ab))))";
        let (out, c) = converted(src, 20210606);
        assert_eq!(c.arcs, 1);
        // centre (1,1), old end (1,3), sweep -90: end = rotate((0,2e6), +90) = (y,-x) = (2e6, 0)
        // -> abs (3,1); swap: start (3,1), end (1,3); angles: start 0, end 90; mid = start
        // rotated -45 about (1,1): rel (2e6,0) -> (1414214, 1414214) -> abs (2.414214, 2.414214)
        assert!(
            out.contains("(fp_arc locked (start 3 1) (mid 2.414214 2.414214) (end 1 3) (layer F.SilkS) (tstamp ab))"),
            "{out}"
        );
    }

    #[test]
    fn broken_old_arc_is_left_alone_with_a_warning() {
        let (out, c) = converted("(kicad_pcb (gr_arc (start 0 0) (end 10 0)))", 20210925);
        assert_eq!(c.arcs, 0);
        assert_eq!(c.warnings.len(), 1);
        assert!(out.contains("(gr_arc (start 0 0) (end 10 0))"));
    }

    #[test]
    fn overbar_in_net_table_pads_and_net_classes() {
        let src = r#"(kicad_pcb (net 0 "") (net 1 "~RESET~") (net_class Default "" (add_net "~RESET~") (add_net GND))
            (module X (pad 1 smd rect (net 1 "~RESET~") (net 2 "A~~B"))) (segment (net 1)))"#;
        let (out, c) = converted(src, 20201115);
        assert_eq!(c.overbars, 4);
        assert!(out.contains(r#"(net 1 "~{RESET}")"#));
        assert!(out.contains(r#"(add_net "~{RESET}")"#));
        assert!(out.contains(r#"(net 2 "A~B")"#));
        assert!(out.contains("(add_net GND)"));
        // 20210606 and later are not converted
        let (out, c) = converted(src, 20210606);
        assert_eq!(c.overbars, 0);
        assert!(out.contains(r#"(net 1 "~RESET~")"#));
    }

    #[test]
    fn tstamp_is_read_as_uuid_unless_uuid_is_present() {
        let t = Tree::parse(
            "(a (footprint (tstamp 11111111-1111-1111-1111-111111111111)) \
             (footprint (uuid \"22222222\") (tstamp 33333333)) (footprint))",
        )
        .unwrap();
        let a = t.root().child("a").unwrap();
        let ids: Vec<_> = a
            .children("footprint")
            .map(|f| item_uuid(f).map(|u| u.into_owned()))
            .collect();
        assert_eq!(
            ids,
            [
                Some("11111111-1111-1111-1111-111111111111".to_owned()),
                Some("22222222".to_owned()),
                None
            ]
        );
    }
}
