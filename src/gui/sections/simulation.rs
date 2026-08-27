//! The Simulation workspace: run controls, the live canvas and a hover readout.

use eframe::egui::{self, RichText, Ui};

use crate::gui::app::CellariumGui;
use crate::gui::canvas::world::{ChannelView, render_world_canvas};
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
        ui.label("Brush");
        let mut radius = app.world_canvas().brush_radius;
        if ui
            .add(egui::DragValue::new(&mut radius).range(0..=64).prefix("r "))
            .on_hover_text("Brush radius in cells")
            .changed()
        {
            app.world_canvas_mut().brush_radius = radius;
        }
        let mut value = app.world_canvas().brush_value;
        if ui
            .add(egui::Slider::new(&mut value, 0.0..=1.0).text("value"))
            .on_hover_text("Value the left button paints")
            .changed()
        {
            app.world_canvas_mut().brush_value = value;
        }
    });
}

fn canvas(app: &mut CellariumGui, ui: &mut Ui) {
    let snapshot = app.snapshot();
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
        // Pointer paint is batched into one ordered command per frame rather
        // than one command per cell.
        app.send_simulation(SimulationCommand::EditWorld(response.edits));
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
    } else {
        ui.label(RichText::new("hover the world to inspect a cell").weak());
    }
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
