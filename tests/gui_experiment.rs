//! Apply & Run, and the Experiment review workspace.

use cellarium::gui::{CellariumGui, Section, layout};
use cellarium::sim::backend_selector::BackendPolicy;
use cellarium::sim::experiment_model::ExperimentSpec;
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

type Gui = Harness<'static, CellariumGui>;

fn gui_fixture() -> Gui {
    let spec = ExperimentSpec::single_channel_lenia(16, 16)
        .normalize_rules()
        .expect("the fixture normalizes");
    let mut app = CellariumGui::new(spec);
    app.select_backend(BackendPolicy::RequireCpu);
    app.navigation_mut().select(Section::Experiment);
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

#[test]
fn failed_candidate_keeps_active_world_and_successful_apply_runs() {
    let mut gui = gui_fixture();
    let active = gui.state().document().active().clone();
    let revision = gui.state().document().active_revision();

    // A growth program that cannot compile must not become the running world.
    gui.state_mut().set_growth_source("unknown_symbol()");
    gui.run();
    click(&mut gui, "Apply & Run");
    assert_eq!(
        gui.state().document().active(),
        &active,
        "a rejected candidate must leave the active experiment untouched"
    );
    assert_eq!(gui.state().document().active_revision(), revision);
    assert!(
        gui.state().notice().is_some(),
        "the refusal must say something"
    );

    // A sound one is applied and starts running.
    gui.state_mut().set_growth_source("self * 0.5");
    gui.run();
    click(&mut gui, "Apply & Run");
    assert!(
        gui.state().document().active_revision() > revision,
        "a successful apply advances the active revision"
    );
    assert!(gui.state().running_intent(), "applying starts the run");
    let snapshot = gui.state().wait_for_simulation(|state| state.running);
    assert!(snapshot.running);
}

#[test]
fn the_experiment_workspace_names_what_is_wrong_and_leads_to_the_fix() {
    let mut gui = gui_fixture();
    assert!(gui.state().draft_problems().is_empty());
    gui.get_by_label(
        format!(
            "{} this draft is ready to run",
            cellarium::gui::theme::state_glyph(cellarium::gui::theme::State::Live)
        )
        .as_str(),
    );

    gui.state_mut().set_growth_source("unknown_symbol()");
    gui.run();
    let problems = gui.state().draft_problems();
    assert!(!problems.is_empty(), "an invalid draft has problems");
    // Each problem leads to the workspace that owns it.
    let section = cellarium::gui::sections::experiment::section_for(&problems[0])
        .expect("this problem names a workspace");
    click(&mut gui, format!("Fix in {}", section.label()).as_str());
    assert_eq!(gui.state().navigation().selected(), section);
}

#[test]
fn the_summary_reports_the_experiment_the_draft_actually_describes() {
    let mut gui = gui_fixture();
    gui.get_by_label("World and lattice");
    gui.get_by_label("16 x 16 cells");
    gui.get_by_label("Channel summary");
    gui.get_by_label("Growth program");
    gui.get_by_label("Compute backends");

    // The summary follows the draft rather than a cached copy of it.
    gui.state_mut().add_kernel();
    gui.run();
    let kernels = gui.state().kernel_cards().len();
    gui.get_by_label(format!("{kernels} kernels").as_str());
}

#[test]
fn applying_twice_without_an_edit_is_harmless() {
    let mut gui = gui_fixture();
    click(&mut gui, "Apply & Run");
    let revision = gui.state().document().active_revision();
    click(&mut gui, "Apply & Run");
    assert!(
        gui.state().document().active_revision() >= revision,
        "a repeated apply must not fail or regress"
    );
    assert!(gui.state().draft_problems().is_empty());
}
