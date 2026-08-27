//! The Tiling workspace driven through its visible controls and the pointer.

use cellarium::gui::{CellariumGui, Section, layout};
use cellarium::sim::experiment_model::ExperimentSpec;
use cellarium::sim::tiling::{PrototypeShape, Vec2};
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

type Gui = Harness<'static, CellariumGui>;

fn world(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

/// A workspace with no tiling yet, so construction starts from nothing.
fn tiling_gui_blank() -> Gui {
    let mut spec = ExperimentSpec::single_channel_lenia(16, 16);
    spec.tiling = None;
    let mut app = CellariumGui::for_test(spec);
    app.navigation_mut().select(Section::Tiling);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 720.0))
        .build_ui_state(|ui, app: &mut CellariumGui| layout::draw(app, ui), app);
    harness.run();
    harness
}

fn click(gui: &mut Gui, label: &str) {
    gui.get_by_label(label).click();
    gui.run();
}

/// Click a world point through the transform the canvas actually drew with, so
/// the test exercises the same mapping the user's pointer does.
fn canvas_click(gui: &mut Gui, point: Vec2) {
    let transform = gui
        .state()
        .tiling_canvas()
        .transform
        .expect("the canvas must be rendered before it can be clicked");
    let screen = transform.world_to_screen([point.x, point.y]);
    gui.event(egui::Event::PointerMoved(screen));
    gui.event(egui::Event::PointerButton {
        pos: screen,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    gui.event(egui::Event::PointerButton {
        pos: screen,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    gui.run();
}

#[test]
fn user_draws_undoes_and_closes_a_triangle_with_visible_neighbors() {
    let mut gui = tiling_gui_blank();
    click(&mut gui, "Draw from scratch");
    canvas_click(&mut gui, world(-0.5, -0.4));
    canvas_click(&mut gui, world(0.5, -0.4));
    canvas_click(&mut gui, world(0.0, 0.5));
    click(&mut gui, "Undo point");
    assert_eq!(gui.state().construction_vertices(), 2);
    click(&mut gui, "Redo point");
    click(&mut gui, "Finish polygon");
    assert_eq!(gui.state().draft_basis_count(), 1);
    assert!(gui.state().visible_neighbor_copies() >= 6);
}

#[test]
fn an_invalid_vertex_is_refused_with_a_visible_reason_and_changes_nothing() {
    let mut gui = tiling_gui_blank();
    click(&mut gui, "Draw from scratch");
    canvas_click(&mut gui, world(-0.5, -0.4));
    canvas_click(&mut gui, world(0.5, -0.4));
    assert_eq!(gui.state().construction_vertices(), 2);

    // Clicking a vertex that already exists cannot extend the path.
    canvas_click(&mut gui, world(0.5, -0.4));
    assert_eq!(
        gui.state().construction_vertices(),
        2,
        "a refused vertex must leave the construction alone"
    );
    let reason = gui
        .state()
        .tiling_canvas()
        .rejection
        .clone()
        .expect("the refusal must carry a reason");
    // The reason is on screen, not only in the state.
    gui.get_by_label(reason.as_str());
}

#[test]
fn a_polygon_cannot_be_closed_before_it_is_a_polygon() {
    let mut gui = tiling_gui_blank();
    click(&mut gui, "Draw from scratch");
    canvas_click(&mut gui, world(-0.5, -0.4));
    canvas_click(&mut gui, world(0.5, -0.4));
    assert!(
        gui.get_by_label("Finish polygon")
            .accesskit_node()
            .is_disabled(),
        "two points are not a polygon"
    );
    canvas_click(&mut gui, world(0.0, 0.5));
    assert!(
        !gui.get_by_label("Finish polygon")
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn cancelling_a_drawing_leaves_the_draft_untouched() {
    let mut gui = tiling_gui_blank();
    let before = gui.state().draft_basis_count();
    click(&mut gui, "Draw from scratch");
    canvas_click(&mut gui, world(-0.5, -0.4));
    canvas_click(&mut gui, world(0.5, -0.4));
    canvas_click(&mut gui, world(0.0, 0.5));
    click(&mut gui, "Cancel drawing");
    assert_eq!(gui.state().construction_vertices(), 0);
    assert_eq!(gui.state().draft_basis_count(), before);
    // The drawing controls are gone and the ordinary tools are back.
    gui.get_by_label("Draw from scratch");
}

#[test]
fn every_preset_card_installs_its_own_unit_cell() {
    for (label, bases) in [
        ("Square", 1),
        ("Triangles", 2),
        ("Hexagon", 1),
        ("Octagon + square", 2),
    ] {
        let mut gui = tiling_gui_blank();
        click(&mut gui, label);
        assert_eq!(
            gui.state().notice(),
            None,
            "{label} must be accepted"
        );
        assert_eq!(
            gui.state().draft_basis_count(),
            bases,
            "{label} must install {bases} bases"
        );
        assert!(
            gui.state().visible_neighbor_copies() >= 6,
            "{label} must show its periodic neighbours"
        );
    }
}

#[test]
fn a_preset_reports_that_it_tiles_the_plane() {
    let mut gui = tiling_gui_blank();
    click(&mut gui, "Hexagon");
    // The coverage verdict is stated, not left for the user to infer.
    let glyph = cellarium::gui::theme::state_glyph(cellarium::gui::theme::State::Live);
    gui.get_by_label(format!("{glyph} tiles the plane").as_str());
}

#[test]
fn seams_are_proposed_with_a_residual_and_only_hold_once_accepted() {
    let mut gui = tiling_gui_blank();
    click(&mut gui, "Square");
    click(&mut gui, "Solve seams");
    let proposed = gui
        .state()
        .seam_proposals()
        .expect("solving must propose the pairs it found")
        .len();
    assert!(proposed > 0, "a square tiling has full-edge pairs");

    click(&mut gui, "Cancel seams");
    assert!(gui.state().seam_proposals().is_none());
    assert!(
        gui.state().tiling_canvas().seams.is_empty(),
        "cancelling must not hold anything"
    );

    click(&mut gui, "Solve seams");
    click(&mut gui, "Accept seams");
    assert_eq!(gui.state().tiling_canvas().seams.len(), proposed);
    assert!(gui.state().seam_proposals().is_none());
    gui.get_by_label(format!("{proposed} seams held").as_str());
}

#[test]
fn a_vertex_drag_moves_the_polygon_and_a_held_seam_moves_its_whole_class() {
    let mut gui = tiling_gui_blank();
    click(&mut gui, "Square");
    let before = square_vertices(&gui);

    // Free drag: only the grabbed vertex moves.
    drag_vertex(&mut gui, before[2], Vec2::new(1.3, 1.2));
    let free = square_vertices(&gui);
    assert_ne!(free[2], before[2], "the grabbed vertex must move");
    let free_moved = free
        .iter()
        .zip(&before)
        .filter(|(after, start)| after != start)
        .count();
    assert_eq!(free_moved, 1, "an unconstrained drag moves one vertex");

    // Held seams: the drag moves the equivalence class the seams define.
    let mut gui = tiling_gui_blank();
    click(&mut gui, "Square");
    click(&mut gui, "Solve seams");
    click(&mut gui, "Accept seams");
    drag_vertex(&mut gui, before[2], Vec2::new(1.3, 1.2));
    let held = square_vertices(&gui);
    let held_moved = held
        .iter()
        .zip(&before)
        .filter(|(after, start)| after != start)
        .count();
    assert!(
        held_moved > free_moved,
        "a held seam must carry its partners along, moved {held_moved} of {}",
        before.len()
    );
}

fn square_vertices(gui: &Gui) -> Vec<Vec2> {
    let tiling = gui
        .state()
        .spec()
        .tiling
        .as_ref()
        .expect("a preset was installed");
    let PrototypeShape::SimplePolygon { vertices } = &tiling.prototypes[0].shape else {
        panic!("the square preset is a simple polygon");
    };
    vertices.clone()
}

/// Press on a vertex handle and drag it to a world point, the way a user does.
fn drag_vertex(gui: &mut Gui, from: Vec2, to: Vec2) {
    let transform = gui
        .state()
        .tiling_canvas()
        .transform
        .expect("the canvas must be rendered before it can be dragged");
    let start = transform.world_to_screen([from.x, from.y]);
    let end = transform.world_to_screen([to.x, to.y]);
    gui.event(egui::Event::PointerMoved(start));
    gui.event(egui::Event::PointerButton {
        pos: start,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    gui.run();
    gui.event(egui::Event::PointerMoved(end));
    gui.run();
    gui.event(egui::Event::PointerButton {
        pos: end,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    gui.run();
}
