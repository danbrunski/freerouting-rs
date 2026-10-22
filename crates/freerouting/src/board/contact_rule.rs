//! N04: `router.pad_contacts`, where a trace end connects to a pin.
//!
//! Java's `Trace.getNormalContacts` and `DrillItem.getNormalContacts` connect
//! a trace to a pin only when the trace's end corner is exactly the pin's
//! centre. KiCad connects a track to a pad wherever the track's end lies on
//! the pad, so a board laid out in KiCad with track ends off the pad centre
//! (KiCad's ecc83-pp demo: off the centre on 14 pins) is fully connected in
//! KiCad and has open connections in Java, which the router then "routes"
//! pass after pass without closing them. The setting chooses:
//!
//! - `center` (the default, also when unset): Java's rule; nothing changes,
//!   so routes are byte-identical to Java's;
//! - `inside_pad`: a trace end also connects to a pin of a common net when
//!   its cap, the disc of half the trace's width around the end point,
//!   overlaps the pin's pad shape on the trace's layer (borders included),
//!   by the board's own shape intersection in integer units with no added
//!   tolerance. So an end inside the pad connects, and so does an end just
//!   outside it whose round end lies on the pad, as KiCad connects a track
//!   whose copper meets the pad. A deliberate divergence
//!   (`docs/DIVERGENCES.md`, "Board model").
//!
//! The rule changes connectivity only, never copper: the trace keeps its
//! geometry, and the SES writer writes its end back as it is (its endpoint
//! snap, Java's `SesWriter.snappedEndpoint`, is off under `inside_pad`, see
//! [`snapped_endpoint`](crate::io::specctra::snapped_endpoint)). Every walker
//! of connectivity reads it through [`BasicBoard::normal_contacts`],
//! [`BasicBoard::trace_normal_contacts`] and
//! [`BasicBoard::normal_contact_point`]: the incompletes and the routing
//! ledger, the batch optimizer's connected sets, pull-tight's end contacts,
//! trace tails, the DRC's dangling-trace check and the rip-up walk.
//!
//! **T junctions (N04.4).** Under `inside_pad` a trace end also connects to
//! a trace of a common net on the same layer when its cap overlaps that
//! trace's copper (the shapes of its segments, the same exact test): a
//! track ending on another track's segment or inner corner, as KiCad
//! connects it. Java has no such contact (its router splits a trace where
//! another ends on it, so all its trace contacts are end to end). The
//! contact is symmetric: the ending trace has the other in the contacts of
//! that end ([`BasicBoard::trace_normal_contacts`]), and the other has it in
//! its [`BasicBoard::normal_contacts`] but at none of its own ends. So:
//! - the incompletes, the ledger, trace tails and the DRC's dangling traces
//!   see the junction;
//! - a walk along a connection (`connection_items`, the rip-up walk) that
//!   reaches a trace in its middle stops there, as at Java's fork of three
//!   traces ([`BasicBoard::is_trace_body_contact`]);
//! - the end-corner smoothing and `swap_connection_to_pin` don't treat the
//!   other trace as ending there;
//! - pull-tight splits a trace that others end on at those points first
//!   when they all lie on its centre line ([`BasicBoard::t_junctions`],
//!   `split_trace_at_point`), so the junctions become Java's end-to-end
//!   contacts, which pull-tight keeps. It leaves the trace as it is when
//!   one of them meets its copper off the centre line, where no split joins
//!   the ends. The split happens only there, so loaded copper keeps its
//!   traces (and the SES its wires) until pull-tight would move one.
//!
//! **Vias keep Java's rule.** A trace end inside a via's pad but off its
//! centre, and a via centre inside a pin's pad, connect in KiCad as well;
//! N04.3's graded run checks KiCad's via connectivity first.
//!
//! The job sets the rule from the settings ([`apply`], in
//! `job::prepare`), after the board is read: the DSN and SES readers use
//! Java's rule. A [`BoardSnapshot`](crate::board::BoardSnapshot) keeps it.

use std::collections::BTreeSet;

use freerouting_geometry::{Circle, IntBox, Point, PolylineShapeOps, TileShape, TileShapeOps};

use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId, ItemKind, PolylineTrace};
use crate::job::{LogLevel, LogLine};
use crate::settings::RouterSettings;

/// The T junctions on a trace ([`BasicBoard::t_junctions`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TJunctions {
    /// Every one lies on the trace's centre line, at these points.
    Split(Vec<Point>),
    /// One meets the trace's copper off its centre line.
    Keep,
}

/// The settings key, as the CLI and the JSON file name it.
pub const SETTINGS_KEY: &str = "router.pad_contacts";

/// Where a trace end connects to a pin.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ContactRule {
    /// Java's rule: only at the pin's centre.
    #[default]
    Center,
    /// KiCad's rule: anywhere inside the pin's pad on the trace's layer.
    InsidePad,
}

impl ContactRule {
    /// The rule of a setting value; unset is [`ContactRule::Center`]. Fails
    /// on any other value, naming it.
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.map(str::trim) {
            None | Some("") | Some("center") => Ok(ContactRule::Center),
            Some("inside_pad") => Ok(ContactRule::InsidePad),
            Some(other) => Err(format!(
                "{SETTINGS_KEY}: unknown value '{other}' (expected 'center' or 'inside_pad')"
            )),
        }
    }

    /// The settings value (`center`, `inside_pad`).
    pub fn name(self) -> &'static str {
        match self {
            ContactRule::Center => "center",
            ContactRule::InsidePad => "inside_pad",
        }
    }
}

/// Sets the board's contact rule as `settings` ask (the C03 apply step).
/// Returns the log line, if any.
pub fn apply(board: &mut BasicBoard, settings: &RouterSettings) -> Result<Option<LogLine>, String> {
    let rule = ContactRule::parse(settings.pad_contacts.as_deref())?;
    board.set_contact_rule(rule);
    Ok((rule == ContactRule::InsidePad).then(|| LogLine {
        level: LogLevel::Info,
        text: format!(
            "{SETTINGS_KEY} = inside_pad: a trace end inside a same-net pin's pad connects to \
             the pin (KiCad's rule)."
        ),
    }))
}

impl BasicBoard {
    /// Where a trace end connects to a pin ([`ContactRule`]).
    pub fn contact_rule(&self) -> ContactRule {
        self.contact_rule
    }

    /// Sets the contact rule; the routing ledger is rebuilt on its next
    /// read, because the counts depend on it.
    pub fn set_contact_rule(&mut self, rule: ContactRule) {
        if self.contact_rule != rule {
            self.contact_rule = rule;
            self.ledger.invalidate();
            self.pour_contacts.forget_all();
        }
    }

    /// True if, under [`ContactRule::InsidePad`], the cap of a trace end at
    /// `end` on `layer` (the disc of radius `half_width` around it) overlaps
    /// the pad of pin `pin` on that layer, by the board's own shape
    /// intersection (`Shape::intersects_circle`, borders included). An end
    /// at a rational point tests the point alone (a disc needs an integer
    /// centre). Always false under Java's rule, for vias and for other
    /// items.
    pub(crate) fn pad_touches_trace_end(
        &self,
        pin: &Item,
        end: &Point,
        layer: i32,
        half_width: i32,
    ) -> bool {
        if self.contact_rule != ContactRule::InsidePad {
            return false;
        }
        let ItemKind::Pin(p) = &pin.kind else {
            return false;
        };
        let Some(shape) = p.drill.shape_on_layer(layer) else {
            return false;
        };
        match end {
            Point::Int(centre) => shape.intersects_circle(&Circle::new(*centre, half_width)),
            _ => shape.contains(end),
        }
    }

    /// Under [`ContactRule::InsidePad`], adds to `result` the pins (of a
    /// common net unless `ignore_net`) whose pad the cap of `trace_item`'s
    /// end `end` overlaps. Java's contacts at the centre are found by the
    /// caller as well.
    pub(crate) fn add_trace_end_pad_contacts(
        &self,
        trace_item: &Item,
        end: &Point,
        ignore_net: bool,
        result: &mut BTreeSet<ItemId>,
    ) {
        if self.contact_rule != ContactRule::InsidePad {
            return;
        }
        let Some(trace) = trace_item.as_trace() else {
            return;
        };
        let region = TileShape::get_instance_point(end).offset(f64::from(trace.half_width));
        for other_id in self.overlapping_objects(&TileShape::IntBox(region), trace.layer) {
            let other = self.item_ref(other_id);
            if other_id == trace_item.id()
                || !other.shares_layer(trace_item)
                || !(ignore_net || other.shares_net(trace_item))
            {
                continue;
            }
            if self.pad_touches_trace_end(other, end, trace.layer, trace.half_width) {
                result.insert(other_id);
            }
        }
    }

    /// Under [`ContactRule::InsidePad`], adds to `result` the traces of a
    /// common net with an end whose cap overlaps the pad of pin `pin` on
    /// their layer. The traces with an end at the centre are Java's
    /// contacts and found by the caller as well.
    pub(crate) fn add_pad_trace_contacts(&self, pin: &Item, result: &mut BTreeSet<ItemId>) {
        if self.contact_rule != ContactRule::InsidePad || !pin.is_pin() {
            return;
        }
        let drill = pin.drill().expect("a pin");
        let reach = f64::from(self.max_trace_half_width());
        for (index, shape) in drill.shapes.iter().enumerate() {
            let Some(shape) = shape else {
                continue;
            };
            let layer = drill.first_layer + index as i32;
            let region: IntBox = shape.bounding_box().offset(reach);
            for other_id in self.overlapping_objects(&TileShape::IntBox(region), layer) {
                let other = self.item_ref(other_id);
                let Some(trace) = other.as_trace() else {
                    continue;
                };
                if trace.layer != layer || !other.shares_net(pin) {
                    continue;
                }
                let touches =
                    |end: &Point| self.pad_touches_trace_end(pin, end, layer, trace.half_width);
                if touches(&trace.first_corner()) || touches(&trace.last_corner()) {
                    result.insert(other_id);
                }
            }
        }
    }

    /// True if, under [`ContactRule::InsidePad`], the cap of a trace end at
    /// `end` (the disc of radius `half_width` around it) overlaps the copper
    /// of trace `other`, one of the shapes of its segments
    /// (`Polyline::offset_shape` at its half width), by the board's own
    /// shape intersection (`TileShape::intersects_circle`, borders
    /// included). An end at a rational point tests the point alone. False
    /// when `end` is an end corner of `other` (Java's contact, which the
    /// callers find), under Java's rule and for items that are no traces.
    /// The caller checks the layer and the nets.
    pub(crate) fn trace_end_touches_trace(
        &self,
        end: &Point,
        half_width: i32,
        other: &Item,
    ) -> bool {
        if self.contact_rule != ContactRule::InsidePad {
            return false;
        }
        let Some(trace) = other.as_trace() else {
            return false;
        };
        if *end == trace.first_corner() || *end == trace.last_corner() {
            return false;
        }
        (0..trace.tile_shape_count())
            .filter_map(|i| trace.polyline.offset_shape(trace.half_width, i))
            .any(|shape| end_cap_meets(&shape, end, half_width))
    }

    /// Under [`ContactRule::InsidePad`], adds to `result` the traces of a
    /// common net on `trace_item`'s layer whose copper the cap of its end
    /// `end` overlaps ([`Self::trace_end_touches_trace`]): a T junction,
    /// where the end lies on another trace's segment or inner corner, or
    /// near enough that the copper meets. The traces ending at `end` are
    /// Java's contacts and found by the caller.
    pub(crate) fn add_trace_end_trace_contacts(
        &self,
        trace_item: &Item,
        end: &Point,
        result: &mut BTreeSet<ItemId>,
    ) {
        if self.contact_rule != ContactRule::InsidePad {
            return;
        }
        let Some(trace) = trace_item.as_trace() else {
            return;
        };
        let region = TileShape::get_instance_point(end).offset(f64::from(trace.half_width));
        for other_id in self.overlapping_objects(&TileShape::IntBox(region), trace.layer) {
            let other = self.item_ref(other_id);
            if other_id == trace_item.id()
                || other.as_trace().is_none_or(|t| t.layer != trace.layer)
                || !other.shares_net(trace_item)
            {
                continue;
            }
            if self.trace_end_touches_trace(end, trace.half_width, other) {
                result.insert(other_id);
            }
        }
    }

    /// Under [`ContactRule::InsidePad`], adds to `result` the traces of a
    /// common net on `trace_item`'s layer with an end whose cap overlaps
    /// `trace_item`'s copper: the other side of
    /// [`Self::add_trace_end_trace_contacts`], so that a T junction is in
    /// both traces' [`normal_contacts`](Self::normal_contacts). These are
    /// no contacts at an end of `trace_item`
    /// ([`Self::trace_normal_contacts`] leaves them out).
    pub(crate) fn add_trace_body_contacts(&self, trace_item: &Item, result: &mut BTreeSet<ItemId>) {
        if self.contact_rule != ContactRule::InsidePad {
            return;
        }
        let Some(trace) = trace_item.as_trace() else {
            return;
        };
        let reach = f64::from(self.max_trace_half_width());
        for index in 0..trace.tile_shape_count() {
            let Some(shape) = trace.polyline.offset_shape(trace.half_width, index) else {
                continue;
            };
            let region = TileShape::IntBox(shape.bounding_box().offset(reach));
            for other_id in self.overlapping_objects(&region, trace.layer) {
                if other_id == trace_item.id() || result.contains(&other_id) {
                    continue;
                }
                let other = self.item_ref(other_id);
                let Some(other_trace) = other.as_trace() else {
                    continue;
                };
                if other_trace.layer != trace.layer || !other.shares_net(trace_item) {
                    continue;
                }
                let touches = |end: &Point| {
                    *end != trace.first_corner()
                        && *end != trace.last_corner()
                        && end_cap_meets(&shape, end, other_trace.half_width)
                };
                if touches(&other_trace.first_corner()) || touches(&other_trace.last_corner()) {
                    result.insert(other_id);
                }
            }
        }
    }

    /// Under [`ContactRule::InsidePad`], the T junctions on trace `id`: the
    /// ends of other traces whose cap overlaps its copper
    /// ([`Self::add_trace_body_contacts`]). `None` without any;
    /// [`TJunctions::Split`] with their points (in contact order) when each
    /// lies on the trace's centre line, so that splitting the trace there
    /// makes them Java's end-to-end contacts; [`TJunctions::Keep`] when one
    /// does not (the copper meets off the centre line, where no split
    /// joins the ends).
    pub(crate) fn t_junctions(&self, id: ItemId) -> Option<TJunctions> {
        if self.contact_rule != ContactRule::InsidePad {
            return None;
        }
        let item = self.item(id)?;
        let trace = item.as_trace()?;
        let mut body = BTreeSet::new();
        self.add_trace_body_contacts(item, &mut body);
        let mut points: Vec<Point> = Vec::new();
        for other_id in body {
            let other = self.item_ref(other_id).as_trace().expect("a trace contact");
            for end in [other.first_corner(), other.last_corner()] {
                if !self.trace_end_touches_trace(&end, other.half_width, item) {
                    continue;
                }
                if !matches!(end, Point::Int(_)) || !trace.polyline.contains(&end) {
                    return Some(TJunctions::Keep);
                }
                if !points.contains(&end) {
                    points.push(end);
                }
            }
        }
        (!points.is_empty()).then_some(TJunctions::Split(points))
    }

    /// True if, under [`ContactRule::InsidePad`], `point` lies on trace
    /// `item` but is none of its end corners: a walk along the contacts
    /// that reaches the trace there meets it at a T junction, which ends
    /// the connection as a fork does (Java has the trace split at that
    /// point, so three traces meet there).
    pub fn is_trace_body_contact(&self, item: &Item, point: &Point) -> bool {
        self.contact_rule == ContactRule::InsidePad
            && item
                .as_trace()
                .is_some_and(|t| *point != t.first_corner() && *point != t.last_corner())
    }

    /// Java `Item.normalContactPoint` under the board's contact rule: where
    /// `a` and `b` meet, if they are in normal contact. Under
    /// [`ContactRule::InsidePad`] a trace and a pin that meet at the pin's
    /// pad meet at the trace's end there (its first corner if both ends
    /// touch the pad), and two traces that meet at a T junction meet at the
    /// end whose cap overlaps the other trace, if exactly one end of the
    /// two does (else there is no unique point, as in Java for two traces
    /// that share both ends).
    pub fn normal_contact_point(&self, a: &Item, b: &Item) -> Option<Point> {
        let java = a.normal_contact_point(b);
        if java.is_some() || self.contact_rule != ContactRule::InsidePad {
            return java;
        }
        if let (ItemKind::Trace(ta), ItemKind::Trace(tb)) = (&a.kind, &b.kind) {
            return self.trace_trace_contact_point(a, ta, b, tb);
        }
        let (trace_item, pin) = match (&a.kind, &b.kind) {
            (ItemKind::Trace(_), ItemKind::Pin(_)) => (a, b),
            (ItemKind::Pin(_), ItemKind::Trace(_)) => (b, a),
            _ => return None,
        };
        let trace = trace_item.as_trace().expect("a trace");
        [trace.first_corner(), trace.last_corner()]
            .into_iter()
            .find(|end| self.pad_touches_trace_end(pin, end, trace.layer, trace.half_width))
    }

    /// The T junction point of traces `a` and `b` (N04.4): the one end of
    /// either whose cap overlaps the other's copper, the same for both
    /// argument orders; `None` without one or with several.
    fn trace_trace_contact_point(
        &self,
        a: &Item,
        ta: &PolylineTrace,
        b: &Item,
        tb: &PolylineTrace,
    ) -> Option<Point> {
        if ta.layer != tb.layer || !a.shares_net(b) {
            return None;
        }
        let mut points: Vec<Point> = Vec::new();
        for (trace, other) in [(ta, b), (tb, a)] {
            for end in [trace.first_corner(), trace.last_corner()] {
                if !points.contains(&end)
                    && self.trace_end_touches_trace(&end, trace.half_width, other)
                {
                    points.push(end);
                }
            }
        }
        match points.as_slice() {
            [point] => Some(point.clone()),
            _ => None,
        }
    }
}

/// True if the cap of a trace end at `end` (radius `half_width`) overlaps
/// `shape`, borders included; an end at a rational point tests the point
/// alone (a disc needs an integer centre).
fn end_cap_meets(shape: &TileShape, end: &Point, half_width: i32) -> bool {
    match end {
        Point::Int(centre) => shape.intersects_circle(&Circle::new(*centre, half_width)),
        _ => shape.contains(end),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_the_two_values_and_unset() {
        assert_eq!(ContactRule::parse(None), Ok(ContactRule::Center));
        assert_eq!(ContactRule::parse(Some("")), Ok(ContactRule::Center));
        assert_eq!(
            ContactRule::parse(Some(" center ")),
            Ok(ContactRule::Center)
        );
        assert_eq!(
            ContactRule::parse(Some("inside_pad")),
            Ok(ContactRule::InsidePad)
        );
        let error = ContactRule::parse(Some("overlap")).unwrap_err();
        assert!(error.contains(SETTINGS_KEY) && error.contains("'overlap'"));
        for rule in [ContactRule::Center, ContactRule::InsidePad] {
            assert_eq!(ContactRule::parse(Some(rule.name())), Ok(rule));
        }
    }
}
