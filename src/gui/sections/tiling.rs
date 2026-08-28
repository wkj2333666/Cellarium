//! The Tiling workspace: preset cards, polygon construction and seam solving.

use eframe::egui::{self, RichText, Ui};

use crate::document::DocumentCommand;
use crate::document::tiling::ConstructionTarget;
use crate::gui::app::CellariumGui;
use crate::gui::canvas::tiling::render_tiling_canvas;
use crate::gui::theme;
use crate::sim::tiling::{TilingPreset, propose_full_edge_seams, validate_coverage};

/// Tolerance a seam proposal must meet before it is offered. Edges further
/// apart than this are not the same seam, they are two edges near each other.
const SEAM_TOLERANCE: f64 = 1e-3;

pub fn preset_label(preset: TilingPreset) -> &'static str {
    match preset {
        TilingPreset::Square => "Square",
        TilingPreset::EquilateralTriangles => "Triangles",
        TilingPreset::RegularHexagon => "Hexagon",
        TilingPreset::OctagonSquare => "Octagon + square",
    }
}

pub fn preset_hint(preset: TilingPreset) -> &'static str {
    match preset {
        TilingPreset::Square => "One square per cell on an orthogonal lattice",
        TilingPreset::EquilateralTriangles => "Two triangles per cell on a 60-degree lattice",
        TilingPreset::RegularHexagon => "One hexagon per cell on a 60-degree lattice",
        TilingPreset::OctagonSquare => "An octagon and a square sharing one cell",
    }
}

pub fn draw(app: &mut CellariumGui, ui: &mut Ui) {
    toolbar(app, ui);
    ui.separator();
    canvas(app, ui);
}

fn toolbar(app: &mut CellariumGui, ui: &mut Ui) {
    ui.horizontal_wrapped(|ui| {
        if app.tiling_canvas().drawing() {
            construction_controls(app, ui);
        } else {
            if ui
                .button("Draw from scratch")
                .on_hover_text("Place vertices with the pointer to add a new basis polygon")
                .clicked()
            {
                app.tiling_canvas_mut().begin_new_basis();
            }
            let selected = app.tiling_canvas().selected_prototype;
            if ui
                .add_enabled(selected.is_some(), egui::Button::new("Redraw selected"))
                .on_hover_text("Replace the selected polygon with a new outline")
                .clicked()
                && let Some(prototype) = selected
            {
                app.tiling_canvas_mut().begin_reshape(prototype);
            }
            ui.separator();
            presets(app, ui);
            ui.separator();
            seams(app, ui);
        }
        ui.separator();
        if ui
            .button("Fit tiling")
            .on_hover_text("Fit the unit cell and its neighbours in view")
            .clicked()
        {
            app.tiling_canvas_mut().request_fit();
        }
    });
}

/// Preset cards. Each is a button carrying its own description, so choosing a
/// starting point never requires knowing the vocabulary first.
fn presets(app: &mut CellariumGui, ui: &mut Ui) {
    for preset in TilingPreset::ALL {
        if ui
            .button(preset_label(preset))
            .on_hover_text(preset_hint(preset))
            .clicked()
        {
            app.dispatch_document(DocumentCommand::ApplyTilingPreset { preset, scale: 1.0 });
            // A new unit cell is a new thing to frame.
            app.tiling_canvas_mut().request_fit();
            app.tiling_canvas_mut().seams.clear();
        }
    }
}

fn construction_controls(app: &mut CellariumGui, ui: &mut Ui) {
    let placed = app.tiling_canvas().construction().len();
    ui.label(
        RichText::new(match app.tiling_canvas().target {
            ConstructionTarget::NewBasis => "Drawing a new basis",
            ConstructionTarget::ReplacePrototype(_) => "Redrawing the selected basis",
        })
        .color(theme::state_color(theme::State::Draft)),
    );
    ui.label(theme::plural(placed, "vertex", "vertices"));
    let can_undo = app.tiling_canvas().can_undo_point();
    if ui
        .add_enabled(can_undo, egui::Button::new("Undo point"))
        .on_hover_text("Remove the last placed vertex")
        .clicked()
    {
        app.tiling_canvas_mut().undo_point();
    }
    let can_redo = app.tiling_canvas().can_redo_point();
    if ui
        .add_enabled(can_redo, egui::Button::new("Redo point"))
        .on_hover_text("Put back the vertex that was undone")
        .clicked()
    {
        app.tiling_canvas_mut().redo_point();
    }
    if ui
        .add_enabled(placed >= 3, egui::Button::new("Finish polygon"))
        .on_hover_text("Close the outline and add it to the unit cell")
        .clicked()
    {
        app.finish_tiling_polygon();
    }
    if ui
        .button("Cancel drawing")
        .on_hover_text("Discard the outline and go back to selecting")
        .clicked()
    {
        app.tiling_canvas_mut().cancel();
    }
}

/// Seam solving. A proposal is shown with its residual and is never applied
/// until the user accepts it.
fn seams(app: &mut CellariumGui, ui: &mut Ui) {
    let has_tiling = app.spec().tiling.is_some();
    if ui
        .add_enabled(has_tiling, egui::Button::new("Solve seams"))
        .on_hover_text("Propose the full-edge pairs that glue the tiling together")
        .clicked()
        && let Some(draft) = app.spec().tiling.clone()
    {
        match propose_full_edge_seams(&draft, SEAM_TOLERANCE) {
            Ok(proposals) => app.set_seam_proposals(proposals),
            Err(reason) => app.set_notice(Some(reason)),
        }
    }
    let accepted = app.tiling_canvas().seams.len();
    if accepted > 0 {
        ui.label(
            RichText::new(format!("{accepted} seams held"))
                .color(theme::state_color(theme::State::Live)),
        );
    }
    coverage(app, ui);
}

/// Say plainly whether the drawn cell actually tiles the plane. Copies are
/// drawn either way, so without this line a draft that leaves gaps looks
/// finished.
///
/// The verdict stays short: a coverage diagnostic runs long enough to wrap the
/// toolbar onto a second row and collide with the controls beside it, so the
/// detail is on hover.
fn coverage(app: &CellariumGui, ui: &mut Ui) {
    let Some(draft) = app.spec().tiling.as_ref() else {
        return;
    };
    let (state, verdict, detail) = match validate_coverage(draft) {
        Ok(report) => (
            theme::State::Live,
            "tiles the plane",
            format!(
                "covers {:.4} of a {:.4} unit cell with no gaps or overlaps",
                report.covered_area, report.patch_area
            ),
        ),
        Err(diagnostics) => (
            theme::State::Invalid,
            "does not tile",
            // One problem is a thing to fix; forty are a wall of text over the
            // drawing the user needs to look at to fix them. The rest are
            // counted, not listed, and they are usually the same problem seen
            // from every repeat of the lattice anyway.
            summarize(&diagnostics),
        ),
    };
    ui.label(
        RichText::new(format!("{} {verdict}", theme::state_glyph(state)))
            .color(theme::state_color(state)),
    )
    .on_hover_text(detail);
}

/// The first few problems, and a count of the rest.
fn summarize(diagnostics: &[crate::sim::tiling::TilingDiagnostic]) -> String {
    const SHOWN: usize = 3;
    let mut lines: Vec<String> = diagnostics
        .iter()
        .take(SHOWN)
        .map(|entry| entry.message.clone())
        .collect();
    if diagnostics.len() > SHOWN {
        lines.push(format!("and {} more like these", diagnostics.len() - SHOWN));
    }
    lines.join("\n")
}

fn canvas(app: &mut CellariumGui, ui: &mut Ui) {
    if let Some(proposals) = app.seam_proposals() {
        proposal_bar(app, ui, proposals.len(), worst_residual(app));
        ui.separator();
    }

    let draft = app.spec().tiling.clone();
    // Leave a row for the readout, otherwise the canvas claims the whole panel
    // and the readout is clipped off the bottom edge.
    let readout_height =
        ui.text_style_height(&egui::TextStyle::Body) + ui.spacing().item_spacing.y * 2.0;
    let size = egui::vec2(
        ui.available_width(),
        (ui.available_height() - readout_height).max(64.0),
    );
    let response = {
        let state = app.tiling_canvas_mut();
        render_tiling_canvas(ui, size, draft.as_ref(), state)
    };

    if let Some(commit) = response.commit {
        app.dispatch_document(DocumentCommand::SetTilingDraft(Box::new(commit)));
    }

    readout(app, ui, response.hovered);
}

fn worst_residual(app: &CellariumGui) -> f64 {
    app.seam_proposals()
        .map(|proposals| {
            proposals
                .iter()
                .map(|proposal| proposal.residual)
                .fold(0.0_f64, f64::max)
        })
        .unwrap_or_default()
}

/// Accept or cancel a solve. Showing the residual next to the count is what
/// makes "accept" an informed choice rather than a leap.
fn proposal_bar(app: &mut CellariumGui, ui: &mut Ui, count: usize, residual: f64) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(format!(
                "{count} full-edge pairs proposed, worst residual {residual:.2e}"
            ))
            .color(theme::state_color(theme::State::Draft)),
        );
        if ui
            .button("Accept seams")
            .on_hover_text("Hold these edges together when vertices are dragged")
            .clicked()
        {
            app.accept_seam_proposals();
        }
        if ui
            .button("Cancel seams")
            .on_hover_text("Discard the proposal and change nothing")
            .clicked()
        {
            app.clear_seam_proposals();
        }
    });
}

fn readout(app: &CellariumGui, ui: &mut Ui, hovered: Option<crate::sim::tiling::Vec2>) {
    // General messages belong in the status bar, where every workspace can see
    // them. This line is for what the tiling canvas itself is saying.
    if let Some(reason) = &app.tiling_canvas().rejection {
        ui.add(
            egui::Label::new(
                RichText::new(reason).color(theme::state_color(theme::State::Invalid)),
            )
            .truncate(),
        )
        .on_hover_text(reason);
        return;
    }
    match hovered {
        Some(point) => {
            let basis = app
                .spec()
                .tiling
                .as_ref()
                .and_then(|draft| crate::gui::canvas::tiling::hit_basis(draft, point));
            let where_ = match basis {
                Some(basis) => format!("basis {}", basis.0),
                None => "outside every tile".to_string(),
            };
            ui.label(RichText::new(format!("({:.3}, {:.3}) {where_}", point.x, point.y)).weak());
        }
        None => {
            ui.label(RichText::new("hover the tiling to inspect it").weak());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_has_a_distinct_label_and_hint() {
        for (index, preset) in TilingPreset::ALL.iter().enumerate() {
            assert!(!preset_label(*preset).is_empty());
            assert!(!preset_hint(*preset).is_empty());
            for other in &TilingPreset::ALL[index + 1..] {
                assert_ne!(preset_label(*preset), preset_label(*other));
                assert_ne!(preset_hint(*preset), preset_hint(*other));
            }
        }
    }
}
