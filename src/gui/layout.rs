use eframe::egui::{self, RichText, Ui};

use crate::gui::app::{
    CellariumGui, InspectorTab, NoticeLevel, PendingIntent, Section, ShellAction,
};
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
                let hint = match action.shortcut_text(ui.ctx()) {
                    Some(keys) => format!("{}  ({keys})", action.tooltip()),
                    None => action.tooltip().to_string(),
                };
                let button = ui
                    .add(egui::Button::new(label).min_size(egui::vec2(0.0, 24.0)))
                    .on_hover_text(hint);
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
    ui.label(RichText::new("Getting started").strong());
    for line in [
        "1. Press Run to start the simulation.",
        "2. Press Randomize to fill the world with something to watch.",
        "3. Paint on the world with the left button; the right button erases.",
        "4. Edit the experiment in the workspaces on the left, then Apply & Run.",
    ] {
        // Wrapped explicitly: this panel is narrow and resizable, and a label
        // that refuses to wrap widens the whole column instead.
        ui.add(egui::Label::new(line).wrap());
    }
    ui.separator();

    ui.label(RichText::new("Shortcuts").strong());
    ui.add(
        egui::Label::new("Shortcuts only accelerate actions that already have a visible control.")
            .wrap(),
    );
    ui.separator();
    // The key is the point of this list. Naming the commands without their
    // keys made a panel titled Shortcuts that documented no shortcut.
    for action in ShellAction::ALL {
        // The key and the name share one wrapping line, and the description is
        // on hover. A fixed two-column row is wider than this panel, which
        // pushed the whole list sideways out of view.
        ui.horizontal_wrapped(|ui| {
            match action.shortcut_text(ui.ctx()) {
                Some(keys) => ui.label(RichText::new(keys).monospace().strong()),
                None => ui.label(RichText::new("—").weak()),
            };
            ui.label(action.label());
        })
        .response
        .on_hover_text(action.tooltip());
    }
    let _ = app;
}

fn workspace(app: &mut CellariumGui, ui: &mut Ui) {
    egui::CentralPanel::default().show(ui, |ui| {
        // Every section has a workspace now; there is no placeholder left to
        // fall through to.
        match app.navigation().selected() {
            Section::Simulation => crate::gui::sections::simulation::draw(app, ui),
            Section::Tiling => crate::gui::sections::tiling::draw(app, ui),
            Section::Channels => crate::gui::sections::channels::draw(app, ui),
            Section::Kernels => crate::gui::sections::kernels::draw(app, ui),
            Section::Growth => crate::gui::sections::growth::draw(app, ui),
            Section::Experiment => crate::gui::sections::experiment::draw(app, ui),
        }
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
            // While replaying, the canvas is showing a recorded frame. The bar
            // has to describe that frame: reporting the live tick and rate
            // beside a picture of something else is two truths at once.
            match &status.replay {
                Some(replay) => {
                    ui.label(
                        RichText::new(format!("replay {} / {}", replay.frame, replay.frames))
                            .color(theme::state_color(theme::State::Stale)),
                    );
                    ui.separator();
                    ui.label(format!("tick {}", replay.tick));
                }
                None => {
                    ui.label(format!("tick {}", status.tick));
                    ui.separator();
                    ui.label(format!("sim {} Hz", format_rate(status.simulation_hz)));
                }
            }
            ui.separator();
            ui.label(format!("frame {} Hz", format_rate(status.frame_hz)));
            ui.separator();
            ui.label(&status.backend);
            if let Some(notice) = &status.notice {
                ui.separator();
                let state = match status.notice_level {
                    NoticeLevel::Problem => theme::State::Invalid,
                    NoticeLevel::Info => theme::State::Live,
                };
                // A long reason is truncated rather than run off the edge of
                // the window, with the whole of it on hover: a message cut
                // mid-word is a message the user cannot act on.
                ui.add(
                    egui::Label::new(RichText::new(notice).color(theme::state_color(state)))
                        .truncate(),
                )
                .on_hover_text(notice);
            }
        });
    });
}

/// Questions that have to be answered before anything else continues.
///
/// Both of these exist because work was being lost silently: a session that
/// ended without saving threw away a draft it had already written to disk, and
/// New replaced an edited experiment without asking.
pub fn modals(app: &mut CellariumGui, ctx: &egui::Context) {
    recovery_prompt(app, ctx);
    unsaved_prompt(app, ctx);
}

fn recovery_prompt(app: &mut CellariumGui, ctx: &egui::Context) {
    let Some(spec) = app.pending_recovery() else {
        return;
    };
    let summary = format!(
        "\u{201c}{}\u{201d} \u{2014} {} x {}, {}",
        spec.name,
        match &spec.geometry {
            GeometrySpec::RasterGrid(grid) => grid.width,
        },
        match &spec.geometry {
            GeometrySpec::RasterGrid(grid) => grid.height,
        },
        theme::plural(spec.channels.len(), "channel", "channels"),
    );
    let mut decision = None;
    egui::Modal::new(egui::Id::new("recovery_prompt")).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.label(RichText::new("Restore your last session?").strong());
        ui.label("The last session closed without saving. This is what it was working on:");
        ui.label(RichText::new(summary).monospace());
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .button("Restore it")
                .on_hover_text("Open the recovered experiment and carry on")
                .clicked()
            {
                decision = Some(true);
            }
            if ui
                .button("Start fresh")
                .on_hover_text("Discard the recovered experiment and begin a new one")
                .clicked()
            {
                decision = Some(false);
            }
        });
    });
    match decision {
        Some(true) => app.accept_recovery(),
        Some(false) => app.decline_recovery(),
        None => {}
    }
}

fn unsaved_prompt(app: &mut CellariumGui, ctx: &egui::Context) {
    let Some(intent) = app.pending_intent() else {
        return;
    };
    let action = match intent {
        PendingIntent::New => "Starting a new experiment",
        PendingIntent::Open => "Opening another experiment",
    };
    let mut decision = None;
    egui::Modal::new(egui::Id::new("unsaved_prompt")).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.label(RichText::new("You have unsaved changes").strong());
        ui.label(format!("{action} will replace what is on screen."));
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .button("Save first")
                .on_hover_text("Write this experiment to disk, then continue")
                .clicked()
            {
                decision = Some(Choice::Save);
            }
            if ui
                .button("Discard changes")
                .on_hover_text("Continue without saving")
                .clicked()
            {
                decision = Some(Choice::Discard);
            }
            if ui.button("Cancel").clicked() {
                decision = Some(Choice::Cancel);
            }
        });
    });
    match decision {
        Some(Choice::Save) => {
            // Save first, then carry on with what was asked for. If the
            // experiment has no name yet this opens the file dialog, and the
            // original intent is dropped rather than firing behind it.
            app.resolve_pending_intent(false);
            app.save_experiment();
        }
        Some(Choice::Discard) => app.resolve_pending_intent(true),
        Some(Choice::Cancel) => app.resolve_pending_intent(false),
        None => {}
    }
}

enum Choice {
    Save,
    Discard,
    Cancel,
}
