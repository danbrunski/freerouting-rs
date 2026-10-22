//! The read report (K06.7): what the reader decided, beside the warnings.
//!
//! Per rule area its name, layers, flags and the kinds emitted; counts of
//! the arcs turned into polylines, the footprint copper zones dropped and
//! the custom pads exported as their convex hull (as KiCad's DSN export does).
//! The CLI and the GUI print it (K07).

use std::fmt;

use crate::rule_areas::RuleAreaReport;

/// What [`crate::read`] found and did, beside its warnings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadReport {
    /// Every rule area, board areas first, then those of footprints in
    /// footprint order.
    pub rule_areas: Vec<RuleAreaReport>,
    /// Copper zones turned into planes (one per zone and copper layer).
    pub planes: usize,
    /// Wires and vias made from tracks and vias with a net.
    pub wires: usize,
    pub vias: usize,
    /// Track arcs followed as polylines (KiCad's export chords them).
    pub track_arcs_flattened: usize,
    /// Arcs inside zone outlines turned into polylines.
    pub zone_arcs_flattened: usize,
    /// Copper zones owned by footprints, dropped like KiCad's export does
    /// ([`crate::ReadOptions::drop_footprint_zones`]): `(footprint reference,
    /// zone name)`.
    pub footprint_zones_dropped: Vec<(String, Option<String>)>,
    /// Copper zones owned by footprints, kept as planes (the default).
    pub footprint_zones_kept: Vec<(String, Option<String>)>,
    /// Custom pads built as the convex hull of their parts.
    pub custom_pad_hulls: usize,
    /// Planes cut where higher-priority zones of other nets fill (K15.15), in
    /// plane order.
    pub plane_cuts: Vec<PlaneCut>,
}

/// One plane cut by higher-priority zones ([`crate::zone_priority`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlaneCut {
    /// The zone's name, else its net.
    pub zone: String,
    pub layer: String,
    /// The higher zones of other nets that cut a window (names, else nets), in
    /// zone order.
    pub by: Vec<String>,
    /// The windows cut.
    pub windows: usize,
}

impl fmt::Display for ReadReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "rule areas: {}", self.rule_areas.len())?;
        for a in &self.rule_areas {
            let name = a
                .name
                .as_deref()
                .filter(|n| !n.is_empty())
                .unwrap_or("(unnamed)");
            write!(f, "  {name} (line {})", a.line)?;
            if let Some(o) = &a.owner {
                write!(f, " of footprint {o}")?;
            }
            writeln!(f, " on {}", a.layers.join(", "))?;
            writeln!(f, "    flags: {}", a.flags.describe())?;
            if a.emitted.is_empty() {
                writeln!(f, "    emitted: nothing")?;
            } else {
                writeln!(f, "    emitted: {}", a.emitted.join(", "))?;
            }
        }
        writeln!(f, "planes: {}", self.planes)?;
        for c in &self.plane_cuts {
            writeln!(
                f,
                "  {} on {}: {} window(s) where higher-priority zones fill ({})",
                c.zone,
                c.layer,
                c.windows,
                c.by.join(", ")
            )?;
        }
        writeln!(f, "wires: {}, vias: {}", self.wires, self.vias)?;
        writeln!(
            f,
            "arcs followed: {} track, {} zone",
            self.track_arcs_flattened, self.zone_arcs_flattened
        )?;
        writeln!(
            f,
            "footprint copper zones: {} kept as planes, {} dropped",
            self.footprint_zones_kept.len(),
            self.footprint_zones_dropped.len()
        )?;
        writeln!(f, "custom pads as convex hull: {}", self.custom_pad_hulls)
    }
}
