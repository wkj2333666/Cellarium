//! The Tiling workspace: preset cards, polygon construction and the seam
//! assistant.
//!
//! The assistant is live. It has an opinion about the drawing at all times,
//! including — especially — while the drawing is wrong, because that is when
//! its opinion is worth having. The control that closes the seams moves the
//! geometry; the readout beneath the canvas says which way and how far.

use eframe::egui::{self, RichText, Ui};

use crate::document::DocumentCommand;
use crate::document::tiling::ConstructionTarget;
use crate::gui::app::CellariumGui;
use crate::gui::canvas::tiling::render_tiling_canvas;
use crate::gui::{style, theme};
use crate::sim::tiling::{SeamAssessment, SeamBucket, TilingPreset, Vec2, validate_coverage};

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
    if !app.tiling_canvas().drawing() {
        assistant_bar(app, ui);
    }
    ui.separator();
    canvas(app, ui);
}

fn toolbar(app: &mut CellariumGui, ui: &mut Ui) {
    ui.horizontal_wrapped(|ui| {
        if app.tiling_canvas().drawing() {
            construction_controls(app, ui);
        } else {
            style::group_caption(ui, "DRAW");
            if ui
                .add(style::secondary("Draw from scratch"))
                .on_hover_text("Place vertices with the pointer to add a new basis polygon")
                .clicked()
            {
                app.tiling_canvas_mut().begin_new_basis();
            }
            let selected = app.tiling_canvas().selected_prototype;
            if ui
                .add_enabled(selected.is_some(), style::secondary("Redraw selected"))
                .on_hover_text("Replace the selected polygon with a new outline")
                .on_disabled_hover_text("Click a polygon on the canvas to select it first")
                .clicked()
                && let Some(prototype) = selected
            {
                app.tiling_canvas_mut().begin_reshape(prototype);
            }
            ui.separator();
            style::group_caption(ui, "START FROM");
            presets(app, ui);
        }
        ui.separator();
        if ui
            .add(style::secondary("Fit tiling"))
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
            .add(style::secondary(preset_label(preset)))
            .on_hover_text(preset_hint(preset))
            .clicked()
        {
            app.dispatch_document(DocumentCommand::ApplyTilingPreset { preset, scale: 1.0 });
            // A new unit cell is a new thing to frame.
            app.tiling_canvas_mut().request_fit();
            app.release_seams();
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
    ui.label(style::dim_readout(theme::plural(
        placed, "vertex", "vertices",
    )));
    let can_undo = app.tiling_canvas().can_undo_point();
    if ui
        .add_enabled(can_undo, style::secondary("Undo point"))
        .on_hover_text("Remove the last placed vertex")
        .on_disabled_hover_text("No vertex has been placed yet")
        .clicked()
    {
        app.tiling_canvas_mut().undo_point();
    }
    let can_redo = app.tiling_canvas().can_redo_point();
    if ui
        .add_enabled(can_redo, style::secondary("Redo point"))
        .on_hover_text("Put back the vertex that was undone")
        .on_disabled_hover_text("No vertex has been undone")
        .clicked()
    {
        app.tiling_canvas_mut().redo_point();
    }
    if ui
        .add_enabled(placed >= 3, style::primary("Finish polygon"))
        .on_hover_text("Close the outline and add it to the unit cell")
        .on_disabled_hover_text("Place at least three vertices to close an outline")
        .clicked()
    {
        app.finish_tiling_polygon();
    }
    if ui
        .add(style::secondary("Cancel drawing"))
        .on_hover_text("Discard the outline and go back to selecting")
        .clicked()
    {
        app.tiling_canvas_mut().cancel();
    }
}

/// The assistant's standing opinion of the drawing.
///
/// This row is never blank while a tiling exists. The control it replaces only
/// spoke about drawings that were already correct: one thousandth of a unit of
/// pointer inaccuracy and it proposed nothing at all, beside an Accept button
/// that did nothing.
fn assistant_bar(app: &mut CellariumGui, ui: &mut Ui) {
    let Some(assessment) = app.seam_assessment() else {
        return;
    };
    let coverage = app
        .spec()
        .tiling
        .as_ref()
        .map(|draft| validate_coverage(draft).map_err(|issues| summarize(&issues)));

    ui.horizontal_wrapped(|ui| {
        let (state, line) = verdict(&assessment, coverage.as_ref().is_some_and(Result::is_ok));
        ui.label(RichText::new(line).color(theme::state_color(state)))
            .on_hover_text(hover_detail(&assessment, coverage.as_ref()));

        ui.separator();
        // Enabled whenever there is any pairing at all — including a drawing
        // that is already exact, where closing moves nothing and holds the
        // seams, which is the only way to ask for linked dragging.
        //
        // It counts *every* candidate, not only the confident ones. Counting
        // the confident ones alone was how a drawing with one already-closed
        // pair and two distant ones offered a live button that closed the pair
        // needing no work and left the two the user was pointing at exactly
        // where they were.
        let pairs = assessment.candidates.len();
        let closeable = pairs > 0;
        if ui
            .add_enabled(closeable, style::primary("Close seams"))
            .on_hover_text(format!(
                "Move the drawing the smallest amount that makes {} meet, and hold them \
                 together from now on",
                theme::plural(pairs, "this edge pair", "these edge pairs")
            ))
            .on_disabled_hover_text(
                "no two edges have been paired yet — the readout below says what is in the way",
            )
            .clicked()
        {
            app.close_seams();
        }

        let held = app.tiling_canvas().seams.len();
        if held > 0
            && ui
                .add(style::secondary(&format!("Release {held}")))
                .on_hover_text("Stop holding these seams, so vertices move one at a time")
                .clicked()
        {
            app.release_seams();
        }
    });
    hint_line(app, ui, &assessment);
}

/// The one thing the assistant has to say, seams and coverage together.
///
/// These were two chips side by side, and a drawing whose edges meet exactly
/// while its tiles overlap their own copies showed a green "every seam closes"
/// next to a red "does not tile". Both were true — closing a seam is about
/// endpoints meeting, tiling is about interiors staying out of each other's
/// way — but a user reading two contradictory-looking verdicts has to work out
/// which one to believe. One sentence carries both facts and no argument.
pub fn verdict(assessment: &SeamAssessment, tiles: bool) -> (theme::State, String) {
    let (state, sentence) = if assessment.edge_count == 0 {
        (theme::State::Draft, assessment.summary())
    } else {
        match (assessment.is_closed(), tiles) {
            (true, true) => (
                theme::State::Live,
                format!(
                    "every seam closes and the plane is covered: {} pairs holding",
                    assessment.candidates.len()
                ),
            ),
            (true, false) => (
                theme::State::Invalid,
                "every seam meets, but the tiles still overlap their own copies".to_string(),
            ),
            (false, true) => (
                theme::State::Draft,
                format!("{} — the plane is covered", assessment.summary()),
            ),
            (false, false) => (assessment_state(assessment), assessment.summary()),
        }
    };
    // The glyph carries the state as well as the colour does. `theme` promises
    // colour is never the only indicator, and a verdict is exactly the place
    // that promise has to hold.
    (state, format!("{} {sentence}", theme::state_glyph(state)))
}

fn assessment_state(assessment: &SeamAssessment) -> theme::State {
    if assessment.is_closed() {
        theme::State::Live
    } else if !assessment.orphans.is_empty() {
        theme::State::Invalid
    } else {
        theme::State::Draft
    }
}

fn hover_detail(
    assessment: &SeamAssessment,
    coverage: Option<&Result<crate::sim::tiling::CoverageReport, String>>,
) -> String {
    let mut lines = vec![format!(
        "{} boundary edges, every one of them accounted for",
        assessment.edge_count
    )];
    match coverage {
        Some(Ok(report)) => lines.push(format!(
            "covers {:.4} of a {:.4} unit cell with no gaps or overlaps",
            report.covered_area, report.patch_area
        )),
        Some(Err(detail)) => lines.push(detail.clone()),
        None => {}
    }
    for candidate in assessment.candidates.iter().take(6) {
        lines.push(format!(
            "{}: edge {} of basis {} to edge {} of basis {}, gap {:.4}",
            candidate.bucket.label(),
            candidate.constraint.lhs.edge,
            candidate.constraint.lhs.tile.0,
            candidate.constraint.rhs.edge,
            candidate.constraint.rhs.tile.0,
            candidate.score.endpoint_gap,
        ));
    }
    for orphan in assessment.orphans.iter().take(4) {
        lines.push(orphan.describe());
    }
    lines.join("\n")
}

/// The single most useful sentence about what to do next.
///
/// An unpaired edge is the more serious problem, so it is named first; failing
/// that, the seam that is furthest from closing gets its direction spelled out.
fn hint_line(app: &CellariumGui, ui: &mut Ui, assessment: &SeamAssessment) {
    let _ = app;
    if assessment.is_closed() {
        return;
    }
    if let Some(orphan) = assessment.orphans.first() {
        ui.label(RichText::new(orphan.describe()).color(theme::state_color(theme::State::Invalid)));
        return;
    }
    let worst = assessment
        .candidates
        .iter()
        .filter(|candidate| candidate.bucket != SeamBucket::Held)
        .max_by(|left, right| left.score.endpoint_gap.total_cmp(&right.score.endpoint_gap));
    if let Some(candidate) = worst {
        ui.label(
            RichText::new(format!(
                "furthest seam: edge {} of basis {} needs to move {}",
                candidate.constraint.lhs.edge,
                candidate.constraint.lhs.tile.0,
                describe_move(candidate.hint()),
            ))
            .color(theme::state_color(theme::State::Draft)),
        );
    }
}

/// A vector, in words a person can act on.
///
/// World `y` grows upwards on this canvas, so a positive `y` is described as
/// up. A hint that names the wrong direction is worse than no hint.
pub fn describe_move(delta: Vec2) -> String {
    let horizontal = if delta.x >= 0.0 { "right" } else { "left" };
    let vertical = if delta.y >= 0.0 { "up" } else { "down" };
    format!(
        "{:.3} {horizontal} and {:.3} {vertical}",
        delta.x.abs(),
        delta.y.abs()
    )
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

fn readout(app: &CellariumGui, ui: &mut Ui, hovered: Option<Vec2>) {
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
    let broken = app.tiling_canvas().broken.len();
    if broken > 0 {
        // A drag is allowed to pull a held seam apart; what is not allowed is
        // letting that happen silently.
        ui.label(
            RichText::new(format!(
                "{} held {} no longer {} — press Close seams to draw them back together, or \
                 Release to stop holding",
                broken,
                if broken == 1 { "seam" } else { "seams" },
                if broken == 1 { "closes" } else { "close" },
            ))
            .color(theme::state_color(theme::State::Stale)),
        );
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
            ui.label(style::dim_readout(format!(
                "({:.3}, {:.3})  {where_}",
                point.x, point.y
            )));
        }
        None => {
            ui.label(style::dim_readout("hover the tiling to inspect it"));
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

    /// A hint that names the wrong direction is worse than no hint, so the
    /// mapping from sign to word is pinned rather than assumed.
    #[test]
    fn a_move_is_described_in_the_direction_it_actually_goes() {
        assert_eq!(
            describe_move(Vec2::new(0.25, 0.5)),
            "0.250 right and 0.500 up"
        );
        assert_eq!(
            describe_move(Vec2::new(-0.25, -0.5)),
            "0.250 left and 0.500 down"
        );
    }

    #[test]
    fn a_move_is_reported_as_a_magnitude_never_as_a_negative_distance() {
        for delta in [
            Vec2::new(-1.5, -2.5),
            Vec2::new(-0.001, 0.001),
            Vec2::new(3.0, -4.0),
        ] {
            let sentence = describe_move(delta);
            assert!(
                !sentence.contains('-'),
                "a distance must not be written as negative: {sentence}"
            );
        }
    }

    use crate::sim::tiling::{PrototypeShape, assess_seams, build_preset, polygon};

    fn drawn(preset: TilingPreset) -> crate::sim::tiling::PeriodicTilingDraft {
        let mut draft = build_preset(preset, 1.0);
        for prototype in &mut draft.prototypes {
            let vertices = polygon::prototype_vertices(&prototype.shape).unwrap();
            prototype.shape = PrototypeShape::SimplePolygon { vertices };
        }
        draft
    }

    /// Seams meeting and the plane being covered are different claims, and the
    /// interface used to make them separately and in different colours.
    #[test]
    fn a_drawing_whose_seams_meet_but_whose_tiles_overlap_says_exactly_that() {
        let assessment = assess_seams(&drawn(TilingPreset::Square)).unwrap();
        assert!(assessment.is_closed());

        let (state, line) = verdict(&assessment, false);
        assert_eq!(state, theme::State::Invalid);
        assert!(
            line.contains("overlap"),
            "the verdict has to name the real problem: {line}"
        );
        assert!(
            !line.contains("every seam closes and"),
            "it must not also read as finished: {line}"
        );

        let (state, line) = verdict(&assessment, true);
        assert_eq!(state, theme::State::Live);
        assert!(line.contains("every seam closes"), "{line}");
    }

    #[test]
    fn an_empty_drawing_is_not_reported_as_a_failure() {
        let mut draft = drawn(TilingPreset::Square);
        draft.instances.clear();
        draft.prototypes.clear();
        let assessment = assess_seams(&draft).unwrap();
        let (state, _) = verdict(&assessment, false);
        assert_ne!(
            state,
            theme::State::Invalid,
            "nothing drawn yet is not the same as something drawn wrong"
        );
    }

    /// Colour is never the only state indicator, and the verdict is the line
    /// most worth reading without it.
    #[test]
    fn every_verdict_carries_its_state_as_a_glyph_too() {
        let assessment = assess_seams(&drawn(TilingPreset::Square)).unwrap();
        for tiles in [true, false] {
            let (state, line) = verdict(&assessment, tiles);
            assert!(
                line.starts_with(theme::state_glyph(state)),
                "the verdict must be readable without colour: {line}"
            );
        }
    }
}
