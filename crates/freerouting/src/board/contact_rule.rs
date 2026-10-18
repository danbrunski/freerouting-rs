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
//! **Vias keep Java's rule.** A trace end inside a via's pad but off its
//! centre, and a via centre inside a pin's pad, connect in KiCad as well;
//! N04.3's graded run checks KiCad's via connectivity first. A trace end on
//! another trace's segment or inner corner (a T junction) is not a contact
//! either (N04.4).
//!
//! The job sets the rule from the settings ([`apply`], in
//! `job::prepare`), after the board is read: the DSN and SES readers use
//! Java's rule. A [`BoardSnapshot`](crate::board::BoardSnapshot) keeps it.

use std::collections::BTreeSet;

use freerouting_geometry::{Circle, IntBox, Point, TileShape};

use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId, ItemKind};
use crate::job::{LogLevel, LogLine};
use crate::settings::RouterSettings;

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

    /// Java `Item.normalContactPoint` under the board's contact rule: where
    /// `a` and `b` meet, if they are in normal contact. Under
    /// [`ContactRule::InsidePad`] a trace and a pin that meet at the pin's
    /// pad meet at the trace's end there (its first corner if both ends
    /// touch the pad).
    pub fn normal_contact_point(&self, a: &Item, b: &Item) -> Option<Point> {
        let java = a.normal_contact_point(b);
        if java.is_some() || self.contact_rule != ContactRule::InsidePad {
            return java;
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
