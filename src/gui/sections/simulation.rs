//! The Simulation workspace: run controls, the live canvas and a hover readout.

use eframe::egui::{self, RichText, Ui};

use crate::document::brush::{BrushKind, BrushTarget};
use crate::document::recording::ReplayState;
use crate::gui::app::CellariumGui;
use crate::gui::canvas::world::{ChannelView, render_world_canvas};
use crate::gui::theme;
use crate::render::channels::automatic_palette;
use crate::sim::worker::SimulationCommand;

/// A control the user can reach with the pointer. Kept as data so the toolbar
/// and the tests agree on what exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SimulationControl {
    RunPause,
    Step,
    Reset,
    Randomize,
    Clear,
    Fit,
}

impl SimulationControl {
    pub const ALL: [SimulationControl; 6] = [
        SimulationControl::RunPause,
        SimulationControl::Step,
        SimulationControl::Reset,
        SimulationControl::Randomize,
        SimulationControl::Clear,
        SimulationControl::Fit,
    ];

    pub fn label(self, running: bool) -> &'static str {
        match self {
            SimulationControl::RunPause if running => "Pause",
            SimulationControl::RunPause => "Run",
            SimulationControl::Step => "Step",
            SimulationControl::Reset => "Reset",
            SimulationControl::Randomize => "Randomize",
            SimulationControl::Clear => "Clear",
            SimulationControl::Fit => "Fit",
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            SimulationControl::RunPause => "Run or pause the simulation",
            SimulationControl::Step => "Advance one tick",
            SimulationControl::Reset => "Restore the initial state",
            SimulationControl::Randomize => "Fill the world with random values",
            SimulationControl::Clear => "Set every cell to zero",
            SimulationControl::Fit => "Fit the whole world in view",
        }
    }
}

pub fn draw(app: &mut CellariumGui, ui: &mut Ui) {
    toolbar(app, ui);
    ui.separator();
    canvas(app, ui);
}

fn toolbar(app: &mut CellariumGui, ui: &mut Ui) {
    let running = app.running();
    ui.horizontal_wrapped(|ui| {
        for control in SimulationControl::ALL {
            if ui
                .button(control.label(running))
                .on_hover_text(control.tooltip())
                .clicked()
            {
                app.dispatch_simulation(control);
            }
        }
        ui.separator();

        let channels = app.spec().channels.len();
        let mut view = app.world_canvas().view;
        egui::ComboBox::from_id_salt("channel_view")
            .selected_text(match view {
                ChannelView::Composite => "Composite".to_string(),
                ChannelView::Solo(index) => format!("Solo {}", index + 1),
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut view, ChannelView::Composite, "Composite");
                for index in 0..channels {
                    ui.selectable_value(
                        &mut view,
                        ChannelView::Solo(index),
                        format!("Solo {}", index + 1),
                    );
                }
            });
        app.world_canvas_mut().view = view;

        ui.separator();
        let capturing = app.recording().is_capturing();
        if ui
            .add(
                egui::Button::selectable(
                    capturing,
                    if capturing {
                        "Stop recording"
                    } else {
                        "Record"
                    },
                )
                .min_size(egui::vec2(104.0, 0.0)),
            )
            .on_hover_text(if capturing {
                "Stop adding frames to the take"
            } else {
                "Keep every frame the simulation shows, so it can be replayed"
            })
            .clicked()
        {
            app.toggle_recording();
        }
    });
    brush_bar(app, ui);
    // The replay controls only exist once there is a take to replay. An empty
    // row of disabled buttons is clutter a new user has to read past.
    if app.recording().frames() > 0 {
        recording_bar(app, ui);
    }
}

/// What the pointer paints with.
///
/// The tools are named and described where they are chosen, so picking one
/// never requires knowing what a falloff profile is.
fn brush_bar(app: &mut CellariumGui, ui: &mut Ui) {
    ui.horizontal_wrapped(|ui| {
        // egui's default slider is wide enough that three of them plus the tool
        // buttons run past the panel and clip the last label.
        ui.spacing_mut().slider_width = 84.0;
        ui.label("Brush")
            .on_hover_text("The left button paints, the right button erases");
        let mut brush = app.world_canvas().brush;
        let mut changed = false;
        for kind in BrushKind::ALL {
            if ui
                .add(egui::Button::selectable(brush.kind == kind, kind.label()))
                .on_hover_text(kind.hint())
                .clicked()
                && brush.kind != kind
            {
                brush.select_kind(kind);
                changed = true;
            }
        }

        ui.separator();
        // A pencil is one cell by definition, so its size control is disabled
        // rather than silently ignored.
        let sized = brush.kind.has_radius();
        ui.add_enabled_ui(sized, |ui| {
            changed |= ui
                .add(
                    egui::DragValue::new(&mut brush.radius)
                        .range(0..=32)
                        .prefix("size "),
                )
                .on_hover_text(if sized {
                    "Radius of the brush, in cells"
                } else {
                    "A pencil always covers exactly one cell"
                })
                .changed();
        });

        ui.separator();
        let mut percent = (brush.flow * 100.0).round();
        if ui
            .add(
                egui::Slider::new(&mut percent, 0.0..=100.0)
                    .suffix("%")
                    .text("strength"),
            )
            .on_hover_text("How far one pass moves a cell towards the value below")
            .changed()
        {
            brush.flow = percent / 100.0;
            changed = true;
        }

        // An eraser always paints zero, so offering it a value would be
        // offering a control that does nothing.
        ui.add_enabled_ui(brush.kind != BrushKind::Eraser, |ui| {
            changed |= ui
                .add(egui::Slider::new(&mut brush.value, 0.0..=1.0).text("value"))
                .on_hover_text("The value a full-strength stroke paints")
                .changed();
        });

        let channels = app.spec().channels.len();
        if channels > 1 {
            ui.separator();
            let label = match brush.target {
                BrushTarget::AllChannels => "All channels".to_string(),
                BrushTarget::Channel(index) => app
                    .spec()
                    .channels
                    .get(index)
                    .map(|channel| channel.name.clone())
                    .unwrap_or_else(|| format!("channel {}", index + 1)),
            };
            egui::ComboBox::from_id_salt("brush_target")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(
                            &mut brush.target,
                            BrushTarget::AllChannels,
                            "All channels",
                        )
                        .changed();
                    for index in 0..channels {
                        let name = app
                            .spec()
                            .channels
                            .get(index)
                            .map(|channel| channel.name.clone())
                            .unwrap_or_else(|| format!("channel {}", index + 1));
                        changed |= ui
                            .selectable_value(&mut brush.target, BrushTarget::Channel(index), name)
                            .changed();
                    }
                });
            ui.label(RichText::new("paint into").weak());
        }

        if changed {
            app.world_canvas_mut().brush = brush;
        }
    });
}

/// Recording the run, and playing it back.
fn recording_bar(app: &mut CellariumGui, ui: &mut Ui) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().slider_width = 120.0;
        let frames = app.recording().frames();
        let replaying = app.recording().is_replaying();
        ui.label(RichText::new("Take").weak());

        ui.add_enabled_ui(frames > 0, |ui| {
            let playing = app.recording().state() == ReplayState::Playing;
            if ui
                .add(
                    egui::Button::new(if playing { "Pause replay" } else { "Play" })
                        .min_size(egui::vec2(96.0, 0.0)),
                )
                .on_hover_text("Play the recorded frames back")
                .clicked()
            {
                app.toggle_replay();
            }
            if ui
                .button("<")
                .on_hover_text("Step back one recorded frame")
                .clicked()
            {
                app.recording_mut().nudge(-1);
            }
            if ui
                .button(">")
                .on_hover_text("Step forward one recorded frame")
                .clicked()
            {
                app.recording_mut().nudge(1);
            }

            // The scrubber is the whole point of a replay: being able to go
            // back to the moment something happened.
            let mut playhead = app.recording().playhead();
            let last = frames.saturating_sub(1);
            if ui
                .add(
                    egui::Slider::new(&mut playhead, 0..=last)
                        .text("frame")
                        .clamping(egui::SliderClamping::Always),
                )
                .on_hover_text("Scrub through the take")
                .changed()
            {
                app.recording_mut().seek(playhead);
            }

            let mut speed = app.recording().speed();
            if ui
                .add(egui::Slider::new(&mut speed, 1.0..=120.0).text("play/s"))
                .on_hover_text("Frames per second of playback")
                .changed()
            {
                app.recording_mut().set_speed(speed);
            }
        });

        let mut rate = app.recording().capture_rate();
        if ui
            .add(egui::DragValue::new(&mut rate).range(1.0..=120.0).prefix("record ").suffix("/s"))
            .on_hover_text(
                "Frames captured per second. Frames are large, so a high rate fills memory quickly.",
            )
            .changed()
        {
            app.recording_mut().set_capture_rate(rate);
        }

        if replaying
            && ui
                .button("Back to live")
                .on_hover_text("Stop replaying and show the running world again")
                .clicked()
        {
            app.recording_mut().resume_live();
        }
        if ui
            .add_enabled(frames > 0, egui::Button::new("Clear take"))
            .on_hover_text("Discard every recorded frame and free the memory")
            .clicked()
        {
            app.recording_mut().clear();
        }

        // What the take costs, said plainly. Frames are large, and a user who
        // leaves recording on deserves to see the number climbing.
        if frames > 0 {
            ui.separator();
            ui.label(
                RichText::new(app.recording().summary())
                    .weak(),
            );
        }
    });
}

fn canvas(app: &mut CellariumGui, ui: &mut Ui) {
    // While replaying, the canvas shows the recorded frame. Everything beside
    // it then describes that frame, so the readouts never belong to a world the
    // user is not looking at.
    let replaying = app.recording().is_replaying();
    let snapshot = app.displayed_snapshot();
    let colors = automatic_palette(app.spec().channels.len());
    // Leave a row for the hover readout, otherwise the canvas claims the whole
    // panel and the readout is clipped off the bottom edge.
    let readout_height =
        ui.text_style_height(&egui::TextStyle::Body) + ui.spacing().item_spacing.y * 2.0;
    let size = egui::vec2(
        ui.available_width(),
        (ui.available_height() - readout_height).max(64.0),
    );
    let response = {
        let state = app.world_canvas_mut();
        render_world_canvas(ui, size, snapshot.as_deref(), &colors, state)
    };

    if !response.edits.is_empty() {
        if replaying {
            // Painting a recorded frame would either be discarded or would
            // rewrite history. Saying so is better than accepting a stroke that
            // quietly does nothing.
            app.set_notice(Some(
                "this is a recorded frame — press Back to live to paint".into(),
            ));
        } else {
            // Pointer paint is batched into one ordered command per frame rather
            // than one command per cell.
            app.send_simulation(SimulationCommand::EditWorld(response.edits));
        }
    }

    if replaying {
        let frame = app.recording().playhead() + 1;
        let total = app.recording().frames();
        let tick = app
            .recording()
            .current_tick()
            .map(|tick| tick.to_string())
            .unwrap_or_else(|| "?".into());
        ui.label(
            RichText::new(format!("replaying frame {frame} of {total} — tick {tick}"))
                .color(theme::state_color(theme::State::Stale)),
        );
        return;
    }

    if let Some((basis, x, y)) = response.hovered_cell {
        let readout = snapshot
            .as_ref()
            .and_then(|snapshot| {
                snapshot
                    .layout
                    .index_by_position(0, x, y, basis)
                    .and_then(|index| snapshot.cells.get(index).copied())
            })
            .map(|value| format!("({x}, {y}) basis {basis} = {value:.3}"))
            .unwrap_or_else(|| format!("({x}, {y}) basis {basis}"));
        ui.label(RichText::new(readout).weak());
    } else if is_world_empty(snapshot.as_deref()) {
        // A new user presses Run, sees a counter climbing beside a black
        // square, and has no way to know the world simply has nothing in it.
        ui.label(
            RichText::new("the world is empty — press Randomize, or paint on it with the mouse")
                .color(theme::state_color(theme::State::Draft)),
        );
    } else {
        ui.label(RichText::new("hover the world to inspect a cell").weak());
    }
}

/// Whether every cell is zero, so nothing would be visible.
fn is_world_empty(snapshot: Option<&crate::sim::worker::SimulationSnapshot>) -> bool {
    snapshot.is_some_and(|snapshot| snapshot.cells.iter().all(|value| *value == 0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_control_has_a_label_and_tooltip() {
        for (index, control) in SimulationControl::ALL.iter().enumerate() {
            assert!(!control.label(false).is_empty());
            assert!(!control.tooltip().is_empty());
            for other in &SimulationControl::ALL[index + 1..] {
                assert_ne!(control.tooltip(), other.tooltip());
            }
        }
    }

    #[test]
    fn the_run_control_renames_itself_when_running() {
        assert_eq!(SimulationControl::RunPause.label(false), "Run");
        assert_eq!(SimulationControl::RunPause.label(true), "Pause");
    }
}
