use eframe::egui::{self, RichText, Ui};

use crate::gui::app::{
    CellariumGui, InspectorTab, NoticeLevel, PendingIntent, Section, ShellAction,
};
use crate::gui::style;
use crate::gui::theme;
use crate::sim::experiment_model::GeometrySpec;

pub fn draw(app: &mut CellariumGui, ui: &mut Ui) {
    top_actions(app, ui);
    status_bar(app, ui);
    navigation(app, ui);
    inspector(app, ui);
    workspace(app, ui);
}

/// The window toolbar.
///
/// Actions are grouped by what they do to your work — the file, the history,
/// the run — and weighted by consequence. Before this they were eleven
/// identical rectangles in which `Apply & Run`, the reason the application is
/// open, looked exactly like `Save as`, and `Reset`, which throws the world
/// away, looked exactly like `Step`.
///
/// This is also the only home for the transport. The Simulation workspace used
/// to repeat Run, Step and Reset directly beneath these, at the same size and
/// with the same words, so neither row could be described as the one that
/// works.
fn top_actions(app: &mut CellariumGui, ui: &mut Ui) {
    egui::Panel::top("top_actions").show(ui, |ui| {
        ui.add_space(3.0);
        ui.horizontal_wrapped(|ui| {
            for action in [
                ShellAction::New,
                ShellAction::Open,
                ShellAction::Save,
                ShellAction::SaveAs,
            ] {
                shell_button(app, ui, action);
            }
            ui.separator();
            for action in [ShellAction::Undo, ShellAction::Redo] {
                shell_button(app, ui, action);
            }
            ui.separator();
            shell_button(app, ui, ShellAction::ApplyAndRun);
            for action in [
                ShellAction::ToggleRunning,
                ShellAction::Step,
                ShellAction::Reset,
            ] {
                shell_button(app, ui, action);
            }
            ui.separator();
            shell_button(app, ui, ShellAction::Backend);
        });
        ui.add_space(3.0);
    });
}

/// How much weight an action carries in the toolbar.
fn action_weight(action: ShellAction) -> Weight {
    match action {
        // One filled button in the window. If a second ever appears here,
        // neither of them is primary any more.
        ShellAction::ApplyAndRun => Weight::Primary,
        // Reset discards the running world. It has to be findable without
        // looking like the button beside it.
        ShellAction::Reset => Weight::Danger,
        _ => Weight::Normal,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Weight {
    Primary,
    Normal,
    Danger,
}

fn shell_button(app: &mut CellariumGui, ui: &mut Ui, action: ShellAction) {
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
    let button = match action_weight(action) {
        Weight::Primary => style::primary(label),
        Weight::Danger => style::danger(label),
        Weight::Normal => style::secondary(label),
    };
    if ui
        .add(button.min_size(egui::vec2(0.0, 26.0)))
        .on_hover_text(hint)
        .clicked()
    {
        app.dispatch(action);
    }
}

fn navigation(app: &mut CellariumGui, ui: &mut Ui) {
    egui::Panel::left("navigation")
        .resizable(true)
        .default_size(164.0)
        .size_range(120.0..=280.0)
        .show(ui, |ui| {
            ui.add_space(6.0);
            style::group_caption(ui, "WORKSPACE");
            ui.add_space(2.0);
            let selected = app.navigation().selected();
            for section in Section::ALL {
                // Full width, so the whole row is the target rather than the
                // few pixels the word happens to cover.
                let width = ui.available_width();
                let response = ui
                    .add_sized(
                        egui::vec2(width, 28.0),
                        egui::Button::selectable(selected == section, section.label()),
                    )
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
        .default_size(276.0)
        .size_range(200.0..=520.0)
        .show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (tab, label) in [
                    (InspectorTab::Properties, "Properties"),
                    (InspectorTab::Help, "Help"),
                ] {
                    if ui
                        .add(egui::Button::selectable(app.inspector_tab() == tab, label))
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

/// One fact, as a name and a value on the same line.
///
/// The value is monospaced and pushed to the right so a column of them can be
/// read down rather than word by word, and so a number that changes every
/// frame stops shifting the text beside it.
fn fact(ui: &mut Ui, name: &str, value: impl Into<String>) {
    let value = value.into();
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).color(style::TEXT_DIM));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Truncated, with the whole of it on hover. A value wider than the
            // panel — "CUDA (NVIDIA GeForce RTX 2080 Ti)" is the one that
            // caught this — otherwise grows leftwards until it is drawn on top
            // of its own label, and two strings in the same pixels are less
            // readable than either alone.
            ui.add(egui::Label::new(style::readout(value.clone())).truncate())
                .on_hover_text(value);
        });
    });
}

/// What the inspector says about the workspace you are in.
///
/// This panel used to hold three lines above an empty column the height of the
/// window. Each workspace now answers the questions it is actually asked while
/// it is open.
fn properties(app: &CellariumGui, ui: &mut Ui) {
    let section = app.navigation().selected();
    style::section_header(ui, section.label());
    ui.add(egui::Label::new(RichText::new(section.hint()).color(style::TEXT_DIM)).wrap());
    ui.separator();

    let spec = app.spec();
    let GeometrySpec::RasterGrid(grid) = &spec.geometry;
    let status = app.status();
    style::group_caption(ui, "EXPERIMENT");
    fact(ui, "World", format!("{} x {}", grid.width, grid.height));
    fact(ui, "Channels", spec.channels.len().to_string());
    fact(ui, "Tick", status.tick.to_string());
    // Not "Backend": that is the name of the toolbar button that opens the
    // backend picker, and two nodes with one label is how a user — or a
    // screen reader — reaches the wrong one.
    fact(ui, "Runs on", status.backend.clone());
    fact(
        ui,
        "Draft",
        if status.draft_clean { "clean" } else { "dirty" },
    );
    ui.separator();

    match section {
        Section::Tiling => tiling_properties(app, ui),
        Section::Channels => channel_properties(app, ui),
        other => section_facts(app, ui, other),
    }
}

/// The seam assistant's standing verdict, visible without pressing anything.
fn tiling_properties(app: &CellariumGui, ui: &mut Ui) {
    style::group_caption(ui, "SEAMS");
    let Some(draft) = app.spec().tiling.as_ref() else {
        ui.add(
            egui::Label::new(
                RichText::new("No tiling yet. Pick a preset or draw a polygon.")
                    .color(style::TEXT_DIM),
            )
            .wrap(),
        );
        return;
    };
    match crate::sim::tiling::assess_seams(draft) {
        Ok(assessment) => {
            fact(ui, "Edges", assessment.edge_count.to_string());
            for bucket in [
                crate::sim::tiling::SeamBucket::Held,
                crate::sim::tiling::SeamBucket::Ready,
                crate::sim::tiling::SeamBucket::Near,
            ] {
                fact(ui, bucket.label(), assessment.count(bucket).to_string());
            }
            fact(ui, "unpaired", assessment.orphans.len().to_string());
            fact(ui, "accepted", app.tiling_canvas().seams.len().to_string());
        }
        Err(reason) => {
            ui.add(
                egui::Label::new(
                    RichText::new(reason).color(theme::state_color(theme::State::Invalid)),
                )
                .wrap(),
            );
        }
    }
}

/// Facts about the channels that the card strip does not already show.
///
/// Deliberately not a list of names. The strip beside this panel is the list,
/// and repeating each name here put two nodes with the same label on screen —
/// the same "which one did I just reach" problem this pass removed from the
/// toolbars. A summary answers a different question instead: how many of these
/// are actually contributing to what I am looking at.
fn channel_properties(app: &CellariumGui, ui: &mut Ui) {
    style::group_caption(ui, "CHANNELS");
    let channels = &app.spec().channels;
    let hidden = channels
        .iter()
        .filter(|channel| !channel.display.visible)
        .count();
    let frozen = channels.iter().filter(|channel| channel.frozen).count();
    fact(ui, "Total", channels.len().to_string());
    fact(ui, "Visible", (channels.len() - hidden).to_string());
    fact(ui, "Frozen", frozen.to_string());

    ui.add_space(4.0);
    // The palette as swatches, in the order the strip lays the cards out, so
    // the composite view can be read back to the channel that painted it.
    let palette = crate::render::channels::automatic_palette(channels.len());
    ui.horizontal_wrapped(|ui| {
        for (index, channel) in channels.iter().enumerate() {
            let colour = palette
                .get(index)
                .copied()
                .unwrap_or(crate::render::channels::Rgb8::new(255, 255, 255));
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
            let fill = egui::Color32::from_rgb(colour.red, colour.green, colour.blue);
            ui.painter().rect_filled(
                rect,
                3.0,
                if channel.display.visible {
                    fill
                } else {
                    fill.gamma_multiply(0.25)
                },
            );
            response.on_hover_text(if channel.display.visible {
                channel.name.clone()
            } else {
                format!("{} (hidden)", channel.name)
            });
        }
    });
}

fn section_facts(app: &CellariumGui, ui: &mut Ui, section: Section) {
    match section {
        Section::Kernels => {
            style::group_caption(ui, "KERNELS");
            fact(ui, "Defined", app.spec().kernels.len().to_string());
        }
        Section::Growth => {
            style::group_caption(ui, "GROWTH");
            fact(ui, "Programs", app.spec().growth.len().to_string());
            fact(ui, "Rule sets", app.spec().rules.sets.len().to_string());
            fact(ui, "Bindings", app.spec().rules.bindings.len().to_string());
        }
        Section::Simulation => {
            style::group_caption(ui, "RUN");
            let status = app.status();
            fact(ui, "Running", if app.running() { "yes" } else { "no" });
            fact(
                ui,
                "Sim",
                format!("{} Hz", format_rate(status.simulation_hz)),
            );
            fact(ui, "Frame", format!("{} Hz", format_rate(status.frame_hz)));
            fact(ui, "Recorded", app.recording().frames().to_string());
        }
        _ => {}
    }
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
