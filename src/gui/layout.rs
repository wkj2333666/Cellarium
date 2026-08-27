use eframe::egui::{self, RichText, Ui};

use crate::gui::app::{CellariumGui, InspectorTab, Section, ShellAction};
use crate::gui::theme;
use crate::sim::experiment_model::GeometrySpec;

pub fn draw(app: &mut CellariumGui, ui: &mut Ui) {
    top_actions(app, ui);
    status_bar(app, ui);
    navigation(app, ui);
    inspector(app, ui);
    workspace(app, ui);
}

fn top_actions(app: &mut CellariumGui, ui: &mut Ui) {
    egui::Panel::top("top_actions").show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for action in ShellAction::ALL {
                let label = if action == ShellAction::ToggleRunning && app.running() {
                    "Pause"
                } else if action == ShellAction::ToggleRunning {
                    "Run"
                } else {
                    action.label()
                };
                let button = ui
                    .add(egui::Button::new(label).min_size(egui::vec2(0.0, 24.0)))
                    .on_hover_text(action.tooltip());
                if button.clicked() {
                    app.dispatch(action);
                }
            }
        });
    });
}

fn navigation(app: &mut CellariumGui, ui: &mut Ui) {
    egui::Panel::left("navigation")
        .resizable(true)
        .default_size(148.0)
        .size_range(96.0..=280.0)
        .show(ui, |ui| {
            ui.add_space(4.0);
            ui.label(RichText::new("Workspace").strong());
            ui.separator();
            let selected = app.navigation().selected();
            for section in Section::ALL {
                let response = ui
                    .selectable_label(selected == section, section.label())
                    .on_hover_text(section.hint());
                if response.clicked() {
                    app.navigation_mut().select(section);
                }
            }
        });
}

fn inspector(app: &mut CellariumGui, ui: &mut Ui) {
    egui::Panel::right("inspector")
        .resizable(true)
        .default_size(260.0)
        .size_range(180.0..=520.0)
        .show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                for (tab, label) in [
                    (InspectorTab::Properties, "Properties"),
                    (InspectorTab::Help, "Help"),
                ] {
                    if ui
                        .selectable_label(app.inspector_tab() == tab, label)
                        .clicked()
                    {
                        app.set_inspector_tab(tab);
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                if app.backend_panel_open() {
                    backend(app, ui);
                    ui.separator();
                }
                match app.inspector_tab() {
                    InspectorTab::Properties => properties(app, ui),
                    InspectorTab::Help => help(app, ui),
                }
            });
        });
}

fn backend(app: &mut CellariumGui, ui: &mut Ui) {
    let status = app.status();
    let choice = crate::gui::widgets::backend_picker(
        ui,
        crate::gui::widgets::BackendPickerModel {
            policy: app.backend_policy(),
            probes: app.probes(),
            active: Some(status.backend.as_str()),
            notice: status.notice.as_deref(),
        },
    );
    if let Some(crate::gui::widgets::BackendChoice::Select(policy)) = choice {
        app.select_backend(policy);
    }
}

fn properties(app: &CellariumGui, ui: &mut Ui) {
    let section = app.navigation().selected();
    ui.label(RichText::new(section.label()).strong());
    ui.label(section.hint());
    ui.separator();
    let spec = app.spec();
    let GeometrySpec::RasterGrid(grid) = &spec.geometry;
    ui.label(format!("World: {} x {}", grid.width, grid.height));
    ui.label(format!("Channels: {}", spec.channels.len()));
}

fn help(app: &CellariumGui, ui: &mut Ui) {
    ui.label(RichText::new("Shortcuts").strong());
    ui.label("Shortcuts only accelerate actions that already have a visible control.");
    ui.separator();
    for action in ShellAction::ALL {
        ui.label(format!("{} — {}", action.label(), action.tooltip()));
    }
    let _ = app;
}

fn workspace(app: &mut CellariumGui, ui: &mut Ui) {
    egui::CentralPanel::default().show(ui, |ui| {
        let section = app.navigation().selected();
        match section {
            Section::Simulation => return crate::gui::sections::simulation::draw(app, ui),
            Section::Tiling => return crate::gui::sections::tiling::draw(app, ui),
            Section::Channels => return crate::gui::sections::channels::draw(app, ui),
            Section::Kernels => return crate::gui::sections::kernels::draw(app, ui),
            Section::Growth => return crate::gui::sections::growth::draw(app, ui),
            _ => {}
        }
        ui.heading(section.label());
        ui.label(section.hint());
        ui.separator();
        ui.centered_and_justified(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} workspace arrives in a later task",
                    section.label()
                ))
                .italics()
                .color(theme::state_color(theme::State::Draft)),
            );
        });
    });
}

/// A slow backend still deserves an honest rate: rounding 0.4 Hz to "0 Hz"
/// reads as stopped while the simulation is visibly advancing.
fn format_rate(rate: f32) -> String {
    if rate > 0.0 && rate < 10.0 {
        format!("{rate:.1}")
    } else {
        format!("{rate:.0}")
    }
}

fn status_bar(app: &mut CellariumGui, ui: &mut Ui) {
    egui::Panel::bottom("status").show(ui, |ui| {
        let status = app.status();
        ui.horizontal(|ui| {
            let state = if status.draft_clean {
                theme::State::Live
            } else {
                theme::State::Draft
            };
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    theme::state_glyph(state),
                    if status.draft_clean { "Clean" } else { "Dirty" }
                ))
                .color(theme::state_color(state)),
            );
            ui.separator();
            ui.label(format!("tick {}", status.tick));
            ui.separator();
            ui.label(format!("sim {} Hz", format_rate(status.simulation_hz)));
            ui.separator();
            ui.label(format!("frame {} Hz", format_rate(status.frame_hz)));
            ui.separator();
            ui.label(&status.backend);
            if let Some(notice) = &status.notice {
                ui.separator();
                ui.label(RichText::new(notice).color(theme::state_color(theme::State::Invalid)));
            }
        });
    });
}
