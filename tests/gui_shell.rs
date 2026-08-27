use cellarium::gui::layout;
use cellarium::gui::{CellariumGui, InspectorTab, Section, ShellAction};
use cellarium::sim::experiment_model::ExperimentSpec;
use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

/// The shell harness starts on a section whose workspace has no toolbar of its
/// own. The Simulation workspace deliberately repeats Run, Step and Reset, so
/// testing the shell there would address two controls by the same name.
fn shell_harness() -> Harness<'static, CellariumGui> {
    let mut app = CellariumGui::for_test(ExperimentSpec::single_channel_lenia(64, 64));
    app.navigation_mut().select(Section::Tiling);
    Harness::builder()
        .with_size(egui::vec2(1280.0, 720.0))
        .build_ui_state(|ui, app: &mut CellariumGui| layout::draw(app, ui), app)
}

#[test]
fn every_navigation_section_is_visible_and_selectable() {
    let mut harness = shell_harness();
    harness.run();

    // The workspace heading repeats the section name, so navigation items are
    // addressed by role to stay unambiguous.
    for section in Section::ALL {
        harness.get_by_role_and_label(Role::Button, section.label());
    }

    for section in [Section::Growth, Section::Channels, Section::Experiment] {
        harness
            .get_by_role_and_label(Role::Button, section.label())
            .click();
        harness.run();
        assert_eq!(harness.state().navigation().selected(), section);
    }
}

#[test]
fn every_top_level_action_is_visible_and_clickable() {
    let mut harness = shell_harness();
    harness.run();

    for action in ShellAction::ALL {
        if action == ShellAction::ToggleRunning {
            continue;
        }
        harness.get_by_label(action.label()).click();
        harness.run();
        assert_eq!(harness.state().last_action(), Some(action));
    }
}

#[test]
fn the_run_control_toggles_between_run_and_pause() {
    let mut harness = shell_harness();
    harness.run();

    assert!(!harness.state().running());
    harness.get_by_label("Run").click();
    harness.run();
    assert!(harness.state().running());

    harness.get_by_label("Pause").click();
    harness.run();
    assert!(!harness.state().running());
}

#[test]
fn the_inspector_exposes_properties_and_help_tabs() {
    let mut harness = shell_harness();
    harness.run();

    assert_eq!(harness.state().inspector_tab(), InspectorTab::Properties);
    harness.get_by_label("Help").click();
    harness.run();
    assert_eq!(harness.state().inspector_tab(), InspectorTab::Help);

    harness.get_by_label("Properties").click();
    harness.run();
    assert_eq!(harness.state().inspector_tab(), InspectorTab::Properties);
}
