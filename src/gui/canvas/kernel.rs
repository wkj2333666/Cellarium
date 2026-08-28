//! The kernel stencil canvas.
//!
//! Cells are drawn and hit-tested through the same [`CanvasTransform`], so the
//! cell a click lands on is the cell under the cursor at any zoom. Every state
//! a cell can be in has its own colour and a permanent legend entry: a weight
//! of zero that still contributes is a different thing from a cell that has
//! been switched off, and the two must not look alike.

use eframe::egui::{self, Rect, Sense, Stroke, Ui};

use crate::gui::canvas::CanvasTransform;
use crate::gui::theme;

/// Which property the pointer edits.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum KernelTool {
    #[default]
    Weights,
    Support,
}

impl KernelTool {
    pub const ALL: [KernelTool; 2] = [KernelTool::Weights, KernelTool::Support];

    pub fn label(self) -> &'static str {
        match self {
            KernelTool::Weights => "Weights",
            KernelTool::Support => "Support",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            KernelTool::Weights => "Left paints the active value, right sets zero",
            KernelTool::Support => "Left switches a cell on, right switches it off",
        }
    }
}

/// What one cell of the stencil is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CellState {
    Positive,
    Negative,
    /// Contributing, but currently worth nothing.
    ActiveZero,
    /// Switched off: it contributes nothing whatever weight it holds.
    Inactive,
}

impl CellState {
    pub fn label(self) -> &'static str {
        match self {
            CellState::Positive => "positive",
            CellState::Negative => "negative",
            CellState::ActiveZero => "active zero",
            CellState::Inactive => "inactive",
        }
    }

    pub fn color(self) -> egui::Color32 {
        match self {
            CellState::Positive => theme::KERNEL_POSITIVE,
            CellState::Negative => theme::KERNEL_NEGATIVE,
            CellState::ActiveZero => theme::KERNEL_ACTIVE_ZERO,
            CellState::Inactive => theme::KERNEL_INACTIVE,
        }
    }

    /// The cell's colour with its weight shown as intensity.
    ///
    /// A view called "weights" has to distinguish them. `magnitude` is the
    /// cell's share of the largest weight in the stencil, so a flat kernel
    /// reads flat and a ring kernel shows its ring. The floor keeps the
    /// smallest non-zero weight visibly different from an active zero rather
    /// than fading into the background.
    pub fn shaded(self, magnitude: f32) -> egui::Color32 {
        match self {
            CellState::Positive | CellState::Negative => {
                let base = self.color();
                let t = MIN_SHADE + (1.0 - MIN_SHADE) * magnitude.clamp(0.0, 1.0).sqrt();
                egui::Color32::from_rgb(
                    lerp_channel(theme::KERNEL_WEIGHT_FLOOR.r(), base.r(), t),
                    lerp_channel(theme::KERNEL_WEIGHT_FLOOR.g(), base.g(), t),
                    lerp_channel(theme::KERNEL_WEIGHT_FLOOR.b(), base.b(), t),
                )
            }
            other => other.color(),
        }
    }
}

/// Faintest a non-zero weight is ever drawn, as a fraction of full colour.
const MIN_SHADE: f32 = 0.28;

fn lerp_channel(from: u8, to: u8, t: f32) -> u8 {
    (from as f32 + (to as f32 - from as f32) * t)
        .round()
        .clamp(0.0, 255.0) as u8
}

pub fn classify(weight: f32, active: bool) -> CellState {
    if !active {
        CellState::Inactive
    } else if weight > 0.0 {
        CellState::Positive
    } else if weight < 0.0 {
        CellState::Negative
    } else {
        CellState::ActiveZero
    }
}

/// The stencil the canvas draws, flattened out of whatever representation the
/// kernel uses so the canvas never has to know about rule models.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KernelStencil {
    pub width: usize,
    pub height: usize,
    pub anchor_x: usize,
    pub anchor_y: usize,
    pub weights: Vec<f32>,
    pub active: Vec<bool>,
}

impl KernelStencil {
    pub fn weight(&self, x: usize, y: usize) -> f32 {
        self.weights.get(y * self.width + x).copied().unwrap_or(0.0)
    }

    pub fn is_active(&self, x: usize, y: usize) -> bool {
        self.active.get(y * self.width + x).copied().unwrap_or(true)
    }

    pub fn state(&self, x: usize, y: usize) -> CellState {
        classify(self.weight(x, y), self.is_active(x, y))
    }

    /// Largest absolute weight among the contributing cells.
    pub fn peak_magnitude(&self) -> f32 {
        (0..self.height)
            .flat_map(|y| (0..self.width).map(move |x| (x, y)))
            .filter(|(x, y)| self.is_active(*x, *y))
            .map(|(x, y)| self.weight(x, y).abs())
            .fold(0.0f32, f32::max)
    }

    /// This cell's weight as a fraction of the stencil's largest.
    ///
    /// Relative rather than absolute because kernels are normalized: a
    /// sum-to-one kernel over a thousand cells holds weights near 0.001, and
    /// an absolute scale would render all of them black.
    pub fn relative_magnitude(&self, x: usize, y: usize) -> f32 {
        let peak = self.peak_magnitude();
        if peak <= 0.0 {
            return 0.0;
        }
        self.weight(x, y).abs() / peak
    }

    /// Sum of the weights that actually contribute. Shown beside the canvas
    /// because normalization depends on it and a zero sum is a real failure.
    pub fn active_sum(&self) -> f32 {
        (0..self.height)
            .flat_map(|y| (0..self.width).map(move |x| (x, y)))
            .filter(|(x, y)| self.is_active(*x, *y))
            .map(|(x, y)| self.weight(x, y))
            .sum()
    }
}

/// Transient canvas state the GUI owns between frames.
#[derive(Default)]
pub struct KernelCanvasState {
    pub transform: Option<CanvasTransform>,
    pub tool: KernelTool,
    /// The value the left button paints.
    pub paint_value: f32,
    pub selected_cell: Option<(usize, usize)>,
    /// Cell whose exact value is being typed, if any.
    pub editing: Option<(usize, usize)>,
}

impl KernelCanvasState {
    pub fn new() -> Self {
        Self {
            paint_value: 1.0,
            ..Self::default()
        }
    }

    pub fn request_fit(&mut self) {
        self.transform = None;
    }

    /// Nudge the painted value. The steps are coarse, fine and fast so the
    /// wheel can reach an exact number without a keyboard.
    pub fn adjust_paint_value(&mut self, steps: f32, modifiers: egui::Modifiers) {
        let increment = if modifiers.shift {
            0.005
        } else if modifiers.command {
            0.5
        } else {
            0.05
        };
        self.paint_value = (self.paint_value + steps * increment).clamp(-8.0, 8.0);
    }
}

/// One edit the pointer asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KernelEdit {
    Weight { x: usize, y: usize, value: f32 },
    Active { x: usize, y: usize, active: bool },
}

#[derive(Clone, Debug, Default)]
pub struct KernelCanvasResponse {
    pub edits: Vec<KernelEdit>,
    pub hovered: Option<(usize, usize)>,
    /// A cell was double-clicked and wants an exact value typed into it.
    pub exact_value_requested: Option<(usize, usize)>,
}

pub fn render_kernel_canvas(
    ui: &mut Ui,
    size: egui::Vec2,
    stencil: &KernelStencil,
    state: &mut KernelCanvasState,
) -> KernelCanvasResponse {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme::DOMAIN_EXTERIOR);
    let mut result = KernelCanvasResponse::default();

    if stencil.width == 0 || stencil.height == 0 {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "this kernel has no stencil",
            egui::FontId::proportional(14.0),
            theme::state_color(theme::State::Invalid),
        );
        return result;
    }

    let transform = match &mut state.transform {
        Some(transform) => {
            transform.viewport = rect;
            *transform
        }
        None => {
            let fitted =
                CanvasTransform::fit(rect, [stencil.width as f64, stencil.height as f64], 24.0);
            state.transform = Some(fitted);
            fitted
        }
    };

    // Draw every cell, including the ones worth nothing: a stencil with holes
    // in it is information, not an empty area.
    for y in 0..stencil.height {
        for x in 0..stencil.width {
            let cell = Rect::from_min_max(
                transform.world_to_screen([x as f64, y as f64]),
                transform.world_to_screen([x as f64 + 1.0, y as f64 + 1.0]),
            );
            let state_of = stencil.state(x, y);
            painter.rect_filled(
                cell.shrink(0.5),
                0.0,
                state_of.shaded(stencil.relative_magnitude(x, y)),
            );
            if state_of == CellState::Inactive {
                // An inactive cell keeps a faint outline so the stencil's shape
                // stays visible instead of dissolving into the background.
                painter.rect_stroke(
                    cell.shrink(0.5),
                    0.0,
                    Stroke::new(1.0, theme::CELL_STROKE),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }

    // The anchor is where the kernel sits over the cell being updated.
    let anchor = Rect::from_min_max(
        transform.world_to_screen([stencil.anchor_x as f64, stencil.anchor_y as f64]),
        transform.world_to_screen([stencil.anchor_x as f64 + 1.0, stencil.anchor_y as f64 + 1.0]),
    );
    painter.rect_stroke(
        anchor,
        0.0,
        Stroke::new(2.0, theme::KERNEL_ANCHOR),
        egui::StrokeKind::Inside,
    );

    if let Some((x, y)) = state.selected_cell {
        let cell = Rect::from_min_max(
            transform.world_to_screen([x as f64, y as f64]),
            transform.world_to_screen([x as f64 + 1.0, y as f64 + 1.0]),
        );
        painter.rect_stroke(
            cell,
            0.0,
            Stroke::new(2.0, theme::SELECTION),
            egui::StrokeKind::Outside,
        );
    }

    if let Some(pointer) = response.hover_pos() {
        let (scroll, modifiers) = ui.input(|input| (input.smooth_scroll_delta.y, input.modifiers));
        if scroll != 0.0 {
            if cell_at(&transform, pointer, stencil).is_some() {
                // Over a cell the wheel changes the value being painted, which
                // is the thing the user is about to apply.
                state.adjust_paint_value(scroll / 120.0, modifiers);
            } else if let Some(transform) = &mut state.transform {
                // Over empty space it zooms, so every cell stays reachable.
                transform.zoom_at(pointer, (scroll as f64 / 120.0).exp2());
            }
        }
        result.hovered = cell_at(&transform, pointer, stencil);
    }
    if response.dragged_by(egui::PointerButton::Middle)
        && let Some(transform) = &mut state.transform
    {
        transform.pan_screen(response.drag_delta());
    }

    if response.double_clicked()
        && let Some(pointer) = response.interact_pointer_pos()
        && let Some(cell) = cell_at(&transform, pointer, stencil)
    {
        state.selected_cell = Some(cell);
        result.exact_value_requested = Some(cell);
        return result;
    }

    for (button, primary) in [
        (egui::PointerButton::Primary, true),
        (egui::PointerButton::Secondary, false),
    ] {
        if !(response.dragged_by(button)
            || response.clicked_by(button)
            || response.drag_started_by(button))
        {
            continue;
        }
        let Some(pointer) = response.interact_pointer_pos() else {
            continue;
        };
        let Some((x, y)) = cell_at(&transform, pointer, stencil) else {
            continue;
        };
        state.selected_cell = Some((x, y));
        result.edits.push(match (state.tool, primary) {
            (KernelTool::Weights, true) => KernelEdit::Weight {
                x,
                y,
                value: state.paint_value,
            },
            (KernelTool::Weights, false) => KernelEdit::Weight { x, y, value: 0.0 },
            (KernelTool::Support, active) => KernelEdit::Active { x, y, active },
        });
    }

    result
}

fn cell_at(
    transform: &CanvasTransform,
    pointer: egui::Pos2,
    stencil: &KernelStencil,
) -> Option<(usize, usize)> {
    let world = transform.screen_to_world(pointer);
    if world[0] < 0.0 || world[1] < 0.0 {
        return None;
    }
    let x = world[0] as usize;
    let y = world[1] as usize;
    (x < stencil.width && y < stencil.height).then_some((x, y))
}

/// The legend is drawn every frame, not on demand: a colour code the user has
/// to remember is a colour code they will misread.
pub fn legend(ui: &mut Ui) {
    ui.horizontal_wrapped(|ui| {
        for state in [
            CellState::Positive,
            CellState::Negative,
            CellState::ActiveZero,
            CellState::Inactive,
        ] {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
            ui.painter().rect_filled(rect, 2.0, state.color());
            ui.painter().rect_stroke(
                rect,
                2.0,
                Stroke::new(1.0, theme::CELL_STROKE),
                egui::StrokeKind::Inside,
            );
            ui.label(egui::RichText::new(state.label()).small());
        }
        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
        ui.painter().rect_stroke(
            rect,
            0.0,
            Stroke::new(2.0, theme::KERNEL_ANCHOR),
            egui::StrokeKind::Inside,
        );
        ui.label(egui::RichText::new("anchor").small());
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    fn stencil() -> KernelStencil {
        KernelStencil {
            width: 3,
            height: 3,
            anchor_x: 1,
            anchor_y: 1,
            weights: vec![0.0, 1.0, -1.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0],
            active: vec![true, true, true, true, true, true, false, true, true],
        }
    }

    #[test]
    fn a_zero_that_contributes_is_not_the_same_state_as_a_switched_off_cell() {
        assert_eq!(classify(0.0, true), CellState::ActiveZero);
        assert_eq!(classify(0.0, false), CellState::Inactive);
        assert_eq!(classify(0.9, false), CellState::Inactive);
        assert_ne!(
            CellState::ActiveZero.color(),
            CellState::Inactive.color(),
            "two different states must not share one colour"
        );
    }

    /// Rough perceptual lightness, enough to tell "can a person separate these"
    /// from "are these two different u8 triples".
    fn luminance(color: egui::Color32) -> f32 {
        0.2126 * color.r() as f32 + 0.7152 * color.g() as f32 + 0.0722 * color.b() as f32
    }

    #[test]
    fn every_state_is_distinguishable_not_merely_unequal() {
        let states = [
            CellState::Positive,
            CellState::Negative,
            CellState::ActiveZero,
            CellState::Inactive,
        ];
        for (index, state) in states.iter().enumerate() {
            for other in &states[index + 1..] {
                assert_ne!(state.label(), other.label());
                assert_ne!(state.color(), other.color());
                // Two colours that differ by a few units are different values
                // and the same colour to a person looking at the screen.
                let separated = (luminance(state.color()) - luminance(other.color())).abs() >= 24.0
                    || channel_distance(state.color(), other.color()) >= 96.0;
                assert!(
                    separated,
                    "{} and {} are too close to tell apart",
                    state.label(),
                    other.label()
                );
            }
        }
    }

    fn channel_distance(left: egui::Color32, right: egui::Color32) -> f32 {
        let dr = left.r() as f32 - right.r() as f32;
        let dg = left.g() as f32 - right.g() as f32;
        let db = left.b() as f32 - right.b() as f32;
        (dr * dr + dg * dg + db * db).sqrt()
    }

    #[test]
    fn the_stencil_reports_the_state_of_each_cell() {
        let stencil = stencil();
        assert_eq!(stencil.state(1, 0), CellState::Positive);
        assert_eq!(stencil.state(2, 0), CellState::Negative);
        assert_eq!(stencil.state(0, 0), CellState::ActiveZero);
        assert_eq!(stencil.state(0, 2), CellState::Inactive);
    }

    #[test]
    fn the_active_sum_ignores_cells_that_are_switched_off() {
        let mut stencil = stencil();
        stencil.weights[6] = 5.0;
        assert!(
            (stencil.active_sum() - 0.5).abs() < 1e-6,
            "an inactive cell must not enter the sum, got {}",
            stencil.active_sum()
        );
    }

    #[test]
    fn the_wheel_steps_coarse_fine_and_fast() {
        let mut state = KernelCanvasState::new();
        state.paint_value = 0.0;

        state.adjust_paint_value(1.0, egui::Modifiers::NONE);
        assert!((state.paint_value - 0.05).abs() < 1e-6);

        state.paint_value = 0.0;
        state.adjust_paint_value(1.0, egui::Modifiers::SHIFT);
        assert!((state.paint_value - 0.005).abs() < 1e-6);

        state.paint_value = 0.0;
        state.adjust_paint_value(1.0, egui::Modifiers::COMMAND);
        assert!((state.paint_value - 0.5).abs() < 1e-6);
    }

    #[test]
    fn the_painted_value_stays_inside_a_range_a_kernel_can_hold() {
        let mut state = KernelCanvasState::new();
        for _ in 0..1_000 {
            state.adjust_paint_value(1.0, egui::Modifiers::COMMAND);
        }
        assert!(state.paint_value.is_finite());
        assert!(state.paint_value <= 8.0);
    }

    #[test]
    fn every_cell_is_reachable_through_the_shared_transform() {
        let stencil = stencil();
        let rect = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(300.0, 300.0));
        let transform =
            CanvasTransform::fit(rect, [stencil.width as f64, stencil.height as f64], 24.0);
        for y in 0..stencil.height {
            for x in 0..stencil.width {
                // Aim at the centre of the drawn cell and read it back.
                let centre = transform.world_to_screen([x as f64 + 0.5, y as f64 + 0.5]);
                assert_eq!(
                    cell_at(&transform, centre, &stencil),
                    Some((x, y)),
                    "cell ({x}, {y}) was not reachable"
                );
            }
        }
        let outside = transform.world_to_screen([-1.0, -1.0]);
        assert_eq!(cell_at(&transform, outside, &stencil), None);
    }

    #[test]
    fn each_tool_says_what_both_buttons_do() {
        for tool in KernelTool::ALL {
            assert!(!tool.label().is_empty());
            assert!(tool.hint().contains("Left") && tool.hint().contains("right"));
        }
        assert_ne!(KernelTool::Weights.hint(), KernelTool::Support.hint());
    }
}
