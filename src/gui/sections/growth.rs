//! The Growth workspace: signature, source editor and response plot.

use eframe::egui::{self, RichText, Ui};

use crate::document::selection::{PlotAxes, PlotSymbol};
use crate::gui::app::CellariumGui;
use crate::gui::canvas::growth::{PlotScene, render_growth_plot};
use crate::gui::theme;
use crate::gui::widgets::code_editor::{Diagnostic, code_editor, position_of};
use crate::sim::experiment_model::{KernelId, UpdateMode};

/// Which axis a chip assigns a symbol to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Axis {
    X,
    Y,
}

pub fn draw(app: &mut CellariumGui, ui: &mut Ui) {
    header(app, ui);
    ui.separator();
    editor(app, ui);
    ui.separator();
    controls(app, ui);
    plot(app, ui);
}

fn header(app: &mut CellariumGui, ui: &mut Ui) {
    let signature = app.growth_signature();
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(signature.rendered()).monospace().strong());
        ui.separator();
        let mut mode = app.growth_mode();
        for (candidate, label) in [
            (UpdateMode::GrowthRate, "Rate"),
            (UpdateMode::DirectUpdate, "Value"),
        ] {
            if ui
                .add(egui::Button::selectable(mode == candidate, label))
                .on_hover_text(match candidate {
                    UpdateMode::GrowthRate => "The result is added to the channel's value",
                    UpdateMode::DirectUpdate => "The result replaces the channel's value",
                })
                .clicked()
            {
                mode = candidate;
            }
        }
        if mode != app.growth_mode() {
            app.set_growth_mode(mode);
        }
    });

    // Kernel chips: each one navigates to the kernel it names, so a symbol in
    // the signature is a way into the thing it refers to.
    let referenced = app.growth_referenced();
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Inputs").weak());
        for (symbol, id) in signature
            .kernel_inputs
            .iter()
            .zip(&signature.kernel_ids)
            .map(|(symbol, id)| (symbol.clone(), *id))
            .collect::<Vec<_>>()
        {
            let used = referenced.contains(&symbol);
            let text = if used {
                RichText::new(&symbol).color(theme::state_color(theme::State::Live))
            } else {
                // A declared input the program ignores is shown as available
                // rather than as something the program depends on.
                RichText::new(&symbol).weak()
            };
            if ui
                .button(text)
                .on_hover_text(if used {
                    "This program reads this kernel. Click to edit it."
                } else {
                    "Available to this program but not read. Click to edit it."
                })
                .clicked()
            {
                app.open_kernel(id);
            }
            // The chips say which axis this input is already on, and the one
            // that could only decline is disabled rather than left looking
            // clickable: a control that promises an axis and then quietly does
            // nothing reads as an application that has stopped responding.
            let (on_x, on_y) = match app.plot_axes() {
                PlotAxes::Curve(x) => (x == PlotSymbol::Kernel(id), false),
                PlotAxes::Heatmap(x, y) => {
                    (x == PlotSymbol::Kernel(id), y == PlotSymbol::Kernel(id))
                }
            };
            for (axis, label, active) in [(Axis::X, "x", on_x), (Axis::Y, "y", on_y)] {
                // Putting one symbol on both axes plots a diagonal and says
                // nothing, so y is unavailable while this input holds x.
                let usable = !(axis == Axis::Y && on_x);
                if ui
                    .add_enabled(
                        usable,
                        egui::Button::selectable(active, RichText::new(label).small()),
                    )
                    .on_hover_text(format!("Plot against {symbol} on the {label} axis"))
                    .on_disabled_hover_text(format!(
                        "{symbol} is already the x axis; put another input on y"
                    ))
                    .clicked()
                {
                    app.set_plot_axis(axis, PlotSymbol::Kernel(id));
                }
            }
        }
        ui.separator();
        let self_on_x = matches!(
            app.plot_axes(),
            PlotAxes::Curve(PlotSymbol::SelfValue) | PlotAxes::Heatmap(PlotSymbol::SelfValue, _)
        );
        if ui
            .add(egui::Button::selectable(self_on_x, "self"))
            .on_hover_text("Plot against the channel's own value")
            .clicked()
        {
            app.set_plot_axis(Axis::X, PlotSymbol::SelfValue);
        }
    });
}

fn editor(app: &mut CellariumGui, ui: &mut Ui) {
    let diagnostics: Vec<Diagnostic> = app
        .growth_diagnostics()
        .iter()
        .map(|diagnostic| Diagnostic {
            code: diagnostic.code.clone(),
            start: diagnostic.start,
            end: diagnostic.end,
        })
        .collect();
    let mut source = app.growth_source();
    if code_editor(ui, "growth_source", &mut source, &diagnostics, 8) {
        app.set_growth_source(source.clone());
    }
    if diagnostics.is_empty() {
        ui.label(RichText::new("compiles").color(theme::state_color(theme::State::Live)));
    } else {
        for diagnostic in &diagnostics {
            let (line, column) = position_of(&source, diagnostic.start);
            // The location is stated in the coordinates the editor shows, not
            // as a byte offset the user would have to count out.
            ui.label(
                RichText::new(format!(
                    "line {line}, column {column}: {}",
                    crate::document::growth::describe_diagnostic(
                        &diagnostic.code,
                        source.get(diagnostic.start..diagnostic.end).unwrap_or("")
                    )
                ))
                .color(theme::state_color(theme::State::Invalid)),
            );
        }
    }
}

fn controls(app: &mut CellariumGui, ui: &mut Ui) {
    ui.horizontal_wrapped(|ui| {
        let mut domain = app.growth_plot().domain;
        let mut changed = false;
        changed |= ui
            .add(
                egui::DragValue::new(&mut domain[0])
                    .speed(0.01)
                    .prefix("min "),
            )
            .on_hover_text("Lowest input value the plot samples")
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut domain[1])
                    .speed(0.01)
                    .prefix("max "),
            )
            .on_hover_text("Highest input value the plot samples")
            .changed();
        if changed && domain[1] > domain[0] {
            app.growth_plot_mut().domain = domain;
        }

        ui.separator();
        // Pinned inputs, which is what the axes are held against.
        let mut self_value = app.growth_plot().pinned.self_value;
        if ui
            .add(egui::Slider::new(&mut self_value, 0.0..=1.0).text("pin self"))
            .on_hover_text("Value the channel is held at when self is not an axis")
            .changed()
        {
            app.growth_plot_mut().pinned.self_value = self_value;
        }
    });

    let signature = app.growth_signature();
    let axes = app.plot_axes();
    ui.horizontal_wrapped(|ui| {
        for (symbol, id) in signature
            .kernel_inputs
            .iter()
            .zip(&signature.kernel_ids)
            .map(|(symbol, id)| (symbol.clone(), *id))
            .collect::<Vec<_>>()
        {
            if is_axis(axes, PlotSymbol::Kernel(id)) {
                continue;
            }
            let mut value = app.growth_plot().pinned.kernel(id);
            if ui
                .add(egui::Slider::new(&mut value, 0.0..=1.0).text(format!("pin {symbol}")))
                .on_hover_text(format!("{symbol} is held here while the axes vary"))
                .changed()
            {
                app.growth_plot_mut().pinned.set_kernel(id, value);
            }
        }
    });
}

fn is_axis(axes: PlotAxes, symbol: PlotSymbol) -> bool {
    match axes {
        PlotAxes::Curve(only) => only == symbol,
        PlotAxes::Heatmap(x, y) => x == symbol || y == symbol,
    }
}

fn plot(app: &mut CellariumGui, ui: &mut Ui) {
    let scene = app.growth_scene();
    let stale = !app.growth_diagnostics().is_empty();
    let quantity = app.growth_quantity();
    let domain = app.growth_plot().domain;

    let caption_height =
        ui.text_style_height(&egui::TextStyle::Body) + ui.spacing().item_spacing.y * 2.0;
    let size = egui::vec2(
        ui.available_width(),
        (ui.available_height() - caption_height).max(96.0),
    );
    match &scene {
        Some(scene) => render_growth_plot(ui, size, scene, quantity, domain, stale),
        None => {
            ui.allocate_exact_size(size, egui::Sense::hover());
            ui.label(
                RichText::new("the program does not compile, so there is nothing to plot")
                    .color(theme::state_color(theme::State::Invalid)),
            );
            return;
        }
    }
    ui.label(RichText::new(axes_caption(app.plot_axes(), app)).weak());
}

fn axes_caption(axes: PlotAxes, app: &CellariumGui) -> String {
    let name = |symbol: PlotSymbol| match symbol {
        PlotSymbol::SelfValue => "self".to_string(),
        PlotSymbol::Kernel(id) => app.kernel_symbol(id),
    };
    match axes {
        PlotAxes::Curve(symbol) => format!("x: {}", name(symbol)),
        PlotAxes::Heatmap(x, y) => format!("x: {}, y: {}", name(x), name(y)),
    }
}

/// The plot the section would draw, exposed so a caller can assert on it.
pub fn scene_is_empty(scene: &Option<PlotScene>) -> bool {
    scene.is_none()
}

/// Kernel ids the section offers as axes, in signature order.
pub fn axis_candidates(app: &CellariumGui) -> Vec<KernelId> {
    app.growth_signature().kernel_ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_axis_symbol_is_recognised_in_both_plot_shapes() {
        let curve = PlotAxes::Curve(PlotSymbol::Kernel(KernelId(1)));
        assert!(is_axis(curve, PlotSymbol::Kernel(KernelId(1))));
        assert!(!is_axis(curve, PlotSymbol::Kernel(KernelId(2))));

        let heatmap = PlotAxes::Heatmap(
            PlotSymbol::Kernel(KernelId(1)),
            PlotSymbol::Kernel(KernelId(2)),
        );
        assert!(is_axis(heatmap, PlotSymbol::Kernel(KernelId(2))));
        assert!(!is_axis(heatmap, PlotSymbol::SelfValue));
    }
}
