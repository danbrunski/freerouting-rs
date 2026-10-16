//! The rule dialogs' edits (B08.4), on Issue026-J2_reference and Issue145-smoothieboard.
//!
//! - After a clearance value change, the DRC report equals that of a fresh board loaded from
//!   the DSN text with the same rule edited (`(clearance 50 (type smd_smd))` -> 400). Both
//!   boards import the same session first.
//! - After adding a clearance class, assigning items to it and pruning, the board is the
//!   original again: the report, the class count and the items' classes equal the originals'.
//! - The search trees after each edit equal the trees after the primitive calls the module
//!   docs list (`rules::edit`), run by hand on a second copy of the board.
//! - The net class, net class assignment and via edits change rules only; their tests check the
//!   rules, and that the report does not change (no item and no tree is touched).

mod board_probe;

use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::search::fingerprint::TreeFingerprint;
use freerouting::drc::{DesignRulesChecker, DrcViolation, load_design};
use freerouting::rules::edit::{ClearanceLayer, NetClassRemoveError, ViaEditError};

const BOARDS: [&str; 2] = ["Issue026-J2_reference", "Issue145-smoothieboard"];
const OLD_RULE: &str = "(clearance 50 (type smd_smd))";
const NEW_RULE: &str = "(clearance 400 (type smd_smd))";

fn load(board: &str) -> RoutingBoard {
    let row = board_probe::fixture(board);
    let mut board = board_probe::load(&row);
    board_probe::import_session(&mut board, &row);
    board
}

/// The board of the DSN text with the smd_smd rule edited, and the session imported.
fn load_edited(board: &str) -> RoutingBoard {
    let row = board_probe::fixture(board);
    let text = std::fs::read_to_string(board_probe::repo_root().join(&row.dsn)).unwrap();
    assert_eq!(
        text.matches(OLD_RULE).count(),
        1,
        "{board}: the rule is in the DSN once"
    );
    let edited = text.replace(OLD_RULE, NEW_RULE);
    let mut loaded = load_design(edited.as_bytes()).expect("edited DSN");
    board_probe::import_session(&mut loaded, &row);
    loaded
}

fn report(board: &RoutingBoard) -> (Vec<DrcViolation>, Vec<DrcViolation>) {
    let mut checker = DesignRulesChecker::new(board);
    checker.set_check_zone_islands(false);
    let report = checker.generate_report("board.dsn", "mm");
    (report.violations, report.unconnected_items)
}

fn trees(board: &RoutingBoard) -> Vec<(i32, TreeFingerprint)> {
    board
        .search_trees()
        .trees()
        .iter()
        .map(|t| (t.compensated_clearance_class(), t.fingerprint()))
        .collect()
}

fn classes_of_items(board: &RoutingBoard) -> Vec<(ItemId, i32)> {
    board
        .items()
        .map(|i| (i.id(), i.clearance_class()))
        .collect()
}

#[test]
fn clearance_value_change_matches_a_dsn_with_the_edited_rule() {
    for name in BOARDS {
        let mut board = load(name);
        let before = report(&board);
        let fresh = load_edited(name);
        let smd = board.rules.clearance_matrix.get_no("smd");
        assert!(smd > 0, "{name}: class smd");
        let new_value = fresh.rules.clearance_matrix.get_value(smd, smd, 0, false);
        assert!(new_value > board.rules.clearance_matrix.get_value(smd, smd, 0, false));
        assert!(board.clearance_items_assigned(smd, smd), "{name}");
        board.apply_clearance_value(smd as usize, smd as usize, ClearanceLayer::All, new_value);
        for layer in 0..board.layer_count() as i32 {
            assert_eq!(
                board
                    .rules
                    .clearance_matrix
                    .get_value(smd, smd, layer, false),
                new_value
            );
        }
        let after = report(&board);
        assert_ne!(before, after, "{name}: the change shows in the report");
        assert_eq!(
            after,
            report(&fresh),
            "{name}: the report equals the edited DSN's"
        );
        // The trees follow the Java call list: clearanceValueChanged after the values.
        let mut mirror = load(name);
        mirror
            .rules
            .clearance_matrix
            .set_value(smd as usize, smd as usize, new_value);
        mirror.clearance_value_changed();
        assert_eq!(trees(&board), trees(&mirror), "{name}");
    }
}

#[test]
fn clearance_value_on_one_layer_and_inner_layers() {
    let mut board = load(BOARDS[1]);
    let layers = board.layer_count();
    assert!(layers > 2, "the smoothieboard has inner layers");
    let smd = board.rules.clearance_matrix.get_no("smd") as usize;
    board.apply_clearance_value(smd, smd, ClearanceLayer::Inner, 600);
    let m = &board.rules.clearance_matrix;
    assert_eq!(m.get_value(smd as i32, smd as i32, 1, false), 600);
    assert_eq!(
        m.get_value(smd as i32, smd as i32, layers as i32 - 2, false),
        600
    );
    assert_ne!(m.get_value(smd as i32, smd as i32, 0, false), 600);
    board.apply_clearance_value(smd, 1, ClearanceLayer::Layer(0), 800);
    let m = &board.rules.clearance_matrix;
    // Symmetric, and only on that layer.
    assert_eq!(m.get_value(smd as i32, 1, 0, false), 800);
    assert_eq!(m.get_value(1, smd as i32, 0, false), 800);
    assert_ne!(m.get_value(1, smd as i32, 1, false), 800);
    // An out of range class or layer changes nothing.
    let before = board.rules.clearance_matrix.clone();
    board.apply_clearance_value(99, 1, ClearanceLayer::All, 10);
    board.apply_clearance_value(1, 1, ClearanceLayer::Layer(99), 10);
    assert_eq!(
        before.get_value(1, 1, 0, false),
        board.rules.clearance_matrix.get_value(1, 1, 0, false)
    );
}

#[test]
fn add_class_then_prune_restores_the_board() {
    for name in BOARDS {
        let mut board = load(name);
        let original_report = report(&board);
        let original_classes = classes_of_items(&board);
        let class_count = board.rules.clearance_matrix.class_count() as i32;
        let compensated = board.search_trees().is_clearance_compensation_used();

        assert!(board.add_clearance_class("extra"));
        // A name that exists in another case is refused too.
        assert!(!board.add_clearance_class("EXTRA"));
        let extra = class_count;
        assert_eq!(
            board.rules.clearance_matrix.class_count() as i32,
            class_count + 1
        );
        assert!(
            !board.search_trees().is_clearance_compensation_used(),
            "adding a class turns clearance compensation off (was {compensated})"
        );

        // Move every 10th item of the default class to the new one, and give it a tree.
        let moved: Vec<ItemId> = classes_of_items(&board)
            .into_iter()
            .filter(|(_, class)| *class == 1)
            .map(|(id, _)| id)
            .step_by(10)
            .collect();
        assert!(moved.len() > 3, "{name}: {} items moved", moved.len());
        for id in &moved {
            board.change_clearance_class(*id, extra);
        }
        board.autoroute_search_tree(extra);
        let trees_with_extra = board.search_trees().trees().len();
        assert!(trees_with_extra >= 2);

        let mut asked = Vec::new();
        let outcome = board.prune_clearance_matrix(&mut |class| {
            asked.push(class.to_string());
            class == "extra"
        });
        assert!(!outcome.failed, "{name}");
        assert_eq!(outcome.removed, ["extra"], "{name}");
        assert_eq!(
            asked.first().map(String::as_str),
            Some("extra"),
            "{name}: the last class first"
        );
        assert_eq!(
            board.rules.clearance_matrix.class_count() as i32,
            class_count
        );
        assert!(
            board.search_trees().trees().len() < trees_with_extra,
            "the tree of the class left"
        );

        // Items moved back to a class with the same row; all the others are untouched.
        for (id, class) in classes_of_items(&board) {
            let original = original_classes.iter().find(|(i, _)| *i == id).unwrap().1;
            if moved.contains(&id) {
                assert!(
                    board.rules.clearance_matrix.is_equal(original, class),
                    "{name} {id}"
                );
            } else {
                assert_eq!(class, original, "{name} {id}");
            }
        }
        assert_eq!(report(&board), original_report, "{name}");

        // The same through the primitives of the call list.
        let mut mirror = load(name);
        assert!(mirror.rules.clearance_matrix.append_class("extra"));
        mirror.set_clearance_compensation_used(false);
        for id in &moved {
            mirror.change_clearance_class(*id, extra);
        }
        mirror.autoroute_search_tree(extra);
        let j = (0..extra)
            .rev()
            .find(|j| mirror.rules.clearance_matrix.is_equal(extra, *j))
            .expect("an equal class");
        mirror.change_clearance_class_index(extra, j);
        assert!(mirror.remove_clearance_class(extra));
        mirror.clearance_class_removed(extra);
        assert_eq!(trees(&board), trees(&mirror), "{name}");
    }
}

#[test]
fn remove_clearance_class_refuses_a_class_in_use() {
    let mut board = load(BOARDS[0]);
    let used = board.items().map(|i| i.clearance_class()).max().unwrap();
    assert!(used > 0);
    let classes = board.rules.clearance_matrix.class_count();
    assert!(!board.remove_clearance_class(used), "an item uses it");
    assert_eq!(board.rules.clearance_matrix.class_count(), classes);
    // The default class is used by a net class and by the via infos.
    assert!(!board.remove_clearance_class(1));
}

#[test]
fn net_class_edits_change_the_rules_only() {
    for name in BOARDS {
        let mut board = load(name);
        let before = report(&board);
        let trees_before = trees(&board);
        let items_before = classes_of_items(&board);
        let default = board.rules.default_net_class();
        let classes = board.rules.net_classes.count();

        let new = board.add_net_class();
        assert_eq!(board.rules.net_classes.count(), classes + 1);
        let (d, n) = (
            board.rules.net_classes.get(default),
            board.rules.net_classes.get(new),
        );
        assert_eq!(n.trace_half_width(0), d.trace_half_width(0));
        assert_eq!(n.trace_clearance_class(), d.trace_clearance_class());
        assert_eq!(n.via_rule(), d.via_rule());

        let default_name = board.rules.net_classes.get(default).name().to_string();
        assert!(!board.rename_net_class(new, &default_name));
        assert!(board.rename_net_class(new, "wide"));
        assert_eq!(board.rules.net_classes.get(new).name(), "wide");
        assert!(board.set_net_class_trace_half_width(new, 1234));
        assert!(!board.set_net_class_trace_half_width(new, 0));
        assert_eq!(board.rules.net_classes.get(new).trace_half_width(0), 1234);
        board.set_net_class_shove_fixed(new, true);
        assert!(board.rules.net_classes.get(new).is_shove_fixed());
        assert!(!board.rules.net_classes.get(new).pull_tight());
        board.set_net_class_shove_fixed(new, false);
        assert!(board.rules.net_classes.get(new).pull_tight());
        board.set_net_class_ignore_cycles_with_areas(new, true);
        board.set_net_class_minimum_trace_length(new, 5000.0);
        board.set_net_class_maximum_trace_length(new, 9000.0);
        board.set_net_class_ignored_by_autorouter(new, true);
        let c = board.rules.net_classes.get(new);
        assert!(c.ignore_cycles_with_areas() && c.is_ignored_by_autorouter);
        assert_eq!(
            (c.minimum_trace_length(), c.maximum_trace_length()),
            (5000.0, 9000.0)
        );
        assert!(board.set_net_class_clearance_class(new, "default"));
        assert!(!board.set_net_class_clearance_class(new, "no such class"));
        assert_eq!(board.rules.net_classes.get(new).trace_clearance_class(), 1);
        let rule_name = board
            .rules
            .via_rules
            .iter()
            .next()
            .map(|(_, r)| r.name.clone())
            .expect("a via rule");
        assert!(board.set_net_class_via_rule(new, &rule_name));
        assert!(!board.set_net_class_via_rule(new, "no such rule"));

        // Assign net 1 to the new class; removing it is then refused (net 1 is below the
        // last net number), and the last net is not checked: Java's loop stops before it.
        assert!(board.assign_net_class(1, new));
        assert_eq!(board.rules.nets.get(1).unwrap().net_class(), new);
        assert_eq!(
            board.remove_net_class(new),
            Err(NetClassRemoveError::InUse(
                board.rules.nets.get(1).unwrap().name.clone()
            ))
        );
        let last = board.rules.nets.max_net_number();
        assert!(board.assign_net_class(1, default));
        assert!(board.assign_net_class(last, new));
        assert_eq!(
            board.remove_net_class(new),
            Ok(()),
            "the last net is skipped by Java's loop"
        );
        assert!(board.assign_net_class(last, default));

        assert_eq!(report(&board), before, "{name}: no item changed");
        assert_eq!(trees(&board), trees_before, "{name}: no tree changed");
        assert_eq!(classes_of_items(&board), items_before, "{name}");
    }
}

#[test]
fn via_edits_follow_the_dialogs() {
    let mut board = load(BOARDS[1]);
    let vias = board.rules.via_infos.count();
    assert!(vias > 0);

    // WindowEditVias: add, rename, change, remove.
    let new = board.add_via_info().expect("a via info");
    assert_eq!(board.rules.via_infos.get(new).name, "newVia1");
    assert!(!board.rules.via_infos.get(new).attach_smd_allowed);
    let second = board.add_via_info().expect("a via info");
    assert_eq!(board.rules.via_infos.get(second).name, "newVia2");
    assert_eq!(
        board.rename_via_info(second, "newVia1"),
        Err(ViaEditError::BadName)
    );
    assert_eq!(board.rename_via_info(second, "mine"), Ok(()));
    assert!(board.set_via_info_clearance_class(second, "default"));
    assert!(!board.set_via_info_clearance_class(second, "nothing"));
    board.set_via_info_attach_smd(second, true);
    let padstack_name = board
        .library
        .get_via_padstack(0)
        .map(|p| p.name.clone())
        .expect("a via padstack");
    assert!(board.set_via_info_padstack(second, &padstack_name));
    assert!(!board.set_via_info_padstack(second, "no such padstack"));
    assert_eq!(board.rules.via_infos.get(second).clearance_class, 1);
    assert!(board.rules.via_infos.get(second).attach_smd_allowed);

    // WindowVia: via rules; WindowViaRule: append, swap, remove.
    assert_eq!(board.add_via_rule("  "), Err(ViaEditError::BadName));
    let rule = board.add_via_rule(" mine ").expect("a rule");
    assert_eq!(board.rules.via_rules.get(rule).name, "mine");
    board.append_via_to_rule(rule, new);
    board.append_via_to_rule(rule, second);
    assert!(board.swap_vias_in_rule(rule, new, second));
    assert_eq!(board.rules.via_rules.get(rule).vias(), [second, new]);
    assert_eq!(
        board.remove_via_info(new),
        Err(ViaEditError::InRule("mine".into()))
    );
    assert!(board.remove_via_from_rule(rule, new));
    assert_eq!(board.remove_via_info(new), Ok(()));
    assert!(board.remove_via_rule(rule));

    // A via padstack that a via info uses stays; one nobody uses can go.
    let used = board.rules.via_infos.get(second).padstack;
    let user = board.rules.via_infos.get(second).name.clone();
    assert_eq!(
        board.remove_via_padstack(used),
        Err(ViaEditError::PadstackInUse(
            board
                .rules
                .via_infos
                .iter()
                .find(|(_, v)| v.padstack == used)
                .unwrap()
                .1
                .name
                .clone()
        ))
    );
    let _ = user;
    let last_layer = board.layer_count() as i32 - 1;
    let fresh = board.library.padstacks.add_layer_range(
        freerouting::geometry::ConvexShape::Circle(freerouting::geometry::Circle::new(
            freerouting::geometry::IntPoint::new(0, 0),
            250,
        )),
        0,
        last_layer,
    );
    assert!(board.add_via_padstack(fresh));
    assert!(!board.add_via_padstack(fresh), "the name exists");
    assert_eq!(board.remove_via_padstack(fresh), Ok(()));
}

// ---------------------------------------------------------------------------
// The search trees against Java: probe `rules-edit` (`RulesEditProbe.java`, whose doc comment
// lists the stages) runs the clearance matrix window's calls on Java's board and hashes the
// trees after each.

mod java {
    use super::*;
    use board_probe::{Line, compare, fixture, header, hex};
    use freerouting::board::search::ShapeSearchTree;
    use freerouting::datastructures::IdGenerator;
    use freerouting::rules::edit::ClearanceLayer;
    use std::collections::BTreeSet;

    const VALUE: i32 = 3000;

    fn tree_line(stage: &str, tree: &ShapeSearchTree) -> String {
        let fingerprint = tree.fingerprint();
        Line::new()
            .str("stage", stage)
            .str("tree", &tree.key())
            .num("nodes", fingerprint.nodes as i64)
            .num("leaves", fingerprint.leaves as i64)
            .num("maxDepth", fingerprint.max_depth as i64)
            .str("shapeHash", &hex(fingerprint.shape_hash))
            .str("contentDigest", &hex(tree.content_digest()))
            .end()
    }

    fn hash_all(lines: &mut Vec<String>, stage: &str, step: &str, board: &RoutingBoard) {
        lines.push(
            Line::new()
                .str("stage", stage)
                .str("step", step)
                .num(
                    "maxGeneratedId",
                    i64::from(board.communication.id_generator.max_generated_id()),
                )
                .num("items", board.items().count() as i64)
                .end(),
        );
        for tree in board.search_trees().trees() {
            lines.push(tree_line(stage, tree));
        }
    }

    fn autoroute_trees(board: &mut RoutingBoard) {
        let classes = &board.rules.net_classes;
        let set: BTreeSet<i32> = (0..classes.count())
            .filter_map(|i| classes.nth(i))
            .map(|id| classes.get(id).trace_clearance_class())
            .collect();
        for class in set {
            board.autoroute_search_tree(class);
        }
    }

    fn check(name: &str) {
        let row = fixture(name);
        let mut board = board_probe::load(&row);
        board_probe::import_session(&mut board, &row);
        let mut lines = vec![header("rules-edit", &row.board)];
        autoroute_trees(&mut board);
        hash_all(&mut lines, "load", "load", &board);

        let smd = board.rules.clearance_matrix.get_no("smd") as usize;
        board.apply_clearance_value(smd, smd, ClearanceLayer::All, VALUE);
        hash_all(&mut lines, "value-changed", "smd-smd", &board);

        let extra = board.rules.clearance_matrix.class_count() as i32;
        assert!(board.add_clearance_class("extra"));
        hash_all(&mut lines, "class-added", "extra", &board);

        let class_one: Vec<ItemId> = board
            .items()
            .filter(|i| i.clearance_class() == 1)
            .map(|i| i.id())
            .collect();
        for id in class_one.iter().step_by(10) {
            board.change_clearance_class(*id, extra);
        }
        board.autoroute_search_tree(extra);
        hash_all(
            &mut lines,
            "assigned",
            &format!("moved-{}", class_one.len().div_ceil(10)),
            &board,
        );

        board.prune_clearance_matrix(&mut |class| class == "extra");
        hash_all(
            &mut lines,
            "pruned",
            &format!("classes-{}", board.rules.clearance_matrix.class_count()),
            &board,
        );
        if let Err(message) = compare("rules-edit", &row, &lines) {
            panic!("{message}");
        }
    }

    #[test]
    fn rules_edit_trees_match_java_on_j2() {
        check(BOARDS[0]);
    }

    #[test]
    fn rules_edit_trees_match_java_on_smoothieboard() {
        check(BOARDS[1]);
    }
}
