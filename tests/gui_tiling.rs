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
        assert_eq!(gui.state().notice(), None, "{label} must be accepted");
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
    // The coverage verdict is stated, not left for the user to infer, and it
    // is stated in the same breath as the seam verdict so the two cannot look
    // like they disagree.
    let assessment = gui
        .state()
        .seam_assessment()
        .expect("a preset is assessable");
    let (state, line) = cellarium::gui::sections::tiling::verdict(&assessment, true);
    assert_eq!(state, cellarium::gui::theme::State::Live);
    assert!(line.contains("the plane is covered"), "{line}");
    gui.get_by_label(line.as_str());
}

#[test]
fn closing_the_seams_holds_them_and_moves_the_drawing() {
    let mut gui = tiling_gui_blank();
    click(&mut gui, "Square");

    // An exact preset already closes, so there is nothing to move and the
    // control says so rather than pretending to work.
    let assessment = gui
        .state()
        .seam_assessment()
        .expect("a preset tiling must be assessable");
    assert!(
        assessment.is_closed(),
        "the square preset is exact: {}",
        assessment.summary()
    );

    // Pull it out of true the way a pointer would, then let the assistant
    // close it again.
    drag_vertex(&mut gui, world(1.0, 1.0), Vec2::new(1.04, 0.97));
    let crooked = gui.state().seam_assessment().expect("still assessable");
    assert!(
        !crooked.is_closed(),
        "a moved vertex must stop the drawing reading as finished"
    );
    assert!(
        crooked.candidates.len() >= 2,
        "the assistant must keep speaking about a rough drawing, said: {}",
        crooked.summary()
    );

    let before = square_vertices(&gui);
    click(&mut gui, "Close seams");
    let after = square_vertices(&gui);
    assert_ne!(
        before, after,
        "closing the seams has to move the drawing, not merely record an opinion about it"
    );
    assert!(
        !gui.state().tiling_canvas().seams.is_empty(),
        "closing the seams also holds them"
    );
    let closed = gui.state().seam_assessment().expect("still assessable");
    assert!(
        closed.is_closed(),
        "after closing, every seam must meet: {}",
        closed.summary()
    );
}

/// The defect this whole feature exists to fix. A drawing that is off by less
/// than a pointer can aim used to leave the assistant with nothing to say: the
/// square lost both its seam pairs at one thousandth of a unit, and the
/// interface offered an Accept button over an empty proposal.
#[test]
fn a_rough_drawing_is_never_answered_with_silence() {
    let mut gui = tiling_gui_blank();
    click(&mut gui, "Square");
    drag_vertex(&mut gui, world(1.0, 1.0), Vec2::new(1.002, 0.999));

    let assessment = gui.state().seam_assessment().expect("assessable");
    assert!(
        !assessment.candidates.is_empty() || !assessment.orphans.is_empty(),
        "the assistant said nothing at all about a rough drawing"
    );
    assert_eq!(
        assessment.candidates.len() * 2 + assessment.orphans.len(),
        assessment.edge_count,
        "every edge must be either paired or explained"
    );
    // And the verdict reaches the screen rather than staying in the model.
    let tiles = gui
        .state()
        .spec()
        .tiling
        .as_ref()
        .is_some_and(|draft| cellarium::sim::tiling::validate_coverage(draft).is_ok());
    let (_, line) = cellarium::gui::sections::tiling::verdict(&assessment, tiles);
    gui.get_by_label(line.as_str());
}

/// Releasing gives the vertices back, one at a time.
#[test]
fn releasing_the_seams_stops_them_being_held() {
    let mut gui = tiling_gui_blank();
    click(&mut gui, "Square");
    drag_vertex(&mut gui, world(1.0, 1.0), Vec2::new(1.04, 0.97));
    click(&mut gui, "Close seams");
    let held = gui.state().tiling_canvas().seams.len();
    assert!(held > 0, "closing must hold the seams it closed");

    click(&mut gui, format!("Release {held}").as_str());
    assert!(
        gui.state().tiling_canvas().seams.is_empty(),
        "releasing must let go of every seam"
    );
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
    click(&mut gui, "Close seams");
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

/// The defect a user hit on v0.4.0: two edges visibly far apart, an enabled
/// "Close seams", and pressing it changed nothing.
///
/// The cause was that closing only ever solved the pairs the assistant was
/// confident about. A drawing with one already-closed pair and two distant
/// ones therefore "closed" the pair that needed no work, moved the drawing by
/// zero, and left the two the user was pointing at exactly where they were —
/// which from the outside is a button that does not work.
#[test]
fn closing_acts_on_the_far_apart_pairs_the_user_can_see() {
    let mut gui = tiling_gui_blank();
    click(&mut gui, "Triangles");

    // Pull a corner well past the point where a pairing stops being a slip of
    // the pointer and becomes a deliberate distance.
    let before = gui
        .state()
        .seam_assessment()
        .expect("a preset is assessable");
    let start = triangle_vertex(&gui, 0);
    drag_vertex(&mut gui, start, Vec2::new(start.x + 0.45, start.y + 0.3));

    let crooked = gui.state().seam_assessment().expect("still assessable");
    assert!(
        crooked.count(cellarium::sim::tiling::SeamBucket::Near) > 0,
        "this test is only meaningful while some pair is far apart: {}",
        crooked.summary()
    );
    assert!(
        !crooked.is_closed(),
        "the drawing must not read as finished: {}",
        crooked.summary()
    );
    let _ = before;

    let geometry_before = triangle_vertices(&gui);
    click(&mut gui, "Close seams");
    let geometry_after = triangle_vertices(&gui);
    assert_ne!(
        geometry_before, geometry_after,
        "closing has to move the drawing; acting only on the pairs that already \
         closed is indistinguishable from a button that does nothing"
    );

    let closed = gui.state().seam_assessment().expect("still assessable");
    assert_eq!(
        closed.count(cellarium::sim::tiling::SeamBucket::Near),
        0,
        "no pair may be left far apart after closing: {}",
        closed.summary()
    );
}

fn triangle_vertices(gui: &Gui) -> Vec<Vec2> {
    let tiling = gui
        .state()
        .spec()
        .tiling
        .as_ref()
        .expect("a preset was installed");
    let PrototypeShape::SimplePolygon { vertices } = &tiling.prototypes[0].shape else {
        panic!("a preset polygon is editable");
    };
    vertices.clone()
}

fn triangle_vertex(gui: &Gui, index: usize) -> Vec2 {
    triangle_vertices(gui)[index]
}

/// Closing either moves the drawing or explains itself in terms of something
/// on screen. Which of the two happens depends on how reachable the geometry
/// is, and both are acceptable; silently doing neither is not.
///
/// The wording of a refusal is pinned directly in `gui::app`, where the state
/// can be built rather than danced into through the pointer.
#[test]
fn closing_either_moves_the_drawing_or_explains_itself() {
    let mut gui = tiling_gui_blank();
    click(&mut gui, "Triangles");

    // Fold a shared vertex deep into the cell: closing every pair from here
    // would flatten a triangle, so the solve genuinely cannot succeed.
    let start = triangle_vertex(&gui, 1);
    drag_vertex(&mut gui, start, Vec2::new(start.x - 0.45, start.y - 0.55));

    let before = triangle_vertices(&gui);
    click(&mut gui, "Close seams");

    let status = gui.state().status();
    // A success also leaves a notice, so the level is what separates them.
    if status.notice_level == cellarium::gui::app::NoticeLevel::Info {
        assert_ne!(
            before,
            triangle_vertices(&gui),
            "a reported success has to have moved the drawing"
        );
        return;
    }
    let notice = status
        .notice
        .clone()
        .expect("a refusal has to say something");
    assert!(
        notice.contains("edge") && notice.contains("basis"),
        "a refusal has to name an edge the user can see: {notice}"
    );
    assert!(
        notice.contains("drag"),
        "a refusal has to say what to do next: {notice}"
    );
}
