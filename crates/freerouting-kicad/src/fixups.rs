//! K14: the KiCad plugin's DSN fix-ups (K09, `kicad-plugin/freerouting_rs/dsn_fixups.py`),
//! done on the reader's tree for a `.kicad_pcb` input.
//!
//! The plugin rewrites the text of KiCad's DSN export; a `.kicad_pcb` input has no such
//! text, so the engine applies the same changes to the tree [`crate::read`] builds, from
//! the router settings `router.input_fixups.*` (`job::prepare` in the engine, after the
//! settings are merged and before the board is built):
//!
//! - [`InputFixups::class_widths`] (`class_widths`): a class's track width, the first
//!   `Rule::Width` of the class, as the plugin writes the class's `(rule (width W))`;
//! - [`InputFixups::route_on_plane_layers`] (`route_on_plane_layers`): a power layer
//!   becomes a signal layer, as the export's `(type power)` becomes `(type signal)`;
//!   its planes stay;
//! - [`InputFixups::drop_pour_nets`] (`drop_pour_nets`): the planes of the listed nets on
//!   the outer copper layers (the first and last layer of the structure) go, as the
//!   plugin drops their `(plane ...)`; inner-layer planes of those nets stay.
//!
//! The values are names only. The plugin resolves its `auto` lists from the board
//! (`dsn_fixups.resolve_on_board`) and passes explicit names. A class with no width and a
//! layer that does not exist or is not a power layer are errors that name it
//! ([`FixupError`]), checked before anything changes, as the DSN fix-ups raise them. A net
//! the board does not have is a warning line and is left out (the DSN fix-up passes it
//! silently). A class whose width an applied `.kicad_dru` `track_width` rule set keeps the
//! rule's width ([`apply_with_rules`]), as on the DSN path, where `--kicad-dru` runs after
//! the plugin's text fix-up. The tree is the
//! export's except where `docs/DIVERGENCES.md` says otherwise, so on the same board the
//! fixed-up tree equals the plugin's fixed-up export, parsed (`tests/k14_fixups.rs`).

use std::fmt;

use freerouting_dsn::ast::{LayerRef, LayerType, Pcb, Rule};

/// The settings' section, for messages.
pub const SECTION: &str = "router.input_fixups";

/// The fix-ups to apply, from the `router.input_fixups.*` settings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputFixups {
    /// `(class, track width in µm)`, in the order given.
    pub class_widths: Vec<(String, f64)>,
    /// Power layers to open to routing, by DSN layer name.
    pub route_on_plane_layers: Vec<String>,
    /// Nets whose outer-layer planes are dropped.
    pub drop_pour_nets: Vec<String>,
}

/// A fix-up that does not fit the board, or a value that does not parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixupError {
    /// The setting, such as `router.input_fixups.class_widths`.
    pub setting: String,
    pub message: String,
}

impl fmt::Display for FixupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.setting, self.message)
    }
}

impl std::error::Error for FixupError {}

fn error(name: &str, message: String) -> FixupError {
    FixupError {
        setting: format!("{SECTION}.{name}"),
        message,
    }
}

impl InputFixups {
    /// True when no fix-up is asked for.
    pub fn is_empty(&self) -> bool {
        self.class_widths.is_empty()
            && self.route_on_plane_layers.is_empty()
            && self.drop_pour_nets.is_empty()
    }

    /// The fix-ups of the settings' lists. `class_widths` entries are
    /// `<class>=<width µm>`, split at the last `=`; the width must be a positive
    /// number. Empty entries are left out.
    pub fn from_lists(
        class_widths: &[String],
        route_on_plane_layers: &[String],
        drop_pour_nets: &[String],
    ) -> Result<InputFixups, FixupError> {
        let mut widths = Vec::new();
        for entry in class_widths
            .iter()
            .map(|e| e.trim())
            .filter(|e| !e.is_empty())
        {
            let bad = || {
                error(
                    "class_widths",
                    format!("`{entry}` is not `<class>=<width in um>`"),
                )
            };
            let (name, value) = entry.rsplit_once('=').ok_or_else(bad)?;
            let name = name.trim();
            let um: f64 = value.trim().parse().map_err(|_| bad())?;
            if name.is_empty() || !um.is_finite() || um <= 0.0 {
                return Err(bad());
            }
            widths.push((name.to_owned(), um));
        }
        let names = |list: &[String]| -> Vec<String> {
            list.iter()
                .map(|n| n.trim())
                .filter(|n| !n.is_empty())
                .map(str::to_owned)
                .collect()
        };
        Ok(InputFixups {
            class_widths: widths,
            route_on_plane_layers: names(route_on_plane_layers),
            drop_pour_nets: names(drop_pour_nets),
        })
    }
}

/// Applies `fixups` to `pcb`. Returns one log line per change, worded like the
/// plugin's log; on an error nothing has changed.
pub fn apply(pcb: &mut Pcb, fixups: &InputFixups) -> Result<Vec<String>, FixupError> {
    apply_with_rules(pcb, fixups, &[])
}

/// [`apply`] on a tree the board's `.kicad_dru` rules were applied to. `rule_widths`
/// ([`crate::dru::DruReport::track_widths`]) are the classes an applied `track_width`
/// rule set, with the rule: `class_widths` leaves them at the rule's width, with one
/// line naming the class and the rule (the last rule that set it). On the DSN path the
/// plugin's text fix-up comes first and `--kicad-dru` after it, so the rule wins there
/// too, and the board's own width rule is never undercut.
pub fn apply_with_rules(
    pcb: &mut Pcb,
    fixups: &InputFixups,
    rule_widths: &[(String, String)],
) -> Result<Vec<String>, FixupError> {
    check(pcb, fixups)?;
    let mut log = Vec::new();
    if let Some(network) = pcb.network.as_mut() {
        let mut widths: Vec<&(String, f64)> = fixups.class_widths.iter().collect();
        widths.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, um) in widths {
            let rule = rule_widths.iter().rev().find(|(c, _)| c == name);
            let class = network.classes.iter_mut().find(|c| &c.name == name);
            if let Some(Rule::Width(w)) = class.and_then(|c| first_width(&mut c.rules)) {
                match rule {
                    Some((_, rule)) => log.push(format!(
                        "class {name}: width {} um kept, set by the .kicad_dru rule '{rule}' \
                         (class_widths skipped)",
                        fmt_um(*w)
                    )),
                    None => {
                        log.push(format!("class {name}: width {} -> {um} um", fmt_um(*w)));
                        *w = *um;
                    }
                }
            }
        }
    }
    // an unknown net warns and is left out: the DSN fix-up passes it silently, and a run
    // never fails just because of the input choice (decided 2026-10-21)
    let known = known_nets(pcb);
    let mut nets: Vec<String> = Vec::new();
    for name in &fixups.drop_pour_nets {
        if known.contains(&name.as_str()) {
            nets.push(name.clone());
        } else {
            log.push(format!(
                "WARNING: {SECTION}.drop_pour_nets: the board has no net {name}; left out"
            ));
        }
    }
    if let Some(structure) = pcb.structure.as_mut() {
        if !nets.is_empty() {
            let outer = outer_layers(&structure.layers);
            let before = structure.planes.len();
            structure.planes.retain(|p| {
                !(nets.contains(&p.net)
                    && matches!(p.area.shape.layer(), LayerRef::Named(l) if outer.contains(&l.as_str())))
            });
            nets.sort();
            nets.dedup();
            log.push(format!(
                "pours: {} outer plane(s) removed (dropped nets: {})",
                before - structure.planes.len(),
                nets.join(", ")
            ));
        }
        for name in &fixups.route_on_plane_layers {
            if let Some(layer) = structure.layers.iter_mut().find(|l| &l.name == name)
                && layer.layer_type == Some(LayerType::Power)
            {
                layer.layer_type = Some(LayerType::Signal);
                log.push(format!("layer {name}: power -> signal"));
            }
        }
    }
    Ok(log)
}

fn first_width(rules: &mut [Rule]) -> Option<&mut Rule> {
    rules.iter_mut().find(|r| matches!(r, Rule::Width(_)))
}

/// A width as the DSN prints it: no `.0` on whole micrometres.
fn fmt_um(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// The outer copper layers: the first and the last of the structure's layers.
fn outer_layers(layers: &[freerouting_dsn::ast::Layer]) -> Vec<&str> {
    layers
        .first()
        .into_iter()
        .chain(layers.last())
        .map(|l| l.name.as_str())
        .collect()
}

/// Every class and layer the fix-ups name is on the board.
fn check(pcb: &Pcb, fixups: &InputFixups) -> Result<(), FixupError> {
    let classes = pcb.network.as_ref().map(|n| &n.classes[..]).unwrap_or(&[]);
    for (name, _) in &fixups.class_widths {
        let has_width = classes
            .iter()
            .find(|c| &c.name == name)
            .is_some_and(|c| c.rules.iter().any(|r| matches!(r, Rule::Width(_))));
        if !has_width {
            return Err(error(
                "class_widths",
                format!("the board has no net class {name} with a track width"),
            ));
        }
    }
    let layers = pcb.structure.as_ref().map(|s| &s.layers[..]).unwrap_or(&[]);
    for name in &fixups.route_on_plane_layers {
        match layers.iter().find(|l| &l.name == name) {
            None => {
                return Err(error(
                    "route_on_plane_layers",
                    format!("the board has no copper layer {name}"),
                ));
            }
            Some(l) if l.layer_type != Some(LayerType::Power) => {
                return Err(error(
                    "route_on_plane_layers",
                    format!("layer {name} is not a plane (power) layer"),
                ));
            }
            Some(_) => {}
        }
    }
    Ok(())
}

/// The nets of the network and of the planes.
fn known_nets(pcb: &Pcb) -> Vec<&str> {
    let nets = pcb
        .network
        .iter()
        .flat_map(|n| &n.nets)
        .map(|n| n.name.as_str());
    let planes = pcb
        .structure
        .iter()
        .flat_map(|s| &s.planes)
        .map(|p| p.net.as_str());
    nets.chain(planes).collect()
}
