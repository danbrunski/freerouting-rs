//! The feasibility report: one entry per candidate pin, a text line per
//! infeasible pin, and JSON with schema `freerouting-feasibility/1`
//! (Q03.4).

use freerouting_geometry::{FloatPoint, IntBox};

use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId};
use crate::board::structure::Unit;
use crate::drc::json::JsonValue;

use super::neck::NeckLadder;
use super::raster::NO_LABEL;
use super::widest_path::{Field, PathOutcome};
use super::{FeasibilityOptions, PinContext, Verdict, component_and_pin, verdict_of};

/// The JSON schema name.
pub const SCHEMA: &str = "freerouting-feasibility/1";

/// How far around the throat (in cells) the second `near` item is sought.
const NEAR_SEARCH_CELLS: i64 = 4;

/// An item next to the throat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NearItem {
    pub id: ItemId,
    /// The item's type name (`pin`, `polylinetrace`, `obstaclearea`, ...).
    pub kind: &'static str,
    /// The name of the item's first net.
    pub net: Option<String>,
    /// For a pin: the component and pin name.
    pub component: Option<String>,
    pub pin: Option<String>,
}

impl NearItem {
    fn new(board: &BasicBoard, id: ItemId) -> Option<NearItem> {
        let item = board.item(id)?;
        let net = item
            .net_numbers()
            .first()
            .and_then(|n| board.rules.nets.get(*n))
            .map(|n| n.name.clone());
        let (component, pin) = match component_and_pin(board, item) {
            Some((c, p)) if item.is_pin() => (Some(c), Some(p)),
            _ => (None, None),
        };
        Some(NearItem {
            id,
            kind: item.type_name(),
            net,
            component,
            pin,
        })
    }

    /// `pin U1-23 (net GND)`, `polylinetrace #812 (net +5V)`, `boardoutline #1`.
    pub fn describe(&self) -> String {
        let mut text = match (&self.component, &self.pin) {
            (Some(c), Some(p)) => format!("{} {c}-{p}", self.kind),
            _ => format!("{} #{}", self.kind, self.id.0),
        };
        if let Some(net) = &self.net {
            text.push_str(&format!(" (net {net})"));
        }
        text
    }
}

/// Where the corridor is narrowest.
#[derive(Clone, Debug, PartialEq)]
pub struct Throat {
    pub layer: i32,
    pub layer_name: String,
    /// The throat cell centre in board units.
    pub x: i32,
    pub y: i32,
    /// The same point in mm, in DSN coordinates.
    pub x_mm: f64,
    pub y_mm: f64,
}

/// The result for one pin.
#[derive(Clone, Debug, PartialEq)]
pub struct PinFeasibility {
    pub id: ItemId,
    pub net_number: i32,
    pub net: String,
    pub component: String,
    pub pin: String,
    /// The pin's routing layers (names).
    pub layers: Vec<String>,
    /// The class width on each routing layer of the window, board units.
    pub required: Vec<(String, i64)>,
    /// The width `W` the verdict compares with (board units): the
    /// requirement at the throat, else the class width on the pin's first
    /// routing layer.
    pub width: i64,
    /// The widest corridor `C` in board units; `None` when the pin is
    /// walled in, `i64::MAX` when it already touches its net.
    pub corridor: Option<i64>,
    /// `margin(throat)` in board units.
    pub margin: Option<i64>,
    pub verdict: Verdict,
    pub throat: Option<Throat>,
    pub near: Vec<NearItem>,
    /// The raster step in board units.
    pub step: i32,
    /// The window's cell centres span, board units.
    pub window: IntBox,
    /// The neck ladder, for infeasible pins.
    pub neck: Option<NeckLadder>,
}

/// The report of [`super::check_board`].
#[derive(Clone, Debug, PartialEq)]
pub struct FeasibilityReport {
    pub options: FeasibilityOptions,
    /// The raster step in board units.
    pub step: i32,
    pub units_per_mm: f64,
    /// The number of candidate pins.
    pub candidates: usize,
    /// One entry per checked pin, by ascending item id.
    pub pins: Vec<PinFeasibility>,
}

fn to_mm(board: &BasicBoard, x: i32, y: i32) -> (f64, f64) {
    let comm = &board.communication;
    let [dx, dy] = comm.coordinate_transform.board_to_dsn_point(FloatPoint {
        x: f64::from(x),
        y: f64::from(y),
    });
    (
        Unit::scale(dx, comm.unit, Unit::Mm),
        Unit::scale(dy, comm.unit, Unit::Mm),
    )
}

/// The `near` items of a throat: its own label, and the nearest different
/// label among the cells around it whose nearest source lies more than 90°
/// from the first's direction.
fn near_items(
    board: &BasicBoard,
    ctx: &PinContext<'_>,
    field: &Field,
    throat: usize,
) -> Vec<NearItem> {
    let frame = &ctx.window.frame;
    let first = field.label[throat];
    if first == NO_LABEL {
        return Vec::new();
    }
    let mut result: Vec<NearItem> = NearItem::new(board, ItemId(first)).into_iter().collect();
    let (slot, ti, tj) = frame.coords(throat);
    let at = |cell: usize| {
        let (_, i, j) = frame.coords(cell);
        let p = frame.centre(i, j);
        (i64::from(p.x), i64::from(p.y))
    };
    let (tx, ty) = at(throat);
    let source_a = at(field.nearest[throat] as usize);
    let dir_a = (source_a.0 - tx, source_a.1 - ty);
    for radius in 1..=NEAR_SEARCH_CELLS {
        let mut best: Option<(i64, i32)> = None;
        for dj in -radius..=radius {
            for di in -radius..=radius {
                if di.abs().max(dj.abs()) != radius {
                    continue;
                }
                let (i, j) = (ti as i64 + di, tj as i64 + dj);
                if i < 0 || j < 0 || i >= frame.nx as i64 || j >= frame.ny as i64 {
                    continue;
                }
                let cell = frame.index(slot, i as usize, j as usize);
                let label = field.label[cell];
                if label == NO_LABEL || label == first {
                    continue;
                }
                let source = at(field.nearest[cell] as usize);
                let v = (source.0 - tx, source.1 - ty);
                let opposite = dir_a == (0, 0) || dir_a.0 * v.0 + dir_a.1 * v.1 < 0;
                if !opposite {
                    continue;
                }
                let key = (v.0 * v.0 + v.1 * v.1, label);
                if best.is_none_or(|b| key < b) {
                    best = Some(key);
                }
            }
        }
        if let Some((_, label)) = best {
            result.extend(NearItem::new(board, ItemId(label)));
            break;
        }
    }
    result
}

/// Builds the report entry of the pin of `ctx`.
pub(super) fn pin_report(
    board: &BasicBoard,
    ctx: &PinContext<'_>,
    field: &Field,
    outcome: &PathOutcome,
    neck: Option<NeckLadder>,
) -> PinFeasibility {
    let item: &Item = ctx.item;
    let frame = &ctx.window.frame;
    let layer_name = |l: i32| board.layer_structure.layers[l as usize].name.clone();
    let (component, pin) = component_and_pin(board, item).unwrap_or_default();
    let net = board
        .rules
        .nets
        .get(ctx.net)
        .map(|n| n.name.clone())
        .unwrap_or_default();
    let pin_layers: Vec<i32> = frame
        .layers
        .iter()
        .copied()
        .filter(|l| item.drill().and_then(|d| d.shape_on_layer(*l)).is_some())
        .collect();
    let first_slot = pin_layers
        .first()
        .and_then(|l| frame.slot_of(*l))
        .unwrap_or(0);
    let (verdict, corridor) = verdict_of(outcome, ctx.delta);
    let (width, margin, throat, near) = match *outcome {
        PathOutcome::Throat {
            cell,
            margin,
            required,
            ..
        } => {
            let (slot, i, j) = frame.coords(cell);
            let p = frame.centre(i, j);
            let (x_mm, y_mm) = to_mm(board, p.x, p.y);
            let layer = frame.layers[slot];
            let throat = Throat {
                layer,
                layer_name: layer_name(layer),
                x: p.x,
                y: p.y,
                x_mm,
                y_mm,
            };
            (
                required,
                Some(margin),
                Some(throat),
                near_items(board, ctx, field, cell),
            )
        }
        _ => (ctx.width[first_slot], None, None, Vec::new()),
    };
    PinFeasibility {
        id: item.id(),
        net_number: ctx.net,
        net,
        component,
        pin,
        layers: pin_layers.iter().map(|l| layer_name(*l)).collect(),
        required: frame
            .layers
            .iter()
            .zip(&ctx.width)
            .map(|(l, w)| (layer_name(*l), *w))
            .collect(),
        width,
        corridor,
        margin,
        verdict,
        throat,
        near,
        step: frame.step,
        window: frame.bounds(),
        neck,
    }
}

fn mm(value: f64) -> String {
    format!("{value:.3}")
}

impl PinFeasibility {
    /// The text line of an infeasible pin (`None` for other verdicts).
    pub fn text_line(&self, units_per_mm: f64) -> Option<String> {
        if self.verdict != Verdict::Infeasible {
            return None;
        }
        let w = mm(self.width as f64 / units_per_mm);
        let head = format!(
            "cannot route {} to {}-{} at width {w} mm",
            self.net, self.component, self.pin
        );
        let (Some(c), Some(t)) = (self.corridor, &self.throat) else {
            return Some(format!(
                "{head}: walled in, no corridor reaches the net's copper or the window border"
            ));
        };
        let between = match self.near.as_slice() {
            [] => "nothing".to_string(),
            [a] => a.describe(),
            [a, b, ..] => format!("{} and {}", a.describe(), b.describe()),
        };
        let mut line = format!(
            "{head}: widest corridor {} mm at ({}, {}, {}) between {between}",
            mm(c as f64 / units_per_mm),
            t.layer_name,
            mm(t.x_mm),
            mm(t.y_mm)
        );
        if let Some(neck) = &self.neck {
            match neck.needed_mm {
                Some(r) => line.push_str(&format!("; neck needed {} mm", mm(r))),
                None => line.push_str("; no neck up to 4 mm helps"),
            }
        }
        Some(line)
    }

    fn to_json(&self, units_per_mm: f64) -> JsonValue {
        let num = |v: f64| JsonValue::Number(format!("{v:.4}"));
        let opt_int = |v: Option<i64>| match v {
            Some(i64::MAX) => JsonValue::string("connected"),
            Some(v) => JsonValue::int(v),
            None => JsonValue::string("enclosed"),
        };
        let mut members = vec![
            ("id".into(), JsonValue::int(i64::from(self.id.0))),
            ("net".into(), JsonValue::string(&self.net)),
            (
                "net_number".into(),
                JsonValue::int(i64::from(self.net_number)),
            ),
            ("component".into(), JsonValue::string(&self.component)),
            ("pin".into(), JsonValue::string(&self.pin)),
            (
                "layers".into(),
                JsonValue::Array(self.layers.iter().map(JsonValue::string).collect()),
            ),
            (
                "required_width".into(),
                JsonValue::Object(
                    self.required
                        .iter()
                        .map(|(l, w)| (l.clone(), JsonValue::int(*w)))
                        .collect(),
                ),
            ),
            ("width".into(), JsonValue::int(self.width)),
            ("width_mm".into(), num(self.width as f64 / units_per_mm)),
            ("corridor".into(), opt_int(self.corridor)),
        ];
        if let Some(c) = self.corridor.filter(|c| *c != i64::MAX) {
            members.push(("corridor_mm".into(), num(c as f64 / units_per_mm)));
        }
        if let Some(m) = self.margin {
            members.push(("margin".into(), JsonValue::int(m)));
        }
        members.push(("verdict".into(), JsonValue::string(self.verdict.name())));
        if let Some(t) = &self.throat {
            members.push((
                "throat".into(),
                JsonValue::Object(vec![
                    ("layer".into(), JsonValue::string(&t.layer_name)),
                    ("x".into(), JsonValue::int(i64::from(t.x))),
                    ("y".into(), JsonValue::int(i64::from(t.y))),
                    ("x_mm".into(), num(t.x_mm)),
                    ("y_mm".into(), num(t.y_mm)),
                ]),
            ));
        }
        members.push((
            "near".into(),
            JsonValue::Array(self.near.iter().map(near_json).collect()),
        ));
        members.push(("step".into(), JsonValue::int(i64::from(self.step))));
        members.push((
            "window".into(),
            JsonValue::Array(
                [
                    self.window.ll.x,
                    self.window.ll.y,
                    self.window.ur.x,
                    self.window.ur.y,
                ]
                .iter()
                .map(|v| JsonValue::int(i64::from(*v)))
                .collect(),
            ),
        ));
        if let Some(neck) = &self.neck {
            members.push(("neck".into(), neck_json(neck, units_per_mm)));
        }
        JsonValue::Object(members)
    }
}

fn near_json(near: &NearItem) -> JsonValue {
    let mut members = vec![
        ("id".into(), JsonValue::int(i64::from(near.id.0))),
        ("kind".into(), JsonValue::string(near.kind)),
    ];
    for (name, value) in [
        ("net", &near.net),
        ("component", &near.component),
        ("pin", &near.pin),
    ] {
        if let Some(v) = value {
            members.push((name.into(), JsonValue::string(v)));
        }
    }
    JsonValue::Object(members)
}

fn neck_json(neck: &NeckLadder, units_per_mm: f64) -> JsonValue {
    let num = |v: f64| JsonValue::Number(format!("{v:.4}"));
    let rungs = neck
        .rungs
        .iter()
        .map(|r| {
            let mut m = vec![
                ("radius_mm".into(), num(r.radius_mm)),
                ("verdict".into(), JsonValue::string(r.verdict.name())),
            ];
            match r.corridor {
                Some(i64::MAX) => m.push(("corridor".into(), JsonValue::string("connected"))),
                Some(c) => m.push(("corridor_mm".into(), num(c as f64 / units_per_mm))),
                None => m.push(("corridor".into(), JsonValue::string("enclosed"))),
            }
            JsonValue::Object(m)
        })
        .collect();
    JsonValue::Object(vec![
        (
            "neck_width".into(),
            JsonValue::Array(neck.width.iter().map(|w| JsonValue::int(*w)).collect()),
        ),
        (
            "neck_needed_mm".into(),
            match neck.needed_mm {
                Some(r) => num(r),
                None => JsonValue::string("none"),
            },
        ),
        ("rungs".into(), JsonValue::Array(rungs)),
    ])
}

impl FeasibilityReport {
    /// The pins with verdict `verdict`.
    pub fn with_verdict(&self, verdict: Verdict) -> impl Iterator<Item = &PinFeasibility> {
        self.pins.iter().filter(move |p| p.verdict == verdict)
    }

    /// One line per infeasible pin, then a summary line.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for pin in &self.pins {
            if let Some(line) = pin.text_line(self.units_per_mm) {
                out.push_str(&line);
                out.push('\n');
            }
        }
        out.push_str(&format!(
            "{} candidate pins: {} infeasible, {} marginal, {} passable\n",
            self.candidates,
            self.with_verdict(Verdict::Infeasible).count(),
            self.with_verdict(Verdict::Marginal).count(),
            self.with_verdict(Verdict::Passable).count(),
        ));
        out
    }

    /// The JSON report; `board_sha256` is the hex digest of the board file.
    pub fn to_json(&self, board_sha256: &str) -> JsonValue {
        let options = JsonValue::Object(vec![
            (
                "step_um".into(),
                JsonValue::Number(format!("{:.3}", self.options.step_um)),
            ),
            ("start".into(), JsonValue::string(self.options.start.name())),
            ("all_pins".into(), JsonValue::Bool(self.options.all_pins)),
            (
                "neck_ladder".into(),
                JsonValue::Bool(self.options.neck_ladder),
            ),
        ]);
        JsonValue::Object(vec![
            ("schema".into(), JsonValue::string(SCHEMA)),
            ("board_sha256".into(), JsonValue::string(board_sha256)),
            ("options".into(), options),
            ("step".into(), JsonValue::int(i64::from(self.step))),
            (
                "units_per_mm".into(),
                JsonValue::Number(format!("{:.3}", self.units_per_mm)),
            ),
            ("candidates".into(), JsonValue::int(self.candidates as i64)),
            (
                "infeasible".into(),
                JsonValue::int(self.with_verdict(Verdict::Infeasible).count() as i64),
            ),
            (
                "pins".into(),
                JsonValue::Array(
                    self.pins
                        .iter()
                        .map(|p| p.to_json(self.units_per_mm))
                        .collect(),
                ),
            ),
        ])
    }
}

/// The SHA-256 digest of `bytes` as lower-case hex (FIPS 180-4).
pub fn sha256_hex(bytes: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = bytes.to_vec();
    let bit_len = (bytes.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in message.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (t, word) in chunk.chunks_exact(4).enumerate() {
            w[t] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for t in 16..64 {
            let s0 = w[t - 15].rotate_right(7) ^ w[t - 15].rotate_right(18) ^ (w[t - 15] >> 3);
            let s1 = w[t - 2].rotate_right(17) ^ w[t - 2].rotate_right(19) ^ (w[t - 2] >> 10);
            w[t] = w[t - 16]
                .wrapping_add(s0)
                .wrapping_add(w[t - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for t in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[t])
                .wrapping_add(w[t]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for (a, b) in h.iter_mut().zip(v) {
            *a = a.wrapping_add(b);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let long = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(
            sha256_hex(long),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }
}
