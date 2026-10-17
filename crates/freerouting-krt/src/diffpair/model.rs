// SPDX-License-Identifier: GPL-3.0-only AND MIT
// SPDX-FileCopyrightText: 2026 drandyhaas (KiCadRoutingTools)
// Portions ported from KiCadRoutingTools py_router/net_queries.py
// (extract_diff_pair_base, find_differential_pairs) at 0486e799.

//! The pair model (X08.1): which nets are differential pairs, and each
//! pair's track width and edge-to-edge gap per layer.
//!
//! **Sources**, in this order:
//! 1. **K05.3's net classes.** A KiCad project class other than `Default`
//!    that sets `diff_pair_width` or `diff_pair_gap` itself is a pair
//!    class. Its member nets (a net is a member when its effective class,
//!    or one member of its aggregate class, is the pair class) are paired
//!    by their names with [`name_key`]; width and gap are the class's,
//!    a missing one filled from the project's `Default` class (KiCad's
//!    built-in 0.2 mm and 0.25 mm when that has none either). The
//!    project's classes don't say which net pairs with which, so the names
//!    decide inside a class; a member without a partner is reported.
//! 2. **Names, the fallback** (`router.diff_pairs.infer_from_names`, off
//!    by default): every net no class paired, paired by [`name_key`]. Its
//!    width is the net's own trace width on each layer and its gap the
//!    trace-to-trace clearance of its clearance class there (a DSN has no
//!    pair rule).
//!
//! **Names** ([`name_key`]): KRT's suffix conventions, tried in KRT's
//! order: `Net-(…)` auto-names peeled to their last path segment;
//! `DPLUS`/`DMINUS` and `DP`/`DM`/`DN` (USB); `_P0`/`_N0` (indexed);
//! `_P`/`_N`; `P`/`N` after a digit, `_` or capital; `+`/`-` (not `-+` or
//! `--`); `+_x`/`-_x`; `_t_x`/`_c_x`, `_tX`/`_cX` and `_t`/`_c` (DDR, last).
//! Two nets pair when they give the same base and convention with opposite
//! polarity.
//!
//! **Order and ambiguity** (D2, X00 rule 6): nets are read in net-number
//! order; a key with two nets of the same polarity (a net matching two
//! partners, or subnets of one name) is reported and skipped, where KRT's
//! dictionary would keep the last one silently. Pairs are listed in
//! ascending P-net number. Character classes are ASCII where KRT's Python
//! regular expressions would take any Unicode digit; letters use Rust's
//! Unicode `is_alphabetic` and `is_uppercase` as Python's `isalpha` and
//! `isupper` do.

use std::collections::BTreeMap;

use freerouting::board::BasicBoard;
use freerouting::io::kicad::reader::classes::ClassTable;
use freerouting::io::kicad::reader::project::{ClassDef, Project};

use crate::length::Units;

/// KiCad's built-in Default `diff_pair_width` (0.2 mm), in nm.
pub const KICAD_DEFAULT_DP_WIDTH_NM: i32 = 200_000;
/// KiCad's built-in Default `diff_pair_gap` (0.25 mm), in nm.
pub const KICAD_DEFAULT_DP_GAP_NM: i32 = 250_000;

/// One differential pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pair {
    /// The positive net.
    pub p: i32,
    /// The negative net.
    pub n: i32,
    /// The track width on each board layer, in board units.
    pub width: Vec<i32>,
    /// The edge-to-edge gap on each board layer, in board units.
    pub gap: Vec<i32>,
    /// The pair class's name, or `names` for the fallback.
    pub class: String,
    /// The name base both nets share (`USB_D` of `USB_D+`).
    pub base: String,
}

impl Pair {
    /// The track half width on `layer`.
    pub fn half_width(&self, layer: i32) -> i32 {
        self.width[layer as usize] / 2
    }

    /// The distance between the P and N track centres on `layer`:
    /// `2·hw + g`, with `hw` the inserted half width.
    pub fn centre_distance(&self, layer: i32) -> i32 {
        2 * self.half_width(layer) + self.gap[layer as usize]
    }

    /// The compound half width of the centreline on `layer`: the outer
    /// track edge's distance from it, `hw + (2·hw + g)/2`, which is
    /// `w + g/2` for an even `w`.
    pub fn compound_half_width(&self, layer: i32) -> i32 {
        let d = self.centre_distance(layer);
        self.half_width(layer) + d - d / 2
    }

    /// The half width the centreline is searched at: the compound half
    /// width plus one unit for the offset's rounding
    /// ([`super::offset`]).
    pub fn search_half_width(&self, layer: i32) -> i32 {
        self.compound_half_width(layer) + 1
    }
}

/// The pairs of a board and what the model builder reported.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PairModel {
    /// The pairs in ascending P-net number.
    pub pairs: Vec<Pair>,
    /// Ambiguous keys and class members without a partner, one line each.
    pub notes: Vec<String>,
}

/// The K05.3 classes that make pairs ([`model docs`](self)).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PairClasses {
    project: Option<Project>,
}

impl PairClasses {
    /// No project: no class pairs.
    pub fn none() -> PairClasses {
        PairClasses { project: None }
    }

    /// The pair classes of a KiCad project (K05.3).
    pub fn from_project(project: &Project) -> PairClasses {
        PairClasses {
            project: Some(project.clone()),
        }
    }

    /// The pair classes with their width and gap in nm, in project order.
    pub fn classes(&self) -> Vec<(String, i32, i32)> {
        let Some(project) = &self.project else {
            return Vec::new();
        };
        let dflt = &project.default;
        project
            .classes
            .iter()
            .filter(|c| c.name != "Default" && is_pair_class(c))
            .map(|c| {
                let width = c
                    .diff_pair_width
                    .or(dflt.diff_pair_width)
                    .unwrap_or(KICAD_DEFAULT_DP_WIDTH_NM);
                let gap = c
                    .diff_pair_gap
                    .or(dflt.diff_pair_gap)
                    .unwrap_or(KICAD_DEFAULT_DP_GAP_NM);
                (c.name.clone(), width, gap)
            })
            .collect()
    }
}

fn is_pair_class(c: &ClassDef) -> bool {
    c.diff_pair_width.is_some() || c.diff_pair_gap.is_some()
}

/// Which naming convention a key came from (KRT's `style`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Style {
    /// `DPLUS`/`DMINUS`, `DP`/`DM`/`DN`.
    Usb,
    /// `_P`/`_N` and `_P0`/`_N0`.
    UnderscorePn,
    /// `P`/`N`.
    Pn,
    /// `+`/`-`.
    PlusMinus,
    /// `_t`/`_c`.
    TrueComplement,
}

/// A net name's pair key: the base, the polarity and the convention.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameKey {
    pub base: String,
    pub positive: bool,
    pub style: Style,
}

fn key(base: String, positive: bool, style: Style) -> Option<NameKey> {
    Some(NameKey {
        base,
        positive,
        style,
    })
}

/// KRT `extract_diff_pair_base`: the pair key of `name`, `None` if the
/// name follows no pair convention ([module docs](self)).
pub fn name_key(name: &str) -> Option<NameKey> {
    if name.is_empty() {
        return None;
    }
    let mut name = name.to_string();
    let auto = name.starts_with("Net-(") && name.ends_with(')') && name.len() >= 6;
    if auto {
        name = name[5..name.len() - 1].to_string();
        let path = name.replace("{slash}", "/");
        if let Some(i) = path.rfind('/') {
            name = path[i + 1..].to_string();
        }
    }
    let c: Vec<char> = name.chars().collect();
    let n = c.len();
    let s = |from: usize, to: usize| -> String { c[from..to].iter().collect() };
    let usb_guard = |head: usize| head == 0 || !c[head - 1].is_alphabetic();
    // ^(.*?)D(PLUS|MINUS)$, case-insensitive.
    for (word, positive) in [("PLUS", true), ("MINUS", false)] {
        let len = word.len() + 1;
        if n >= len {
            let tail: String = s(n - len, n).to_uppercase();
            if tail == format!("D{word}") && usb_guard(n - len) {
                return key(s(0, n - len) + "D", positive, Style::Usb);
            }
        }
    }
    // ^(.*?)D([PMN])$, case-insensitive.
    if n >= 2 && c[n - 2].eq_ignore_ascii_case(&'D') && usb_guard(n - 2) {
        match c[n - 1].to_ascii_uppercase() {
            'P' => return key(s(0, n - 2) + "D", true, Style::Usb),
            'M' | 'N' => return key(s(0, n - 2) + "D", false, Style::Usb),
            _ => {}
        }
    }
    // ^(.+)_([PN])(\d+)$
    let digits = c.iter().rev().take_while(|ch| ch.is_ascii_digit()).count();
    if digits > 0 && n >= digits + 3 {
        let pn = c[n - digits - 1];
        if matches!(pn, 'P' | 'N') && c[n - digits - 2] == '_' {
            let base = s(0, n - digits - 2) + "_X" + &s(n - digits, n);
            return key(base, pn == 'P', Style::UnderscorePn);
        }
    }
    if name.ends_with("_P") {
        return key(s(0, n - 2), true, Style::UnderscorePn);
    }
    if name.ends_with("_N") {
        return key(s(0, n - 2), false, Style::UnderscorePn);
    }
    if n > 1 && matches!(c[n - 1], 'P' | 'N') {
        let before = c[n - 2];
        if before.is_ascii_digit() || before == '_' || before.is_uppercase() {
            return key(s(0, n - 1), c[n - 1] == 'P', Style::Pn);
        }
    }
    if name.ends_with('+') && !name.ends_with("-+") {
        return key(s(0, n - 1), true, Style::PlusMinus);
    }
    if name.ends_with('-') && !name.ends_with("--") {
        return key(s(0, n - 1), false, Style::PlusMinus);
    }
    // ^(.+?)([+-])(_[A-Za-z0-9_]*)$ with the head not ending in + or -.
    for i in 1..n.saturating_sub(1) {
        if matches!(c[i], '+' | '-')
            && c[i + 1] == '_'
            && c[i + 2..]
                .iter()
                .all(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        {
            if matches!(c[i - 1], '+' | '-') {
                break;
            }
            return key(s(0, i) + &s(i + 1, n), c[i] == '+', Style::PlusMinus);
        }
    }
    let tc = |ch: char| match ch {
        't' | 'T' => Some(true),
        'c' | 'C' => Some(false),
        _ => None,
    };
    // ^(.+)_([tc])_(.+)$, case-insensitive, the last such infix.
    if n >= 5 {
        for i in (1..=n - 4).rev() {
            if c[i] == '_'
                && c[i + 2] == '_'
                && let Some(positive) = tc(c[i + 1])
            {
                return key(
                    s(0, i) + "_X_" + &s(i + 3, n),
                    positive,
                    Style::TrueComplement,
                );
            }
        }
    }
    // ^(.+)_([tc])([A-Za-z0-9])$
    if n >= 4
        && c[n - 3] == '_'
        && c[n - 1].is_ascii_alphanumeric()
        && let Some(positive) = tc(c[n - 2])
    {
        return key(
            s(0, n - 3) + "_X" + &s(n - 1, n),
            positive,
            Style::TrueComplement,
        );
    }
    // _t / _c
    if n >= 2 && c[n - 2] == '_' {
        if let Some(positive) = tc(c[n - 1]) {
            return key(s(0, n - 2), positive, Style::TrueComplement);
        }
    }
    None
}

/// The nets of `nets` (name, number; in ascending number) paired by name.
/// Returns the `(p, n, base)` triples in ascending P-net number and the
/// ambiguity notes.
pub fn pair_by_names(nets: &[(String, i32)]) -> (Vec<(i32, i32, String)>, Vec<String>) {
    type Halves = (Vec<(i32, String)>, Vec<(i32, String)>);
    let mut keys: BTreeMap<(String, Style), Halves> = BTreeMap::new();
    for (name, net) in nets {
        let Some(k) = name_key(name) else { continue };
        let entry = keys.entry((k.base, k.style)).or_default();
        let half = if k.positive {
            &mut entry.0
        } else {
            &mut entry.1
        };
        half.push((*net, name.clone()));
    }
    let mut pairs = Vec::new();
    let mut notes = Vec::new();
    for ((base, style), (p, n)) in keys {
        match (p.as_slice(), n.as_slice()) {
            ([(p, _)], [(n, _)]) => pairs.push((*p, *n, base)),
            ([], _) | (_, []) => {}
            _ => {
                let list = |h: &[(i32, String)]| {
                    h.iter()
                        .map(|(net, name)| format!("{net} ({name})"))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                notes.push(format!(
                    "diff pairs: ambiguous names for pair {base} ({style:?}): positive {}; \
                     negative {}; skipped",
                    list(&p),
                    list(&n)
                ));
            }
        }
    }
    pairs.sort_by_key(|(p, _, _)| *p);
    (pairs, notes)
}

/// The board's nets with a name, in ascending net number.
pub fn board_nets(board: &BasicBoard) -> Vec<(String, i32)> {
    (1..=board.rules.nets.max_net_number())
        .filter_map(|n| {
            let net = board.rules.nets.get(n)?;
            (!net.name.is_empty()).then(|| (net.name.clone(), n))
        })
        .collect()
}

/// Builds the pair model of `board` ([module docs](self)).
pub fn build_pairs(board: &BasicBoard, classes: &PairClasses, infer_from_names: bool) -> PairModel {
    let nets = board_nets(board);
    let units = Units::of(board);
    let layers = board.layer_count();
    let mut model = PairModel::default();
    let mut used: Vec<i32> = Vec::new();
    if let Some(project) = &classes.project {
        let table = ClassTable::new(project);
        for (class, width_nm, gap_nm) in classes.classes() {
            let members: Vec<(String, i32)> = nets
                .iter()
                .filter(|(name, _)| table.effective(name).name.split(',').any(|m| m == class))
                .cloned()
                .collect();
            let (pairs, notes) = pair_by_names(&members);
            model.notes.extend(notes);
            let to_units = |nm: i32| units.from_nm(i64::from(nm)).round() as i32;
            for (p, n, base) in pairs {
                used.extend([p, n]);
                model.pairs.push(Pair {
                    p,
                    n,
                    width: vec![to_units(width_nm); layers],
                    gap: vec![to_units(gap_nm); layers],
                    class: class.clone(),
                    base,
                });
            }
            for (name, net) in &members {
                if !used.contains(net) {
                    model.notes.push(format!(
                        "diff pairs: net {net} ({name}) is in pair class {class} but has no \
                         partner by name; not paired"
                    ));
                }
            }
        }
    }
    if infer_from_names {
        let rest: Vec<(String, i32)> = nets
            .iter()
            .filter(|(_, net)| !used.contains(net))
            .cloned()
            .collect();
        let (pairs, notes) = pair_by_names(&rest);
        model.notes.extend(notes);
        for (p, n, base) in pairs {
            model.pairs.push(fallback_pair(board, p, n, base));
        }
    }
    model.pairs.sort_by_key(|pair| pair.p);
    model
}

/// A name-inferred pair: P's trace width and its class's trace-to-trace
/// clearance on each layer.
fn fallback_pair(board: &BasicBoard, p: i32, n: i32, base: String) -> Pair {
    let rules = &board.rules;
    let cl = rules
        .nets
        .get(p)
        .map(|net| {
            rules
                .net_classes
                .get(net.net_class())
                .trace_clearance_class()
        })
        .unwrap_or(1);
    let layers = board.layer_count() as i32;
    Pair {
        p,
        n,
        width: (0..layers)
            .map(|l| 2 * rules.trace_half_width(p, l))
            .collect(),
        gap: (0..layers)
            .map(|l| rules.clearance_matrix.get_value(cl, cl, l, false))
            .collect(),
        class: "names".to_string(),
        base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(name: &str) -> Option<(String, bool, Style)> {
        name_key(name).map(|k| (k.base, k.positive, k.style))
    }

    fn some(base: &str, positive: bool, style: Style) -> Option<(String, bool, Style)> {
        Some((base.to_string(), positive, style))
    }

    #[test]
    fn each_naming_convention_gives_its_key() {
        use Style::*;
        assert_eq!(k("LVDS_P"), some("LVDS", true, UnderscorePn));
        assert_eq!(k("LVDS_N"), some("LVDS", false, UnderscorePn));
        assert_eq!(k("CLK_P0"), some("CLK_X0", true, UnderscorePn));
        assert_eq!(k("CLK_N12"), some("CLK_X12", false, UnderscorePn));
        assert_eq!(k("TXP"), some("TX", true, Pn));
        assert_eq!(k("REFCLKN"), some("REFCLK", false, Pn));
        assert_eq!(k("RX0P"), some("RX0", true, Pn));
        assert_eq!(k("WAKEn"), None);
        assert_eq!(k("/CLK+"), some("/CLK", true, PlusMinus));
        assert_eq!(k("/CLK-"), some("/CLK", false, PlusMinus));
        assert_eq!(k("BZ--"), None);
        assert_eq!(k("/D+_L"), some("/D_L", true, PlusMinus));
        assert_eq!(k("/D-_L"), some("/D_L", false, PlusMinus));
        assert_eq!(k("3V3-MCU"), None);
        assert_eq!(k("USB_DP"), some("USB_D", true, Usb));
        assert_eq!(k("USB_DM"), some("USB_D", false, Usb));
        assert_eq!(k("USB_DN"), some("USB_D", false, Usb));
        assert_eq!(k("usb_dplus"), some("usb_D", true, Usb));
        assert_eq!(k("USB_DMINUS"), some("USB_D", false, Usb));
        assert_eq!(
            k("LCDP"),
            some("LCD", true, Pn),
            "the USB guard rejects, P/N takes it"
        );
        assert_eq!(k("CK_t"), some("CK", true, TrueComplement));
        assert_eq!(k("CK_C"), some("CK", false, TrueComplement));
        assert_eq!(k("DQS0_t_A"), some("DQS0_X_A", true, TrueComplement));
        assert_eq!(k("DQS0_c_A"), some("DQS0_X_A", false, TrueComplement));
        assert_eq!(k("DQS0_TA"), some("DQS0_XA", true, TrueComplement));
        assert_eq!(
            k("TARGET_C_SENSE+"),
            some("TARGET_C_SENSE", true, PlusMinus)
        );
        assert_eq!(k("Net-(U12-USB_D+)"), some("U12-USB_D", true, PlusMinus));
        assert_eq!(
            k("Net-(U12-GPIO19/U1RTS/USB_D-)"),
            some("USB_D", false, PlusMinus)
        );
        assert_eq!(k("Net-(R1-Pad1)"), None);
        assert_eq!(k("GND"), None);
        assert_eq!(k(""), None);
    }

    #[test]
    fn names_pair_in_net_number_order_and_ambiguity_is_reported() {
        let nets: Vec<(String, i32)> = [
            ("USB_D-", 2),
            ("GND", 3),
            ("USB_D+", 5),
            ("CLK_P", 6),
            ("CLK_N", 7),
            ("CLK+", 8),
            ("A_P", 9),
            ("A_P", 10),
            ("A_N", 11),
        ]
        .iter()
        .map(|(n, i)| (n.to_string(), *i))
        .collect();
        let (pairs, notes) = pair_by_names(&nets);
        assert_eq!(
            pairs,
            vec![(5, 2, "USB_D".to_string()), (6, 7, "CLK".to_string())]
        );
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(
            notes[0].contains("ambiguous names for pair A"),
            "{}",
            notes[0]
        );
        assert!(notes[0].contains("9 (A_P), 10 (A_P)"), "{}", notes[0]);
    }
}
