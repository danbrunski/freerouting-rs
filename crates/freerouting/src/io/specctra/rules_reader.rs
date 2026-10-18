//! Reading a Specctra `.rules` file onto a board (C04).
//!
//! Port of `io.specctra.RulesReader.read` and its `apply*` helpers (Java at
//! `cda540f`). `freerouting-dsn` reads the file into a [`RulesFile`]; the
//! scopes are then applied **in file order** through the functions the DSN
//! reader uses for the same things, so there is no second copy of the
//! clearance, via, padstack and net class logic:
//!
//! | Scope | Applied by |
//! |---|---|
//! | `(rule (width w))` | the default trace half width, `round(dsn_to_board(w) / 2)`, on every layer |
//! | `(rule (clear ...))` | `Structure.setClearanceRule` |
//! | `(layer name (rule ...))` | the same on one layer |
//! | `(padstack ...)` | `Library.readPadstackScope` |
//! | `(via ...)` | `Network.readViaInfo`; a via with an existing name replaces it and moves to the end |
//! | `(via_rule ...)` | `Network.addViaRule` |
//! | `(class ...)` | `Network.insertNetClass(..., viaAtSmdAllowed = false)` |
//! | `(snap_angle ...)` | the trace angle restriction |
//! | `(autoroute_settings ...)` | returned in [`RulesOutcome::autoroute`] for the caller to put on top of its settings |
//!
//! Java's quirks are kept, each pinned by a test in `tests/rules_reader.rs`:
//! - a `layer` scope naming an unknown layer applies to **all** layers
//!   (`applyRules` keeps `layerIndex = -1`);
//! - the clearance matrix is changed in place and the search trees are not
//!   rebuilt; disabling `java_quirks.rules_matrix_trees_not_rebuilt` refreshes
//!   every tree after applying the scopes;
//! - a header name that differs from the design name is only a warning;
//! - `autoroute_settings` are read against the board's layers, with every
//!   layer routable and `autoroute` and `postroute` on unless the scope says
//!   otherwise (`AutorouteSettings.readScope`), so applying them re-enables
//!   all layers and the optimizer.
//!
//! Where it differs from Java (docs/DIVERGENCES.md, "Rules files"): the file
//! is parsed completely before anything is applied, so a file with a syntax
//! error leaves the board untouched, where Java applies the scopes before the
//! error.

use freerouting_dsn::ast::{self, RulesFile, RulesItem};

use super::parser::layer::{DsnLayer, DsnLayerStructure};
use super::parser::library::read_padstack_scope;
use super::parser::network::{
    add_rules_via_rule, insert_rules_net_class, read_rules_via_info, replace_via_info,
};
use super::parser::structure::{set_clearance_rule, snap_angle};
use super::parser::{ReadContext, last_scope_identifier};
use crate::board::RoutingBoard;
use crate::job::{LogLevel, LogLine};
use crate::settings::SettingsPatch;
use crate::settings::autoroute_scope::read_scope;
use freerouting_geometry::jmath::round_to_i32;

/// What applying a rules file gave besides its effect on the board.
#[derive(Clone, Debug, Default)]
pub struct RulesOutcome {
    /// The settings of the file's `autoroute_settings` scopes, as one patch
    /// (Java's `AutorouteSettings.readScope` result): the caller applies it
    /// on top of its merged settings with `SettingsPatch::apply_from`. `None`
    /// when the file has no such scope.
    pub autoroute: Option<SettingsPatch>,
    /// Java's log lines of the read, in order.
    pub log: Vec<LogLine>,
}

/// Why a rules file was not applied. The board is unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RulesError(pub String);

impl std::fmt::Display for RulesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Failed to read the rules file: {}", self.0)
    }
}

impl std::error::Error for RulesError {}

/// The board's layers as the DSN reader's layer structure
/// (`new LayerStructure(board.layerStructure)`).
fn dsn_layers(board: &RoutingBoard) -> DsnLayerStructure {
    DsnLayerStructure::new(
        board
            .layer_structure
            .layers
            .iter()
            .enumerate()
            .map(|(i, layer)| DsnLayer::new(layer.name.clone(), i as i32, layer.is_signal, vec![]))
            .collect(),
    )
}

/// `RulesReader.applyRules`: the width and clearance rules of a `rule` scope
/// (`layer_name` is `None`) or of a `layer` scope.
fn apply_rules(ctx: &mut ReadContext, rules: &[ast::Rule], layer_name: Option<&str>) {
    let mut layer_index = -1;
    if let Some(name) = layer_name {
        layer_index = ctx.board().layer_structure.get_no(name);
        if layer_index < 0 {
            ctx.log(format!("RulesReader.applyRules: layer not found: '{name}'"));
        }
    }
    let transform = ctx.coordinate_transform;
    let quote = ctx.string_quote.clone();
    for rule in rules {
        match rule {
            ast::Rule::Width(width) => {
                let half_width = round_to_i32(transform.dsn_to_board(*width) / 2.0);
                let board_rules = &mut ctx.board().rules;
                if layer_index < 0 {
                    board_rules.set_default_trace_half_widths(half_width);
                } else {
                    board_rules.set_default_trace_half_width(layer_index as usize, half_width);
                }
            }
            ast::Rule::Clearance { value, types } => {
                // `set_clearance_rule` logs through a context of its own, so
                // that the board's rules can be borrowed beside it.
                let mut scratch = ReadContext::new();
                set_clearance_rule(
                    &mut scratch,
                    *value,
                    types,
                    layer_index,
                    &transform,
                    &mut ctx.board().rules,
                    &quote,
                );
                for message in scratch.log {
                    ctx.log(message);
                }
            }
            ast::Rule::Length(_) => {}
        }
    }
}

/// The `autoroute_settings` scopes of the file, read against the board's
/// layers and combined in file order into one patch.
pub fn autoroute_patch(rules: &RulesFile, layer_names: &[String]) -> Option<SettingsPatch> {
    let mut combined: Option<SettingsPatch> = None;
    for item in &rules.items {
        if let RulesItem::AutorouteSettings(scope) = item
            && let Some(patch) = read_scope(scope, layer_names)
        {
            combined
                .get_or_insert_with(SettingsPatch::default)
                .apply_from(&patch);
        }
    }
    combined
}

/// Applies a parsed rules file to `board` (`RulesReader.read`). The scopes
/// are applied in file order; the board is returned with the rules in place.
///
/// `design_name` is the name the header should carry; a different name gives
/// a warning and the read goes on.
pub fn apply_rules_file(
    board: RoutingBoard,
    rules: &RulesFile,
    design_name: &str,
) -> (RoutingBoard, RulesOutcome) {
    let layer_names: Vec<String> = board
        .layer_structure
        .layers
        .iter()
        .map(|l| l.name.clone())
        .collect();
    let old_matrix = (!board.java_quirks().rules_matrix_trees_not_rebuilt)
        .then(|| board.rules.clearance_matrix.clone());
    let log_start = board.log.len();
    let mut ctx = ReadContext::new();
    ctx.coordinate_transform = board.communication.coordinate_transform;
    ctx.string_quote = board
        .communication
        .specctra_parser_info
        .as_ref()
        .map_or_else(|| "\"".to_string(), |info| info.string_quote.clone());
    ctx.layer_structure = dsn_layers(&board);
    ctx.via_at_smd_allowed = false;
    ctx.board = Some(board);

    if rules.design != design_name {
        ctx.log(format!(
            "RulesReader.read: designName not matching at '{}' (expected '{design_name}', got '{}')",
            last_scope_identifier(),
            rules.design
        ));
    }

    for item in &rules.items {
        match item {
            RulesItem::Rule(list) => apply_rules(&mut ctx, list, None),
            RulesItem::Layer { name, rules } => apply_rules(&mut ctx, rules, Some(name)),
            RulesItem::Padstack(padstack) => {
                read_padstack_scope(&mut ctx, padstack);
            }
            RulesItem::Via(via) => {
                if let Some(info) = read_rules_via_info(&mut ctx, via) {
                    replace_via_info(ctx.board(), info);
                }
            }
            RulesItem::ViaRule(rule) => {
                add_rules_via_rule(ctx.board(), rule);
            }
            RulesItem::Class(class) => insert_rules_net_class(&mut ctx, class),
            RulesItem::SnapAngle(angle) => {
                ctx.board()
                    .rules
                    .set_trace_angle_restriction(snap_angle(*angle));
            }
            // Read below, in one patch.
            RulesItem::AutorouteSettings(_) => {}
        }
    }

    let mut board = ctx.board.take().expect("the board stays in the context");
    // PARITY(quirk, java_quirks.rules_matrix_trees_not_rebuilt): Java leaves
    // compensated shapes cached against the old clearance matrix. Without
    // compensation no shape depends on the matrix, so nothing is rebuilt.
    if let Some(old) = old_matrix.filter(|_| board.search_trees().is_clearance_compensation_used())
    {
        let matrix = &board.rules.clearance_matrix;
        let changed = old.class_count() != matrix.class_count()
            || (0..matrix.class_count() as i32).any(|a| {
                (0..matrix.class_count() as i32).any(|b| {
                    (0..matrix.layer_count() as i32).any(|layer| {
                        old.get_value(a, b, layer, false) != matrix.get_value(a, b, layer, false)
                    })
                })
            });
        if changed {
            board.reinsert_tree_items();
        }
    }
    let log = board.log[log_start..]
        .iter()
        .map(|text| LogLine {
            level: LogLevel::Warn,
            text: text.clone(),
        })
        .collect();
    let outcome = RulesOutcome {
        autoroute: autoroute_patch(rules, &layer_names),
        log,
    };
    (board, outcome)
}

/// Reads rules text and applies it to `board` (`RulesReader.read`). On a
/// syntax error the board is returned unchanged with the error (Java
/// returns `false`).
pub fn read_rules(
    board: RoutingBoard,
    text: &str,
    design_name: &str,
) -> (RoutingBoard, Result<RulesOutcome, RulesError>) {
    match RulesFile::parse_with_warnings(text) {
        Ok((rules, warnings)) => {
            let (mut board, mut outcome) = apply_rules_file(board, &rules, design_name);
            let mut log: Vec<LogLine> = warnings
                .iter()
                .map(|w| LogLine {
                    level: LogLevel::Warn,
                    text: w.to_string(),
                })
                .collect();
            log.append(&mut outcome.log);
            outcome.log = log;
            board.log.extend(
                outcome
                    .log
                    .iter()
                    .take(warnings.len())
                    .map(|l| l.text.clone()),
            );
            (board, Ok(outcome))
        }
        Err(e) => (board, Err(RulesError(e.to_string()))),
    }
}
