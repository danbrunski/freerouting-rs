//! G04: the board windows (nets, components, library), the visibility
//! sliders, the virtual layers, units, rotation and mirroring, the colour
//! manager, the status bar, the log and About.
//!
//! G04.3 (the item info window) waits for B08.2's `item_info`; its test is
//! not here yet.

mod common;

use std::collections::{BTreeSet, HashMap};

use common::{SIZE, app_harness, fixtures, plain_harness, rendering_enabled, wait_for_load};
use eframe::egui::accesskit::Role;
use eframe::egui::{Color32, Event, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use freerouting::board::BasicBoard;
use freerouting::board::items::{ItemId, ItemKind};
use freerouting::board::structure::Unit;
use freerouting::drc::DesignRulesChecker;
use freerouting_gui::App;
use freerouting_gui::args::Args;
use freerouting_gui::canvas::chunks::{KINDS, kind_index, slot_index};
use freerouting_gui::canvas::{self, color_table};
use freerouting_gui::commands::{Command, ListKind, WindowKind};
use freerouting_gui::look::{ItemColor, Object, OtherColor, virtual_index};
use freerouting_gui::scene::{Kind, Slot};
use freerouting_gui::view::Mirror;
use freerouting_gui::viewport::hover_of;
use freerouting_gui::{panels, text};

const ISSUE159: &str = "Issue159-setonix_2hp-pcb.dsn";
const ISSUE690: &str = "Issue690-kit-dev-coldfire-xilinx_5213.dsn";
const ISSUE754: &str = "Issue754-avionics_hub.dsn";

/// Runs frames until the UI is still. A background worker that asks for a
/// repaint right then must not fail the test, so the step limit is not an
/// error here.
macro_rules! settle {
    ($h:expr) => {
        let _ = $h.run_ok();
    };
}

fn exec(h: &mut Harness<'_, App>, command: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().execute(&ctx, command);
    settle!(h);
}

fn board<'a>(h: &'a Harness<'_, App>) -> &'a BasicBoard {
    h.state()
        .loaded()
        .expect("board loaded")
        .session
        .board
        .basic()
}

fn row_texts(h: &Harness<'_, App>, kind: ListKind) -> Vec<String> {
    h.state()
        .lists()
        .built_rows(kind)
        .unwrap_or_default()
        .iter()
        .map(|r| r.text.clone())
        .collect()
}

/// The net number of a net row (`Net #n (name) - Class: c`).
fn net_number(text: &str) -> i32 {
    text.strip_prefix("Net #")
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .expect("a net row")
}

// ---------------------------------------------------------------------------
// G04.1: Info > Nets
// ---------------------------------------------------------------------------

#[test]
fn nets_filter_and_select() {
    let mut h = plain_harness(&[ISSUE754]);
    let nets: Vec<(i32, String)> = board(&h)
        .rules
        .nets
        .iter()
        .map(|n| (n.net_number, n.name.clone()))
        .collect();
    exec(&mut h, Command::ShowList(ListKind::Nets));
    // Every net has a row in Java's format, in net number order.
    let rows = row_texts(&h, ListKind::Nets);
    assert_eq!(rows.len(), nets.len());
    let class = board(&h)
        .rules
        .net_classes
        .get(board(&h).rules.nets.get(nets[0].0).unwrap().net_class())
        .name()
        .to_string();
    assert_eq!(
        rows[0],
        format!("Net #{} ({}) - Class: {class}", nets[0].0, nets[0].1)
    );
    assert!(
        rows.windows(2)
            .all(|w| net_number(&w[0]) < net_number(&w[1]))
    );

    // Typing GND leaves the nets whose name contains it (ignoring case).
    let filter = h.get_by_role(Role::TextInput);
    filter.click();
    filter.type_text("GND");
    settle!(h);
    let expected: Vec<i32> = nets
        .iter()
        .filter(|(_, name)| name.to_lowercase().contains("gnd"))
        .map(|(n, _)| *n)
        .collect();
    eprintln!(
        "nets_filter_and_select: {} nets, {} match GND",
        nets.len(),
        expected.len()
    );
    assert!(!expected.is_empty(), "Issue754 has no GND net");
    assert!(expected.len() < nets.len());
    let visible: Vec<i32> = h
        .state()
        .lists()
        .visible_rows(ListKind::Nets)
        .iter()
        .map(|r| net_number(&r.text))
        .collect();
    assert_eq!(visible, expected);

    // Choose the first and Select on Board: the selection is the net's
    // connectable items.
    let first = h.state().lists().visible_rows(ListKind::Nets)[0].clone();
    h.get_by_label(&first.text).click();
    settle!(h);
    h.get_by_label(text::NETS_SELECT).click();
    settle!(h);
    let want: BTreeSet<ItemId> = board(&h)
        .connectable_items(net_number(&first.text))
        .into_iter()
        .collect();
    assert!(!want.is_empty());
    let got: BTreeSet<ItemId> = h.state().selection().ids().into_iter().collect();
    assert_eq!(got, want);
}

#[test]
fn nets_incomplete_only() {
    let mut h = plain_harness(&[ISSUE754]);
    exec(&mut h, Command::ShowList(ListKind::Nets));
    h.get_by_label(text::NETS_INCOMPLETE_ONLY).click();
    settle!(h);
    // The rows wait for the ratsnest.
    let start = std::time::Instant::now();
    while !h
        .state()
        .analysis()
        .is_idle(&h.state().loaded().unwrap().session)
    {
        assert!(
            start.elapsed().as_secs() < 300,
            "the analysis did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
        h.step();
    }
    settle!(h);
    let mut checker = DesignRulesChecker::new(board(&h));
    let expected: Vec<i32> = board(&h)
        .rules
        .nets
        .iter()
        .map(|n| n.net_number)
        .filter(|n| checker.incomplete_count_of(*n) > 0)
        .collect();
    eprintln!(
        "nets_incomplete_only: {} of {} nets have incomplete connections",
        expected.len(),
        board(&h).rules.nets.iter().count()
    );
    assert!(!expected.is_empty());
    let rows: Vec<i32> = row_texts(&h, ListKind::Nets)
        .iter()
        .map(|t| net_number(t))
        .collect();
    assert_eq!(rows, expected);
    assert!(h.state().lists().nets_incomplete_only());
}

// ---------------------------------------------------------------------------
// G04.2: components, packages, padstacks
// ---------------------------------------------------------------------------

/// The box of the visible part of the canvas, in millimetres.
fn visible_box(h: &Harness<'_, App>) -> (freerouting_gui::scene::P, freerouting_gui::scene::P) {
    let view = h.state().view();
    let camera = view.camera.expect("camera");
    (
        camera.to_board(view.canvas, view.canvas.left_bottom()),
        camera.to_board(view.canvas, view.canvas.right_top()),
    )
}

/// Java's `WindowComponents.fillList` lists `components.count()` components
/// (all of them, not only the placed ones; on Issue159 they are the same
/// set), `WindowPackages` and `WindowPadstacks` list the library.
#[test]
fn library_window_counts() {
    let mut h = plain_harness(&[ISSUE159]);
    let (components, packages, padstacks, placed) = {
        let b = board(&h);
        (
            b.components.count(),
            b.library.packages.count(),
            b.library.padstacks.count(),
            b.components.iter().filter(|c| c.is_placed()).count(),
        )
    };
    eprintln!(
        "library: {components} components ({placed} placed), {packages} packages, {padstacks} padstacks"
    );
    assert_eq!(placed, components, "every component of Issue159 is placed");
    for kind in [
        ListKind::Components,
        ListKind::Packages,
        ListKind::Padstacks,
    ] {
        exec(&mut h, Command::ShowList(kind));
    }
    assert_eq!(row_texts(&h, ListKind::Components).len(), components);
    assert_eq!(row_texts(&h, ListKind::Packages).len(), packages);
    assert_eq!(row_texts(&h, ListKind::Padstacks).len(), padstacks);
    // Sorted by name without regard to case, as Java sorts them.
    for kind in [
        ListKind::Components,
        ListKind::Packages,
        ListKind::Padstacks,
    ] {
        let names = row_texts(&h, kind);
        let mut sorted = names.clone();
        sorted.sort_by_key(|n| n.to_lowercase());
        assert_eq!(names, sorted, "{kind:?}");
    }

    // Select on the first component zooms so that its items are visible.
    let mut h = plain_harness(&[ISSUE159]);
    exec(&mut h, Command::ShowList(ListKind::Components));
    let first = h.state().lists().built_rows(ListKind::Components).unwrap()[0].clone();
    assert!(!first.items.is_empty());
    h.get_by_label(&first.text).click();
    settle!(h);
    h.get_by_label(text::SELECT).click();
    settle!(h);
    let got: BTreeSet<ItemId> = h.state().selection().ids().into_iter().collect();
    assert_eq!(got, first.items.iter().copied().collect());
    let b = board(&h);
    let frame = freerouting_gui::overlays::Frame::new(b);
    let want = frame.bounds(&b.bounding_box_of(&first.items)).unwrap();
    let (min, max) = visible_box(&h);
    assert!(
        min.x <= want.min.x && min.y <= want.min.y && max.x >= want.max.x && max.y >= want.max.y,
        "visible ({min:?} - {max:?}) must contain {want:?}"
    );
}

#[test]
fn package_and_padstack_select() {
    let mut h = plain_harness(&[ISSUE159]);
    exec(&mut h, Command::ShowList(ListKind::Padstacks));
    let rows = h
        .state()
        .lists()
        .built_rows(ListKind::Padstacks)
        .unwrap()
        .to_vec();
    // The pins and vias of a padstack, as Java's `selectInstances` takes them.
    let row = rows
        .iter()
        .find(|r| !r.items.is_empty())
        .expect("a used padstack");
    let b = board(&h);
    for id in &row.items {
        let item = b.item(*id).unwrap();
        assert!(item.is_pin() || item.is_via());
    }
    let used: usize = rows.iter().map(|r| r.items.len()).sum();
    let drill_items = b.items().filter(|i| i.drill().is_some()).count();
    assert_eq!(
        used, drill_items,
        "every pin and via has exactly one padstack"
    );
}

// ---------------------------------------------------------------------------
// G04.4: visibility sliders
// ---------------------------------------------------------------------------

fn layer_count(h: &Harness<'_, App>) -> usize {
    h.state().loaded().unwrap().scene.layers.len()
}

/// The scene items the plan of the current view draws, by slot.
fn drawn_items(h: &Harness<'_, App>) -> HashMap<Slot, usize> {
    let loaded = h.state().loaded().unwrap();
    let vis = h.state().view().visibility();
    let layers = loaded.scene.layers.len();
    let everywhere = loaded.scene.bounds;
    loaded.tess.with(|set| {
        let plan = canvas::plan(set, &vis, &everywhere, false);
        let chunks: BTreeSet<u32> = plan.draws.iter().map(|d| d.chunk).collect();
        let mut by_slot: HashMap<Slot, usize> = HashMap::new();
        for id in chunks {
            let chunk = set.chunk(id);
            assert!(slot_index(chunk.slot, layers) < canvas::chunks::slot_count(layers));
            *by_slot.entry(chunk.slot).or_default() += chunk.entries.len();
        }
        by_slot
    })
}

#[test]
fn visibility_slider_levels() {
    let mut h = app_harness(&[ISSUE690]);
    let layers = layer_count(&h);
    assert!(layers >= 2);
    let trace = kind_index(Kind::Trace);
    let mut drawn = Vec::new();
    let mut colours = Vec::new();
    for level in [0u8, 50, 100] {
        exec(&mut h, Command::SetLayerLevel(0, level));
        // The slider snaps to its steps and 0 hides the layer.
        assert_eq!(h.state().view().look.layer_level(0), level);
        let slot_items = drawn_items(&h);
        drawn.push(slot_items.get(&Slot::Layer(0)).copied().unwrap_or(0));
        let table = color_table(&h.state().view().look, layers, 0);
        colours.push(table[trace]);
        if rendering_enabled() {
            h.snapshot(format!("visibility_issue690_{level}"));
        }
    }
    // 0 % draws nothing of the top layer, 50 % and 100 % draw all of it.
    eprintln!("top layer items drawn at 0/50/100 %: {drawn:?}");
    assert_eq!(drawn[0], 0);
    assert!(drawn[1] > 0 && drawn[1] == drawn[2], "{drawn:?}");
    // 100 % is the layer's own colour, 50 % mixes it into the background.
    let own = h.state().view().look.item_color(0, ItemColor::Trace);
    assert_eq!(colours[2], own);
    let background = h.state().view().look.background();
    assert_eq!(colours[1], background.lerp_to_gamma(own, 0.5));
    // A value between two steps snaps to the nearer.
    exec(&mut h, Command::SetLayerLevel(0, 44));
    assert_eq!(h.state().view().look.layer_level(0), 40);
}

/// An in-memory `eframe::Storage`.
#[derive(Default)]
struct MemStorage(HashMap<String, String>);

impl eframe::Storage for MemStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.to_string(), value);
    }
    fn flush(&mut self) {}
}

fn app_from(storage: &MemStorage) -> Harness<'static, App> {
    Harness::builder()
        .with_size(SIZE)
        .build_eframe(|cc| App::build(&cc.egui_ctx, Some(storage), Args::default()))
}

fn open(h: &mut Harness<'static, App>, file: &str) {
    exec(h, Command::Open(fixtures().join(file)));
    wait_for_load(h);
}

#[test]
fn visibility_persists() {
    let mut h = plain_harness(&[ISSUE690]);
    exec(&mut h, Command::SetLayerLevel(0, 30));
    exec(&mut h, Command::SetObjectLevel(Object::ConductionAreas, 70));
    exec(&mut h, Command::SetObjectLevel(Object::Highlight, 0));
    exec(&mut h, Command::SetDim(80));
    exec(&mut h, Command::ToggleVirtualLayer(2));
    let name = h.state().loaded().unwrap().scene.layers[0].name.clone();
    let mut storage = MemStorage::default();
    eframe::App::save(h.state_mut(), &mut storage);

    let mut fresh = app_from(&storage);
    open(&mut fresh, ISSUE690);
    let look = &fresh.state().view().look;
    assert_eq!(look.layer_level(0), 30, "layer {name}");
    assert_eq!(look.layer_level(1), 100);
    assert_eq!(look.object_level(Object::ConductionAreas), 70);
    assert_eq!(look.object_level(Object::Highlight), 0);
    assert_eq!(look.object_level(Object::Traces), 100);
    assert_eq!(look.dim(), 80);
    assert!(!look.virtual_on(2) && look.virtual_on(3));
    // Reset puts everything back.
    exec(&mut fresh, Command::ResetLevels);
    assert_eq!(fresh.state().view().look.layer_level(0), 100);
    assert_eq!(fresh.state().view().look.dim(), 40);
}

#[test]
fn visibility_window_sliders_produce_commands() {
    let mut h = plain_harness(&[ISSUE159]);
    exec(&mut h, Command::ShowWindow(WindowKind::Visibility));
    assert!(h.state().windows().is_open(WindowKind::Visibility));
    // The window has one slider per copper layer and per object.
    h.get_by_label(text::VISIBILITY_RESET);
    // "Traces" is also a check box of the side panel.
    assert_eq!(h.query_all_by_label(text::OBJ_TRACES).count(), 2);
    h.get_by_label(text::VISIBILITY_LAYER_SECTION);
    // Reset is a command, so the state it leaves is the default.
    exec(&mut h, Command::SetLayerLevel(1, 20));
    h.get_by_label(text::VISIBILITY_RESET).click();
    settle!(h);
    assert_eq!(h.state().view().look.layer_level(1), 100);
}

// ---------------------------------------------------------------------------
// G04.5: virtual layers
// ---------------------------------------------------------------------------

/// Issue159's DSN has courtyard, fabrication and silkscreen outlines.
#[test]
fn virtual_layer_counts() {
    let mut h = plain_harness(&[ISSUE159]);
    let (total, courtyards, fabs) = {
        let b = board(&h);
        let outlines: Vec<_> = b
            .items()
            .filter_map(|i| match &i.kind {
                ItemKind::ComponentOutline(o) => Some((o.is_courtyard, o.is_fabrication)),
                _ => None,
            })
            .collect();
        (
            outlines.len(),
            outlines.iter().filter(|(c, _)| *c).count(),
            outlines.iter().filter(|(c, f)| !c && *f).count(),
        )
    };
    eprintln!("outlines: {total} total, {courtyards} courtyard, {fabs} fabrication");
    assert!(courtyards > 0, "Issue159 must have courtyard outlines");
    assert!(fabs > 0 && total > courtyards + fabs);

    // The six slots' scene items add up to the outlines on the board.
    let scene = &h.state().loaded().unwrap().scene;
    let mut by_virtual = [0usize; 6];
    for item in scene
        .items
        .iter()
        .filter(|i| i.kind == Kind::ComponentOutline)
    {
        by_virtual[virtual_index(item.slot).expect("an outline is on a virtual layer")] += 1;
    }
    eprintln!(
        "by virtual layer: {by_virtual:?}, {} part numbers",
        scene.part_numbers.len()
    );
    assert_eq!(by_virtual.iter().sum::<usize>(), total);
    assert_eq!(by_virtual[2] + by_virtual[3], courtyards);
    assert_eq!(by_virtual[4] + by_virtual[5], fabs);
    // Part numbers go with the fabrication layers; designators stay.
    assert!(!scene.part_numbers.is_empty());
    assert!(scene.part_numbers.len() <= scene.labels.len());

    // Hiding the courtyard toggles removes exactly the courtyard items from
    // the draw list.
    let outline_items = |h: &Harness<'_, App>| {
        drawn_items(h)
            .into_iter()
            .filter(|(slot, _)| virtual_index(*slot).is_some())
            .map(|(_, n)| n)
            .sum::<usize>()
    };
    assert_eq!(outline_items(&h), total);
    exec(&mut h, Command::ToggleVirtualLayer(2));
    exec(&mut h, Command::ToggleVirtualLayer(3));
    eprintln!(
        "draw list: {total} outline items, {} without courtyards",
        outline_items(&h)
    );
    assert_eq!(outline_items(&h), total - courtyards);
    exec(&mut h, Command::ToggleVirtualLayer(2));
    exec(&mut h, Command::ToggleVirtualLayer(3));
    assert_eq!(outline_items(&h), total);
}

#[test]
fn snapshot_courtyards_only() {
    if !rendering_enabled() {
        eprintln!("skipped: rendering tests need Linux or FREEROUTING_GUI_SNAPSHOTS=1");
        return;
    }
    let mut h = app_harness(&[ISSUE159]);
    // Hide the copper, the other kinds and the other virtual layers.
    for layer in 0..layer_count(&h) {
        exec(&mut h, Command::SetLayerLevel(layer, 0));
    }
    for i in [0, 1, 4, 5] {
        exec(&mut h, Command::ToggleVirtualLayer(i));
    }
    exec(&mut h, Command::ToggleLabels);
    exec(&mut h, Command::ToggleRatsnest);
    let drawn = drawn_items(&h);
    assert!(
        drawn
            .keys()
            .all(|s| matches!(s, Slot::Courtyard(_) | Slot::Board))
    );
    h.snapshot("courtyards_issue159");
}

// ---------------------------------------------------------------------------
// G04.6: units, crosshair, rotation, mirroring
// ---------------------------------------------------------------------------

fn pointer(h: &mut Harness<'_, App>, pos: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.step();
    h.step();
}

/// A pad of Issue159 that nothing covers at its centre, with the name of its
/// owner and its place on the board.
fn known_pad(h: &Harness<'_, App>) -> (String, freerouting_gui::scene::P) {
    let loaded = h.state().loaded().unwrap();
    let scene = &loaded.scene;
    let view = h.state().view();
    for item in scene.items.iter().filter(|i| i.kind == Kind::Pad) {
        let Some(owner) = &item.owner else { continue };
        let centre = item.geom.bounds().center();
        let Slot::Layer(layer) = item.slot else {
            continue;
        };
        let vis = view.visibility();
        let picked = scene.pick(
            centre,
            0.0,
            |i| vis.slot(i.slot) && vis.kind(i.kind),
            Some(layer),
        );
        if picked.is_some_and(|p| p.kind == Kind::Pad && p.owner.as_deref() == Some(owner)) {
            return (owner.clone(), centre);
        }
    }
    panic!("no pad to hover");
}

#[test]
fn hover_under_rotation() {
    let mut h = plain_harness(&[ISSUE159]);
    let (owner, centre) = known_pad(&h);
    let mut tried = 0;
    for mirror in [Mirror::None, Mirror::LeftRight, Mirror::TopBottom] {
        for turns in [0u8, 1, 2, 3] {
            exec(&mut h, Command::SetMirror(mirror));
            exec(&mut h, Command::SetRotation(turns));
            // The view is fitted again after a turn; give it its frames.
            h.run_steps(3);
            let view = h.state().view();
            let camera = view.camera.expect("camera");
            assert_eq!(camera.orientation.quarter_turns, turns);
            assert_eq!(camera.orientation.mirror, mirror);
            let at = camera.to_screen(view.canvas, centre);
            assert!(view.canvas.contains(at), "{turns} {mirror:?}: {at:?}");
            pointer(&mut h, at);
            let hover = hover_of(&h.ctx);
            let description = hover.description.expect("something under the cursor");
            assert!(
                description.starts_with("Pad") && description.contains(&owner),
                "{turns} turns, {mirror:?}: {description}"
            );
            // The cursor reads back the pad's centre.
            let p = hover.cursor.unwrap();
            assert!((p.x - centre.x).abs() < 0.05 && (p.y - centre.y).abs() < 0.05);
            tried += 1;
        }
    }
    assert_eq!(tried, 12);
}

/// The board turned by 90 degrees and mirrored left to right, with the
/// Display Options window open (the GPU draws the turned board through the
/// camera's basis; `ui_render::cpu_and_gpu_match_when_rotated` checks that the
/// CPU painter agrees).
#[test]
fn snapshot_rotated_and_mirrored() {
    if !rendering_enabled() {
        eprintln!("skipped: rendering tests need Linux or FREEROUTING_GUI_SNAPSHOTS=1");
        return;
    }
    let mut h = app_harness(&[ISSUE159]);
    exec(&mut h, Command::SetRotation(1));
    exec(&mut h, Command::SetMirror(Mirror::LeftRight));
    exec(&mut h, Command::ShowWindow(WindowKind::DisplayMisc));
    h.run_steps(3);
    h.snapshot("rotated_mirrored_issue159");
}

#[test]
fn rotation_commands_and_the_display_window() {
    let mut h = plain_harness(&[ISSUE159]);
    exec(&mut h, Command::ShowWindow(WindowKind::DisplayMisc));
    h.get_by_label(text::ROTATION);
    h.get_by_label(text::ROTATION_90).click();
    settle!(h);
    assert_eq!(h.state().view().orientation.quarter_turns, 1);
    h.get_by_label(text::MIRROR_LEFT_RIGHT).click();
    settle!(h);
    assert_eq!(h.state().view().orientation.mirror, Mirror::LeftRight);
    h.get_by_label(text::CROSSHAIR_LARGE).click();
    settle!(h);
    assert!(h.state().view().crosshair);
    // "None" is the first choice of the rotation and of the mirroring.
    h.query_all_by_label(text::ROTATION_NONE)
        .next()
        .expect("the rotation's None")
        .click();
    settle!(h);
    assert_eq!(h.state().view().orientation.quarter_turns, 0);
}

#[test]
fn unit_selector_changes_the_status_bar() {
    let mut h = plain_harness(&[ISSUE159]);
    // The selector's button comes first, the status bar's label after it.
    h.query_all_by_label("mil")
        .next()
        .expect("the unit button")
        .click();
    settle!(h);
    assert_eq!(h.state().view().unit, Unit::Mil);
    let view = h.state().view();
    let camera = view.camera.unwrap();
    let at = view.canvas.center();
    let p = camera.to_board(view.canvas, at);
    pointer(&mut h, at);
    // The position text is the point in the chosen unit, two fraction
    // digits, and the label says which.
    let hover = hover_of(&h.ctx);
    let shown = panels::status::position_text(&hover, Unit::Mil);
    let (x, y) = (p.x / 0.0254, p.y / 0.0254);
    for number in [x, y] {
        let digits = text::format_fixed(number, 2);
        assert!(shown.contains(&digits), "{shown} has no {digits}");
    }
    h.get_by_label_contains("X ");
    assert_eq!(h.query_all_by_label("mil").count(), 2);
    // Other units.
    exec(&mut h, Command::SetUnit(Unit::Um));
    pointer(&mut h, at);
    h.get_by_label("um");
    h.get_by_label("µm");
}

// ---------------------------------------------------------------------------
// G04.7: colour manager
// ---------------------------------------------------------------------------

#[test]
fn colour_change_persists() {
    let mut h = plain_harness(&[ISSUE159]);
    let layers = layer_count(&h);
    let batch = kind_index(Kind::Trace); // slot 0 is layer 0
    let before = color_table(&h.state().view().look, layers, 0)[batch];
    exec(
        &mut h,
        Command::SetItemColor(0, ItemColor::Trace, [10, 200, 30]),
    );
    let after = color_table(&h.state().view().look, layers, 0)[batch];
    assert_ne!(before, after);
    assert_eq!(after, Color32::from_rgb(10, 200, 30));
    assert_eq!(slot_index(Slot::Layer(0), layers) * KINDS + batch, batch);
    // Other colours too.
    exec(
        &mut h,
        Command::SetOtherColor(OtherColor::Background, [1, 2, 3]),
    );
    assert_eq!(
        h.state().view().look.background(),
        Color32::from_rgb(1, 2, 3)
    );

    let mut storage = MemStorage::default();
    eframe::App::save(h.state_mut(), &mut storage);
    let mut fresh = app_from(&storage);
    assert_eq!(
        fresh.state().view().look.background(),
        Color32::from_rgb(1, 2, 3),
        "the other colours come back with the app"
    );
    open(&mut fresh, ISSUE159);
    let restored = color_table(&fresh.state().view().look, layers, 0)[batch];
    assert_eq!(restored, after, "the layer's colour comes back by name");
    // Another layer is untouched.
    assert_eq!(
        fresh.state().view().look.item_color(1, ItemColor::Trace),
        freerouting_gui::look::default_item_color(ItemColor::Trace, 1, layers)
    );

    // Reset to defaults.
    exec(&mut fresh, Command::ShowWindow(WindowKind::Colors));
    fresh.get_by_label(text::COLORS_RESET).click();
    settle!(fresh);
    assert_eq!(
        color_table(&fresh.state().view().look, layers, 0)[batch],
        before
    );
    let mut storage = MemStorage::default();
    eframe::App::save(fresh.state_mut(), &mut storage);
    assert!(
        app_from(&storage)
            .state()
            .settings_snapshot()
            .colors
            .is_empty()
    );
}

// ---------------------------------------------------------------------------
// G04.8: status bar, log, About
// ---------------------------------------------------------------------------

#[test]
fn warning_counter_opens_log() {
    let mut h = plain_harness(&[ISSUE754]);
    let warnings: Vec<String> = h.state().loaded().unwrap().session.warnings.clone();
    eprintln!("Issue754: {} loader warnings", warnings.len());
    assert!(!warnings.is_empty(), "Issue754 should have loader warnings");
    h.get_by_label(&format!("Warnings: {}", warnings.len()));
    h.get_by_label("Errors: 0");
    assert!(!h.state().windows().is_open(WindowKind::Log));
    h.get_by_label(&format!("Warnings: {}", warnings.len()))
        .click();
    settle!(h);
    assert!(h.state().windows().is_open(WindowKind::Log));

    // The log lists the loader warnings first, in order, then what the app
    // did.
    let entries = h.state().log_entries();
    let log_warnings: Vec<&str> = entries
        .iter()
        .filter(|e| e.level == panels::log::Level::Warning)
        .map(|e| e.text.as_str())
        .collect();
    assert_eq!(
        log_warnings,
        warnings.iter().map(String::as_str).collect::<Vec<_>>()
    );
    assert_eq!(
        entries[..warnings.len()]
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>(),
        warnings.iter().map(String::as_str).collect::<Vec<_>>()
    );
    assert!(
        entries
            .last()
            .unwrap()
            .text
            .starts_with("Design rules check")
            || entries.iter().any(|e| e.text.starts_with("Loaded "))
    );
    // The window shows them (a text can occur twice, so any match will do).
    let first = &warnings[0];
    assert!(!h.query_all_by_label(first).collect::<Vec<_>>().is_empty());
}

#[test]
fn errors_are_counted() {
    let mut h = plain_harness(&[ISSUE159]);
    exec(&mut h, Command::Open(fixtures().join("no-such-board.dsn")));
    let start = std::time::Instant::now();
    while h.state().error().is_none() {
        assert!(start.elapsed().as_secs() < 30);
        std::thread::sleep(std::time::Duration::from_millis(5));
        h.step();
    }
    settle!(h);
    h.get_by_label("Errors: 1");
}

#[test]
fn about_shows_version() {
    let mut h = plain_harness(&[ISSUE159]);
    h.get_by_label("Help").click();
    settle!(h);
    h.get_by_label(text::MENU_ABOUT).click();
    settle!(h);
    assert!(h.state().windows().is_open(WindowKind::About));
    h.get_by_label(&format!("Version: {}", env!("CARGO_PKG_VERSION")));
    h.get_by_label_contains("License: GPL-3.0");
    // No build stamp yet (R01.2): the commit says so.
    h.get_by_label_contains("Commit:");
}

#[test]
fn status_bar_names_the_layer() {
    let mut h = plain_harness(&[ISSUE159]);
    let name = h.state().loaded().unwrap().scene.layers[0].name.clone();
    h.get_by_label(&format!("Current layer: {name}"));
    exec(&mut h, Command::SetActiveLayer(1));
    let name = h.state().loaded().unwrap().scene.layers[1].name.clone();
    h.get_by_label(&format!("Current layer: {name}"));
}
