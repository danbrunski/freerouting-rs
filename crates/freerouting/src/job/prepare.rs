//! `prepare_job`: Java's load sequence for a routing job (C03.6).
//!
//! Port of the part of `CliRunner.initializeCli`, `RoutingJobScheduler`
//! (`:92-191`) and `HeadlessBoardManager` (`loadFromSpecctraDsn`,
//! `applyRouterSettingsForLoadedBoard`, `applyImmediatePostLoadProcessing`)
//! that turns a DSN file and the settings sources into a board ready to
//! route (Java at `cda540f`). The steps run in Java's order, including the
//! step that looks redundant:
//!
//! 1. the first merge ([`resolve`]): defaults, the routing profile (N01; an
//!    unknown one is a [`JobError::Settings`]), config file, DSN,
//!    environment, CLI (and any extra sources, for example a testing
//!    profile);
//! 2. read the board; a `.kicad_pcb` input's tree first gets the plugin's DSN
//!    fix-ups that `router.input_fixups.*` asks for (K14, [`apply_input_fixups`];
//!    a DSN input ignores them, with a warning); a `.kicad_dru` given for a DSN input
//!    ([`JobInput::kicad_dru`], `--kicad-dru`, Q22.5) is applied to the
//!    parsed tree first, and its `hole_clearance` floor (or a `.kicad_pcb`'s,
//!    [`JobInput::rules_hole_clearance`]) raises `router.hole_clearance_um`
//!    after each merge, over the command line too, as its `hole_to_hole`
//!    floor (Q22.14, [`JobInput::rules_hole_to_hole`]) raises
//!    `router.board_minimums.hole_to_hole_um`. Without either nothing
//!    changes;
//! 3. `set_layer_count` if the counts differ, then the board-specific cost
//!    tuning, the net-class exclusions and the five overrides. **The costs
//!    are tuned on the bounding box before it is expanded**, and the tuning
//!    sets `costs_initialized`, so the second tuning in step 6 is a no-op:
//!    trace costs come from the unexpanded box;
//! 4. expand the bounding box to all items and reduce the nets;
//! 5. only with a rules file (C04): the second merge, then the file onto the
//!    board and its `autoroute_settings` onto the merged settings
//!    ([`apply_rules_file`]), **after** the CLI and every other source;
//! 6. the tuning and the net-class exclusions again;
//! 7. the SES import;
//! 8. the clearance violations of the board as it is now are recorded as
//!    the board's pre-existing ones ([`record_pre_existing_violations`]).
//!
//! **The pre-existing violations.** Java counts them in
//! `HeadlessBoardManager.scheduleDeferredPostLoadProcessing`, a background
//! thread started after step 4 (`preExistingClearanceViolationsCount`, which
//! `BoardStatistics` waits for); its count races the rules file and the
//! session import. The port counts once, after every step that changes the
//! board, the Rust-only `router.wire_keepout`, `router.pad_contacts` and pour
//! guard included, so the statistics' `router_introduced_count` counts only
//! what routing added. Without a rules file, a session and those settings
//! the count is Java's (`tests/pre_existing_violations.rs`).
//!
//! **The scheduler's second merge runs only with a rules file.** Java
//! re-merges at `RoutingJobScheduler.java:101-174`: the sources again, the
//! rules file's `autoroute_settings` at priority 40, and the first merge's
//! result at priority 70 (`ApiSettings`). Without a rules file the result
//! comes back unchanged, because after step 3 it has no null field a lower
//! source could fill. With one it matters: the rules source reads its layer
//! names from the rules file alone (`RulesReader.discoverLayerStructure`), so
//! a `layer_rule` for `In1.Cu` in a file that names only that layer lands on
//! layer index 0 (`F.Cu`) of the board, wherever the board's layers have no
//! cost yet. `RulesReader.read` then puts the same rule on the right layer.
//! `tests/job_prepare_reference.rs` compares the result with Java's dumps,
//! which run the real second merge, on every fixture and scenario.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use super::overrides::{self, OverrideState};
use super::{LogLevel, LogLine};
use crate::board::RoutingBoard;
use crate::dsn::ast::{Pcb, RulesFile};
use crate::io::kicad::reader::dru::{BoardFloors, DruReport, HoleClearance};
use crate::io::kicad::reader::project::BoardMinimums;
pub use crate::io::kicad::{
    DesignKind, KicadInput, KicadReadError, ReadOptions, design_kind, read_kicad_for_route,
    read_kicad_input, same_file,
};
use crate::io::specctra::{
    BoardReadResult, SesImportSummary, apply_rules_file, build_board_with, read_board_from_str_with,
};
use crate::quality::pour_guard;
use crate::settings::autoroute_scope::{patch_from_dsn, patch_from_rules};
use crate::settings::board::{
    BoardRouterSettings, apply_board_specific, apply_net_class_exclusions,
};
use crate::settings::{
    JavaQuirks, ResolveContext, RouterSettings, SettingsPatch, Source, defaults, resolve,
};

/// Which rules file a job uses (`RoutingJobScheduler.java:115-157`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum RulesChoice {
    /// No rules file, not even an adjacent one.
    None,
    /// A rules file given with `-dr` or as `-de a.dsn+b.rules`. If it does
    /// not exist nothing is read, and the adjacent file is **not** tried:
    /// Java's chain stops at the first branch whose setting is present.
    Explicit(PathBuf),
    /// `<file name up to its last '.'>.rules` in the DSN file's directory,
    /// if it exists. So `processor.Z80.dsn` looks for `processor.Z80.rules`,
    /// never for `processor.rules`.
    #[default]
    Adjacent,
}

/// What a job loads.
#[derive(Clone, Debug)]
pub struct JobInput {
    /// The bytes of the DSN file. Empty when [`pcb`](Self::pcb) is set.
    pub dsn: Vec<u8>,
    /// A design that was read already, in place of `dsn`: the tree of a
    /// `.kicad_pcb` (K07.2, [`crate::io::kicad::read_kicad_input`]). The
    /// settings come from its `autoroute_settings` (none for a KiCad tree)
    /// like those of a parsed DSN.
    pub pcb: Option<Pcb>,
    /// Log lines to put in front of the job's own: the reader's warnings and
    /// report for a design read by the caller.
    pub read_log: Vec<LogLine>,
    /// The design name: the file name without its last extension.
    pub design_name: String,
    /// The directory of the DSN file, where [`RulesChoice::Adjacent`] looks.
    pub dsn_dir: Option<PathBuf>,
    /// A session file whose wires and vias are imported last.
    pub ses: Option<Vec<u8>>,
    /// The file name of the session, for the log.
    pub ses_name: Option<String>,
    pub rules: RulesChoice,
    /// The patch of the config file (`freerouting.json`).
    pub config_file: Option<SettingsPatch>,
    /// The patch of the environment variables.
    pub environment: Option<SettingsPatch>,
    /// The patch of the command line.
    pub cli: Option<SettingsPatch>,
    /// Further sources, for example `(Source::Testing, java_fixture_test())`.
    pub extra_sources: Vec<(Source, SettingsPatch)>,
    /// The `.kicad_pcb` the design was read from ([`set_kicad`](Self::set_kicad)),
    /// for a `.kicad_pcb` output (K08).
    pub kicad_source: Option<crate::io::kicad::KicadSource>,
    /// Q22.5: a board's `.kicad_dru` for a DSN input (`--kicad-dru`): its
    /// path, for the log, and its text. [`prepare_job`] applies its rules to
    /// the parsed DSN before the board is built.
    pub kicad_dru: Option<(PathBuf, String)>,
    /// Q22.5: the `hole_clearance` floor of the `.kicad_dru` rules of a
    /// `.kicad_pcb` ([`set_kicad`](Self::set_kicad)). [`prepare_job`] raises
    /// `router.hole_clearance_um` to it after every merge.
    pub rules_hole_clearance: Option<HoleClearance>,
    /// Q22.14: the `hole_to_hole` floor of a `.kicad_pcb`'s `.kicad_dru`
    /// rules; raises `router.board_minimums.hole_to_hole_um` the same way.
    pub rules_hole_to_hole: Option<HoleClearance>,
    /// K14.1: the classes whose width an applied `.kicad_dru` `track_width` rule
    /// of a `.kicad_pcb` set, with the rule ([`set_kicad`](Self::set_kicad)):
    /// `router.input_fixups.class_widths` keeps them at the rule's width.
    pub rules_track_widths: Vec<(String, String)>,
    /// K15.3: the board-wide floors of a `.kicad_pcb`'s `.kicad_dru` rules
    /// (track width, clearance, via drill and annular ring, edge clearance);
    /// [`prepare_job`] raises the matching settings to them after every merge.
    pub rules_floors: BoardFloors,
    /// K15.11: the KiCad rules the DRC report counts with (the unconditional
    /// `.kicad_dru` clearance, KiCad's hole clearance, the pads' holes) of a
    /// `.kicad_pcb` ([`set_kicad`](Self::set_kicad)); a `--kicad-dru` adds its
    /// rules. [`prepare_job`] puts them on the board; nothing routes by them.
    pub kicad_report: crate::drc::KicadReportInputs,
}

impl JobInput {
    /// An input with only the DSN: no session, the adjacent rules file, the
    /// default settings.
    pub fn new(dsn: Vec<u8>, design_name: impl Into<String>) -> JobInput {
        JobInput {
            dsn,
            pcb: None,
            read_log: Vec::new(),
            design_name: design_name.into(),
            dsn_dir: None,
            ses: None,
            ses_name: None,
            rules: RulesChoice::Adjacent,
            config_file: None,
            environment: None,
            cli: None,
            extra_sources: Vec::new(),
            kicad_source: None,
            kicad_dru: None,
            rules_hole_clearance: None,
            rules_hole_to_hole: None,
            rules_track_widths: Vec::new(),
            rules_floors: BoardFloors::default(),
            kicad_report: crate::drc::KicadReportInputs::default(),
        }
    }

    /// Uses a `.kicad_pcb` that the caller read ([`read_kicad_input`], K07.2)
    /// in place of the DSN bytes: its tree is built into the board, and its
    /// warnings (each with its board line, `wire_keepout` areas included),
    /// then the read report and the `.kicad_dru` outcome, lead the job's log
    /// as warning and info lines.
    pub fn set_kicad(&mut self, read: KicadInput) {
        let sources = KicadSources::from_read(&read);
        self.read_log = sources.read_log;
        self.rules_hole_clearance = sources.hole_clearance;
        self.rules_hole_to_hole = sources.hole_to_hole;
        self.rules_track_widths = sources.track_widths;
        self.rules_floors = sources.floors;
        self.kicad_report = sources.kicad_report;
        self.extra_sources.extend(sources.extra_sources);
        self.kicad_source = Some(read.source());
        self.pcb = Some(read.pcb);
    }

    /// The design name of a file name: `getFilenameWithoutExtension`, the
    /// name up to its last `.` (all of it without a `.`).
    pub fn design_name_of(file_name: &str) -> String {
        match file_name.rfind('.') {
            Some(index) => file_name[..index].to_string(),
            None => file_name.to_string(),
        }
    }
}

/// K15.10: what a read `.kicad_pcb` adds to a job besides its tree: the log
/// lines of the read, the settings sources of Board Setup's minimums and the
/// pads' solder-mask apertures, and the floors of its `.kicad_dru` rules.
/// [`JobInput::set_kicad`] puts them in a [`JobInput`] (the CLI, the plugin);
/// the desktop app keeps them with the open board and merges them with
/// [`prepare_job_settings_with`], so both route the same board with the same
/// settings and sources.
#[derive(Clone, Debug, Default)]
pub struct KicadSources {
    /// The reader's warnings, its report and the `.kicad_dru` rules' warnings,
    /// in that order.
    pub read_log: Vec<LogLine>,
    /// Board Setup's minimums ([`BOARD_MINIMUMS_SOURCE`]) and the pads' mask
    /// apertures ([`SOLDER_MASK_SOURCE`]), at the DSN's priority.
    pub extra_sources: Vec<(Source, SettingsPatch)>,
    /// The `.kicad_dru` `hole_clearance` floor ([`JobInput::rules_hole_clearance`]).
    pub hole_clearance: Option<HoleClearance>,
    /// The `.kicad_dru` `hole_to_hole` floor ([`JobInput::rules_hole_to_hole`]).
    pub hole_to_hole: Option<HoleClearance>,
    /// The classes a `.kicad_dru` `track_width` rule set ([`JobInput::rules_track_widths`]).
    pub track_widths: Vec<(String, String)>,
    /// The board-wide floors of the `.kicad_dru` rules ([`JobInput::rules_floors`]).
    pub floors: BoardFloors,
    /// K15.11: the KiCad rules the DRC report counts with ([`JobInput::kicad_report`]).
    pub kicad_report: crate::drc::KicadReportInputs,
}

impl KicadSources {
    /// The sources of `read`, as [`JobInput::set_kicad`] takes them.
    pub fn from_read(read: &KicadInput) -> KicadSources {
        let warnings = read.warnings.iter().map(|w| LogLine {
            level: LogLevel::Warn,
            text: format!("KiCad reader: {w}"),
        });
        let report = read.report_lines().into_iter().map(|text| LogLine {
            level: LogLevel::Info,
            text,
        });
        let rule_warnings = read.rules.warnings.iter().map(|text| LogLine {
            level: LogLevel::Warn,
            text: text.clone(),
        });
        let mut extra_sources = Vec::new();
        // Q18: the `.kicad_pro`'s Board Setup minimums are hard floors, at
        // the DSN's priority: a config file sets them below, the command
        // line over them.
        if let Some(patch) = board_minimums_patch(&read.extras.board_minimums) {
            extra_sources.push((
                Source::Custom {
                    name: BOARD_MINIMUMS_SOURCE.to_owned(),
                    priority: Source::Dsn.priority(),
                },
                patch,
            ));
        }
        // Q18.7: the pads' solder-mask apertures, at the same priority.
        if let Some(patch) = solder_mask_patch(&read.extras.solder_mask) {
            extra_sources.push((
                Source::Custom {
                    name: SOLDER_MASK_SOURCE.to_owned(),
                    priority: Source::Dsn.priority(),
                },
                patch,
            ));
        }
        KicadSources {
            read_log: warnings.chain(report).chain(rule_warnings).collect(),
            extra_sources,
            hole_clearance: read.rules.hole_clearance.clone(),
            hole_to_hole: read.rules.hole_to_hole.clone(),
            track_widths: read.rules.track_widths.clone(),
            floors: read.rules.floors.clone(),
            kicad_report: crate::drc::KicadReportInputs::of_kicad(read),
        }
    }

    /// Raises the settings of `patch` to the `.kicad_dru` floors, as
    /// [`prepare_job`] does after every merge: the larger of the rule and the
    /// setting wins, the command line's value (a dialog's, in the app)
    /// included. The outcome goes to `log` when it is given.
    pub fn raise_floors(&self, patch: &mut SettingsPatch, mut log: Option<&mut Vec<LogLine>>) {
        raise_hole_clearance(patch, self.hole_clearance.as_ref(), log.as_deref_mut());
        raise_hole_to_hole(patch, self.hole_to_hole.as_ref(), log.as_deref_mut());
        raise_board_floors(patch, &self.floors, log);
    }
}

/// K14: `router.input_fixups.*` ([`crate::io::kicad::reader::fixups`]) onto the
/// tree of a `.kicad_pcb` input ([`JobInput::kicad_source`]), with one info line per
/// change. An unknown class or layer, or a layer that is not a plane, is a
/// [`JobError::Settings`] naming it; an unknown net is a warning. A class the board's
/// `.kicad_dru` `track_width` rule set keeps the rule's width
/// ([`JobInput::rules_track_widths`]), as on the DSN path.
/// A DSN input has no reader's tree: the settings are ignored there, with one warning
/// naming them (KiCad's DSN export takes the plugin's text fix-ups instead).
fn apply_input_fixups(
    input: &JobInput,
    patch: &SettingsPatch,
    pcb: Option<&mut Cow<'_, Pcb>>,
    log: &mut Vec<LogLine>,
) -> Result<(), JobError> {
    use crate::io::kicad::reader::fixups::{InputFixups, SECTION, apply_with_fills};
    let section = &patch.input_fixups;
    let list = |v: &Option<Vec<String>>| v.clone().unwrap_or_default();
    let fixups = InputFixups::from_lists(
        &list(&section.class_widths),
        &list(&section.route_on_plane_layers),
        &list(&section.drop_pour_nets),
    )
    .map_err(|e| JobError::Settings(e.to_string()))?
    .with_fixed_pour_classes(&list(&section.fixed_pour_classes));
    if fixups.is_empty() {
        return Ok(());
    }
    match (&input.kicad_source, pcb) {
        (Some(source), Some(tree)) => {
            // K14.4: the fills are read from the file again, for this setting only
            let fills = if fixups.fixed_pour_classes.is_empty() {
                Vec::new()
            } else {
                crate::io::kicad::read_pour_fills(source)
                    .map_err(|e| JobError::Settings(format!("{SECTION}.fixed_pour_classes: {e}")))?
            };
            let lines = apply_with_fills(tree.to_mut(), &fixups, &input.rules_track_widths, &fills)
                .map_err(|e| JobError::Settings(e.to_string()))?;
            for line in lines {
                let level = if line.starts_with("WARNING:") {
                    LogLevel::Warn
                } else {
                    LogLevel::Info
                };
                push(log, level, format!("input fix-up: {line}"));
            }
        }
        _ => {
            let names: Vec<String> = [
                ("class_widths", !fixups.class_widths.is_empty()),
                (
                    "route_on_plane_layers",
                    !fixups.route_on_plane_layers.is_empty(),
                ),
                ("drop_pour_nets", !fixups.drop_pour_nets.is_empty()),
                ("fixed_pour_classes", !fixups.fixed_pour_classes.is_empty()),
            ]
            .into_iter()
            .filter(|(_, on)| *on)
            .map(|(name, _)| format!("{SECTION}.{name}"))
            .collect();
            push(
                log,
                LogLevel::Warn,
                format!(
                    "{} only apply to a .kicad_pcb input; ignored for this DSN input",
                    names.join(", ")
                ),
            );
        }
    }
    Ok(())
}

/// The name of the settings source of a `.kicad_pro`'s Board Setup minimums.
pub const BOARD_MINIMUMS_SOURCE: &str = "kicad_pro board minimums";

/// K15.26: `router.cleanup.pour_min_thickness_um` and `pour_clearance_um` in
/// `board`'s units (0 when unset, negative or not finite); K15.29:
/// `pour_island_removal` and the board's nets named in `pour_islands_kept_nets`;
/// K15.33: the nets' own thicknesses of `pour_min_thickness_nets`, with a
/// warning for each entry left out (not `<µm>:<net>`, a value not positive,
/// or no such net: that net keeps `pour_min_thickness_um`).
fn dsn_pour_rules(
    patch: &SettingsPatch,
    board: &crate::board::BasicBoard,
) -> (crate::drc::DsnPourRules, Vec<String>) {
    let to_board = |um: Option<f64>| match um {
        Some(um) if um.is_finite() && um > 0.0 => crate::board::structure::Unit::scale(
            um * f64::from(board.communication.resolution.max(1)),
            crate::board::structure::Unit::Um,
            board.communication.unit,
        ),
        _ => 0.0,
    };
    // K15.29: the kept nets by name, every subnet of each name.
    let islands_kept_nets = patch
        .cleanup
        .pour_islands_kept_nets
        .iter()
        .flatten()
        .flat_map(|name| board.rules.nets.get_all_by_name(name))
        .map(|net| net.net_number)
        .collect();
    let mut warnings = Vec::new();
    let mut min_thickness_by_net = std::collections::BTreeMap::new();
    for entry in patch.cleanup.pour_min_thickness_nets.iter().flatten() {
        let parsed = entry
            .split_once(':')
            .and_then(|(um, name)| Some((um.trim().parse::<f64>().ok()?, name)));
        let Some((um, name)) = parsed.filter(|(um, _)| um.is_finite() && *um > 0.0) else {
            warnings.push(format!(
                "Ignoring router.cleanup.pour_min_thickness_nets entry {entry:?}: not <um>:<net> with a positive value"
            ));
            continue;
        };
        let nets = board.rules.nets.get_all_by_name(name);
        if nets.is_empty() {
            warnings.push(format!(
                "Ignoring router.cleanup.pour_min_thickness_nets entry {entry:?}: no net {name:?}"
            ));
        }
        for net in nets {
            min_thickness_by_net.insert(net.net_number, to_board(Some(um)));
        }
    }
    let rules = crate::drc::DsnPourRules {
        min_thickness: to_board(patch.cleanup.pour_min_thickness_um),
        clearance: to_board(patch.cleanup.pour_clearance_um),
        island_removal: patch.cleanup.pour_island_removal == Some(true),
        islands_kept_nets,
        min_thickness_by_net,
    };
    (rules, warnings)
}

/// Q18: the settings of a `.kicad_pro`'s Board Setup minimums
/// (`router.board_minimums.*`, `router.copper_to_edge_clearance_um` and
/// `router.hole_clearance_um`), or `None` when the file gives none.
pub fn board_minimums_patch(minimums: &BoardMinimums) -> Option<SettingsPatch> {
    let um = |nm: Option<i32>| nm.map(|nm| f64::from(nm) / 1000.0);
    let mut patch = SettingsPatch::default();
    patch.board_minimums.track_width_um = um(minimums.track_width);
    patch.board_minimums.via_diameter_um = um(minimums.via_diameter);
    patch.board_minimums.via_drill_um = um(minimums.through_drill);
    patch.board_minimums.via_annular_um = um(minimums.via_annular);
    patch.board_minimums.clearance_um = um(minimums.clearance);
    patch.board_minimums.hole_to_hole_um = um(minimums.hole_to_hole);
    patch.copper_to_edge_clearance_um = um(minimums.copper_edge_clearance);
    patch.hole_clearance_um = um(minimums.hole_clearance);
    (*minimums != BoardMinimums::NONE).then_some(patch)
}

/// The name of the settings source of a `.kicad_pcb`'s pad mask apertures.
pub const SOLDER_MASK_SOURCE: &str = "kicad_pcb solder mask";

/// Q18.7: `router.solder_mask_expansions` of the reader's pads, or `None`
/// when no pad's aperture matters.
pub fn solder_mask_patch(
    pads: &[freerouting_kicad::mask::PadMaskExpansion],
) -> Option<SettingsPatch> {
    if pads.is_empty() {
        return None;
    }
    let list: Vec<_> = pads
        .iter()
        .map(|p| super::solder_mask::MaskExpansion {
            component: p.component.clone(),
            pin: p.pin.clone(),
            front_um: f64::from(p.front_nm) / 1000.0,
            back_um: f64::from(p.back_nm) / 1000.0,
        })
        .collect();
    Some(SettingsPatch {
        solder_mask_expansions: Some(super::solder_mask::format_expansions(&list)),
        ..SettingsPatch::default()
    })
}

/// Why a job could not be prepared. The text is Java's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobError {
    /// `There was a parse error while reading board file at '<loc>': <detail>`.
    Parse { location: String, detail: String },
    /// The DSN file has no usable board outline, so no board was built.
    NoBoard,
    /// A setting does not fit the board (Q01: an unknown net class in
    /// `router.plane_obstacle_net_classes`).
    Settings(String),
}

impl fmt::Display for JobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JobError::Parse { location, detail } => write!(
                f,
                "There was a parse error while reading board file at '{location}': {detail}"
            ),
            JobError::NoBoard => f.write_str("The board file has no usable outline"),
            JobError::Settings(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for JobError {}

/// A board ready to route, with the settings that apply to it.
#[derive(Debug)]
pub struct PreparedJob {
    pub board: RoutingBoard,
    /// The final settings with Java's nulls (the tuned `layers` and cost
    /// arrays included).
    pub patch: SettingsPatch,
    /// The final settings read through Java's getter fallbacks.
    pub settings: RouterSettings,
    /// The router's read-only view of `settings`.
    pub board_settings: BoardRouterSettings,
    /// For each setting, the last source that set it.
    pub provenance: BTreeMap<&'static str, Source>,
    /// Java's log lines of the load, in order.
    pub log: Vec<LogLine>,
    /// The result of the session import, if a session was given.
    pub ses_summary: Option<SesImportSummary>,
    /// The rules file that was read, if any.
    pub rules_path: Option<PathBuf>,
}

fn push(log: &mut Vec<LogLine>, level: LogLevel, text: impl Into<String>) {
    log.push(LogLine {
        level,
        text: text.into(),
    });
}

/// The rules file of the job, if one is to be read (Java's chain in
/// `RoutingJobScheduler`: the explicit file if one is named and exists,
/// else, for a DSN input, the adjacent file).
pub fn find_rules_file(input: &JobInput) -> Option<PathBuf> {
    match &input.rules {
        RulesChoice::None => None,
        RulesChoice::Explicit(path) => path.exists().then(|| path.clone()),
        RulesChoice::Adjacent => {
            let adjacent = input
                .dsn_dir
                .as_ref()?
                .join(format!("{}.rules", input.design_name));
            adjacent.exists().then_some(adjacent)
        }
    }
}

/// What step 5 read from the rules file: its text parsed, or nothing.
fn read_rules_file(path: &std::path::Path, log: &mut Vec<LogLine>) -> Option<RulesFile> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) => {
            push(
                log,
                LogLevel::Warn,
                format!("Failed to read rules file: {}: {e}", path.display()),
            );
            return None;
        }
    };
    match RulesFile::parse_with_warnings(&freerouting_dsn::decode(&bytes)) {
        Ok((rules, warnings)) => {
            for warning in warnings {
                push(log, LogLevel::Warn, warning.to_string());
            }
            Some(rules)
        }
        Err(e) => {
            // `RulesReader.read` returns false after logging; the board and
            // the settings are left as they are.
            push(
                log,
                LogLevel::Warn,
                format!("RulesReader.read: {e} in '{}'", path.display()),
            );
            None
        }
    }
}

/// Loads a job the way Java does before it routes. See the module docs for
/// the sequence.
pub fn prepare_job(input: &JobInput, ctx: &ResolveContext) -> Result<PreparedJob, JobError> {
    let mut log = input.read_log.clone();
    let text = freerouting_dsn::decode(&input.dsn);
    let mut pcb: Option<Cow<'_, Pcb>> = match &input.pcb {
        Some(pcb) => Some(Cow::Borrowed(pcb)),
        None => Pcb::parse(&text).ok().map(Cow::Owned),
    };
    // Q22.5: the `.kicad_dru` of a DSN input, onto the tree.
    let mut hole_floor = input.rules_hole_clearance.clone();
    let mut hole_to_hole_floor = input.rules_hole_to_hole.clone();
    let mut board_floors = input.rules_floors.clone();
    let mut kicad_report = input.kicad_report.clone();
    if let (Some((path, rules)), Some(tree)) = (&input.kicad_dru, pcb.as_mut())
        && let Some(report) = apply_kicad_dru(tree.to_mut(), path, rules, &mut log)
    {
        hole_floor = report.hole_clearance.clone().or(hole_floor);
        hole_to_hole_floor = report.hole_to_hole.clone().or(hole_to_hole_floor);
        if !report.floors.is_empty() {
            board_floors = report.floors.clone();
        }
        // K15.11: the report counts with the rules' clearance and hole
        // clearance (the DSN gives no pad holes).
        kicad_report.apply_rules(&report, None);
    }

    // 1. The first merge.
    let mut sources = vec![(Source::Defaults, defaults(ctx))];
    if let Some(patch) = &input.config_file {
        sources.push((Source::ConfigFile, patch.clone()));
    }
    if let Some(pcb) = &pcb {
        sources.push((Source::Dsn, patch_from_dsn(pcb)));
    }
    if let Some(patch) = &input.environment {
        sources.push((Source::Environment, patch.clone()));
    }
    if let Some(patch) = &input.cli {
        sources.push((Source::Cli, patch.clone()));
    }
    sources.extend(input.extra_sources.iter().cloned());
    let merged = resolve(&sources, ctx);
    // N01: an unknown `router.profile` is a settings error.
    if let Err(e) = &merged.profile {
        return Err(JobError::Settings(e.to_string()));
    }
    for warning in &merged.warnings {
        push(&mut log, LogLevel::Warn, warning.clone());
    }
    let mut provenance = merged.provenance;
    let mut patch = merged.patch;
    raise_hole_clearance(&mut patch, hole_floor.as_ref(), Some(&mut log));
    raise_hole_to_hole(&mut patch, hole_to_hole_floor.as_ref(), Some(&mut log));
    raise_board_floors(&mut patch, &board_floors, Some(&mut log));
    // K14: the plugin's DSN fix-ups, on the tree of a `.kicad_pcb` input.
    apply_input_fixups(input, &patch, pcb.as_mut(), &mut log)?;

    // 2. Read the board, with the readers' `router.java_quirks.*` (N06) of the
    // first merge.
    let read_quirks = JavaQuirks::from_patch(&patch.java_quirks);
    let result = match &pcb {
        Some(pcb) => build_board_with(pcb, read_quirks),
        None => read_board_from_str_with(&text, read_quirks),
    };
    let (mut board, warnings) = match result {
        BoardReadResult::Success { board, warnings }
        | BoardReadResult::OutlineMissing {
            board: Some(board),
            warnings,
        } => (*board, warnings),
        BoardReadResult::OutlineMissing { board: None, .. } => return Err(JobError::NoBoard),
        BoardReadResult::ParseError { location, detail } => {
            return Err(JobError::Parse { location, detail });
        }
    };
    for warning in warnings {
        push(&mut log, LogLevel::Warn, warning);
    }
    // K15.19: a `.kicad_pcb` pin's real drilled hole, before the hole
    // clearance override builds the tree shapes with it.
    kicad_report.set_pad_holes(&mut board);

    // 3. Tuning, exclusions and the five overrides, on the unexpanded box.
    if patch.layer_count() != board.layer_count() {
        patch.set_layer_count(board.layer_count());
    }
    apply_board_specific(&mut patch, &board);
    for warning in apply_net_class_exclusions(&patch, &mut board) {
        push(&mut log, LogLevel::Warn, warning);
    }
    let mut state = OverrideState::default();
    let settings = RouterSettings::from_patch(&patch, ctx.cores);
    log.extend(overrides::apply_all(&mut board, &settings, &mut state));

    // 4. Expand the bounding box and reduce the nets. K15.1: an item with an
    // unbounded box is left out of the board box, with a warning.
    for warning in board.expand_bounding_box_to_include_all_items() {
        push(&mut log, LogLevel::Warn, warning);
    }
    board.reduce_nets_of_route_items();

    // 5. The rules file: the second merge, the rules onto the board, their
    // `autoroute_settings` on top of everything.
    let rules_path = find_rules_file(input);
    if let Some(path) = &rules_path
        && let Some(rules) = read_rules_file(path, &mut log)
    {
        let mut second = sources.clone();
        second.push((Source::RulesFile, patch_from_rules(&rules)));
        second.push((Source::Api, patch.clone()));
        let merged = resolve(&second, ctx);
        provenance = merged.provenance;
        patch = merged.patch;
        raise_hole_clearance(&mut patch, hole_floor.as_ref(), None);
        raise_hole_to_hole(&mut patch, hole_to_hole_floor.as_ref(), None);
        raise_board_floors(&mut patch, &board_floors, None);
        let (rules_board, outcome) = apply_rules_file(board, &rules, &input.design_name);
        board = rules_board;
        log.extend(outcome.log);
        if let Some(autoroute) = &outcome.autoroute {
            patch.apply_from(autoroute);
        }
    }

    // 6. The tuning and the exclusions again (the tuning is a no-op unless the
    // rules file reset the flag).
    apply_board_specific(&mut patch, &board);
    for warning in apply_net_class_exclusions(&patch, &mut board) {
        push(&mut log, LogLevel::Warn, warning);
    }

    // N06: the final `router.java_quirks.*` go on the board before the session
    // import, the rules having been read with the first merge's.
    board.set_java_quirks(JavaQuirks::from_patch(&patch.java_quirks));
    // K15.11: the KiCad rules of the DRC report (no routing code reads them).
    let mut report_rules = kicad_report.on_board(&board);
    // K15.26: what a DSN input's pours don't carry, for the stub cleanup.
    let (dsn_pours, pour_warnings) = dsn_pour_rules(&patch, &board);
    report_rules.dsn_pours = dsn_pours;
    for warning in pour_warnings {
        push(&mut log, LogLevel::Warn, warning);
    }
    board.set_kicad_report_rules(report_rules);

    // 7. The session.
    let mut ses_summary = None;
    if let Some(ses) = &input.ses {
        let name = input.ses_name.as_deref().unwrap_or("");
        push(
            &mut log,
            LogLevel::Info,
            format!("Loading SES session data: {name}"),
        );
        match crate::io::read_session(&mut board, &freerouting_dsn::decode(ses)) {
            Ok(summary) => {
                let errors = if summary.errors_encountered > 0 {
                    format!(" ({} errors)", summary.errors_encountered)
                } else {
                    String::new()
                };
                push(
                    &mut log,
                    LogLevel::Info,
                    format!(
                        "SES session loaded: {} wires, {} vias imported{errors}",
                        summary.wires_imported, summary.vias_imported
                    ),
                );
                ses_summary = Some(summary);
            }
            Err(e) => push(
                &mut log,
                LogLevel::Error,
                format!("Failed to load session data: {e}"),
            ),
        }
    }

    // Q02.2: strict sizes need the `widths` report for their exit status.
    let settings = crate::quality::neckdown::ledger::with_strict_report(
        RouterSettings::from_patch(&patch, ctx.cores),
    );
    // Q13: `router.power_layer_traces`; an unknown value fails the job.
    crate::autoroute::power_layer_traces::validate(&settings).map_err(JobError::Settings)?;
    // Q09.3: `router.wire_keepout`; Java's reading unless `traces_only`.
    let wire = crate::io::specctra::wire_keepout::apply(&mut board, &settings)
        .map_err(JobError::Settings)?;
    log.extend(wire);
    // N04: `router.pad_contacts`; an unknown value fails the job.
    let contacts =
        crate::board::contact_rule::apply(&mut board, &settings).map_err(JobError::Settings)?;
    log.extend(contacts);
    // K15.25: `router.pour_fill_target`.
    board.set_pour_fill_target(settings.pour_fill_target == Some(true));
    // K15.31: `router.pour_fill_contacts`.
    board.set_pour_fill_contacts(settings.pour_fill_contacts == Some(true));
    // Q14: `router.via_clearance`; an unknown value fails the job.
    crate::autoroute::via_clearance::validate(&settings).map_err(JobError::Settings)?;
    // N03.1: `router.room_order` and `router.work_unit`; an unknown value
    // fails the job.
    crate::board::search::SearchConfig::from_settings(&settings).map_err(JobError::Settings)?;
    // 8. The pour guard (Q01), on the board as loaded; off unless
    // `router.plane_obstacle_net_classes` lists a class.
    let guard = pour_guard::install(&mut board, &settings)
        .map_err(|e| JobError::Settings(e.to_string()))?;
    log.extend(guard);
    record_pre_existing_violations(&mut board);
    let board_settings = BoardRouterSettings::from_settings(&settings);
    Ok(PreparedJob {
        board,
        patch,
        settings,
        board_settings,
        provenance,
        log,
        ses_summary,
        rules_path,
    })
}

/// Q22.5: applies a `.kicad_dru` to a DSN tree; the report goes to `log`.
/// Returns the report (its `hole_clearance`, Q22.14 `hole_to_hole` and K15.3
/// floors), or `None` if the file could not be read.
fn apply_kicad_dru(
    pcb: &mut Pcb,
    path: &std::path::Path,
    text: &str,
    log: &mut Vec<LogLine>,
) -> Option<DruReport> {
    push(
        log,
        LogLevel::Info,
        format!("KiCad design rules file: {}", path.display()),
    );
    match freerouting_kicad::dru::apply_to_pcb(pcb, text) {
        Ok(report) => {
            for line in report.lines() {
                push(log, LogLevel::Info, line);
            }
            for warning in &report.warnings {
                push(log, LogLevel::Warn, warning.clone());
            }
            Some(report)
        }
        Err(e) => {
            push(
                log,
                LogLevel::Warn,
                format!("the .kicad_dru file could not be read: {e}"),
            );
            None
        }
    }
}

/// Q22.5: raises `router.hole_clearance_um` to the `.kicad_dru` floor, after
/// every source (the command line's value included: a board rule is a
/// floor). Logs the outcome when `log` is given.
fn raise_hole_clearance(
    patch: &mut SettingsPatch,
    floor: Option<&HoleClearance>,
    log: Option<&mut Vec<LogLine>>,
) {
    let setting = "router.hole_clearance_um";
    raise_floor(&mut patch.hole_clearance_um, setting, floor, log);
}

/// Q22.14: raises `router.board_minimums.hole_to_hole_um` (Board Setup's
/// `min_hole_to_hole`, Q22.11) to the `.kicad_dru` floor the same way: the
/// larger wins.
fn raise_hole_to_hole(
    patch: &mut SettingsPatch,
    floor: Option<&HoleClearance>,
    log: Option<&mut Vec<LogLine>>,
) {
    let setting = "router.board_minimums.hole_to_hole_um";
    raise_floor(
        &mut patch.board_minimums.hole_to_hole_um,
        setting,
        floor,
        log,
    );
}

/// K15.3: raises the settings of the board-wide `.kicad_dru` floors
/// ([`BoardFloors`]) the same way: the larger of the rule and the setting wins,
/// the command line's value included.
fn raise_board_floors(
    patch: &mut SettingsPatch,
    floors: &BoardFloors,
    mut log: Option<&mut Vec<LogLine>>,
) {
    let minimums = &mut patch.board_minimums;
    let list: [(&mut Option<f64>, &str, &Option<HoleClearance>); 5] = [
        (
            &mut minimums.track_width_um,
            "router.board_minimums.track_width_um",
            &floors.track_width,
        ),
        (
            &mut minimums.clearance_um,
            "router.board_minimums.clearance_um",
            &floors.clearance,
        ),
        (
            &mut minimums.via_drill_um,
            "router.board_minimums.via_drill_um",
            &floors.via_drill,
        ),
        (
            &mut minimums.via_annular_um,
            "router.board_minimums.via_annular_um",
            &floors.via_annular,
        ),
        (
            &mut patch.copper_to_edge_clearance_um,
            "router.copper_to_edge_clearance_um",
            &floors.edge_clearance,
        ),
    ];
    for (value, setting, floor) in list {
        raise_floor(value, setting, floor.as_ref(), log.as_deref_mut());
    }
}

fn raise_floor(
    value: &mut Option<f64>,
    setting: &str,
    floor: Option<&HoleClearance>,
    log: Option<&mut Vec<LogLine>>,
) {
    let Some(floor) = floor else {
        return;
    };
    let um = floor.nm as f64 / 1000.0;
    let current = value.unwrap_or(0.0);
    let names = floor
        .rules
        .iter()
        .map(|r| format!("'{r}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let raised = um > current;
    if raised {
        *value = Some(um);
    }
    if let Some(log) = log {
        let (level, text) = if raised {
            (
                LogLevel::Warn,
                format!("{setting} raised from {current} to {um} by the .kicad_dru rules {names}"),
            )
        } else {
            (
                LogLevel::Info,
                format!(
                    "{setting} is {current}, no less than the .kicad_dru rules {names} ask ({um})"
                ),
            )
        };
        push(log, level, text);
    }
}

/// Records the board's clearance violations as its pre-existing ones (Java
/// `preExistingClearanceViolationsCount`, set by the post-load DRC of
/// `HeadlessBoardManager`), so that the statistics count only the violations
/// beyond these as router introduced. See the module docs for when it runs.
pub fn record_pre_existing_violations(board: &mut RoutingBoard) {
    let count = crate::drc::DesignRulesChecker::new(board)
        .all_clearance_violations()
        .len();
    board.pre_existing_clearance_violations_count = count;
}

/// The first merge of [`prepare_job`] without a board: the defaults, the
/// design's `autoroute_settings` and `sources`, in that order, with the merge's
/// warnings. It is what a front-end needs to know the readers'
/// `router.java_quirks.*` ([`JavaQuirks::from_patch`] of the result's
/// `java_quirks`) before it reads the board, as `prepare_job` does.
pub fn first_merge(
    pcb: Option<&Pcb>,
    sources: &[(Source, SettingsPatch)],
    ctx: &ResolveContext,
) -> (SettingsPatch, Vec<LogLine>) {
    let (patch, _provenance, log) = merge_sources(pcb, sources, &[], ctx);
    (patch, log)
}

/// [`first_merge`] with `extra` after `sources`, and each setting's last
/// source.
fn merge_sources(
    pcb: Option<&Pcb>,
    sources: &[(Source, SettingsPatch)],
    extra: &[(Source, SettingsPatch)],
    ctx: &ResolveContext,
) -> (SettingsPatch, BTreeMap<&'static str, Source>, Vec<LogLine>) {
    let mut all = vec![(Source::Defaults, defaults(ctx))];
    if let Some(pcb) = pcb {
        all.push((Source::Dsn, patch_from_dsn(pcb)));
    }
    all.extend(sources.iter().cloned());
    all.extend(extra.iter().cloned());
    let merged = resolve(&all, ctx);
    let mut log = Vec::new();
    for warning in &merged.warnings {
        push(&mut log, LogLevel::Warn, warning.clone());
    }
    (merged.patch, merged.provenance, log)
}

/// The settings of a design before the user changes any (G07.1): the first
/// merge of [`prepare_job`] (defaults, the design's `autoroute_settings`
/// and `sources`) with the board-specific tuning of step 3, **on the board
/// as it is now**. Java tunes on the unexpanded bounding box, so call this
/// before [`prepare_board_for_routing`] has run on `board`.
///
/// The GUI shows these in its settings dialog and edits them; the dialog's
/// result goes to [`prepare_board_for_routing`]. The two calls together do
/// what [`prepare_job`] does for a board that has no rules file and no
/// session (`tests/ui_routing.rs::gui_and_cli_write_identical_ses` checks
/// that they agree with it). Returns the patch and the first merge's
/// warnings.
pub fn prepare_job_settings(
    board: &RoutingBoard,
    pcb: Option<&Pcb>,
    sources: &[(Source, SettingsPatch)],
    ctx: &ResolveContext,
) -> (SettingsPatch, Vec<LogLine>) {
    let (patch, _provenance, log) = prepare_job_settings_with(board, pcb, sources, None, ctx);
    (patch, log)
}

/// [`prepare_job_settings`] with the sources of a `.kicad_pcb` (K15.10): its
/// Board Setup minimums and mask apertures join the merge at the DSN's
/// priority, and its `.kicad_dru` floors raise the result, as in
/// [`prepare_job`] with [`JobInput::set_kicad`]. Also returns, for each
/// setting, the last source that set it, and the log lines of the floors
/// (the merge's warnings first).
pub fn prepare_job_settings_with(
    board: &RoutingBoard,
    pcb: Option<&Pcb>,
    sources: &[(Source, SettingsPatch)],
    kicad: Option<&KicadSources>,
    ctx: &ResolveContext,
) -> (SettingsPatch, BTreeMap<&'static str, Source>, Vec<LogLine>) {
    let extra = kicad.map_or(&[][..], |k| &k.extra_sources[..]);
    let (mut patch, provenance, mut log) = merge_sources(pcb, sources, extra, ctx);
    if let Some(kicad) = kicad {
        kicad.raise_floors(&mut patch, Some(&mut log));
    }
    if patch.layer_count() != board.layer_count() {
        patch.set_layer_count(board.layer_count());
    }
    apply_board_specific(&mut patch, board);
    (patch, provenance, log)
}

/// The router's view of a prepared job, from [`prepare_board_for_routing`].
#[derive(Clone, Debug)]
pub struct RoutingReady {
    /// `patch` read through Java's getter fallbacks.
    pub settings: RouterSettings,
    /// The router's read-only view of `settings`.
    pub board_settings: BoardRouterSettings,
    /// Java's log lines of the preparation.
    pub log: Vec<LogLine>,
}

/// The board half of [`prepare_job`]'s steps 3, 4 and 6 for a board that was
/// loaded already (G07.2): the net-class exclusions, the five overrides, the
/// bounding box expanded to all items and the nets of the route items
/// reduced, with the settings of `patch` (see [`prepare_job_settings`]; the
/// tuning is not repeated, because the patch carries `costs_initialized`).
pub fn prepare_board_for_routing(
    board: &mut RoutingBoard,
    patch: &SettingsPatch,
    ctx: &ResolveContext,
) -> RoutingReady {
    let mut log = Vec::new();
    for warning in apply_net_class_exclusions(patch, board) {
        push(&mut log, LogLevel::Warn, warning);
    }
    let mut state = OverrideState::default();
    let settings = RouterSettings::from_patch(patch, ctx.cores);
    // N06: `router.java_quirks.*` on the board, for the code that sees only it.
    board.set_java_quirks(JavaQuirks::from_settings(&settings));
    log.extend(overrides::apply_all(board, &settings, &mut state));
    for warning in board.expand_bounding_box_to_include_all_items() {
        push(&mut log, LogLevel::Warn, warning);
    }
    board.reduce_nets_of_route_items();
    for warning in apply_net_class_exclusions(patch, board) {
        push(&mut log, LogLevel::Warn, warning);
    }
    // Q13: `router.power_layer_traces`; an unknown value is logged and
    // reads as Java's rule.
    if let Err(e) = crate::autoroute::power_layer_traces::validate(&settings) {
        push(&mut log, LogLevel::Error, e);
    }
    // Q09.3: `router.wire_keepout`; an unknown value is logged and ignored.
    match crate::io::specctra::wire_keepout::apply(board, &settings) {
        Ok(line) => log.extend(line),
        Err(e) => push(&mut log, LogLevel::Error, e),
    }
    // N04: `router.pad_contacts`; an unknown value is logged and reads as
    // Java's rule.
    // K15.25: `router.pour_fill_target`.
    board.set_pour_fill_target(settings.pour_fill_target == Some(true));
    // K15.31: `router.pour_fill_contacts`.
    board.set_pour_fill_contacts(settings.pour_fill_contacts == Some(true));
    match crate::board::contact_rule::apply(board, &settings) {
        Ok(line) => log.extend(line),
        Err(e) => {
            board.set_contact_rule(Default::default());
            push(&mut log, LogLevel::Error, e);
        }
    }
    // Q14: `router.via_clearance`; an unknown value is logged and reads as
    // Java's rule.
    if let Err(e) = crate::autoroute::via_clearance::validate(&settings) {
        push(&mut log, LogLevel::Error, e);
    }
    // N03.1: `router.room_order` and `router.work_unit`; an unknown value is
    // logged here and the routing pipeline refuses it.
    if let Err(e) = crate::board::search::SearchConfig::from_settings(&settings) {
        push(&mut log, LogLevel::Error, e);
    }
    // The pour guard (Q01); an unknown class is logged and leaves it off.
    match pour_guard::install(board, &settings) {
        Ok(line) => log.extend(line),
        Err(e) => push(&mut log, LogLevel::Error, e.to_string()),
    }
    record_pre_existing_violations(board);
    let board_settings = BoardRouterSettings::from_settings(&settings);
    RoutingReady {
        settings,
        board_settings,
        log,
    }
}
