//! The Simulation workspace driven through its visible controls.

use cellarium::gui::canvas::CanvasTransform;
use cellarium::gui::canvas::world::ChannelView;
use cellarium::gui::sections::simulation::SimulationControl;
use cellarium::gui::{CellariumGui, Section};
use cellarium::sim::backend_selector::BackendPolicy;
use cellarium::sim::experiment_model::ExperimentSpec;
use eframe::egui::{Rect, pos2, vec2};

/// The CPU backend keeps these tests independent of whatever GPU the machine has.
fn simulation_gui() -> CellariumGui {
    // Choose the backend before the worker starts, so this test never
    // creates a GPU device it is about to replace.
    let mut app = CellariumGui::with_backend(
        ExperimentSpec::single_channel_lenia(16, 16),
        BackendPolicy::RequireCpu,
    );
    app.navigation_mut().select(Section::Simulation);
    app
}

#[test]
fn the_run_control_reaches_the_worker_and_the_canvas_reports_it() {
    let mut app = simulation_gui();
    assert!(!app.running());

    app.dispatch_simulation(SimulationControl::RunPause);
    let snapshot = app.wait_for_simulation(|state| state.running);
    assert!(snapshot.running);
    assert_eq!(SimulationControl::RunPause.label(true), "Pause");

    app.dispatch_simulation(SimulationControl::RunPause);
    let snapshot = app.wait_for_simulation(|state| !state.running);
    assert!(!snapshot.running);
}

#[test]
fn stepping_advances_exactly_one_tick() {
    let mut app = simulation_gui();
    let before = app.snapshot().unwrap().tick;
    app.dispatch_simulation(SimulationControl::Step);
    let snapshot = app.wait_for_simulation(|state| state.tick > before);
    assert_eq!(snapshot.tick, before + 1);
}

#[test]
fn clear_zeroes_the_world_and_randomize_refills_it() {
    let mut app = simulation_gui();
    app.dispatch_simulation(SimulationControl::Clear);
    let cleared = app.wait_for_simulation(|state| state.cells.iter().all(|value| *value == 0.0));
    assert!(cleared.cells.iter().all(|value| *value == 0.0));

    app.dispatch_simulation(SimulationControl::Randomize);
    let filled = app.wait_for_simulation(|state| state.cells.iter().any(|value| *value > 0.0));
    assert!(filled.cells.iter().any(|value| *value > 0.0));
    assert!(
        filled.cells.iter().all(|value| (0.0..=1.0).contains(value)),
        "randomize must stay inside the value range"
    );
}

#[test]
fn reset_restores_the_state_the_worker_started_with() {
    let mut app = simulation_gui();
    let start = app.snapshot().unwrap().cells.clone();
    app.dispatch_simulation(SimulationControl::Clear);
    app.wait_for_simulation(|state| state.cells.iter().all(|value| *value == 0.0));
    app.dispatch_simulation(SimulationControl::Reset);
    let restored = app.wait_for_simulation(|state| state.cells == start);
    assert_eq!(restored.cells, start);
}

#[test]
fn an_edit_derived_from_the_rendered_transform_lands_on_the_cell_under_the_pointer() {
    let mut app = simulation_gui();
    app.dispatch_simulation(SimulationControl::Clear);
    app.wait_for_simulation(|state| state.cells.iter().all(|value| *value == 0.0));

    // Use the same transform the canvas would build for this viewport, then
    // paint the cell the pointer is over.
    let viewport = Rect::from_min_size(pos2(0.0, 0.0), vec2(640.0, 480.0));
    let transform = CanvasTransform::fit(viewport, [16.0, 16.0], 24.0);
    let target = [11.5_f64, 4.5];
    let pointer = transform.world_to_screen(target);
    let world = transform.screen_to_world(pointer);
    let (x, y) = (world[0] as usize, world[1] as usize);
    assert_eq!(
        (x, y),
        (11, 4),
        "the pointer must map back to the same cell"
    );

    app.send_simulation(cellarium::sim::worker::SimulationCommand::EditWorld(vec![
        cellarium::sim::local_backend::WorldEdit {
            channel: 0,
            basis: 0,
            x,
            y,
            value: 0.75,
        },
    ]));

    let snapshot = app.wait_for_simulation(|state| {
        state
            .layout
            .index_by_position(0, x, y, 0)
            .is_some_and(|index| state.cells[index] == 0.75)
    });
    let index = snapshot.layout.index_by_position(0, x, y, 0).unwrap();
    assert_eq!(snapshot.cells[index], 0.75);
}

#[test]
fn fit_restores_a_view_that_shows_the_whole_world() {
    let mut app = simulation_gui();
    let viewport = Rect::from_min_size(pos2(0.0, 0.0), vec2(640.0, 480.0));
    app.world_canvas_mut().transform = Some(CanvasTransform::new(viewport, [0.0, 0.0], 500.0));
    app.dispatch_simulation(SimulationControl::Fit);
    assert!(
        app.world_canvas().transform.is_none(),
        "Fit must let the next frame refit the world"
    );
}

#[test]
fn the_channel_view_and_brush_are_pointer_settable_state() {
    let mut app = simulation_gui();
    assert_eq!(app.world_canvas().view, ChannelView::Composite);
    app.world_canvas_mut().view = ChannelView::Solo(0);
    assert_eq!(app.world_canvas().view, ChannelView::Solo(0));

    app.world_canvas_mut().brush_radius = 5;
    app.world_canvas_mut().brush_value = 0.25;
    assert_eq!(app.world_canvas().brush_radius, 5);
    assert_eq!(app.world_canvas().brush_value, 0.25);
}
