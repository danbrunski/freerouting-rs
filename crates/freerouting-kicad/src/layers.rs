//! Copper layers and the stackup (K05.1).
//!
//! `SPECCTRA_DB::buildLayerMaps` (`specctra_export.cpp` at tag `10.0.0`)
//! lists the copper layers top to bottom: `LSET::AllCuMask( count ).CuStack()`
//! is F.Cu, In1.Cu ... InN.Cu, B.Cu. The DSN layer name is the board's layer
//! name, which is the user name when the `(layers ...)` entry gives one
//! (`(4 "In1.Cu" power "Gnd.Cu")` is `Gnd.Cu`). KiCad 9 and later number the
//! layers F.Cu 0, B.Cu 2, In1.Cu 4 ... and KiCad 6 to 8 number them 0, 1, 2 ...;
//! the order here comes from the names only.

use freerouting_dsn::ast::{Layer, LayerType};

use crate::error::Warning;
use crate::sexpr::NodeRef;
use crate::units;

/// A copper layer of the board.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopperLayer {
    /// The standard name: `F.Cu`, `In1.Cu`, `B.Cu`.
    pub std_name: String,
    /// The name KiCad uses for the layer: the user name when there is one.
    pub name: String,
    /// The type word of the entry: `signal`, `power`, `mixed`, `jumper`.
    pub kind: String,
}

impl CopperLayer {
    /// The DSN layer type. `mixed` becomes `signal` (Freerouting does not
    /// support `mixed`); an unknown word is a signal layer, as in
    /// `FromBOARD`'s `default:` case.
    pub fn layer_type(&self) -> LayerType {
        match self.kind.as_str() {
            "power" => LayerType::Power,
            "jumper" => LayerType::Jumper,
            _ => LayerType::Signal,
        }
    }
}

/// The copper layers, top to bottom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layers {
    pub copper: Vec<CopperLayer>,
}

/// A set of copper layers as KiCad's `LSET` holds them, indexed by stack
/// position: bit 0 is F.Cu, bit k is Ik.Cu, bit 31 is B.Cu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CuMask(pub u32);

impl CuMask {
    pub const ALL: CuMask = CuMask(u32::MAX);
    pub const NONE: CuMask = CuMask(0);

    /// `LSET::AllCuMask( count )`: F.Cu, B.Cu and the first `count - 2` inner layers.
    pub fn all(count: usize) -> CuMask {
        let mut m = 1u32 | (1 << 31);
        for k in 1..count.saturating_sub(1) {
            m |= 1 << k;
        }
        CuMask(m)
    }

    /// Physical position `p` (0 top, `count - 1` bottom) as a mask.
    pub fn at(p: usize, count: usize) -> CuMask {
        if p + 1 == count {
            CuMask(1 << 31)
        } else {
            CuMask(1 << p)
        }
    }

    pub fn any(self) -> bool {
        self.0 != 0
    }

    pub fn and(self, o: CuMask) -> CuMask {
        CuMask(self.0 & o.0)
    }

    pub fn or(self, o: CuMask) -> CuMask {
        CuMask(self.0 | o.0)
    }

    /// `FlipLayer`: F.Cu and B.Cu swap, In<k>.Cu becomes In<count-1-k>.Cu.
    pub fn flipped(self, count: usize) -> CuMask {
        let mut out = 0u32;
        for p in 0..count {
            let m = CuMask::at(p, count).0;
            if self.0 & m != 0 {
                out |= CuMask::at(count - 1 - p, count).0;
            }
        }
        // layers beyond the board's own (inner layers it does not have) stay
        out |= self.0 & !CuMask::all(count).0;
        CuMask(out)
    }
}

/// Parses a layer token from a `(layers ...)` list of an item (`"F.Cu"`,
/// `"*.Cu"`, `"F&B.Cu"`, `"*.Mask"`) into the copper layers it names.
pub fn copper_of_layer_token(token: &str) -> CuMask {
    match token {
        "*.Cu" => CuMask::ALL,
        "F&B.Cu" => CuMask(1 | (1 << 31)),
        "F.Cu" => CuMask(1),
        "B.Cu" => CuMask(1 << 31),
        t => {
            if let Some(n) = t
                .strip_prefix("In")
                .and_then(|r| r.strip_suffix(".Cu"))
                .and_then(|r| r.parse::<u32>().ok())
                .filter(|n| (1..=30).contains(n))
            {
                CuMask(1 << n)
            } else if t == "*.In.Cu" || t == "*In.Cu" {
                CuMask(!(1 | (1 << 31)))
            } else {
                CuMask::NONE
            }
        }
    }
}

impl Layers {
    /// Reads the `(layers ...)` list of a `kicad_pcb`.
    pub fn read(board: NodeRef<'_>, warnings: &mut Vec<Warning>) -> Layers {
        let mut found: Vec<(u32, CopperLayer)> = Vec::new();
        if let Some(list) = board.child("layers") {
            for entry in list.items().skip(1).filter(|e| e.is_list()) {
                let Some(std_name) = entry.str_value(1) else {
                    continue;
                };
                if !std_name.ends_with(".Cu") {
                    continue;
                }
                let kind = entry.atom(2).unwrap_or("signal").to_owned();
                let name = entry
                    .str_value(3)
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| std_name.clone());
                let order = match std_name.as_ref() {
                    "F.Cu" => 0,
                    "B.Cu" => u32::MAX,
                    s => s
                        .strip_prefix("In")
                        .and_then(|r| r.strip_suffix(".Cu"))
                        .and_then(|r| r.parse::<u32>().ok())
                        .unwrap_or(u32::MAX - 1),
                };
                found.push((
                    order,
                    CopperLayer {
                        std_name: std_name.into_owned(),
                        name: name.into_owned(),
                        kind,
                    },
                ));
            }
        }
        if found.is_empty() {
            warnings.push(Warning {
                line: board.line(),
                message: "the board lists no copper layers; assuming F.Cu and B.Cu".to_owned(),
            });
            for (o, n) in [(0, "F.Cu"), (u32::MAX, "B.Cu")] {
                found.push((
                    o,
                    CopperLayer {
                        std_name: n.to_owned(),
                        name: n.to_owned(),
                        kind: "signal".to_owned(),
                    },
                ));
            }
        }
        found.sort_by_key(|(o, _)| *o);
        Layers {
            copper: found.into_iter().map(|(_, l)| l).collect(),
        }
    }

    pub fn count(&self) -> usize {
        self.copper.len()
    }

    /// The `structure` layers of the DSN. The `(property (index n))` of
    /// KiCad's export has no field in the tree and nothing reads it.
    pub fn dsn_layers(&self) -> Vec<Layer> {
        self.copper
            .iter()
            .map(|l| Layer {
                name: l.name.clone(),
                layer_type: Some(l.layer_type()),
                rules: Vec::new(),
                use_net: Vec::new(),
            })
            .collect()
    }

    /// DSN name of physical layer `p`.
    pub fn name(&self, p: usize) -> &str {
        &self.copper[p].name
    }
}

// ---- stackup ----------------------------------------------------------------

/// One layer of the board stackup (`(setup (stackup (layer ...)))`).
#[derive(Clone, Debug, PartialEq)]
pub struct StackupLayer {
    pub name: String,
    /// `copper`, `core`, `prepreg`, `Top Solder Mask`, ...
    pub kind: String,
    /// Thickness in nanometres; the sum over the sheets of a multi-sheet
    /// dielectric.
    pub thickness_nm: Option<i64>,
    pub material: Option<String>,
    /// Relative permittivity; for a multi-sheet dielectric the thickness
    /// weighted mean of its sheets.
    pub epsilon_r: Option<f64>,
    pub loss_tangent: Option<f64>,
    /// Number of sheets (1 for an ordinary layer).
    pub sheets: u32,
}

/// Reads the stackup of `board`, top to bottom, as the file lists it.
pub fn read_stackup(board: NodeRef<'_>) -> Vec<StackupLayer> {
    let Some(stackup) = board.child("setup").and_then(|s| s.child("stackup")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for layer in stackup.children("layer") {
        let one = |n: NodeRef<'_>| -> (Option<i64>, Option<String>, Option<f64>, Option<f64>) {
            (
                n.child("thickness")
                    .and_then(|t| t.atom(1))
                    .and_then(units::nm),
                n.child("material")
                    .and_then(|m| m.str_value(1))
                    .map(|m| m.into_owned()),
                n.child("epsilon_r").and_then(|e| e.f64(1)),
                n.child("loss_tangent").and_then(|e| e.f64(1)),
            )
        };
        let (mut thick, material, mut eps, mut loss) = one(layer);
        let mut sheets = 1u32;
        let mut weighted_eps = eps.map(|e| e * thick.unwrap_or(0) as f64);
        let mut weighted_loss = loss.map(|e| e * thick.unwrap_or(0) as f64);
        for sub in layer.children("addsublayer") {
            let (t, _, e, l) = one(sub);
            sheets += 1;
            let t_nm = t.unwrap_or(0);
            thick = Some(thick.unwrap_or(0) + t_nm);
            if let Some(e) = e {
                weighted_eps = Some(weighted_eps.unwrap_or(0.0) + e * t_nm as f64);
            }
            if let Some(l) = l {
                weighted_loss = Some(weighted_loss.unwrap_or(0.0) + l * t_nm as f64);
            }
        }
        if sheets > 1 {
            let total = thick.unwrap_or(0) as f64;
            if total > 0.0 {
                eps = weighted_eps.map(|w| w / total);
                loss = weighted_loss.map(|w| w / total);
            }
        }
        out.push(StackupLayer {
            name: layer
                .str_value(1)
                .map(|n| n.into_owned())
                .unwrap_or_default(),
            kind: layer
                .child("type")
                .and_then(|t| t.str_value(1))
                .map(|t| t.into_owned())
                .unwrap_or_default(),
            thickness_nm: thick,
            material,
            epsilon_r: eps,
            loss_tangent: loss,
            sheets,
        });
    }
    out
}
