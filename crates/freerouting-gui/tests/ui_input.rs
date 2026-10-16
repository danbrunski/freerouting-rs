//! Java's mouse and key model (G01.7), without rendering.
//!
//! Left button: reserved for tools. Middle button, or Space and the left
//! button: pan. `A`, `Z`, `O`, `C`, `F`: zoom all, in, out, centre, region.

mod common;

use common::plain_harness;
use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2};
use egui_kittest::Harness;
use freerouting_gui::App;
use freerouting_gui::scene::P;
use freerouting_gui::view::Camera;

const BOARD: &str = "Issue159-setonix_2hp-pcb.dsn";

fn harness() -> Harness<'static, App> {
    plain_harness(&[BOARD])
}

fn camera(h: &Harness<'_, App>) -> Camera {
    h.state().view().camera.expect("camera")
}

/// A point `(dx, dy)` pixels from the canvas corner.
fn on_canvas(h: &Harness<'_, App>, dx: f32, dy: f32) -> Pos2 {
    h.state().view().canvas.min + eframe::egui::vec2(dx, dy)
}

fn push(h: &mut Harness<'_, App>, event: Event) {
    h.input_mut().events.push(event);
    h.step();
}

fn hover(h: &mut Harness<'_, App>, pos: Pos2) {
    push(h, Event::PointerMoved(pos));
}

fn button(h: &mut Harness<'_, App>, pos: Pos2, button: PointerButton, pressed: bool) {
    push(
        h,
        Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: Modifiers::NONE,
        },
    );
}

/// Press at `from`, move to `to` in one go, release.
fn drag(h: &mut Harness<'_, App>, b: PointerButton, from: Pos2, to: Pos2) {
    hover(h, from);
    button(h, from, b, true);
    hover(h, to);
    button(h, to, b, false);
    h.run();
}

fn near(a: P, b: P, eps: f64) -> bool {
    (a.x - b.x).abs() < eps && (a.y - b.y).abs() < eps
}

#[test]
fn middle_drag_pans() {
    let mut h = harness();
    let before = camera(&h);
    let from = on_canvas(&h, 300.0, 300.0);
    drag(
        &mut h,
        PointerButton::Middle,
        from,
        from + eframe::egui::vec2(100.0, 0.0),
    );
    let after = camera(&h);
    // The board follows the pointer, so the centre moves the other way.
    assert!(
        near(
            after.center,
            P::new(before.center.x - 100.0 / before.zoom, before.center.y),
            1e-9
        ),
        "{before:?} -> {after:?}"
    );
    assert_eq!(after.zoom, before.zoom);
}

#[test]
fn space_and_left_drag_pans() {
    let mut h = harness();
    let before = camera(&h);
    let from = on_canvas(&h, 300.0, 300.0);
    h.key_down(Key::Space);
    h.step();
    drag(
        &mut h,
        PointerButton::Primary,
        from,
        from + eframe::egui::vec2(0.0, 60.0),
    );
    h.key_up(Key::Space);
    h.run();
    let after = camera(&h);
    assert!(
        near(
            after.center,
            P::new(before.center.x, before.center.y + 60.0 / before.zoom),
            1e-9
        ),
        "{before:?} -> {after:?}"
    );
}

#[test]
fn left_drag_does_not_pan() {
    let mut h = harness();
    let before = camera(&h);
    let from = on_canvas(&h, 300.0, 300.0);
    drag(
        &mut h,
        PointerButton::Primary,
        from,
        from + eframe::egui::vec2(100.0, 0.0),
    );
    let after = camera(&h);
    assert_eq!((after.center, after.zoom), (before.center, before.zoom));
}

#[test]
fn keys_a_z_o_c() {
    let mut h = harness();
    let fit = camera(&h);
    let at = on_canvas(&h, 250.0, 180.0);
    hover(&mut h, at);

    // Z then O at the same cursor point returns to the start.
    h.key_press(Key::Z);
    h.run();
    let zoomed = camera(&h);
    assert!((zoomed.zoom - 2.0 * fit.zoom).abs() < 1e-9 * fit.zoom);
    h.key_press(Key::O);
    h.run();
    let back = camera(&h);
    assert!((back.zoom - fit.zoom).abs() < 1e-6, "{fit:?} {back:?}");
    assert!(near(back.center, fit.center, 1e-6), "{fit:?} {back:?}");

    // C puts the point under the cursor in the centre.
    let under = h.state().view().canvas;
    let board_point = back.to_board(under, at);
    h.key_press(Key::C);
    h.run();
    assert!(near(camera(&h).center, board_point, 1e-6));

    // A fits the board again: the same camera as the first fit.
    h.key_press(Key::A);
    h.run();
    let all = camera(&h);
    assert!(near(all.center, fit.center, 1e-6) && (all.zoom - fit.zoom).abs() < 1e-6);
}

#[test]
fn f_zooms_to_rectangle() {
    let mut h = harness();
    let start = camera(&h);
    let canvas = h.state().view().canvas;
    let (a, b) = (on_canvas(&h, 100.0, 100.0), on_canvas(&h, 300.0, 250.0));
    let (pa, pb) = (start.to_board(canvas, a), start.to_board(canvas, b));
    h.key_press(Key::F);
    h.run();
    assert!(h.state().view().zoom_rect_armed);
    drag(&mut h, PointerButton::Primary, a, b);
    assert!(!h.state().view().zoom_rect_armed);
    let cam = camera(&h);
    assert!(cam.zoom > start.zoom);
    // The new view contains the rectangle's board area.
    let (lo, hi) = (
        cam.to_board(canvas, canvas.left_bottom()),
        cam.to_board(canvas, canvas.right_top()),
    );
    let eps = 1e-6;
    for p in [pa, pb] {
        assert!(
            p.x >= lo.x - eps && p.x <= hi.x + eps,
            "{p:?} in {lo:?}..{hi:?}"
        );
        assert!(
            p.y >= lo.y - eps && p.y <= hi.y + eps,
            "{p:?} in {lo:?}..{hi:?}"
        );
    }
    // And it is centred on it.
    assert!(near(
        cam.center,
        P::new((pa.x + pb.x) / 2.0, (pa.y + pb.y) / 2.0),
        1e-6
    ));
}

#[test]
fn a_left_drag_without_f_leaves_the_view_alone_and_escape_cancels_f() {
    let mut h = harness();
    let start = camera(&h);
    h.key_press(Key::F);
    h.key_press(Key::Escape);
    h.run();
    assert!(!h.state().view().zoom_rect_armed);
    let (a, b) = (on_canvas(&h, 100.0, 100.0), on_canvas(&h, 300.0, 250.0));
    drag(&mut h, PointerButton::Primary, a, b);
    assert_eq!(camera(&h).zoom, start.zoom);
}

#[test]
fn reserved_keys_do_nothing_yet() {
    let mut h = harness();
    let before = camera(&h);
    for key in freerouting_gui::input::RESERVED {
        h.key_press(key);
    }
    h.run();
    let after = camera(&h);
    assert_eq!((after.center, after.zoom), (before.center, before.zoom));
}

#[test]
fn digits_and_plus_minus_change_the_active_layer() {
    let mut h = harness();
    assert_eq!(h.state().view().active_layer, 0);
    h.key_press(Key::Num2);
    h.run();
    assert_eq!(h.state().view().active_layer, 1);
    h.key_press(Key::Minus);
    h.run();
    assert_eq!(h.state().view().active_layer, 0);
    h.key_press(Key::Plus);
    h.run();
    assert_eq!(h.state().view().active_layer, 1);
    // No layer beyond the last.
    h.key_press(Key::Plus);
    h.run();
    assert_eq!(h.state().view().active_layer, 1);
    // `,` toggles the crosshair.
    h.key_press(Key::Comma);
    h.run();
    assert!(h.state().view().crosshair);
    let _ = pos2(0.0, 0.0);
}
