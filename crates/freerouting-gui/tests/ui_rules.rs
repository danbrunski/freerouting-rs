//! G06: the rules dialogs. Each test drives a dialog the way a user does
//! (clicks and typed text through egui_kittest) and reads the result back
//! from the engine board, so what is proved is that the dialog calls B08's
//! facade correctly. B08's own tests prove the facade keeps the search trees
//! right.
//!
//! `tests/data/rules_small.dsn` is a small board with one net class, one via
//! and one non-orthogonal trace; the avionics hub (Issue754) has the real
//! rules.

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::{SIZE, plain_harness};
use eframe::egui::accesskit::Role;
use eframe::egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use freerouting::board::BasicBoard;
use freerouting::board::structure::{AngleRestriction, Unit};
use freerouting::drc::DesignRulesChecker;
use freerouting::geometry::jmath::round_to_i32;
use freerouting::rules::edit::ClearanceLayer;
use freerouting::rules::{NetClassId, ViaInfoId};
use freerouting_gui::App;
use freerouting_gui::commands::Command;
use freerouting_gui::dialogs::{
    RuleView, RulesDialog, RulesEdit, assign_net_class, clearance_matrix, net_classes,
};

const ISSUE754: &str = "Issue754-avionics_hub.dsn";

/// Runs frames until the UI is still. A click needs a second frame to take
/// effect (focus changes ask for no repaint), so at least two are run; a
/// background worker that asks for a repaint right then must not fail the
/// test, so the step limit is not an error.
macro_rules! settle {
    ($h:expr) => {
        $h.run_steps(2);
        let _ = $h.run_ok();
    };
}

fn exec(h: &mut Harness<'_, App>, command: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().execute(&ctx, command);
    settle!(h);
}

fn edit(h: &mut Harness<'_, App>, e: RulesEdit) {
    exec(h, Command::EditRules(e));
}

fn open(h: &mut Harness<'_, App>, dialog: RulesDialog) {
    exec(h, Command::ShowRulesDialog(dialog));
    // A new window is laid out in a sizing pass before it takes input.
    h.run_steps(3);
}

fn board<'a>(h: &'a Harness<'_, App>) -> &'a BasicBoard {
    h.state()
        .loaded()
        .expect("board loaded")
        .session
        .board
        .basic()
}

fn notice(h: &Harness<'_, App>) -> Option<String> {
    h.state().notice().map(str::to_string)
}

/// Steps frames until the analysis delivered the results of the current state.
fn wait_for_analysis(h: &mut Harness<'_, App>) {
    let start = Instant::now();
    loop {
        h.step();
        let state = h.state();
        let loaded = state.loaded().expect("board loaded");
        if state.analysis().is_idle(&loaded.session) {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(300),
            "the analysis did not finish"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    settle!(h);
}

fn small() -> Harness<'static, App> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/rules_small.dsn");
    let mut h = Harness::builder()
        .with_size(SIZE)
        .build_eframe(|cc| App::new(cc, vec![path.clone()]));
    common::wait_for_load(&mut h);
    wait_for_analysis(&mut h);
    h
}

fn analysed(file: &str) -> Harness<'static, App> {
    let mut h = plain_harness(&[file]);
    wait_for_analysis(&mut h);
    h
}

/// The rule view the dialogs get, with lengths in mm (the app's default).
fn with_view<R>(h: &Harness<'_, App>, f: impl FnOnce(&RuleView<'_>) -> R) -> R {
    let session = &h.state().loaded().expect("board loaded").session;
    f(&RuleView::new(session, Unit::Mm))
}

fn class_names(h: &Harness<'_, App>) -> Vec<String> {
    board(h)
        .rules
        .net_classes
        .iter()
        .map(|(_, c)| c.name().to_string())
        .collect()
}

fn class_id(h: &Harness<'_, App>, name: &str) -> NetClassId {
    board(h)
        .rules
        .net_classes
        .get_by_name(name)
        .unwrap_or_else(|| panic!("net class {name}"))
}

fn net_number(h: &Harness<'_, App>, name: &str) -> i32 {
    board(h)
        .rules
        .nets
        .iter()
        .find(|n| n.name == name)
        .unwrap_or_else(|| panic!("net {name}"))
        .net_number
}

fn via_names(h: &Harness<'_, App>) -> Vec<String> {
    board(h)
        .rules
        .via_infos
        .iter()
        .map(|(_, v)| v.name.clone())
        .collect()
}

fn via_id(h: &Harness<'_, App>, name: &str) -> ViaInfoId {
    board(h).rules.via_infos.get_by_name(name).expect("via")
}

/// The `index`-th text field that shows `shown` (labels may show the same).
fn field<'a>(h: &'a Harness<'_, App>, index: usize, shown: &str) -> egui_kittest::Node<'a> {
    h.get_all_by_role(Role::TextInput)
        .filter(|n| n.value().as_deref() == Some(shown))
        .nth(index)
        .unwrap_or_else(|| panic!("text field {index} showing {shown:?}"))
}

/// Clicks the text field that shows `shown`, replaces its text and ends the
/// edit with Enter, as a user does in a table cell.
fn type_into(h: &mut Harness<'_, App>, index: usize, shown: &str, typed: &str) {
    field(h, index, shown).click();
    settle!(h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    settle!(h);
    field(h, index, shown).type_text(typed);
    settle!(h);
    h.key_press(Key::Enter);
    settle!(h);
}

fn click(h: &mut Harness<'_, App>, label: &str) {
    h.get_by_label(label).click();
    settle!(h);
}

/// The `index`-th drop-down that shows `value`; a click opens its list.
fn combo<'a>(h: &'a Harness<'_, App>, index: usize, value: &'a str) -> egui_kittest::Node<'a> {
    h.get_all_by_role_and_label(Role::ComboBox, value)
        .nth(index)
        .unwrap_or_else(|| panic!("drop-down {index} showing {value:?}"))
}

fn click_last(h: &mut Harness<'_, App>, label: &str) {
    h.get_all_by_label(label).last().expect("a node").click();
    settle!(h);
}

fn click_button(h: &mut Harness<'_, App>, label: &str) {
    h.get_all_by_role_and_label(Role::Button, label)
        .next()
        .unwrap_or_else(|| panic!("button {label}"))
        .click();
    settle!(h);
}

fn click_nth(h: &mut Harness<'_, App>, label: &str, n: usize) {
    h.get_all_by_label(label).nth(n).unwrap().click();
    settle!(h);
}

// ---------------------------------------------------------------------------
// G06.1: Net classes
// ---------------------------------------------------------------------------

#[test]
fn edit_width_changes_engine_value() {
    let mut h = analysed(ISSUE754);
    open(&mut h, RulesDialog::NetClasses);
    let default = board(&h)
        .rules
        .default_net_class_id()
        .expect("a default class");
    let shown = with_view(&h, |v| net_classes::trace_width_summary(v, default));
    // The default class is the first row, so its width field is the first
    // field that shows this text.
    type_into(&mut h, 0, &shown, "0.3");

    // The formula of Java's `applyTraceWidth`: `round(userToBoard(0.5 * w))`
    // with `w` in the unit of the design file.
    let b = board(&h);
    let user = Unit::scale(0.3, Unit::Mm, b.communication.unit);
    let expected = round_to_i32(
        b.communication
            .coordinate_transform
            .dsn_to_board(0.5 * user),
    );
    assert!(expected > 0);
    let class = b.rules.net_classes.get(default);
    for (i, layer) in b.layer_structure.layers.iter().enumerate() {
        if layer.is_signal {
            assert_eq!(
                class.trace_half_width(i as i32),
                expected,
                "layer {}",
                layer.name
            );
        }
    }

    // The edit starts a new DRC; its count is the engine's on the same board.
    wait_for_analysis(&mut h);
    let session = &h.state().loaded().unwrap().session;
    let shown_count = h
        .state()
        .analysis()
        .drc(session)
        .expect("a current DRC")
        .clearance
        .len();
    let fresh = DesignRulesChecker::new(session.board.basic()).check();
    assert_eq!(shown_count, fresh.clearance_violations.len());
    h.get_by_label(&format!("Clearance violations: {shown_count}"));
}

#[test]
fn typed_width_that_is_refused_changes_nothing() {
    let mut h = small();
    open(&mut h, RulesDialog::NetClasses);
    let default = class_id(&h, "default");
    let before = board(&h).rules.net_classes.get(default).trace_half_width(0);
    let shown = with_view(&h, |v| net_classes::trace_width_summary(v, default));
    type_into(&mut h, 0, &shown, "-50");
    assert_eq!(
        board(&h).rules.net_classes.get(default).trace_half_width(0),
        before
    );
    // A decimal comma is accepted (`parseTraceWidthValue`).
    type_into(&mut h, 0, &shown, "0,35");
    let b = board(&h);
    let user = Unit::scale(0.35, Unit::Mm, b.communication.unit);
    let expected = round_to_i32(
        b.communication
            .coordinate_transform
            .dsn_to_board(0.5 * user),
    );
    assert_eq!(
        b.rules.net_classes.get(default).trace_half_width(0),
        expected
    );
}

#[test]
fn remove_default_class_refused() {
    let mut h = small();
    assert_eq!(class_names(&h), ["default"]);
    open(&mut h, RulesDialog::NetClasses);
    click_button(&mut h, "default");
    click(&mut h, "Remove");
    assert_eq!(
        notice(&h).as_deref(),
        Some("The default net class cannot be removed.")
    );
    assert_eq!(class_names(&h), ["default"]);
}

#[test]
fn remove_used_class_refused() {
    let mut h = small();
    edit(&mut h, RulesEdit::AddNetClass);
    let new = class_id(&h, "class1");
    // The class of the *last* net: Java's loop stops one net short and would
    // remove it; this port refuses (docs/DIVERGENCES.md, "Rules dialogs").
    let last = net_number(&h, "N3");
    assert_eq!(last, board(&h).rules.nets.max_net_number());
    edit(
        &mut h,
        RulesEdit::SetNetClass {
            net: last,
            class: new,
        },
    );
    open(&mut h, RulesDialog::NetClasses);
    click(&mut h, "class1");
    click(&mut h, "Remove");
    assert_eq!(
        notice(&h).as_deref(),
        Some("Net class not removed because it is still used by net N3.")
    );
    assert_eq!(class_names(&h), ["default", "class1"]);

    // The first of several nets is named, in ascending net number.
    let first = net_number(&h, "N1");
    edit(
        &mut h,
        RulesEdit::SetNetClass {
            net: first,
            class: new,
        },
    );
    click(&mut h, "Remove");
    assert_eq!(
        notice(&h).as_deref(),
        Some("Net class not removed because it is still used by net N1.")
    );
}

#[test]
fn add_then_remove_unused_class() {
    let mut h = small();
    open(&mut h, RulesDialog::NetClasses);
    let count = class_names(&h).len();
    click(&mut h, "Add");
    assert_eq!(class_names(&h).len(), count + 1);
    let new = class_names(&h).pop().unwrap();
    click(&mut h, &new);
    click(&mut h, "Remove");
    assert_eq!(notice(&h), Some(format!("Net class {new} removed.")));
    assert_eq!(class_names(&h).len(), count);
}

#[test]
fn net_class_cells_send_their_edits() {
    let mut h = small();
    edit(&mut h, RulesEdit::AddNetClass);
    open(&mut h, RulesDialog::NetClasses);
    let class = class_id(&h, "class1");
    // The checkboxes of the second row: shove fixed, cycles, ignored.
    // The side panel has checkboxes too; the table's six come last.
    let boxes = h.get_all_by_role(Role::CheckBox).count();
    assert!(boxes >= 6);
    let table = boxes - 6;
    let before = {
        let c = board(&h).rules.net_classes.get(class);
        (
            c.is_shove_fixed(),
            c.pull_tight(),
            c.ignore_cycles_with_areas(),
            c.is_ignored_by_autorouter,
        )
    };
    h.get_all_by_role(Role::CheckBox)
        .nth(table + 3)
        .unwrap()
        .click(); // shove fixed
    settle!(h);
    h.get_all_by_role(Role::CheckBox)
        .nth(table + 4)
        .unwrap()
        .click(); // cycles with areas
    settle!(h);
    h.get_all_by_role(Role::CheckBox)
        .nth(table + 5)
        .unwrap()
        .click(); // ignored by the autorouter
    settle!(h);
    let c = board(&h).rules.net_classes.get(class);
    // `applyShoveFixedSelection` sets pull tight the other way round.
    assert_eq!((c.is_shove_fixed(), c.pull_tight()), (!before.0, !before.1));
    assert_eq!(c.ignore_cycles_with_areas(), !before.2);
    assert_eq!(c.is_ignored_by_autorouter, !before.3);
}

#[test]
fn select_takes_the_items_of_all_chosen_classes() {
    let mut h = small();
    edit(&mut h, RulesEdit::AddNetClass);
    edit(&mut h, RulesEdit::AddNetClass);
    let (c1, c2) = (class_id(&h, "class1"), class_id(&h, "class2"));
    let (n1, n2) = (net_number(&h, "N1"), net_number(&h, "N2"));
    edit(&mut h, RulesEdit::SetNetClass { net: n1, class: c1 });
    edit(&mut h, RulesEdit::SetNetClass { net: n2, class: c2 });
    open(&mut h, RulesDialog::NetClasses);
    click(&mut h, "class1");
    click(&mut h, "Select");
    let mut expected = board(&h).net_items(n1);
    expected.sort();
    assert_eq!(h.state().selection().ids(), expected);
    // Ctrl adds the second class; Java compares with the item's net index
    // instead and would miss it.
    h.get_by_label("class2").click_modifiers(Modifiers::COMMAND);
    settle!(h);
    click(&mut h, "Select");
    let mut both = board(&h).net_items(n1);
    both.extend(board(&h).net_items(n2));
    both.sort();
    both.dedup();
    assert_eq!(h.state().selection().ids(), both);
}

#[test]
fn layer_rules_dialog_sets_the_active_layers() {
    let mut h = analysed(ISSUE754);
    open(&mut h, RulesDialog::NetClasses);
    let default = board(&h).rules.default_net_class_id().unwrap();
    let summary = with_view(&h, |v| net_classes::layer_summary(v.board, default));
    assert_eq!(summary, "All Layers");
    click_nth(&mut h, &summary, 0);
    click(&mut h, "Outer");
    click(&mut h, "OK");
    let b = board(&h);
    let signal: Vec<usize> = (0..b.layer_structure.count())
        .filter(|&i| b.layer_structure.layers[i].is_signal)
        .collect();
    let class = b.rules.net_classes.get(default);
    for (n, &layer) in signal.iter().enumerate() {
        let outer = n == 0 || n == signal.len() - 1;
        assert_eq!(
            class.is_active_routing_layer(layer as i32),
            outer,
            "layer {layer}"
        );
    }
    assert_eq!(net_classes::layer_summary(b, default), "Outer Layers");
}

// ---------------------------------------------------------------------------
// G06.2: Assign net class
// ---------------------------------------------------------------------------

#[test]
fn assign_net_class_changes_net() {
    let mut h = small();
    edit(&mut h, RulesEdit::AddNetClass);
    let new = class_id(&h, "class1");
    let n2 = net_number(&h, "N2");
    // The Assign button of the Net Classes window opens the window.
    open(&mut h, RulesDialog::NetClasses);
    click(&mut h, "Assign");
    assert!(h.state().dialogs_open(RulesDialog::AssignNetClass));
    h.get_by_label("Net name");
    exec(&mut h, Command::CloseRulesDialog(RulesDialog::NetClasses));
    // The rows are in name order: N1, N2, N3. Open the combo of N2 (the
    // second "default") and choose the new class.
    let order = with_view(&h, |v| assign_net_class::net_order(v.board));
    assert_eq!(order, [net_number(&h, "N1"), n2, net_number(&h, "N3")]);
    let runs = h.state().analysis().runs_started();
    combo(&h, 1, "default").click();
    settle!(h);
    click(&mut h, "class1");
    assert_eq!(board(&h).rules.nets.get(n2).unwrap().net_class(), new);
    assert_ne!(
        board(&h)
            .rules
            .nets
            .get(net_number(&h, "N1"))
            .unwrap()
            .net_class(),
        new
    );
    // The ratsnest runs again, and its result carries the new edit generation.
    wait_for_analysis(&mut h);
    assert!(h.state().analysis().runs_started() > runs);
    let session = &h.state().loaded().unwrap().session;
    assert!(session.edit_generation > 0);
    assert!(h.state().analysis().ratsnest(session).is_some());
    assert_eq!(
        h.state().analysis().tag_of(session).edit_generation,
        session.edit_generation
    );
}

#[test]
fn assign_net_class_on_the_avionics_hub() {
    let mut h = analysed(ISSUE754);
    edit(&mut h, RulesEdit::AddNetClass);
    let new = class_id(&h, "class1");
    let gnd = net_number(&h, "GTN1_GND");
    let before = h.state().analysis().runs_started();
    edit(
        &mut h,
        RulesEdit::SetNetClass {
            net: gnd,
            class: new,
        },
    );
    assert_eq!(board(&h).rules.nets.get(gnd).unwrap().net_class(), new);
    wait_for_analysis(&mut h);
    assert!(h.state().analysis().runs_started() > before);
    let session = &h.state().loaded().unwrap().session;
    assert!(h.state().analysis().ratsnest(session).is_some());
}

// ---------------------------------------------------------------------------
// G06.3: Clearance matrix
// ---------------------------------------------------------------------------

#[test]
fn clearance_edit_is_symmetric() {
    let mut h = small();
    edit(
        &mut h,
        RulesEdit::AddClearanceClass {
            name: "extra".into(),
        },
    );
    open(&mut h, RulesDialog::ClearanceMatrix);
    // The editable cells are those of the rows after the first class and the
    // columns after it: (1,1) (1,2) (2,1) (2,2). Cell (1, 2):
    assert_eq!(h.get_all_by_role(Role::TextInput).count(), 4);
    let shown = with_view(&h, |v| {
        clearance_matrix::cell_text(v, 1, 2, ClearanceLayer::All)
    });
    // Distinguish it from the other cells by giving (1, 2) a value of its own.
    type_into(&mut h, 1, &shown, "0.35");
    let expected = with_view(&h, |v| {
        clearance_matrix::cell_board_value(v, "0.35").unwrap()
    });
    let m = &board(&h).rules.clearance_matrix;
    for layer in 0..board(&h).layer_structure.count() as i32 {
        assert_eq!(m.get_value(1, 2, layer, false), expected, "layer {layer}");
        assert_eq!(m.get_value(2, 1, layer, false), expected, "layer {layer}");
    }
}

#[test]
fn clearance_cell_asks_when_items_use_both_classes() {
    let mut h = small();
    open(&mut h, RulesDialog::ClearanceMatrix);
    let shown = with_view(&h, |v| {
        clearance_matrix::cell_text(v, 1, 1, ClearanceLayer::All)
    });
    let before = board(&h).rules.clearance_matrix.get_value(1, 1, 0, false);
    // Cell (1, 1): the items of the default class are in both classes.
    type_into(&mut h, 0, &shown, "0.5");
    h.get_by_label(
        "The selected items are already assigned to clearance class default. Change the clearance anyway?",
    );
    click(&mut h, "No");
    assert_eq!(
        board(&h).rules.clearance_matrix.get_value(1, 1, 0, false),
        before
    );
    type_into(&mut h, 0, &shown, "0.5");
    click(&mut h, "Yes");
    let expected = with_view(&h, |v| {
        clearance_matrix::cell_board_value(v, "0.5").unwrap()
    });
    assert_eq!(
        board(&h).rules.clearance_matrix.get_value(1, 1, 0, false),
        expected
    );
}

fn findings(b: &BasicBoard) -> (usize, usize, usize) {
    let f = DesignRulesChecker::new(b).check();
    (
        f.clearance_violations.len(),
        f.unconnected_items.len(),
        f.zone_islands.len(),
    )
}

#[test]
fn prune_merges_equal_classes() {
    let mut h = small();
    edit(
        &mut h,
        RulesEdit::AddClearanceClass {
            name: "extra".into(),
        },
    );
    let before_count = board(&h).rules.clearance_matrix.class_count();
    assert!(
        board(&h).rules.clearance_matrix.is_equal(2, 1),
        "a copy of default"
    );
    let before = findings(board(&h));
    open(&mut h, RulesDialog::ClearanceMatrix);
    click(&mut h, "Prune");
    h.get_by_label("Remove clearance class extra?");
    click(&mut h, "Yes");
    assert_eq!(
        board(&h).rules.clearance_matrix.class_count(),
        before_count - 1
    );
    assert_eq!(findings(board(&h)), before);
    // No further question.
    assert!(h.query_by_label("Yes").is_none());
}

#[test]
fn prune_asked_no_keeps_the_class() {
    let mut h = small();
    edit(
        &mut h,
        RulesEdit::AddClearanceClass {
            name: "extra".into(),
        },
    );
    open(&mut h, RulesDialog::ClearanceMatrix);
    click(&mut h, "Prune");
    click(&mut h, "No");
    assert_eq!(board(&h).rules.clearance_matrix.class_count(), 3);
    assert!(h.query_by_label("No").is_none());
}

/// The dialog's walk (one candidate at a time, each merge one B08 call) ends
/// with the same classes as B08's `prune_clearance_matrix` answering Yes to
/// every question, in the same order.
#[test]
fn prune_candidates_equal_the_engines_prune() {
    for file in ["Issue754-avionics_hub.dsn", "Issue159-setonix_2hp-pcb.dsn"] {
        let h = analysed(file);
        let mut by_dialog = (*h.state().loaded().unwrap().session.board).clone();
        let mut by_engine = by_dialog.clone();
        // Make sure there is something to prune.
        for name in ["twin_a", "twin_b"] {
            for b in [&mut by_dialog, &mut by_engine] {
                assert!(b.add_clearance_class(name));
            }
        }
        let mut removed_by_dialog = Vec::new();
        let mut next_i = by_dialog.rules.clearance_matrix.class_count() as i32 - 1;
        while let Some((i, j)) =
            clearance_matrix::next_prune_candidate(&by_dialog.rules.clearance_matrix, next_i)
        {
            let name = by_dialog
                .rules
                .clearance_matrix
                .get_name(i)
                .unwrap()
                .to_string();
            let outcome = freerouting_gui::dialogs::rules_edit::apply(
                &mut by_dialog,
                &RulesEdit::MergeClearanceClass { from: i, to: j },
            );
            assert!(outcome.changed, "{file}: merging {name}");
            removed_by_dialog.push(name);
            next_i = i - 1;
        }
        let outcome = by_engine.prune_clearance_matrix(&mut |_| true);
        assert!(!outcome.failed);
        assert_eq!(removed_by_dialog, outcome.removed, "{file}");
        assert!(!removed_by_dialog.is_empty(), "{file}");
        let names = |b: &BasicBoard| -> Vec<String> {
            let m = &b.rules.clearance_matrix;
            (0..m.class_count() as i32)
                .map(|i| m.get_name(i).unwrap().to_string())
                .collect()
        };
        assert_eq!(names(&by_dialog), names(&by_engine), "{file}");
        assert_eq!(findings(&by_dialog), findings(&by_engine), "{file}");
    }
}

#[test]
fn add_class_turns_off_compensation() {
    let mut h = small();
    edit(&mut h, RulesEdit::SetClearanceCompensation(true));
    assert!(board(&h).search_trees().is_clearance_compensation_used());
    open(&mut h, RulesDialog::ClearanceMatrix);
    click(&mut h, "Add Class");
    h.get_by_label("Enter a name for the new class.");
    // An illegal name (Java asks again) and an existing name are not added.
    type_prompt(&mut h, "", "bad_name");
    assert_eq!(board(&h).rules.clearance_matrix.class_count(), 2);
    h.get_by_label("Enter a name for the new class.");
    type_prompt(&mut h, "bad_name", "extra");
    assert_eq!(board(&h).rules.clearance_matrix.class_count(), 3);
    assert!(!board(&h).search_trees().is_clearance_compensation_used());
    assert!(
        h.query_by_label("Enter a name for the new class.")
            .is_none()
    );
}

/// Replaces the text of the name prompt, which shows `current`, and presses OK.
fn type_prompt(h: &mut Harness<'_, App>, current: &str, name: &str) {
    let empty = |n: &egui_kittest::Node<'_>| n.value().unwrap_or_default() == current;
    h.get_all_by_role(Role::TextInput)
        .rfind(empty)
        .unwrap()
        .click();
    settle!(h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    settle!(h);
    h.get_all_by_role(Role::TextInput)
        .rfind(empty)
        .unwrap()
        .type_text(name);
    settle!(h);
    h.get_by_label("OK").click();
    settle!(h);
}

// ---------------------------------------------------------------------------
// G06.4: Vias
// ---------------------------------------------------------------------------

#[test]
fn add_via_names_follow_java() {
    let mut h = small();
    open(&mut h, RulesDialog::EditVias);
    assert_eq!(via_names(&h), ["v"]);
    click(&mut h, "Add");
    click(&mut h, "Add");
    assert_eq!(via_names(&h), ["v", "new_via_1", "new_via_2"]);
    // A name in use is skipped: the smallest free number.
    let second = via_id(&h, "new_via_1");
    edit(
        &mut h,
        RulesEdit::RenameViaInfo {
            via: second,
            name: "kept".into(),
        },
    );
    click(&mut h, "Add");
    assert_eq!(via_names(&h), ["v", "kept", "new_via_2", "new_via_1"]);
    // Padstack 0, the default class's via clearance class, attach SMD off.
    let b = board(&h);
    let v = b.rules.via_infos.get(via_id(&h, "new_via_1"));
    assert_eq!(v.padstack, b.library.via_padstack_ids()[0]);
    assert!(!v.attach_smd_allowed);
    let default = b
        .rules
        .net_classes
        .get(b.rules.default_net_class_id().unwrap());
    assert_eq!(
        v.clearance_class,
        default
            .default_item_clearance_classes
            .get(freerouting::rules::ItemClass::Via)
    );
}

#[test]
fn remove_via_used_by_rule_refused() {
    let mut h = small();
    open(&mut h, RulesDialog::EditVias);
    click(&mut h, "Add");
    click(&mut h, "Add");
    // Choose the first via ("v", which the default via rule contains).
    click_nth(&mut h, "▸", 0);
    click(&mut h, "Remove");
    let v = via_id(&h, "v");
    let rule = board(&h)
        .rules
        .via_rules
        .iter()
        .find(|(_, r)| r.contains(v))
        .map(|(_, r)| r.name.clone())
        .expect("a rule contains v");
    assert_eq!(
        notice(&h),
        Some(format!(
            "The via was not removed because it is still used by via rule {rule}."
        ))
    );
    assert_eq!(via_names(&h).len(), 3);
    // A via no rule contains goes.
    click_nth(&mut h, "▸", 2);
    click(&mut h, "Remove");
    assert_eq!(notice(&h).as_deref(), Some("Via new_via_2 removed."));
    assert_eq!(via_names(&h), ["v", "new_via_1"]);
}

#[test]
fn remove_last_via_refused() {
    let mut h = small();
    open(&mut h, RulesDialog::EditVias);
    assert_eq!(via_names(&h).len(), 1);
    click(&mut h, "Remove");
    assert_eq!(
        notice(&h).as_deref(),
        Some("The last via cannot be removed.")
    );
    assert_eq!(via_names(&h).len(), 1);
}

#[test]
fn via_cells_send_their_edits() {
    let mut h = small();
    edit(
        &mut h,
        RulesEdit::AddClearanceClass {
            name: "extra".into(),
        },
    );
    open(&mut h, RulesDialog::EditVias);
    // Attach SMD, then rename.
    h.get_all_by_role(Role::CheckBox).last().unwrap().click();
    settle!(h);
    assert!(
        board(&h)
            .rules
            .via_infos
            .get(via_id(&h, "v"))
            .attach_smd_allowed
    );
    type_into(&mut h, 0, "v", "big_via");
    assert_eq!(via_names(&h), ["big_via"]);
}

// ---------------------------------------------------------------------------
// G06.5: Via padstacks and via rules
// ---------------------------------------------------------------------------

#[test]
fn padstack_create_use_remove() {
    let mut h = small();
    open(&mut h, RulesDialog::ViaRules);
    let before = board(&h).library.via_padstack_count();
    // Create: a name and a radius, OK.
    click_nth(&mut h, "Create", 0);
    h.get_by_label("Enter the name of the new padstack.");
    let name_field = h.get_all_by_role(Role::TextInput).next().unwrap();
    name_field.click();
    settle!(h);
    h.get_all_by_role(Role::TextInput)
        .next()
        .unwrap()
        .type_text("big");
    settle!(h);
    click(&mut h, "OK");
    assert_eq!(board(&h).library.via_padstack_count(), before + 1);
    let created = board(&h)
        .library
        .get_via_padstack_by_name("big")
        .expect("the new padstack")
        .clone();
    // Circles of the default radius (100 design units = 100 um) on both layers.
    let b = board(&h);
    let expected = round_to_i32(
        b.communication
            .coordinate_transform
            .dsn_to_board(Unit::scale(
                Unit::scale(100.0, b.communication.unit, Unit::Mm),
                Unit::Mm,
                b.communication.unit,
            )),
    );
    for layer in 0..2 {
        match created.get_shape(layer) {
            Some(freerouting::geometry::ConvexShape::Circle(c)) => assert_eq!(c.radius, expected),
            other => panic!("layer {layer}: {other:?}"),
        }
    }

    // A via that uses it (the Edit Vias window of G06.4).
    open(&mut h, RulesDialog::EditVias);
    click(&mut h, "Add");
    let new = via_id(&h, "new_via_1");
    edit(
        &mut h,
        RulesEdit::SetViaInfoPadstack {
            via: new,
            padstack: "big".into(),
        },
    );

    // Remove "big": refused, naming the via.
    exec(&mut h, Command::CloseRulesDialog(RulesDialog::EditVias));
    click_nth(&mut h, "Remove", 0);
    h.get_by_label("Choose a padstack to remove.");
    combo(&h, 0, "v").click();
    settle!(h);
    click(&mut h, "big");
    click(&mut h, "OK");
    assert_eq!(
        notice(&h).as_deref(),
        Some("The via padstack was not removed because it is still used by the via new_via_1.")
    );
    assert!(board(&h).library.get_via_padstack_by_name("big").is_some());

    // Once the via goes back to the first padstack, Remove works.
    edit(
        &mut h,
        RulesEdit::SetViaInfoPadstack {
            via: new,
            padstack: "v".into(),
        },
    );
    click_nth(&mut h, "Remove", 0);
    combo(&h, 0, "v").click();
    settle!(h);
    click(&mut h, "big");
    click(&mut h, "OK");
    assert!(board(&h).library.get_via_padstack_by_name("big").is_none());
    assert_eq!(board(&h).library.via_padstack_count(), before);
}

#[test]
fn existing_padstack_name_is_refused_by_the_form() {
    let mut h = small();
    open(&mut h, RulesDialog::ViaRules);
    click_nth(&mut h, "Create", 0);
    h.get_all_by_role(Role::TextInput).next().unwrap().click();
    settle!(h);
    h.get_all_by_role(Role::TextInput)
        .next()
        .unwrap()
        .type_text("v");
    settle!(h);
    h.get_by_label("A name already exists. Choose another name.");
    assert!(
        h.get_by_label("OK").accesskit_node().is_disabled(),
        "OK waits for another name"
    );
}

#[test]
fn via_rule_order() {
    let mut h = small();
    edit(&mut h, RulesEdit::AddViaInfo);
    edit(&mut h, RulesEdit::AddViaInfo);
    edit(
        &mut h,
        RulesEdit::AddViaRule {
            name: "mine".into(),
        },
    );
    let rule = board(&h)
        .rules
        .via_rules
        .get_by_name("mine")
        .expect("the rule");
    let ids: Vec<ViaInfoId> = ["v", "new_via_1", "new_via_2"]
        .iter()
        .map(|n| via_id(&h, n))
        .collect();
    for &via in &ids {
        edit(&mut h, RulesEdit::AppendViaToRule { rule, via });
    }
    open(&mut h, RulesDialog::ViaRules);
    click(&mut h, "mine");
    // The rule Edit button is the second one (the first edits the vias).
    click_nth(&mut h, "Edit", 1);
    click(&mut h, "new_via_2");
    click(&mut h, "Move Up");
    click(&mut h, "Move Up");
    let order = |h: &Harness<'_, App>| -> Vec<ViaInfoId> {
        board(h).rules.via_rules.get(rule).vias().to_vec()
    };
    assert_eq!(order(&h), [ids[2], ids[0], ids[1]]);
    // At the top Move Up does nothing; Move Down goes one step.
    click(&mut h, "Move Up");
    assert_eq!(order(&h), [ids[2], ids[0], ids[1]]);
    click(&mut h, "Move Down");
    assert_eq!(order(&h), [ids[0], ids[2], ids[1]]);
    // Remove asks first (the editor's button is the last one).
    click_last(&mut h, "Remove");
    h.get_by_label("Remove new_via_2 from the rule mine?");
    click(&mut h, "Yes");
    assert_eq!(order(&h), [ids[0], ids[1]]);
}

#[test]
fn via_rule_create_and_remove() {
    let mut h = small();
    open(&mut h, RulesDialog::ViaRules);
    let count = board(&h).rules.via_rules.count();
    click_nth(&mut h, "Create", 1);
    h.get_by_label("Enter the name of the new via rule.");
    h.get_all_by_role(Role::TextInput).next().unwrap().click();
    settle!(h);
    h.get_all_by_role(Role::TextInput)
        .next()
        .unwrap()
        .type_text("  fine ");
    settle!(h);
    click(&mut h, "OK");
    assert_eq!(board(&h).rules.via_rules.count(), count + 1);
    assert!(board(&h).rules.via_rules.get_by_name("fine").is_some());
    click(&mut h, "fine");
    click_nth(&mut h, "Remove", 1);
    h.get_by_label("Remove via rule fine?");
    click(&mut h, "Yes");
    assert_eq!(board(&h).rules.via_rules.count(), count);
}

// ---------------------------------------------------------------------------
// G06.6: Routing settings, the board-rule part
// ---------------------------------------------------------------------------

#[test]
fn route_settings_change_engine_values() {
    let mut h = small();
    open(&mut h, RulesDialog::RouteSettings);
    // Snap angle: none needs no question.
    click(&mut h, "None");
    assert_eq!(
        board(&h).rules.trace_angle_restriction(),
        AngleRestriction::None
    );
    // Ignore conduction areas.
    let ignore = board(&h).rules.ignore_conduction();
    click(&mut h, "Ignore conduction areas");
    assert_eq!(board(&h).rules.ignore_conduction(), !ignore);
    click(&mut h, "Ignore conduction areas");
    assert_eq!(board(&h).rules.ignore_conduction(), ignore);
    // Restrict pin exit directions: the smallest trace half width, then 0.
    // (The DSN's own distance decides how the box starts.)
    if board(&h).rules.pin_edge_to_turn_dist() > 0.0 {
        click(&mut h, "Restrict pin exit directions");
        assert_eq!(board(&h).rules.pin_edge_to_turn_dist(), 0.0);
    }
    click(&mut h, "Restrict pin exit directions");
    let b = board(&h);
    let min = f64::from(b.rules.min_trace_half_width());
    let t = b.communication.coordinate_transform;
    assert_eq!(
        b.rules.pin_edge_to_turn_dist(),
        t.dsn_to_board(t.board_to_dsn(min))
    );
    assert!(b.rules.pin_edge_to_turn_dist() > 0.0);
    click(&mut h, "Restrict pin exit directions");
    assert_eq!(board(&h).rules.pin_edge_to_turn_dist(), 0.0);
    // The gap typed in mm.
    type_into(&mut h, 0, "0", "0.1");
    let b = board(&h);
    let user = Unit::scale(f64::from(0.1f32), Unit::Mm, b.communication.unit);
    assert_eq!(
        b.rules.pin_edge_to_turn_dist(),
        b.communication.coordinate_transform.dsn_to_board(user)
    );
    // Clearance compensation.
    click(&mut h, "Clearance compensation");
    assert!(board(&h).search_trees().is_clearance_compensation_used());
    click(&mut h, "Clearance compensation");
    assert!(!board(&h).search_trees().is_clearance_compensation_used());
}

#[test]
fn snap_angle_change_asks_first() {
    let mut h = small();
    open(&mut h, RulesDialog::RouteSettings);
    assert_eq!(
        board(&h).rules.trace_angle_restriction(),
        AngleRestriction::FortyfiveDegree
    );
    // The board has a 45 degree trace, which is not orthogonal.
    click(&mut h, "90 degrees");
    h.get_by_label(
        "Only orthogonal traces are fully supported with a 90° snap angle. Change the snap angle anyway?",
    );
    click(&mut h, "Cancel");
    assert_eq!(
        board(&h).rules.trace_angle_restriction(),
        AngleRestriction::FortyfiveDegree
    );
    assert!(h.query_by_label("Yes").is_none());
    click(&mut h, "90 degrees");
    click(&mut h, "Yes");
    assert_eq!(
        board(&h).rules.trace_angle_restriction(),
        AngleRestriction::NinetyDegree
    );
    // Back to 45 degrees: the orthogonal-only board question does not arise
    // while every trace is a multiple of 45 degrees.
    click(&mut h, "45 degrees");
    assert_eq!(
        board(&h).rules.trace_angle_restriction(),
        AngleRestriction::FortyfiveDegree
    );
    // Choosing the current angle is a no-op without a question.
    click(&mut h, "45 degrees");
    assert!(h.query_by_label("Yes").is_none());
}

#[test]
fn keepout_outside_outline_toggles_item() {
    let mut h = small();
    open(&mut h, RulesDialog::RouteSettings);
    let flag = |h: &Harness<'_, App>| {
        board(h)
            .outline()
            .and_then(|o| o.as_board_outline())
            .map(|o| o.keepout_outside_outline_generated())
            .expect("an outline")
    };
    assert!(!flag(&h));
    click(&mut h, "Keepout outside board outline");
    assert!(flag(&h));
    // The DRC of the edited board equals a fresh check on the same board.
    wait_for_analysis(&mut h);
    let session = &h.state().loaded().unwrap().session;
    let shown = h.state().analysis().drc(session).expect("a current DRC");
    let fresh = DesignRulesChecker::new(session.board.basic()).check();
    assert_eq!(shown.clearance.len(), fresh.clearance_violations.len());
    assert_eq!(shown.unconnected.len(), fresh.unconnected_items.len());
    // Off again.
    click(&mut h, "Keepout outside board outline");
    assert!(!flag(&h));
    click(&mut h, "Keepout outside board outline");
    assert!(flag(&h));
    click(&mut h, "Keepout outside board outline");
    assert!(!flag(&h));
}

#[test]
fn menu_opens_the_dialogs() {
    let mut h = small();
    click(&mut h, "Rules");
    click(&mut h, "Net Classes");
    assert!(h.state().dialogs_open(RulesDialog::NetClasses));
    click(&mut h, "Rules");
    click(&mut h, "Clearance Matrix");
    assert!(h.state().dialogs_open(RulesDialog::ClearanceMatrix));
    click(&mut h, "Rules");
    click_button(&mut h, "Vias");
    assert!(h.state().dialogs_open(RulesDialog::ViaRules));
    click(&mut h, "Settings");
    click(&mut h, "Routing");
    assert!(h.state().dialogs_open(RulesDialog::RouteSettings));
}

#[test]
fn filter_incompletes_keeps_the_airlines_of_the_chosen_classes() {
    let mut h = small();
    edit(&mut h, RulesEdit::AddNetClass);
    let new = class_id(&h, "class1");
    let n3 = net_number(&h, "N3");
    edit(
        &mut h,
        RulesEdit::SetNetClass {
            net: n3,
            class: new,
        },
    );
    wait_for_analysis(&mut h);
    let airlines_of = |h: &Harness<'_, App>, net: Option<i32>| -> usize {
        let session = &h.state().loaded().unwrap().session;
        h.state()
            .analysis()
            .ratsnest(session)
            .expect("a current ratsnest")
            .airlines
            .iter()
            .filter(|a| net.is_none_or(|n| a.net_number == n))
            .count()
    };
    let (all, only_n3) = (airlines_of(&h, None), airlines_of(&h, Some(n3)));
    assert!(
        only_n3 >= 1 && all > only_n3,
        "N1 and N3 are both incomplete"
    );
    assert_eq!(h.state().stats().overlay.airlines, all);
    open(&mut h, RulesDialog::NetClasses);
    click_button(&mut h, "class1");
    click(&mut h, "Filter Incomplete Connections");
    h.step();
    assert_eq!(h.state().stats().overlay.airlines, only_n3);
    click(&mut h, "Show all connections");
    h.step();
    assert_eq!(h.state().stats().overlay.airlines, all);
}
