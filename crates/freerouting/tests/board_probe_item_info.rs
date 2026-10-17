//! The item info fields against Java (B08.2).
//!
//! The probe `item-info` (`tools/java-reference/probes/board/ItemInfoProbe.java`) calls
//! `Item.printInfo(printer, Locale.ENGLISH)` on a selection of items of each Java class with a
//! recording `ItemInfoPrinter` and writes one line per value the item prints (see the probe's
//! doc comment): links with their text, points and numbers as raw bits, the text values that
//! follow a label, `FixedState` names, contact ids. The localized label words are skipped, so
//! the comparison is of data. This test takes the item ids Java selected, builds the same lines
//! from `BasicBoard::item_info` and compares them line for line.
//!
//! Boards: Issue026-J2_reference (pins, vias, traces, keepouts of a component, component
//! outlines, the board outline), Issue145-smoothieboard (also a conduction area and items with
//! contacts and clearance violations) and Issue039-bug-design (via keepouts). No fixture has a
//! `ComponentObstacleArea` or an area with holes among its selection, so those fields are
//! checked by unit tests below, from the Java source only.

mod board_probe;

use std::collections::BTreeSet;

use board_probe::{Line, compare, fixture, header, repo_root};
use freerouting::board::items::{AreaKind, Item, ItemId, ItemKind};
use freerouting::board::structure::FixedState;
use freerouting::board::{InfoField, RoutingBoard};

const KIND: &str = "item-info";

fn java_type(item: &Item) -> &'static str {
    match &item.kind {
        ItemKind::Pin(_) => "Pin",
        ItemKind::Via(_) => "Via",
        ItemKind::Trace(_) => "PolylineTrace",
        ItemKind::Area(area) => match area.kind {
            AreaKind::Obstacle => "ObstacleArea",
            AreaKind::ViaObstacle => "ViaObstacleArea",
            AreaKind::TraceObstacle => "TraceObstacleArea",
            AreaKind::ComponentObstacle => "ComponentObstacleArea",
            AreaKind::Conduction { .. } => "ConductionArea",
        },
        ItemKind::ComponentOutline(_) => "ComponentOutline",
        ItemKind::BoardOutline(_) => "BoardOutline",
    }
}

/// `NumberFormat.getInstance(Locale.ENGLISH).format(n)` for a count.
fn english_number(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn fixed_name(state: FixedState) -> &'static str {
    match state {
        FixedState::Unfixed => "UNFIXED",
        FixedState::ShoveFixed => "SHOVE_FIXED",
        FixedState::UserFixed => "USER_FIXED",
        FixedState::SystemFixed => "SYSTEM_FIXED",
    }
}

/// The lines of one item, as the probe's recording printer writes them.
fn item_lines(board: &RoutingBoard, id: ItemId) -> Vec<String> {
    let item = board.item(id).expect("item");
    let info = board.item_info(id).expect("info");
    let mut seq = 0;
    let mut line = |op: &str| {
        let l = Line::new()
            .str("stage", "info")
            .num("item", i64::from(id.0))
            .num("seq", seq)
            .str("op", op);
        seq += 1;
        l
    };
    let mut lines = vec![line("head").str("class", java_type(item)).end()];
    for field in &info.fields {
        lines.push(match field {
            InfoField::Component { name, .. } => line("link")
                .str("name", name)
                .str("object", "Component")
                .end(),
            InfoField::PinName(name) => line("text").str("value", name).end(),
            InfoField::Padstack { name, .. } => line("link")
                .str("name", name)
                .str("object", "Padstack")
                .end(),
            InfoField::Location(p) | InfoField::From(p) | InfoField::To(p) => {
                line("point").dbl("x", p.x).dbl("y", p.y).end()
            }
            InfoField::Layer(name) => line("text").str("value", name).end(),
            InfoField::Width(v) | InfoField::Length(v) => line("number").dbl("value", *v).end(),
            InfoField::HoleCount(n) => line("text").str("value", &english_number(*n)).end(),
            InfoField::ClearanceClass { name, .. } => {
                line("link").str("name", name).str("object", "Row").end()
            }
            InfoField::Fixed(state) => line("fixed").str("value", fixed_name(*state)).end(),
            InfoField::Net { name, .. } => {
                line("link").str("name", name).str("object", "Net").end()
            }
            InfoField::Contacts(ids) => {
                let list: Vec<String> = ids.iter().map(|i| i.0.to_string()).collect();
                line("items")
                    .num("count", ids.len() as i64)
                    .raw("ids", &format!("[{}]", list.join(",")))
                    .end()
            }
            InfoField::ClearanceViolations(v) => line("objects").num("count", v.len() as i64).end(),
        });
    }
    if !matches!(item.kind, ItemKind::ComponentOutline(_)) {
        lines.push(line("newline").end());
    }
    lines
}

/// The item ids Java selected, in file order.
fn java_items(board: &str) -> Vec<ItemId> {
    let path = repo_root()
        .join("fixtures/reference/probe")
        .join(KIND)
        .join(format!("{board}.jsonl"));
    let text = std::fs::read_to_string(&path).expect("Java output");
    let mut seen = BTreeSet::new();
    let mut ids = Vec::new();
    for line in text.lines().skip(1) {
        let rest = line
            .strip_prefix("{\"stage\":\"info\",\"item\":")
            .expect("info line");
        let id: i32 = rest.split(',').next().unwrap().parse().unwrap();
        if seen.insert(id) {
            ids.push(ItemId(id));
        }
    }
    ids
}

fn check(board_name: &str) {
    let row = fixture(board_name);
    let mut board = board_probe::load(&row);
    board_probe::import_session(&mut board, &row);
    let mut lines = vec![header(KIND, &row.board)];
    let ids = java_items(board_name);
    assert!(ids.len() > 10, "{board_name}: {} items", ids.len());
    for id in ids {
        lines.extend(item_lines(&board, id));
    }
    if let Err(message) = compare(KIND, &row, &lines) {
        panic!("{message}");
    }
}

#[test]
fn item_info_matches_java_on_j2() {
    check("Issue026-J2_reference");
}

#[test]
fn item_info_matches_java_on_smoothieboard() {
    check("Issue145-smoothieboard");
}

#[test]
fn item_info_matches_java_on_bug_design() {
    check("Issue039-bug-design");
}

// ---------------------------------------------------------------------------
// Field order and presence rules that no fixture selection exercises, from the Java source
// (`Item.java:1169-1248`, `ObstacleArea.java:311-345`, `ViaObstacleArea.java:113`,
// `ComponentObstacleArea.java:90`).

mod synthetic {
    use freerouting::board::items::BoardItemType;
    use freerouting::board::state::Communication;
    use freerouting::board::structure::{FixedState, Layer, LayerStructure};
    use freerouting::board::{InfoField, RoutingBoard};
    use freerouting::geometry::{Area, IntBox, PolylineArea, PolylineShape, Shape, TileShape};
    use freerouting::rules::{BoardRules, ClearanceMatrix};

    fn board() -> RoutingBoard {
        let layers = LayerStructure::new(vec![Layer::new("Top", true)]);
        let matrix = ClearanceMatrix::default_instance(layers.count(), 10);
        let mut rules = BoardRules::new(layers.clone(), matrix);
        rules.create_default_net_class();
        let outline = IntBox::new(0, 0, 100_000, 100_000);
        let mut board = RoutingBoard::new(
            outline,
            layers,
            vec![PolylineShape::Tile(TileShape::from(outline))],
            0,
            rules,
            Communication::default(),
        );
        let class = board.rules.default_net_class();
        board.rules.nets.add("N", 1, false, class);
        board
    }

    fn square(x: i32, y: i32, size: i32) -> PolylineShape {
        PolylineShape::Tile(TileShape::from(IntBox::new(x, y, x + size, y + size)))
    }

    fn kinds(fields: &[InfoField]) -> Vec<&'static str> {
        fields
            .iter()
            .map(|f| match f {
                InfoField::Component { .. } => "component",
                InfoField::PinName(_) => "pin",
                InfoField::Padstack { .. } => "padstack",
                InfoField::Location(_) => "location",
                InfoField::From(_) => "from",
                InfoField::To(_) => "to",
                InfoField::Layer(_) => "layer",
                InfoField::Width(_) => "width",
                InfoField::Length(_) => "length",
                InfoField::HoleCount(_) => "holes",
                InfoField::ClearanceClass { .. } => "class",
                InfoField::Fixed(_) => "fixed",
                InfoField::Net { .. } => "net",
                InfoField::Contacts(_) => "contacts",
                InfoField::ClearanceViolations(_) => "violations",
            })
            .collect()
    }

    #[test]
    fn conduction_area_with_a_hole_lists_holes_before_the_layer() {
        let mut board = board();
        let area = Area::from(PolylineArea::new(
            square(10_000, 10_000, 40_000),
            vec![
                square(20_000, 20_000, 10_000),
                square(35_000, 35_000, 5_000),
            ],
        ));
        let id = board.insert_conduction_area(area, 0, &[1], 1, true, FixedState::UserFixed);
        let info = board.item_info(id).unwrap();
        assert_eq!(info.kind, BoardItemType::ConductionArea);
        assert_eq!(
            kinds(&info.fields),
            ["location", "holes", "layer", "class", "fixed", "net"]
        );
        assert!(matches!(info.fields[1], InfoField::HoleCount(2)));
        assert!(matches!(&info.fields[2], InfoField::Layer(n) if n == "Top"));
        assert!(
            matches!(&info.fields[3], InfoField::ClearanceClass { index: 1, name } if name == "default")
        );
        assert!(matches!(&info.fields[5], InfoField::Net { number: 1, name } if name == "N"));
    }

    #[test]
    fn keepouts_list_only_what_their_class_prints() {
        let mut board = board();
        let shape = || {
            Area::from(Shape::from(TileShape::from(IntBox::new(
                5_000, 5_000, 9_000, 9_000,
            ))))
        };
        // `ObstacleArea.printInfo`: clearance class, fixed state, violations.
        let keepout = board.insert_obstacle(shape(), 0, 1, FixedState::SystemFixed);
        assert_eq!(
            kinds(&board.item_info(keepout).unwrap().fields),
            ["location", "layer", "class", "fixed"]
        );
        // `ViaObstacleArea` and `ComponentObstacleArea` print no fixed state.
        let via_keepout = board.insert_via_obstacle(shape(), 0, 1, FixedState::SystemFixed);
        assert_eq!(
            kinds(&board.item_info(via_keepout).unwrap().fields),
            ["location", "layer", "class"]
        );
        let place_keepout = board.insert_component_obstacle(shape(), 0, 0, FixedState::SystemFixed);
        let info = board.item_info(place_keepout).unwrap();
        assert_eq!(info.kind, BoardItemType::ComponentObstacleArea);
        assert_eq!(kinds(&info.fields), ["location", "layer"]);
    }

    #[test]
    fn item_not_on_the_board_has_no_info() {
        let board = board();
        assert!(
            board
                .item_info(freerouting::board::items::ItemId(777_777))
                .is_none()
        );
    }
}
