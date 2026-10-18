//! A01.2, A01.5: `AutorouteControl`, `DestinationDistance` and the Java id
//! formulas against Java at the pin.
//!
//! - `java_ids_match_java` (kind `java-ids`): the six `getId` formulas on
//!   1,000 inputs each from `new Random(7)` (`nextInt()` over the whole
//!   range), then the ids of real Java objects built on the first 200 tree
//!   shapes of J2's default tree (item-list order).
//! - `control_matches_java` (kind `control`): every field of the control of
//!   every net, built as the batch router builds it with ripup on, then
//!   after a second `rebuild_via_info` (which only grows the radii).
//! - `distance_matches_java` (kind `distance`): per sampled connection a
//!   `DestinationDistance` joined as `MazeSearchEngine.init` joins it, at
//!   1,000 points and 1,000 boxes from `new Random(11)` on every layer.
//!
//! All on the fixture ladder (`tools/java-reference/probes/autoroute/fixtures.tsv`).

mod autoroute_common;

use autoroute_common::{
    Loaded, board_name, compare_probe, kind_fixtures, load, router_control, sample, sample_line,
    writer,
};
use freerouting::autoroute::control::AutorouteControl;
use freerouting::autoroute::distance::DestinationDistance;
use freerouting::autoroute::java_ids::{
    drill_page_id, expansion_door_id, expansion_drill_id, incomplete_room_id, obstacle_room_id,
    target_door_id,
};
use freerouting::autoroute::probe::{ProbeValue, ProbeWriter};
use freerouting::geometry::jmath::JavaRandom;
use freerouting::geometry::{FloatPoint, IntBox, PolylineShapeOps, TileShapeOps};

const ID_SAMPLES: usize = 1000;
const TREE_SHAPES: usize = 200;
const DISTANCE_SAMPLES: usize = 1000;

fn check_all(kind: &str, lines_of: impl Fn(&str) -> Vec<String>) {
    let mut failures = Vec::new();
    for dsn in kind_fixtures(kind) {
        if let Err(e) = compare_probe(kind, &dsn, &lines_of(&dsn)) {
            failures.push(e);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

// ---------------------------------------------------------------------------
// java-ids

fn java_ids_lines(dsn: &str) -> Vec<String> {
    let mut out = writer("java-ids", &board_name(dsn));
    let mut random = JavaRandom::new(7);
    for formula in ["door", "target", "obstacle", "incomplete", "drill", "page"] {
        out.record(&[("stage", formula.into()), ("samples", ID_SAMPLES.into())]);
        for _ in 0..ID_SAMPLES {
            let a = random.next_int_unbounded();
            let b = random.next_int_unbounded();
            let c = if formula == "drill" {
                random.next_int_unbounded()
            } else {
                0
            };
            let id = match formula {
                "door" => expansion_door_id(a, b),
                "target" => target_door_id(a, b),
                "obstacle" => obstacle_room_id(a, b),
                "incomplete" => incomplete_room_id(a, b),
                "drill" => expansion_drill_id(a, b, c),
                _ => drill_page_id(a, b),
            };
            out.record(&[
                ("a", a.into()),
                ("b", b.into()),
                ("c", c.into()),
                ("id", id.into()),
            ]);
        }
    }
    let loaded = load(dsn);
    let board = &loaded.board;
    let ctx = board.shape_context();
    let tree = board.search_trees().default_tree();
    let last_layer = board.layer_count() as i32 - 1;
    out.record(&[("stage", "tree-shapes".into()), ("max", TREE_SHAPES.into())]);
    let mut count = 0;
    'items: for item in board.items() {
        for (index, shape) in tree.tree_shapes(item, &ctx).into_iter().enumerate() {
            if count >= TREE_SHAPES {
                break 'items;
            }
            let Some(shape) = shape else {
                continue;
            };
            let layer = item.shape_layer(index);
            let obstacle = obstacle_room_id(item.id().0, index as i32);
            let incomplete = incomplete_room_id(shape.get_id(), layer);
            let bounds = shape.bounding_box();
            out.record(&[
                ("item", item.id().0.into()),
                ("index", index.into()),
                ("layer", layer.into()),
                ("shape", shape.get_id().into()),
                ("box", bounds.get_id().into()),
                ("obstacle", obstacle.into()),
                ("incomplete", incomplete.into()),
                (
                    "drill",
                    expansion_drill_id(bounds.ll.get_id(), 0, last_layer).into(),
                ),
                // A new DrillPage's net number is -1 (`DrillPage.java:32`).
                ("page", drill_page_id(bounds.get_id(), -1).into()),
                ("door", expansion_door_id(obstacle, incomplete).into()),
                ("target", target_door_id(item.id().0, incomplete).into()),
            ]);
            count += 1;
        }
    }
    out.into_lines()
}

#[test]
fn java_ids_match_java() {
    check_all("java-ids", java_ids_lines);
}

// ---------------------------------------------------------------------------
// control

fn control_record(stage: &str, c: &AutorouteControl, loaded: &Loaded) -> Vec<(String, ProbeValue)> {
    let costs = ProbeValue::List(
        c.trace_costs
            .iter()
            .map(|f| ProbeValue::bits_list([f.horizontal, f.vertical]))
            .collect(),
    );
    let add = ProbeValue::List(
        c.add_via_costs
            .iter()
            .map(|v| ProbeValue::ints(v.to_layer.iter().map(|x| i64::from(*x))))
            .collect(),
    );
    let masks = ProbeValue::List(
        c.via_infos
            .iter()
            .map(|m| {
                ProbeValue::List(vec![
                    m.from_layer.into(),
                    m.to_layer.into(),
                    m.attach_smd_allowed.into(),
                ])
            })
            .collect(),
    );
    let center = match &c.fanout_start_pin_center {
        None => ProbeValue::List(Vec::new()),
        Some(p) => ProbeValue::List(vec![format!("{p:?}").into()]),
    };
    let rule = &loaded.board.rules.via_rules.get(c.via_rule).name;
    let fields: Vec<(&str, ProbeValue)> = vec![
        ("stage", stage.into()),
        ("netNumber", c.net_number.into()),
        ("layerCount", c.layer_count.into()),
        ("traceCosts", costs),
        (
            "bendCosts",
            ProbeValue::bits_list(c.bend_costs.iter().copied()),
        ),
        ("withNeckdown", c.with_neckdown.into()),
        (
            "layerActive",
            ProbeValue::bools(c.layer_active.iter().copied()),
        ),
        (
            "traceHalfWidth",
            ProbeValue::ints(c.trace_half_width.iter().map(|x| i64::from(*x))),
        ),
        (
            "compensatedTraceHalfWidth",
            ProbeValue::ints(c.compensated_trace_half_width.iter().map(|x| i64::from(*x))),
        ),
        (
            "viaRadii",
            ProbeValue::bits_list(c.via_radii.iter().copied()),
        ),
        ("addViaCosts", add),
        (
            "traceClearanceClassIndex",
            c.trace_clearance_class_index.into(),
        ),
        ("viasAllowed", c.vias_allowed.into()),
        ("attachSmdAllowed", c.attach_smd_allowed.into()),
        ("minNormalViaCost", c.min_normal_via_cost.into()),
        ("ripupAllowed", c.ripup_allowed.into()),
        ("ripupCosts", c.ripup_costs.into()),
        ("ripupPassNo", c.ripup_pass_no.into()),
        ("isFanout", c.is_fanout.into()),
        ("isPlaneFanout", c.is_plane_fanout.into()),
        ("fanoutStartPinCenter", center),
        ("fanoutStartPinLayer", c.fanout_start_pin_layer.into()),
        ("removeUnconnectedVias", c.remove_unconnected_vias.into()),
        ("viaRule", rule.as_str().into()),
        ("viaClearanceClass", c.via_clearance_class.into()),
        ("viaInfos", masks),
        ("viaLowerBound", c.via_lower_bound.into()),
        ("viaUpperBound", c.via_upper_bound.into()),
        ("maxViaRadius", c.max_via_radius.into()),
        ("tidyRegionWidth", c.tidy_region_width.into()),
        ("pullTightAccuracy", c.pull_tight_accuracy.into()),
        (
            "maxShoveTraceRecursionDepth",
            c.max_shove_trace_recursion_depth.into(),
        ),
        (
            "maxShoveViaRecursionDepth",
            c.max_shove_via_recursion_depth.into(),
        ),
        (
            "maxSpringOverRecursionDepth",
            c.max_spring_over_recursion_depth.into(),
        ),
        ("minCheapViaCost", c.min_cheap_via_cost.into()),
    ];
    fields
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

fn record_owned(out: &mut ProbeWriter, fields: &[(String, ProbeValue)]) {
    let borrowed: Vec<(&str, ProbeValue)> = fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.clone()))
        .collect();
    out.record(&borrowed);
}

fn plane_net(loaded: &Loaded, net: i32) -> bool {
    loaded
        .board
        .rules
        .nets
        .get(net)
        .is_some_and(|n| n.contains_plane())
}

fn control_lines(dsn: &str) -> Vec<String> {
    let loaded = load(dsn);
    let board = &loaded.board;
    let router = &loaded.router;
    let max_net = board.rules.nets.max_net_number();
    let mut out = writer("control", &board_name(dsn));
    out.record(&[
        ("stage", "board".into()),
        ("maxNetNumber", max_net.into()),
        ("layerCount", board.layer_count().into()),
        ("viaCosts", router.via_costs().into()),
        ("planeViaCosts", router.plane_via_costs().into()),
        ("startRipupCosts", router.start_ripup_costs().into()),
        ("fanout", loaded.settings.fanout.enabled.into()),
    ]);
    for net in 1..=max_net {
        let mut control = match router_control(&loaded, net, true) {
            Ok(control) => control,
            Err(e) => {
                out.record(&[
                    ("stage", format!("net:{net}").into()),
                    ("exception", e.java_exception().into()),
                ]);
                continue;
            }
        };
        record_owned(
            &mut out,
            &control_record(&format!("net:{net}"), &control, &loaded),
        );
        let via_costs = if plane_net(&loaded, net) {
            router.plane_via_costs()
        } else {
            router.via_costs()
        };
        control
            .rebuild_via_info(board, via_costs, net)
            .expect("a second rebuild succeeds where the first did");
        out.record(&[
            ("stage", format!("rebuilt:{net}").into()),
            (
                "viaRadii",
                ProbeValue::bits_list(control.via_radii.iter().copied()),
            ),
            ("maxViaRadius", control.max_via_radius.into()),
            ("minNormalViaCost", control.min_normal_via_cost.into()),
            ("minCheapViaCost", control.min_cheap_via_cost.into()),
        ]);
    }
    out.into_lines()
}

#[test]
fn control_matches_java() {
    check_all("control", control_lines);
}

// ---------------------------------------------------------------------------
// distance

fn distance_lines(dsn: &str) -> Vec<String> {
    let mut loaded = load(dsn);
    let conns = sample(&loaded.board);
    let mut out = writer("distance", &board_name(dsn));
    sample_line(&mut out, &loaded.board, &conns);
    let bb = loaded.board.bounding_box();
    let layers = loaded.board.layer_count();
    for conn in &conns {
        let stage: ProbeValue = format!("conn:{}", conn.net).into();
        if let Some(state) = conn.state {
            out.record(&[
                ("stage", stage),
                ("net", conn.net.into()),
                ("state", state.java_name().into()),
            ]);
            continue;
        }
        let control = match router_control(&loaded, conn.net, false) {
            Ok(control) => control,
            Err(e) => {
                out.record(&[
                    ("stage", stage),
                    ("net", conn.net.into()),
                    ("exception", e.java_exception().into()),
                ]);
                continue;
            }
        };
        let class = control.trace_clearance_class_index;
        loaded.board.autoroute_search_tree(class);
        let board = &loaded.board;
        let tree = board
            .search_trees()
            .tree_by_class(class)
            .expect("the autoroute tree was just created");
        let ctx = board.shape_context();
        let mut distance = DestinationDistance::new(
            &control.trace_costs,
            &control.layer_active,
            control.min_normal_via_cost,
            control.min_cheap_via_cost,
        );
        // PARITY: MazeSearchEngine.init, the destination items in set order,
        // each tree shape's box on its layer.
        let mut joined = 0;
        for id in &conn.dest {
            let item = board.item(*id).expect("destination item");
            for (index, shape) in tree.tree_shapes(item, &ctx).into_iter().enumerate() {
                if let Some(shape) = shape {
                    distance.join(&shape.bounding_box(), item.shape_layer(index));
                    joined += 1;
                }
            }
        }
        out.record(&[
            ("stage", stage),
            ("net", conn.net.into()),
            ("item", conn.item.0.into()),
            ("joined", joined.into()),
        ]);
        sample_distances(&mut out, &distance, &bb, layers);
    }
    out.into_lines()
}

/// The points and boxes of one connection, from `new Random(11)`.
fn sample_distances(out: &mut ProbeWriter, d: &DestinationDistance, bb: &IntBox, layers: usize) {
    let mut random = JavaRandom::new(11);
    let width = f64::from(bb.ur.x.wrapping_sub(bb.ll.x));
    let height = f64::from(bb.ur.y.wrapping_sub(bb.ll.y));
    for _ in 0..DISTANCE_SAMPLES {
        let x = f64::from(bb.ll.x) + random.next_double() * width;
        let y = f64::from(bb.ll.y) + random.next_double() * height;
        let p = FloatPoint::new(x, y);
        let r = (0..layers).map(|layer| d.calculate_point(&p, layer as i32));
        out.record(&[
            ("x", p.x.into()),
            ("y", p.y.into()),
            ("d", ProbeValue::bits_list(r)),
        ]);
    }
    let x_bound = bb.ur.x - bb.ll.x + 1;
    let y_bound = bb.ur.y - bb.ll.y + 1;
    for _ in 0..DISTANCE_SAMPLES {
        let x1 = bb.ll.x + random.next_int(x_bound);
        let y1 = bb.ll.y + random.next_int(y_bound);
        let x2 = bb.ll.x + random.next_int(x_bound);
        let y2 = bb.ll.y + random.next_int(y_bound);
        let b = IntBox::new(x1.min(x2), y1.min(y2), x1.max(x2), y1.max(y2));
        let r = (0..layers).map(|layer| d.calculate_box(&b, layer as i32));
        let cheap = (0..layers).map(|layer| d.calculate_cheap_distance(&b, layer as i32));
        out.record(&[
            (
                "box",
                ProbeValue::ints([b.ll.x, b.ll.y, b.ur.x, b.ur.y].map(i64::from)),
            ),
            ("d", ProbeValue::bits_list(r)),
            ("cheap", ProbeValue::bits_list(cheap)),
        ]);
    }
}

#[test]
fn distance_matches_java() {
    check_all("distance", distance_lines);
}

// ---------------------------------------------------------------------------
// A01.3 unit tests on a small synthetic board: three layers (the middle one a
// power plane of net GND), an SMD net and a through-hole net, two via types
// (V400 with `(attach on)`, and `via_at_smd on`).

const UNIT_BOARD: &str = r#"(pcb unit-control
  (parser (string_quote ") (space_in_quoted_tokens on))
  (resolution um 10)
  (unit um)
  (structure
    (layer F.Cu (type signal) (property (index 0)))
    (layer GND (type power) (property (index 1)))
    (layer B.Cu (type signal) (property (index 2)))
    (boundary (path pcb 0  0 0  20000 0  20000 20000  0 20000  0 0))
    (plane GND (polygon GND 0  100 100  19900 100  19900 19900  100 19900))
    (via V600 V400)
    (control (via_at_smd on))
    (rule (width 250) (clearance 200))
  )
  (placement
    (component SMD (place U1 2000 2000 front 0) (place U2 8000 2000 front 0))
    (component THT (place J1 2000 8000 front 0) (place J2 8000 8000 front 0))
  )
  (library
    (image SMD (pin SmdPad 1 0 0))
    (image THT (pin ThPad 1 0 0))
    (padstack SmdPad (shape (rect F.Cu -400 -400 400 400)) (attach off))
    (padstack ThPad (shape (circle F.Cu 800)) (shape (circle GND 800))
      (shape (circle B.Cu 800)) (attach off))
    (padstack V600 (shape (circle F.Cu 600)) (shape (circle GND 600))
      (shape (circle B.Cu 600)) (attach off))
    (padstack V400 (shape (circle F.Cu 400)) (shape (circle GND 400))
      (shape (circle B.Cu 400)) (attach on))
  )
  (network
    (net SMDNET (pins U1-1 U2-1))
    (net THTNET (pins J1-1 J2-1))
    (net GND)
    (class kicad_default "" SMDNET THTNET GND (circuit (use_via V600 V400)))
  )
)
"#;

fn unit_board() -> Loaded {
    use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
    use freerouting::settings::ResolveContext;
    let mut input = JobInput::new(UNIT_BOARD.as_bytes().to_vec(), "unit-control");
    input.rules = RulesChoice::None;
    let prepared = prepare_job(&input, &ResolveContext { cores: 8 }).expect("the unit board loads");
    Loaded {
        board: prepared.board,
        settings: prepared.settings,
        router: prepared.board_settings,
    }
}

fn net_number(loaded: &Loaded, name: &str) -> i32 {
    loaded
        .board
        .rules
        .nets
        .get_by_name(name, 1)
        .map(|n| n.net_number)
        .unwrap_or_else(|| panic!("net {name}"))
}

#[test]
fn control_plane_layer_is_forced_inactive() {
    use freerouting::settings::board::BoardRouterSettings;
    let mut loaded = unit_board();
    // Mark every layer active in the settings, the plane layer included.
    for layer in loaded.settings.layers.as_mut().expect("layer settings") {
        layer.routable = Some(true);
    }
    let router = BoardRouterSettings::from_settings(&loaded.settings);
    assert!(
        router.layer_active(1),
        "the settings say the plane is active"
    );
    let net = net_number(&loaded, "THTNET");
    let control = AutorouteControl::for_net_default(&loaded.board, net, &router).unwrap();
    assert_eq!(control.layer_active, [true, false, true]);
    assert_eq!(
        control.warnings,
        [
            "Layer 'GND' is a dedicated power plane and cannot be routed. \
          Forcing active state to false."
        ]
    );
    // A plane layer the settings mark inactive gives no warning.
    loaded.settings.layers.as_mut().unwrap()[1].routable = Some(false);
    let router = BoardRouterSettings::from_settings(&loaded.settings);
    let control = AutorouteControl::for_net_default(&loaded.board, net, &router).unwrap();
    assert_eq!(control.layer_active, [true, false, true]);
    assert!(control.warnings.is_empty());
}

#[test]
fn control_pure_smd_net_has_cheaper_vias() {
    let loaded = unit_board();
    let router = &loaded.router;
    let smd =
        AutorouteControl::for_net_default(&loaded.board, net_number(&loaded, "SMDNET"), router)
            .unwrap();
    let tht =
        AutorouteControl::for_net_default(&loaded.board, net_number(&loaded, "THTNET"), router)
            .unwrap();
    assert_eq!(smd.max_via_radius, tht.max_via_radius);
    assert!(tht.max_via_radius > 1.0);
    let via_costs = f64::from(router.via_costs());
    // Java's order: factor = max(maxViaRadius, 1), times 0.1 for a pure SMD
    // net, then viaCosts * factor, then 0.8 * that.
    assert_eq!(
        tht.min_normal_via_cost.to_bits(),
        (via_costs * tht.max_via_radius).to_bits()
    );
    assert_eq!(
        smd.min_normal_via_cost.to_bits(),
        (via_costs * (tht.max_via_radius * 0.1)).to_bits()
    );
    assert_eq!(
        smd.min_cheap_via_cost.to_bits(),
        (0.8 * smd.min_normal_via_cost).to_bits()
    );
    // V400 has `(attach on)`, V600 not.
    assert!(tht.attach_smd_allowed);
    assert_eq!(
        tht.via_infos
            .iter()
            .map(|m| m.attach_smd_allowed)
            .collect::<Vec<_>>(),
        [false, true]
    );
}

#[test]
fn control_missing_net_falls_back() {
    use freerouting::autoroute::control::ControlError;
    let loaded = unit_board();
    let rules = &loaded.board.rules;
    // Net 0 does not exist: clearance class 1, the first via rule, and the
    // trace widths of net 1.
    let control = AutorouteControl::for_net_default(&loaded.board, 0, &loaded.router).unwrap();
    assert_eq!(control.net_number, 0);
    assert_eq!(control.trace_clearance_class_index, 1);
    assert_eq!(Some(control.via_rule), rules.via_rules.nth(0));
    let net1: Vec<i32> = (0..3)
        .map(|layer| rules.trace_half_width(1, layer))
        .collect();
    assert_eq!(control.trace_half_width, net1);
    assert!(net1.iter().all(|w| *w > 0));
    // A missing positive net throws in Java (`getTraceHalfWidth`).
    assert_eq!(
        AutorouteControl::for_net_default(&loaded.board, 99, &loaded.router),
        Err(ControlError::NoSuchNet(99))
    );
}

#[test]
fn control_rebuild_resets_the_via_radii() {
    use freerouting::rules::ViaRule;
    let mut loaded = unit_board();
    let net = net_number(&loaded, "THTNET");
    let small = {
        let rules = &mut loaded.board.rules;
        let v400 = rules.via_infos.get_by_name("V400").expect("via V400");
        let mut rule = ViaRule::new("small");
        rule.append_via(v400);
        rules.via_rules.add(rule)
    };
    let mut control =
        AutorouteControl::for_net_default(&loaded.board, net, &loaded.router).unwrap();
    let max_radius = control.max_via_radius;
    // The rule with only the small via, built fresh, has smaller radii.
    let class = loaded.board.rules.nets.get(net).unwrap().net_class();
    let mut small_board = unit_board();
    small_board.board.rules.via_rules = loaded.board.rules.via_rules.clone();
    small_board
        .board
        .rules
        .net_classes
        .get_mut(class)
        .set_via_rule(Some(small));
    let fresh =
        AutorouteControl::for_net_default(&small_board.board, net, &small_board.router).unwrap();
    assert!(
        fresh.max_via_radius < max_radius,
        "{} < {max_radius}",
        fresh.max_via_radius
    );
    // A rebuild must match a fresh control using the same smaller rule.
    control.via_rule = small;
    control
        .rebuild_via_info(&loaded.board, loaded.router.via_costs(), net)
        .unwrap();
    assert_eq!(control.via_infos.len(), 1);
    assert_eq!(control.via_radii, fresh.via_radii);
    assert_eq!(control.max_via_radius, fresh.max_via_radius);
    assert_eq!(
        control.min_normal_via_cost,
        f64::from(loaded.router.via_costs()) * fresh.max_via_radius
    );
}
