// SPDX-License-Identifier: GPL-3.0-only AND MIT
// SPDX-FileCopyrightText: 2026 drandyhaas (KiCadRoutingTools)
// Portions ported from KiCadRoutingTools py_router/impedance.py
// (get_layer_impedance_params, get_layer_epsilon_eff, get_layer_ps_per_mm,
// get_via_barrel_epsilon_eff, calculate_route_propagation_time_ps) at
// 0486e799.

//! Propagation delay (X07.5): picoseconds per millimetre per layer from the
//! stackup's relative permittivity (K05.1).
//!
//! - A copper layer's εr is that of its dielectric: below the top layer,
//!   above the bottom layer, and the mean of the two neighbours for an
//!   inner layer (KRT `get_layer_impedance_params`; a dielectric without
//!   `epsilon_r` counts as 4.0, KRT's FR4 default there).
//! - `εr_eff = (εr + 1) / 2` on the outer layers (microstrip, width
//!   unknown) and `εr` on the inner layers (stripline); delay
//!   `sqrt(εr_eff) / c`, `c = 299.792458 mm/ns` (KRT `get_layer_epsilon_eff`,
//!   `get_layer_ps_per_mm`, without the width-dependent Hammerstad form).
//! - A via barrel ([`super::measure`]) is delayed at the thickness-weighted
//!   εr of the dielectrics it crosses (KRT `get_via_barrel_epsilon_eff`).
//! - A net's delay is the sum over its traces of length times the layer's
//!   delay, plus its barrels (KRT `calculate_route_propagation_time_ps`).
//!   Pad-to-die lengths add no delay: KiCad adds a pad's `die_delay` in the
//!   time domain and never converts a `die_length`, and the KiCad reader
//!   (K05.1) reads no `die_delay`.
//!
//! Time targets need a stackup; without one the stage refuses them.

use freerouting::board::BasicBoard;

use super::measure::{LengthModel, Stackup, Units, net_vias};

/// The speed of light in vacuum, mm per ns (KRT `SPEED_OF_LIGHT_MM_PER_NS`).
pub const SPEED_OF_LIGHT_MM_PER_NS: f64 = 299.792458;

/// The εr KRT assumes for a dielectric without one.
pub const DEFAULT_EPSILON_R: f64 = 4.0;

const DIELECTRICS: [&str; 2] = ["core", "prepreg"];

fn is_dielectric(kind: &str) -> bool {
    DIELECTRICS.contains(&kind)
}

/// Picoseconds per millimetre at effective permittivity `eps_eff`.
pub fn ps_per_mm(eps_eff: f64) -> f64 {
    1000.0 / (SPEED_OF_LIGHT_MM_PER_NS / eps_eff.sqrt())
}

/// The delays of a board's layers.
#[derive(Clone, Debug, PartialEq)]
pub struct Timing {
    /// Picoseconds per millimetre of each copper layer, top to bottom.
    pub layer_ps_per_mm: Vec<f64>,
    units: Units,
    stackup: Stackup,
}

impl Timing {
    /// The timing of `board` with `model`'s stackup, `None` without one
    /// that fits the board.
    pub fn new(board: &BasicBoard, model: &LengthModel) -> Option<Timing> {
        let stackup = model.stackup_for(board)?.clone();
        let layer_ps_per_mm = (0..stackup.copper_count())
            .map(|i| ps_per_mm(layer_eps_eff(&stackup, i)))
            .collect();
        Some(Timing {
            layer_ps_per_mm,
            units: Units::of(board),
            stackup,
        })
    }

    /// Picoseconds per board unit on board layer `layer`.
    pub fn ps_per_unit(&self, layer: i32) -> f64 {
        let i = (layer.max(0) as usize).min(self.layer_ps_per_mm.len() - 1);
        self.layer_ps_per_mm[i] / self.units.from_um(1000.0)
    }

    /// The delay of a via barrel between copper layers `a` and `b`, in ps.
    pub fn via_ps(&self, a: usize, b: usize) -> f64 {
        let Some(nm) = self.stackup.barrel_nm(a, b) else {
            return 0.0;
        };
        let mm = nm as f64 / 1e6;
        mm * ps_per_mm(barrel_eps(&self.stackup, a, b))
    }

    /// The delay of `net` in picoseconds (module docs).
    pub fn net_delay_ps(&self, board: &BasicBoard, net: i32) -> f64 {
        let mut total = 0.0;
        for item in board.items().filter(|i| i.contains_net(net)) {
            if let Some(trace) = item.as_trace() {
                total += trace.length() * self.ps_per_unit(trace.layer);
            }
        }
        for via in net_vias(board, net) {
            total += self.via_ps(via.first_layer() as usize, via.last_layer() as usize);
        }
        total
    }
}

/// εr_eff of copper layer `copper` (module docs).
pub fn layer_eps_eff(stackup: &Stackup, copper: usize) -> f64 {
    let index = stackup.copper[copper];
    let layers = &stackup.layers;
    let er = |l: &freerouting::io::kicad::reader::layers::StackupLayer| {
        l.epsilon_r
            .filter(|e| *e > 0.0)
            .unwrap_or(DEFAULT_EPSILON_R)
    };
    let above = layers[..index]
        .iter()
        .rev()
        .find(|l| is_dielectric(&l.kind))
        .map_or(DEFAULT_EPSILON_R, er);
    let below = layers[index + 1..]
        .iter()
        .find(|l| is_dielectric(&l.kind))
        .map_or(DEFAULT_EPSILON_R, er);
    let last = stackup.copper_count() - 1;
    if copper == 0 {
        (below + 1.0) / 2.0
    } else if copper == last {
        (above + 1.0) / 2.0
    } else {
        (above + below) / 2.0
    }
}

/// The thickness-weighted εr of the dielectrics between copper layers `a`
/// and `b` (KRT `get_via_barrel_epsilon_eff`).
pub fn barrel_eps(stackup: &Stackup, a: usize, b: usize) -> f64 {
    let (a, b) = (a.min(b), a.max(b));
    let (Some(&from), Some(&to)) = (stackup.copper.get(a), stackup.copper.get(b)) else {
        return DEFAULT_EPSILON_R;
    };
    let mut thickness = 0.0;
    let mut weighted = 0.0;
    for l in &stackup.layers[from..=to] {
        if is_dielectric(&l.kind) {
            let t = l.thickness_nm.unwrap_or(0) as f64;
            let er = l
                .epsilon_r
                .filter(|e| *e > 0.0)
                .unwrap_or(DEFAULT_EPSILON_R);
            weighted += er * t;
            thickness += t;
        }
    }
    if thickness <= 0.0 {
        DEFAULT_EPSILON_R
    } else {
        weighted / thickness
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freerouting::io::kicad::reader::layers::StackupLayer;

    fn layer(name: &str, kind: &str, um: i64, er: Option<f64>) -> StackupLayer {
        StackupLayer {
            name: name.into(),
            kind: kind.into(),
            thickness_nm: Some(um * 1000),
            material: None,
            epsilon_r: er,
            loss_tangent: None,
            sheets: 1,
        }
    }

    fn four_layer() -> Stackup {
        Stackup::new(vec![
            layer("F.Mask", "Top Solder Mask", 10, Some(3.3)),
            layer("F.Cu", "copper", 35, None),
            layer("dielectric 1", "prepreg", 200, Some(4.1)),
            layer("In1.Cu", "copper", 35, None),
            layer("dielectric 2", "core", 1000, Some(4.5)),
            layer("In2.Cu", "copper", 35, None),
            layer("dielectric 3", "prepreg", 200, Some(4.1)),
            layer("B.Cu", "copper", 35, None),
            layer("B.Mask", "Bottom Solder Mask", 10, Some(3.3)),
        ])
        .unwrap()
    }

    #[test]
    fn outer_layers_are_microstrip_and_inner_layers_stripline() {
        let s = four_layer();
        assert_eq!(layer_eps_eff(&s, 0), (4.1 + 1.0) / 2.0);
        assert_eq!(layer_eps_eff(&s, 1), (4.1 + 4.5) / 2.0);
        assert_eq!(layer_eps_eff(&s, 3), (4.1 + 1.0) / 2.0);
        // FR4 microstrip, KRT's fallback number: (4.3 + 1) / 2 = 2.65.
        assert!(
            (ps_per_mm(2.65) - 5.430_06).abs() < 1e-4,
            "{}",
            ps_per_mm(2.65)
        );
        // Barrel εr: thickness weighted over the dielectrics crossed.
        let want = (4.1 * 200.0 + 4.5 * 1000.0 + 4.1 * 200.0) / 1400.0;
        assert!((barrel_eps(&s, 0, 3) - want).abs() < 1e-12);
        assert_eq!(barrel_eps(&s, 0, 1), 4.1);
        assert_eq!(s.barrel_nm(0, 3), Some((35 * 4 + 1400) * 1000));
        assert_eq!(s.barrel_nm(1, 0), Some((35 * 2 + 200) * 1000));
    }
}
