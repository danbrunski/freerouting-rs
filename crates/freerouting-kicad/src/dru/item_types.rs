//! `.kicad_dru` clearances between item types (Q22.5).
//!
//! KiCad's DSN export writes one clearance per net class and drops the
//! `.kicad_dru`, so a rule such as `(condition "A.isPlated() && B.Type ==
//! 'track'")` never reached the router. This module maps the item terms of a
//! condition (`A.Type`, `B.Type == A.Type`, `A.isPlated()`, `A.Pad_Type`)
//! onto the DSN's typed clearances (`(clearance v (type pin_wire))` and so
//! on).
//!
//! # Item types
//!
//! A condition is evaluated for every pair of [`ItemType`]s and every pair of
//! net classes, in both orders. Each item type has one DSN clearance kind
//! ([`Kind`]):
//!
//! | KiCad item | `Type` | `isPlated()` | `Pad_Type` | DSN kind |
//! |---|---|---|---|---|
//! | track or arc | `track` (`arc`) | no | — | `wire` |
//! | via | `via` | yes | — | `via` |
//! | through-hole pad | `pad` | yes | `Through-hole` | `pin` |
//! | SMD pad | `pad` | no | `SMD` | `smd` |
//! | edge-connector pad | `pad` | no | `Edge connector` | `smd` |
//! | NPTH pad | `pad` | no | `NPTH, mechanical` | none (KiCad exports a keepout) |
//!
//! Zones, text and graphics have no kind: KiCad refills zones around the
//! routed copper, and copper text and graphics reach the router as keepouts
//! (Q22.4). A pad-to-pad pair is left out too, because routing never moves a
//! pad.
//!
//! # What is applied
//!
//! A rule whose `min` is larger than the DSN's clearance for one of its pairs
//! is applied to **every net-class pair** of the kind pairs it matches, on
//! every layer. When that is wider than its condition (a net-class term, a
//! `Pad_Type == 'SMD'` that the DSN's `smd` kind cannot tell from an edge
//! connector, a layer clause), the rule is applied anyway and a warning names
//! it ([`super::DruReport::warnings`]). A rule that asks for no more than the
//! DSN gives anywhere is reported and left out: KiCad lets a custom rule lower
//! a class clearance, and the router keeping the larger value is safe. Several
//! rules on the same kind pair keep the largest value.
//!
//! The values reach the board through the DSN reader's clearance classes
//! (Java `Structure.setClearanceRule`, `Network.addClearanceRule` and
//! `Network.addMixedClearanceRule`), whose order matters:
//!
//! - **the default class** (`kicad_default`): typed rules in the
//!   `structure`. They create the `via`, `pin`, `smd` and `area` classes
//!   before the network is read, so the default class's routed vias, input
//!   vias and pins take them. A `wire_wire` value goes in a `class_class`
//!   of the default class with itself, because the class's own `(clearance
//!   v)` would overwrite a structure value;
//! - **every other class** `X`: typed rules after its own clearance. They
//!   create `X-via`, `X-pin`, `X-smd` and `X-area` before `X`'s vias are
//!   made, and the `wire` kind is `X`'s own class (the DSN reader's class 1
//!   is the default class's wires);
//! - **pairs of classes**: typed `class_class` rules, one for every kind
//!   pair, at the DSN's value where the rules ask no more. One that names
//!   the default class makes `default-pin` and the like for it, copied from
//!   class 1, so the default class's block is written again, with the
//!   structure's own typed values (KiCad's `smd_smd`) kept. A routed via of
//!   the default class keeps the structure's `via` class, and every other
//!   class copies its row from class 1, whose `via` entry is the via-to-track
//!   value, and so is every `default-*` class a pair makes; so with several
//!   classes the other vias and pads (input vias of the default class
//!   included) keep that value from a routed default-class via:
//!   conservative.
//!
//! A class-pair rule of K07.5 on the same board is folded into every kind
//! pair of those classes, so splitting a class never loses it.
//!
//! # Hole clearance
//!
//! A `hole_clearance` rule is evaluated for every pair of a hole (via, plated
//! or NPTH pad) and copper (a pad's hole against another pad's copper is
//! left out: routing never moves a pad).
//!
//! - **Plated holes against tracks only** (Q22.15; ch32v003's
//!   `A.isPlated() && B.Type == 'track'`): the via and the pin are round
//!   copper around their hole, so the rule becomes typed copper clearances.
//!   A via hole keeps `min` from tracks as `via_wire` of `min` less the
//!   smallest annular ring of the board's via padstacks, read from their
//!   names (`Via[a-b]_<diameter>:<drill>_um`, KiCad's and the reader's; the
//!   engine takes a via's drill from the same name). A plated pad's hole
//!   keeps `min` from tracks as `pin_wire` of `min` itself, with a warning:
//!   the DSN gives no pad drill, so the pad's copper edge stands for its
//!   hole. When a via name gives no drill the rule is a floor, as below.
//! - **Any other match** (an NPTH hole, which reaches the router as a
//!   keepout, or a hole against pad or via copper) becomes a floor of the
//!   board-wide `router.hole_clearance_um` (Q18):
//!   [`super::DruReport::hole_clearance`] carries the largest value and the
//!   caller raises the setting to it. The global is conservative: it keeps
//!   every hole from all copper, which KiCad's rule may not ask.
//!
//! Before Q22.15 the plated rule was a floor too: on ch32v003 (one net class,
//! `'PTH to Track clearance)'` 0.33 mm over Board Setup's 0.25 mm) it also
//! kept every routed via's copper 0.33 mm from pad drills and other vias'
//! drills, which KiCad's rule does not ask.
//!
//! # Hole to hole
//!
//! A `hole_to_hole` rule (Q22.14) is evaluated for every pair of holes with
//! at least one via (routing places no pad), each pair both of one net and
//! of two nets. One that matches any becomes a floor of
//! `router.board_minimums.hole_to_hole_um` (Q22.11), which keeps every routed
//! via hole that far from every drill, of any net:
//! [`super::DruReport::hole_to_hole`] carries the largest value (decided
//! 2026-10-20, option (a)). It is conservative: a rule for holes of two nets
//! also holds between holes of one net, with a warning.

use std::collections::{BTreeMap, BTreeSet};

use freerouting_dsn::ast::{ClassClass, Network, Rule, Structure};

use super::{ApplyCtx, Cond, DSN_DEFAULT, DruRule, HoleClearance, Probe, Status, kicad_name};

/// A KiCad item, as a `.kicad_dru` condition sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ItemType {
    Track,
    Via,
    PthPad,
    SmdPad,
    EdgePad,
    NpthPad,
}

impl ItemType {
    pub const ALL: [ItemType; 6] = [
        ItemType::Track,
        ItemType::Via,
        ItemType::PthPad,
        ItemType::SmdPad,
        ItemType::EdgePad,
        ItemType::NpthPad,
    ];

    /// KiCad's `Type`, lower case.
    pub fn kicad_type(self) -> &'static str {
        match self {
            ItemType::Track => "track",
            ItemType::Via => "via",
            _ => "pad",
        }
    }

    /// `Type == value` for a lower-case `value`; an arc is a track.
    pub fn has_type(self, value: &str) -> bool {
        match self {
            ItemType::Track => value == "track" || value == "arc",
            _ => value == self.kicad_type(),
        }
    }

    /// `isPlated()`.
    pub fn is_plated(self) -> bool {
        matches!(self, ItemType::Via | ItemType::PthPad)
    }

    /// `Pad_Type`, lower case; `None` for an item that is not a pad.
    pub fn pad_type(self) -> Option<&'static str> {
        match self {
            ItemType::PthPad => Some("through-hole"),
            ItemType::SmdPad => Some("smd"),
            ItemType::EdgePad => Some("edge connector"),
            ItemType::NpthPad => Some("npth, mechanical"),
            _ => None,
        }
    }

    /// The DSN clearance kind; `None` for an NPTH pad.
    pub fn kind(self) -> Option<Kind> {
        match self {
            ItemType::Track => Some(Kind::Wire),
            ItemType::Via => Some(Kind::Via),
            ItemType::PthPad => Some(Kind::Pin),
            ItemType::SmdPad | ItemType::EdgePad => Some(Kind::Smd),
            ItemType::NpthPad => None,
        }
    }

    /// Whether the item has a drilled hole.
    pub fn has_hole(self) -> bool {
        matches!(self, ItemType::Via | ItemType::PthPad | ItemType::NpthPad)
    }

    /// Whether the item has copper.
    pub fn has_copper(self) -> bool {
        self != ItemType::NpthPad
    }

    /// Whether the item is a pad.
    pub fn is_pad(self) -> bool {
        !matches!(self, ItemType::Track | ItemType::Via)
    }
}

/// A DSN clearance kind (`(type <a>_<b>)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Wire,
    Via,
    Pin,
    Smd,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Wire, Kind::Via, Kind::Pin, Kind::Smd];

    pub fn dsn_name(self) -> &'static str {
        match self {
            Kind::Wire => "wire",
            Kind::Via => "via",
            Kind::Pin => "pin",
            Kind::Smd => "smd",
        }
    }

    fn from_name(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.dsn_name() == name)
    }
}

/// A kind pair, the larger kind first.
type KindPair = (Kind, Kind);

const WIRE_WIRE: KindPair = (Kind::Wire, Kind::Wire);

fn pair(a: Kind, b: Kind) -> KindPair {
    if a >= b { (a, b) } else { (b, a) }
}

/// The ten kind pairs, in a fixed order.
fn all_pairs() -> Vec<KindPair> {
    let mut out = Vec::new();
    for (i, a) in Kind::ALL.iter().enumerate() {
        for b in &Kind::ALL[..=i] {
            out.push(pair(*a, *b));
        }
    }
    out
}

/// `pin_wire` and so on.
pub(crate) fn type_name(p: KindPair) -> String {
    format!("{}_{}", p.0.dsn_name(), p.1.dsn_name())
}

fn parse_type(t: &str) -> Option<KindPair> {
    let (a, b) = t.split_once('_')?;
    Some(pair(Kind::from_name(a)?, Kind::from_name(b)?))
}

fn is_pad(k: Kind) -> bool {
    matches!(k, Kind::Pin | Kind::Smd)
}

/// The names the DSN reader takes for KiCad's default class.
fn is_default(name: &str) -> bool {
    name.eq_ignore_ascii_case("kicad_default") || name.eq_ignore_ascii_case("default")
}

fn typed(value: f64, p: KindPair) -> Rule {
    Rule::Clearance {
        value,
        types: vec![type_name(p)],
    }
}

fn max_opt(values: &[Option<f64>]) -> Option<f64> {
    values.iter().flatten().copied().reduce(f64::max)
}

fn mm(nm: i64) -> String {
    format!("{} mm", nm as f64 / 1e6)
}

/// The DSN's clearances before any rule, in DSN units.
#[derive(Clone, Debug, Default)]
pub(crate) struct Base {
    structure: Option<f64>,
    structure_typed: BTreeMap<KindPair, f64>,
    class: BTreeMap<String, f64>,
    default: Option<String>,
}

impl Base {
    pub(crate) fn of(network: &Network, structure: &Structure) -> Base {
        let mut base = Base::default();
        for rule in &structure.rules {
            if let Rule::Clearance { value, types } = rule {
                match types.as_slice() {
                    [] => base.structure = Some(*value),
                    [t] => {
                        if let Some(p) = parse_type(t) {
                            base.structure_typed.insert(p, *value);
                        }
                    }
                    _ => {}
                }
            }
        }
        for class in &network.classes {
            for rule in &class.rules {
                if let Rule::Clearance { value, types } = rule
                    && types.is_empty()
                {
                    base.class.insert(class.name.clone(), *value);
                }
            }
        }
        base.default = network
            .classes
            .iter()
            .find(|c| is_default(&c.name))
            .map(|c| c.name.clone());
        base
    }

    fn default_name(&self) -> String {
        self.default
            .clone()
            .unwrap_or_else(|| DSN_DEFAULT.to_owned())
    }

    /// The clearance the DSN reader gives kinds `k` of classes `a` and `b`.
    pub(crate) fn get(&self, a: &str, b: &str, k: KindPair) -> f64 {
        let s = self.structure;
        let d = self
            .default
            .as_ref()
            .and_then(|n| self.class.get(n).copied());
        let own = |c: &str| self.class.get(c).copied();
        let v = if is_default(a) && is_default(b) {
            if k == WIRE_WIRE {
                d.or(s)
            } else {
                self.structure_typed.get(&k).copied().or(max_opt(&[s, d]))
            }
        } else if a == b {
            own(a).or(max_opt(&[s, d]))
        } else {
            max_opt(&[s, d, own(a), own(b)])
        };
        v.unwrap_or(0.0)
    }
}

/// The item-type clearances of all rules, emitted after the last one.
#[derive(Clone, Debug, Default)]
pub(crate) struct Typed {
    /// The largest applied value per kind pair, in DSN units.
    req: BTreeMap<KindPair, f64>,
    /// K07.5's class-pair values, the class names in sorted order.
    class_pairs: BTreeMap<(String, String), f64>,
}

impl Typed {
    /// Records a K07.5 class-pair clearance (all layers).
    pub(crate) fn class_pair(&mut self, a: &str, b: &str, value: f64) {
        let key = if a <= b {
            (a.to_owned(), b.to_owned())
        } else {
            (b.to_owned(), a.to_owned())
        };
        self.class_pairs.insert(key, value);
    }

    fn require(&mut self, k: KindPair, value: f64) {
        let v = self.req.entry(k).or_insert(value);
        *v = v.max(value);
    }

    /// The kinds that get their own clearance class: all four when a pair
    /// with `wire` is typed (the reader makes them all), else those named.
    fn split_kinds(&self) -> BTreeSet<Kind> {
        if self.req.keys().any(|k| k.1 == Kind::Wire) {
            return Kind::ALL.into_iter().filter(|k| *k != Kind::Wire).collect();
        }
        self.req
            .keys()
            .flat_map(|k| [k.0, k.1])
            .filter(|k| *k != Kind::Wire)
            .collect()
    }

    /// The value to write for kinds `k` of classes `a` and `b`: `None` when
    /// it is no more than the DSN gives.
    fn value(&self, base: &Base, a: &str, b: &str, k: KindPair) -> Option<f64> {
        let key = if a <= b {
            (a.to_owned(), b.to_owned())
        } else {
            (b.to_owned(), a.to_owned())
        };
        let w = if k == WIRE_WIRE {
            None
        } else {
            self.class_pairs.get(&key).copied()
        };
        max_opt(&[self.req.get(&k).copied(), w]).filter(|v| *v > base.get(a, b, k))
    }

    /// Writes the values into the tree; see the module docs for where.
    pub(crate) fn emit(&self, base: &Base, network: &mut Network, structure: &mut Structure) {
        if self.req.is_empty() {
            return;
        }
        let split = self.split_kinds();
        let pairs: Vec<KindPair> = all_pairs()
            .into_iter()
            .filter(|k| {
                if *k == WIRE_WIRE {
                    self.req.contains_key(k)
                } else {
                    split.contains(&k.0) || split.contains(&k.1)
                }
            })
            .collect();
        let d = base.default_name();
        // 1. the default class, in the structure
        for k in pairs.iter().filter(|k| **k != WIRE_WIRE) {
            if let Some(v) = self.value(base, &d, &d, *k) {
                structure.rules.push(typed(v, *k));
            }
        }
        // 2. every other class, after its own rules
        let classes: Vec<String> = network.classes.iter().map(|c| c.name.clone()).collect();
        let several = classes.len() > 1;
        for (i, x) in classes.iter().enumerate() {
            if is_default(x) {
                continue;
            }
            let mut rules: Vec<Rule> = pairs
                .iter()
                .filter_map(|k| self.value(base, x, x, *k).map(|v| typed(v, *k)))
                .collect();
            let materialized = rules.iter().any(|r| match r {
                Rule::Clearance { types, .. } => types.iter().any(|t| t.ends_with("_wire")),
                _ => false,
            });
            if several
                && !materialized
                && let Some(kind) = split.iter().next()
            {
                // makes X-via, X-pin, ... before X's vias, at X's own value
                let k = pair(*kind, Kind::Wire);
                rules.push(typed(base.get(x, x, k), k));
            }
            network.classes[i].rules.extend(rules);
        }
        // 3. pairs of classes
        let mut default_split: BTreeSet<Kind> = BTreeSet::new();
        for (i, a) in classes.iter().enumerate() {
            for b in &classes[i + 1..] {
                if !pairs.iter().any(|k| self.value(base, a, b, *k).is_some()) {
                    continue;
                }
                // the whole block: a class the pair makes is a copy of the
                // default class's row, so the pairs it does not raise are
                // written at the DSN's value
                let rules: Vec<(KindPair, f64)> = pairs
                    .iter()
                    .map(|k| {
                        let v = self.value(base, a, b, *k);
                        (*k, v.unwrap_or_else(|| base.get(a, b, *k)))
                    })
                    .collect();
                if is_default(a) || is_default(b) {
                    for (k, _) in &rules {
                        default_split.extend([k.0, k.1].into_iter().filter(|k| *k != Kind::Wire));
                    }
                }
                network.class_classes.push(ClassClass {
                    classes: vec![a.clone(), b.clone()],
                    rules: rules.into_iter().map(|(k, v)| typed(v, k)).collect(),
                    layer_rules: Vec::new(),
                });
            }
        }
        // 4. the default class with itself: wire_wire, and its block again
        // when step 3 made `default-pin` and the like
        let Some(d) = base.default.clone() else {
            return;
        };
        let mut rules = Vec::new();
        if let Some(v) = self.value(base, &d, &d, WIRE_WIRE) {
            rules.push(typed(v, WIRE_WIRE));
        }
        for k in all_pairs() {
            let named = |x: Kind| x == Kind::Wire || default_split.contains(&x);
            if k == WIRE_WIRE || default_split.is_empty() || !named(k.0) || !named(k.1) {
                continue;
            }
            let v = self
                .value(base, &d, &d, k)
                .unwrap_or_else(|| base.get(&d, &d, k));
            rules.push(typed(v, k));
        }
        if !rules.is_empty() {
            network.class_classes.push(ClassClass {
                classes: vec![d.clone(), d],
                rules,
                layer_rules: Vec::new(),
            });
        }
    }
}

/// The class names a condition is evaluated for (KiCad's names).
pub(super) fn kicad_classes(classes: &[String]) -> Vec<&str> {
    if classes.is_empty() {
        vec!["Default"]
    } else {
        classes.iter().map(|c| kicad_name(c)).collect()
    }
}

/// Whether `cond` holds for the pair in either order.
pub(super) fn holds(cond: Option<&Cond>, a: Probe<'_>, b: Probe<'_>) -> Result<bool, String> {
    match cond {
        None => Ok(true),
        Some(c) => Ok(c.eval(a, Some(b))? || c.eval(b, Some(a))?),
    }
}

/// For every kind pair: (some item pair matches, some does not).
fn match_kinds(
    cond: Option<&Cond>,
    classes: &[String],
) -> Result<BTreeMap<KindPair, (bool, bool)>, String> {
    let names = kicad_classes(classes);
    let mut out: BTreeMap<KindPair, (bool, bool)> = BTreeMap::new();
    for (i, a) in names.iter().enumerate() {
        for b in &names[i..] {
            for ta in ItemType::ALL {
                for tb in ItemType::ALL {
                    let (Some(ka), Some(kb)) = (ta.kind(), tb.kind()) else {
                        continue;
                    };
                    let pa = Probe {
                        class: a,
                        item: Some(ta),
                    };
                    let pb = Probe {
                        class: b,
                        item: Some(tb),
                    };
                    let hit = holds(cond, pa, pb)?;
                    let e = out.entry(pair(ka, kb)).or_default();
                    if hit {
                        e.0 = true;
                    } else {
                        e.1 = true;
                    }
                }
            }
        }
    }
    Ok(out)
}

/// A `clearance` constraint whose condition has item terms.
pub(super) fn clearance(
    rule: &DruRule,
    min_nm: i64,
    cond: Option<&Cond>,
    all_layers: bool,
    classes: &[String],
    ctx: &mut ApplyCtx<'_>,
) -> Status {
    let kinds = match match_kinds(cond, classes) {
        Ok(k) => k,
        Err(e) => return Status::Unsupported(e),
    };
    let matched: Vec<KindPair> = kinds
        .iter()
        .filter(|(k, (hit, _))| *hit && !(is_pad(k.0) && is_pad(k.1)))
        .map(|(k, _)| *k)
        .collect();
    if matched.is_empty() {
        let why = if kinds.values().any(|(hit, _)| *hit) {
            "covers only pad pairs, which routing never moves"
        } else {
            "matches no item pair the router keeps a clearance between"
        };
        return Status::Ignored(why.to_owned());
    }
    if !ctx.apply {
        return Status::Ignored("apply_design_rules is off".to_owned());
    }
    let value = min_nm as f64 / ctx.nm_per_unit;
    let mut names: Vec<String> = classes.to_vec();
    if names.is_empty() {
        names.push(ctx.base.default_name());
    }
    let mut effects = 0;
    for (i, a) in names.iter().enumerate() {
        for b in &names[i..] {
            effects += matched
                .iter()
                .filter(|k| value > ctx.base.get(a, b, **k))
                .count();
        }
    }
    if effects == 0 {
        return Status::Ignored("asks for no more than the DSN gives".to_owned());
    }
    for k in &matched {
        ctx.typed.require(*k, value);
    }
    let exact = matched.iter().all(|k| !kinds[k].1);
    if !exact || !all_layers {
        let list: Vec<String> = matched
            .iter()
            .map(|k| format!("{}-{}", k.0.dsn_name(), k.1.dsn_name()))
            .collect();
        ctx.warnings.push(format!(
            "design rule '{}' (line {}): the router cannot say its condition exactly, so clearance {} \
             applies to every {} pair of every net class on every layer",
            rule.name,
            rule.line,
            mm(min_nm),
            list.join(", ")
        ));
    }
    Status::Applied { effects }
}

/// A `hole_clearance` constraint: typed copper clearances for plated holes
/// against tracks, else a floor of `router.hole_clearance_um` (module docs).
pub(super) fn hole_clearance(
    rule: &DruRule,
    min_nm: i64,
    cond: Option<&Cond>,
    classes: &[String],
    ctx: &mut ApplyCtx<'_>,
) -> Status {
    let names = kicad_classes(classes);
    let mut matched: BTreeSet<(ItemType, ItemType)> = BTreeSet::new();
    let (mut pads_only, mut miss) = (false, false);
    for (i, a) in names.iter().enumerate() {
        for b in &names[i..] {
            for hole in ItemType::ALL.into_iter().filter(|t| t.has_hole()) {
                for copper in ItemType::ALL.into_iter().filter(|t| t.has_copper()) {
                    // `a` and `b` may each be the hole
                    for (x, y) in [(*a, *b), (*b, *a)] {
                        let h = Probe {
                            class: x,
                            item: Some(hole),
                        };
                        let c = Probe {
                            class: y,
                            item: Some(copper),
                        };
                        match holds(cond, h, c) {
                            Ok(true) if hole.is_pad() && copper.is_pad() => pads_only = true,
                            Ok(true) => {
                                matched.insert((hole, copper));
                            }
                            Ok(false) => miss = true,
                            Err(e) => return Status::Unsupported(e),
                        }
                    }
                }
            }
        }
    }
    if matched.is_empty() {
        let why = if pads_only {
            "covers only pad pairs, which routing never moves"
        } else {
            "matches no hole and copper pair"
        };
        return Status::Ignored(why.to_owned());
    }
    if !ctx.apply {
        return Status::Ignored("apply_design_rules is off".to_owned());
    }
    let plated_to_tracks = matched
        .iter()
        .all(|(h, c)| *c == ItemType::Track && h.is_plated());
    if plated_to_tracks && let Some(status) = plated_to_track(rule, min_nm, &matched, classes, ctx)
    {
        return status;
    }
    let floor = ctx.hole.get_or_insert(HoleClearance {
        nm: min_nm,
        rules: Vec::new(),
        replaces_board_setup: false,
    });
    floor.nm = floor.nm.max(min_nm);
    floor.rules.push(rule.name.clone());
    if miss {
        ctx.warnings.push(format!(
            "design rule '{}' (line {}): hole_clearance {} becomes a floor of \
             router.hole_clearance_um, which keeps every hole from all copper",
            rule.name,
            rule.line,
            mm(min_nm)
        ));
    }
    Status::Applied { effects: 1 }
}

/// Q22.15: a plated hole against tracks as `via_wire` (less the vias'
/// annular ring) and `pin_wire`; `None` when a via's drill is unknown.
fn plated_to_track(
    rule: &DruRule,
    min_nm: i64,
    matched: &BTreeSet<(ItemType, ItemType)>,
    classes: &[String],
    ctx: &mut ApplyCtx<'_>,
) -> Option<Status> {
    let has = |t: ItemType| matched.iter().any(|(h, _)| *h == t);
    let mut wanted: Vec<(KindPair, f64)> = Vec::new();
    if has(ItemType::Via) {
        let annular = ctx.via_annular_nm?;
        wanted.push((pair(Kind::Via, Kind::Wire), (min_nm - annular) as f64));
    }
    if has(ItemType::PthPad) {
        wanted.push((pair(Kind::Pin, Kind::Wire), min_nm as f64));
    }
    let mut names: Vec<String> = classes.to_vec();
    if names.is_empty() {
        names.push(ctx.base.default_name());
    }
    let mut effects = 0;
    let mut raised: BTreeSet<KindPair> = BTreeSet::new();
    for (i, a) in names.iter().enumerate() {
        for b in &names[i..] {
            for (k, nm) in &wanted {
                if nm / ctx.nm_per_unit > ctx.base.get(a, b, *k) {
                    effects += 1;
                    raised.insert(*k);
                }
            }
        }
    }
    if effects == 0 {
        return Some(Status::Ignored(
            "asks for no more than the DSN gives".to_owned(),
        ));
    }
    for (k, nm) in wanted.iter().filter(|(k, _)| raised.contains(k)) {
        ctx.typed.require(*k, nm / ctx.nm_per_unit);
    }
    if raised.contains(&pair(Kind::Pin, Kind::Wire)) {
        ctx.warnings.push(format!(
            "design rule '{}' (line {}): hole_clearance {} of plated pads keeps tracks that far \
             from the pad's copper, since the DSN gives no pad drill",
            rule.name,
            rule.line,
            mm(min_nm)
        ));
    }
    Some(Status::Applied { effects })
}

/// A `hole_to_hole` constraint (Q22.14): a floor of
/// `router.board_minimums.hole_to_hole_um` (module docs).
pub(super) fn hole_to_hole(
    rule: &DruRule,
    min_nm: i64,
    cond: Option<&Cond>,
    classes: &[String],
    ctx: &mut ApplyCtx<'_>,
) -> Status {
    let names = kicad_classes(classes);
    let variants: Vec<Option<Cond>> = [false, true]
        .into_iter()
        .map(|same| cond.map(|c| c.with_same_net(same)))
        .collect();
    let (mut via, mut pads, mut miss) = (false, false, false);
    let holes = ItemType::ALL.into_iter().filter(|t| t.has_hole());
    for (i, a) in names.iter().enumerate() {
        for b in &names[i..] {
            for ta in holes.clone() {
                for tb in holes.clone() {
                    let pa = Probe {
                        class: a,
                        item: Some(ta),
                    };
                    let pb = Probe {
                        class: b,
                        item: Some(tb),
                    };
                    for c in &variants {
                        match holds(c.as_ref(), pa, pb) {
                            Ok(true) if ta.is_pad() && tb.is_pad() => pads = true,
                            Ok(true) => via = true,
                            Ok(false) => miss = true,
                            Err(e) => return Status::Unsupported(e),
                        }
                    }
                }
            }
        }
    }
    if !via {
        let why = if pads {
            "covers only pad pairs, which routing never moves"
        } else {
            "matches no pair of holes"
        };
        return Status::Ignored(why.to_owned());
    }
    if !ctx.apply {
        return Status::Ignored("apply_design_rules is off".to_owned());
    }
    let floor = ctx.hole_to_hole.get_or_insert(HoleClearance {
        nm: min_nm,
        rules: Vec::new(),
        replaces_board_setup: false,
    });
    floor.nm = floor.nm.max(min_nm);
    floor.rules.push(rule.name.clone());
    if miss {
        ctx.warnings.push(format!(
            "design rule '{}' (line {}): hole_to_hole {} becomes a floor of \
             router.board_minimums.hole_to_hole_um, which keeps every routed via hole that far \
             from every drill, of any net",
            rule.name,
            rule.line,
            mm(min_nm)
        ));
    }
    Status::Applied { effects: 1 }
}

/// Q22.15: the smallest annular ring, in nanometres, of the via padstacks the
/// DSN names (the structure's, the network's and its classes', and the
/// `Via[`-named ones of `extra`). `None` when there is none or a name gives
/// no drill.
pub(super) fn via_annular_nm(
    network: &Network,
    structure: &Structure,
    extra: &[String],
) -> Option<i64> {
    let listed = structure
        .via
        .iter()
        .flat_map(|v| v.padstacks.iter().chain(&v.spare))
        .chain(network.vias.iter().map(|v| &v.padstack))
        .chain(
            network
                .classes
                .iter()
                .flat_map(|c| c.circuit.iter().flat_map(|c| &c.use_via)),
        );
    let library = extra.iter().filter(|n| n.starts_with("Via["));
    let mut smallest: Option<i64> = None;
    for name in listed.chain(library) {
        let ring = annular_of_name(name)?;
        smallest = Some(smallest.map_or(ring, |s| s.min(ring)));
    }
    smallest
}

/// `Via[a-b]_<diameter>:<drill>_um`: half the difference, in nanometres.
fn annular_of_name(name: &str) -> Option<i64> {
    let sizes = name.strip_suffix("_um")?.rsplit_once('_')?.1;
    let (d, drill) = sizes.split_once(':')?;
    let (d, drill): (f64, f64) = (d.parse().ok()?, drill.parse().ok()?);
    (drill > 0.0 && d >= drill).then(|| ((d - drill) * 500.0).round() as i64)
}
