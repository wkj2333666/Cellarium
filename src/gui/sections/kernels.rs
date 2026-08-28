//! The Kernels workspace: cards per binding, the stencil canvas and metadata.

use eframe::egui::{self, RichText, Ui};

use crate::document::kernels;
use crate::gui::app::CellariumGui;
use crate::gui::canvas::kernel::{KernelEdit, KernelTool, legend, render_kernel_canvas};
use crate::gui::theme;
use crate::gui::widgets::decision_dialog::{DecisionOutcome, decision_dialog};
use crate::gui::widgets::numeric_popover::{NumericOutcome, numeric_popover};
use crate::gui::widgets::object_strip::{CardAction, ObjectCard, StripHit, object_strip};
use crate::sim::experiment_model::KernelId;

pub fn draw(app: &mut CellariumGui, ui: &mut Ui) {
    cards(app, ui);
    ui.separator();
    toolbar(app, ui);
    ui.separator();
    if let Some(decision) = app.kernel_decision().cloned() {
        match decision_dialog(ui, &decision) {
            Some(DecisionOutcome::Confirmed) => app.confirm_kernel_decision(),
            Some(DecisionOutcome::Cancelled) => app.cancel_kernel_decision(),
            None => {}
        }
        ui.separator();
    }
    canvas(app, ui);
}

fn cards(app: &mut CellariumGui, ui: &mut Ui) {
    let binding = app.selected_binding();
    let models = kernels::binding_kernels(app.spec(), binding, app.selected_kernel());
    let deletable = models.len() > 1;
    let cards: Vec<ObjectCard> = models
        .iter()
        .map(|model| {
            ObjectCard::new(u64::from(model.id.0), &model.symbol)
                // The subtitle carries what distinguishes one kernel from
                // another at a glance: where it sits, what it reads and how
                // much of its stencil actually contributes.
                .subtitle(format!(
                    "#{} · {}x{} · {} in support",
                    model.ordinal, model.width, model.height, model.support_cells
                ))
                .selected(model.selected)
                .dimmed(model.support_cells == 0)
                .action(
                    CardAction::new(
                        "Delete",
                        if deletable {
                            "Remove this kernel from the binding"
                        } else {
                            "a binding must keep at least one kernel"
                        },
                    )
                    .enabled(deletable),
                )
        })
        .collect();

    if let Some(hit) = object_strip(ui, "kernel_cards", &cards, Some("Add kernel")) {
        match hit {
            StripHit::Add => app.add_kernel(),
            StripHit::Select(key) => app.select_kernel(KernelId(key as u32)),
            StripHit::Action { key, verb } if verb == "Delete" => {
                app.begin_kernel_removal(KernelId(key as u32));
            }
            StripHit::Action { .. } => {}
        }
    }
}

fn toolbar(app: &mut CellariumGui, ui: &mut Ui) {
    ui.horizontal_wrapped(|ui| {
        let mut tool = app.kernel_canvas().tool;
        for candidate in KernelTool::ALL {
            if ui
                .add(egui::Button::selectable(
                    tool == candidate,
                    candidate.label(),
                ))
                .on_hover_text(candidate.hint())
                .clicked()
            {
                tool = candidate;
            }
        }
        app.kernel_canvas_mut().tool = tool;

        ui.separator();
        let mut value = app.kernel_canvas().paint_value;
        if ui
            .add(
                egui::DragValue::new(&mut value)
                    .speed(0.01)
                    .range(-8.0..=8.0)
                    .prefix("value "),
            )
            .on_hover_text("The value the left button paints; the wheel over a cell also sets it")
            .changed()
        {
            app.kernel_canvas_mut().paint_value = value;
        }

        ui.separator();
        if ui
            .button("Fit kernel")
            .on_hover_text("Fit the whole stencil in view")
            .clicked()
        {
            app.kernel_canvas_mut().request_fit();
        }
        if ui
            .button("Reset rule-set")
            .on_hover_text("Return this binding to its channel's default rule-set")
            .clicked()
        {
            app.reset_rule_set();
        }
    });

    metadata(app, ui);
}

/// Source and output are properties of the kernel, not of the canvas, and both
/// are chosen with the pointer.
fn metadata(app: &mut CellariumGui, ui: &mut Ui) {
    let Some(selected) = app.selected_kernel() else {
        return;
    };
    let binding = app.selected_binding();
    let models = kernels::binding_kernels(app.spec(), binding, Some(selected));
    let Some(model) = models.iter().find(|model| model.id == selected) else {
        return;
    };
    let channels: Vec<_> = app
        .spec()
        .channels
        .iter()
        .map(|channel| (channel.id, channel.name.clone()))
        .collect();
    let current = channels
        .iter()
        .find(|(id, _)| *id == model.source_channel)
        .map(|(_, name)| name.clone())
        .unwrap_or_else(|| "unknown".to_string());

    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("Kernel {}", model.symbol)).strong());
        ui.label("reads");
        egui::ComboBox::from_id_salt("kernel_source")
            .selected_text(current)
            .show_ui(ui, |ui| {
                for (id, name) in &channels {
                    if ui
                        .selectable_label(*id == model.source_channel, name)
                        .clicked()
                    {
                        app.set_kernel_source(selected, *id);
                    }
                }
            });
        ui.separator();
        ui.label(format!(
            "{}x{} stencil, anchor at ({}, {})",
            model.width,
            model.height,
            app.kernel_stencil().anchor_x,
            app.kernel_stencil().anchor_y
        ));
        ui.separator();
        let sum = app.kernel_stencil().active_sum();
        // The sum is shown because normalization depends on it, and a sum of
        // zero is a state the model refuses rather than a curiosity.
        let state = if sum.abs() <= 1e-12 {
            theme::State::Invalid
        } else {
            theme::State::Live
        };
        ui.label(RichText::new(format!("active sum {sum:.4}")).color(theme::state_color(state)));
    });
}

fn canvas(app: &mut CellariumGui, ui: &mut Ui) {
    legend(ui);
    let stencil = app.kernel_stencil();
    let readout_height =
        ui.text_style_height(&egui::TextStyle::Body) + ui.spacing().item_spacing.y * 2.0;
    let size = egui::vec2(
        ui.available_width(),
        (ui.available_height() - readout_height).max(64.0),
    );
    let response = {
        let state = app.kernel_canvas_mut();
        render_kernel_canvas(ui, size, &stencil, state)
    };

    if let Some((x, y)) = response.exact_value_requested {
        let current = stencil.weight(x, y);
        app.kernel_popover_mut().open((x, y), current);
    }
    for edit in response.edits {
        app.apply_kernel_edit(edit);
    }

    if app.kernel_popover().is_open() {
        let outcome = {
            let popover = app.kernel_popover_mut();
            numeric_popover(ui, popover)
        };
        match outcome {
            Some(NumericOutcome::Accepted { x, y, value }) => {
                app.apply_kernel_edit(KernelEdit::Weight { x, y, value });
                app.kernel_popover_mut().close();
            }
            Some(NumericOutcome::Cancelled) => app.kernel_popover_mut().close(),
            None => {}
        }
    }

    match response.hovered {
        Some((x, y)) => {
            ui.label(
                RichText::new(format!(
                    "({x}, {y}) {} = {:.4}",
                    stencil.state(x, y).label(),
                    stencil.weight(x, y)
                ))
                .weak(),
            );
        }
        None => {
            ui.label(RichText::new("hover a cell to inspect it").weak());
        }
    }
}
