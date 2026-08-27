//! The growth response plot.
//!
//! The plot answers "what does this program do", so every number on it is one
//! the program actually produced. Inputs the program does not read are pinned
//! and shown as pinned; a sample that is not finite is reported rather than
//! drawn as a gap the eye fills in.

use std::collections::BTreeMap;

use eframe::egui::{self, Color32, Rect, Sense, Stroke, Ui};

use crate::document::selection::{PlotAxes, PlotSymbol};
use crate::gui::theme;
use crate::sim::experiment_model::KernelId;
use crate::sim::growth::eval::{ScalarInputs, evaluate};
use crate::sim::growth::types::TypedProgram;

/// Samples taken along each axis. Enough to show the shape of a response
/// without making a heatmap cost more than the frame it is drawn in.
const CURVE_SAMPLES: usize = 192;
const HEATMAP_SAMPLES: usize = 96;

/// Which quantity the program's result is.
///
/// This is not a setting of its own: it is the binding's update mode, shown
/// where the result is drawn. A plot that could disagree with the model about
/// what its own numbers mean would be worse than no label.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlotQuantity {
    /// The result is added to the current value.
    #[default]
    Rate,
    /// The result replaces the current value.
    Value,
}

impl PlotQuantity {
    pub const ALL: [PlotQuantity; 2] = [PlotQuantity::Rate, PlotQuantity::Value];

    pub fn of(mode: crate::sim::experiment_model::UpdateMode) -> Self {
        match mode {
            crate::sim::experiment_model::UpdateMode::GrowthRate => PlotQuantity::Rate,
            crate::sim::experiment_model::UpdateMode::DirectUpdate => PlotQuantity::Value,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PlotQuantity::Rate => "Rate",
            PlotQuantity::Value => "Value",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            PlotQuantity::Rate => "The result is added to the channel's current value",
            PlotQuantity::Value => "The result replaces the channel's current value",
        }
    }
}

/// Choose the axes for a program from the symbols it actually reads.
///
/// Defaulting to every declared input would plot axes the program ignores,
/// producing a flat surface that says nothing. A program reading no kernel at
/// all still has one meaningful axis: its own current value.
pub fn default_axes(referenced: &[String], signature_kernels: &[(String, KernelId)]) -> PlotAxes {
    let referenced_ids: Vec<KernelId> = signature_kernels
        .iter()
        .filter(|(symbol, _)| referenced.iter().any(|name| name == symbol))
        .map(|(_, id)| *id)
        .collect();
    match referenced_ids.as_slice() {
        [] => PlotAxes::Curve(PlotSymbol::SelfValue),
        [only] => PlotAxes::Curve(PlotSymbol::Kernel(*only)),
        [first, second, ..] => {
            PlotAxes::Heatmap(PlotSymbol::Kernel(*first), PlotSymbol::Kernel(*second))
        }
    }
}

/// The values held still while the axes vary.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PinnedInputs {
    pub self_value: f32,
    /// Value each kernel input is held at when it is not an axis.
    pub kernels: BTreeMap<u32, f32>,
    pub parameters: BTreeMap<String, f32>,
}

impl PinnedInputs {
    pub fn kernel(&self, id: KernelId) -> f32 {
        self.kernels.get(&id.0).copied().unwrap_or(0.0)
    }

    pub fn set_kernel(&mut self, id: KernelId, value: f32) {
        self.kernels.insert(id.0, value);
    }
}

/// Everything needed to compute a plot.
pub struct PlotInput<'a> {
    pub program: &'a TypedProgram,
    /// Kernel symbols in signature order, with their ids.
    pub signature_kernels: &'a [(String, KernelId)],
    pub axes: PlotAxes,
    pub pinned: &'a PinnedInputs,
    pub domain: [f32; 2],
}

/// A computed plot, or the reason there is nothing to draw.
#[derive(Clone, Debug, PartialEq)]
pub enum PlotScene {
    Curve {
        samples: Vec<(f32, f32)>,
        range: [f32; 2],
        /// Points where the program produced no finite number.
        non_finite: usize,
    },
    Heatmap {
        values: Vec<f32>,
        size: usize,
        range: [f32; 2],
        non_finite: usize,
    },
    /// The program never produced a finite number anywhere in the domain.
    NoFiniteSamples,
}

/// Evaluate the program across the chosen axes.
pub fn compute(input: &PlotInput<'_>) -> PlotScene {
    match input.axes {
        PlotAxes::Curve(symbol) => curve(input, symbol),
        PlotAxes::Heatmap(x, y) => heatmap(input, x, y),
    }
}

fn inputs_for(input: &PlotInput<'_>, overrides: &[(PlotSymbol, f32)]) -> ScalarInputs {
    let mut self_value = input.pinned.self_value;
    let mut kernel_inputs: Vec<f32> = input
        .signature_kernels
        .iter()
        .map(|(_, id)| input.pinned.kernel(*id))
        .collect();
    for (symbol, value) in overrides {
        match symbol {
            PlotSymbol::SelfValue => self_value = *value,
            PlotSymbol::Kernel(id) => {
                if let Some(index) = input
                    .signature_kernels
                    .iter()
                    .position(|(_, candidate)| candidate == id)
                {
                    kernel_inputs[index] = *value;
                }
            }
        }
    }
    ScalarInputs {
        kernel_inputs,
        self_value,
        parameters: input
            .pinned
            .parameters
            .iter()
            .map(|(name, value)| (name.clone(), *value))
            .collect(),
    }
}

fn curve(input: &PlotInput<'_>, symbol: PlotSymbol) -> PlotScene {
    let [low, high] = input.domain;
    let mut samples = Vec::with_capacity(CURVE_SAMPLES);
    let mut non_finite = 0;
    let mut range = [f32::INFINITY, f32::NEG_INFINITY];
    for index in 0..CURVE_SAMPLES {
        let t = index as f32 / (CURVE_SAMPLES - 1) as f32;
        let x = low + (high - low) * t;
        let scalars = inputs_for(input, &[(symbol, x)]);
        match evaluate(input.program, &scalars) {
            Ok(value) if value.is_finite() => {
                range[0] = range[0].min(value);
                range[1] = range[1].max(value);
                samples.push((x, value));
            }
            _ => non_finite += 1,
        }
    }
    if samples.is_empty() {
        return PlotScene::NoFiniteSamples;
    }
    PlotScene::Curve {
        samples,
        range: pad(range),
        non_finite,
    }
}

fn heatmap(input: &PlotInput<'_>, x_symbol: PlotSymbol, y_symbol: PlotSymbol) -> PlotScene {
    let [low, high] = input.domain;
    let mut values = vec![f32::NAN; HEATMAP_SAMPLES * HEATMAP_SAMPLES];
    let mut non_finite = 0;
    let mut range = [f32::INFINITY, f32::NEG_INFINITY];
    for row in 0..HEATMAP_SAMPLES {
        let ty = row as f32 / (HEATMAP_SAMPLES - 1) as f32;
        let y = low + (high - low) * ty;
        for column in 0..HEATMAP_SAMPLES {
            let tx = column as f32 / (HEATMAP_SAMPLES - 1) as f32;
            let x = low + (high - low) * tx;
            let scalars = inputs_for(input, &[(x_symbol, x), (y_symbol, y)]);
            match evaluate(input.program, &scalars) {
                Ok(value) if value.is_finite() => {
                    range[0] = range[0].min(value);
                    range[1] = range[1].max(value);
                    values[row * HEATMAP_SAMPLES + column] = value;
                }
                _ => non_finite += 1,
            }
        }
    }
    if range[0] > range[1] {
        return PlotScene::NoFiniteSamples;
    }
    PlotScene::Heatmap {
        values,
        size: HEATMAP_SAMPLES,
        range: pad(range),
        non_finite,
    }
}

/// A response that is exactly constant would otherwise have zero height and
/// draw as a line on the frame edge.
fn pad(range: [f32; 2]) -> [f32; 2] {
    if (range[1] - range[0]).abs() < 1e-9 {
        [range[0] - 0.5, range[1] + 0.5]
    } else {
        range
    }
}

/// Transient plot state the GUI owns between frames.
pub struct GrowthPlotState {
    pub domain: [f32; 2],
    pub pinned: PinnedInputs,
    /// Axes the user chose, overriding the referenced-symbol default.
    pub chosen_axes: Option<PlotAxes>,
}

impl Default for GrowthPlotState {
    fn default() -> Self {
        Self {
            domain: [0.0, 1.0],
            pinned: PinnedInputs::default(),
            chosen_axes: None,
        }
    }
}

/// Draw the scene with its axes and numbers.
pub fn render_growth_plot(
    ui: &mut Ui,
    size: egui::Vec2,
    scene: &PlotScene,
    quantity: PlotQuantity,
    domain: [f32; 2],
    stale: bool,
) {
    let (rect, _response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme::BOARD_INTERIOR);

    let plot = rect.shrink2(egui::vec2(48.0, 24.0));
    match scene {
        PlotScene::NoFiniteSamples => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "this program produced no finite value anywhere in the range",
                egui::FontId::proportional(14.0),
                theme::state_color(theme::State::Invalid),
            );
            return;
        }
        PlotScene::Curve {
            samples,
            range,
            non_finite,
        } => {
            axes(&painter, plot, domain, *range, quantity);
            let points: Vec<egui::Pos2> = samples
                .iter()
                .map(|(x, y)| {
                    let tx = (x - domain[0]) / (domain[1] - domain[0]).max(1e-9);
                    let ty = (y - range[0]) / (range[1] - range[0]).max(1e-9);
                    egui::pos2(
                        plot.left() + tx * plot.width(),
                        plot.bottom() - ty * plot.height(),
                    )
                })
                .collect();
            painter.add(egui::Shape::line(
                points,
                Stroke::new(2.0, theme::state_color(theme::State::Live)),
            ));
            report_non_finite(&painter, plot, *non_finite);
        }
        PlotScene::Heatmap {
            values,
            size: samples,
            range,
            non_finite,
        } => {
            axes(&painter, plot, domain, *range, quantity);
            let cell = egui::vec2(
                plot.width() / *samples as f32,
                plot.height() / *samples as f32,
            );
            for row in 0..*samples {
                for column in 0..*samples {
                    let value = values[row * samples + column];
                    if !value.is_finite() {
                        continue;
                    }
                    let t = (value - range[0]) / (range[1] - range[0]).max(1e-9);
                    let min = egui::pos2(
                        plot.left() + column as f32 * cell.x,
                        plot.bottom() - (row + 1) as f32 * cell.y,
                    );
                    painter.rect_filled(
                        Rect::from_min_size(min, cell),
                        0.0,
                        ramp(t.clamp(0.0, 1.0)),
                    );
                }
            }
            report_non_finite(&painter, plot, *non_finite);
        }
    }

    if stale {
        // A plot of a program that no longer compiles is a picture of the past.
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(160));
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "the source has changed and does not compile — this plot is stale",
            egui::FontId::proportional(14.0),
            theme::state_color(theme::State::Stale),
        );
    }
}

/// Diverging ramp through a neutral middle, so the sign of a response is
/// readable rather than inferred from brightness alone.
fn ramp(t: f32) -> Color32 {
    let low = theme::KERNEL_NEGATIVE;
    let mid = theme::KERNEL_ACTIVE_ZERO;
    let high = theme::KERNEL_POSITIVE;
    let (from, to, local) = if t < 0.5 {
        (low, mid, t * 2.0)
    } else {
        (mid, high, (t - 0.5) * 2.0)
    };
    Color32::from_rgb(
        lerp(from.r(), to.r(), local),
        lerp(from.g(), to.g(), local),
        lerp(from.b(), to.b(), local),
    )
}

fn lerp(from: u8, to: u8, t: f32) -> u8 {
    (from as f32 + (to as f32 - from as f32) * t).clamp(0.0, 255.0) as u8
}

fn axes(
    painter: &egui::Painter,
    plot: Rect,
    domain: [f32; 2],
    range: [f32; 2],
    quantity: PlotQuantity,
) {
    painter.rect_stroke(
        plot,
        0.0,
        Stroke::new(1.0, theme::CELL_STROKE),
        egui::StrokeKind::Inside,
    );
    let font = egui::FontId::proportional(11.0);
    // The numeric range is spelled out; an unlabelled axis is decoration.
    for (position, text, align) in [
        (
            plot.left_bottom(),
            format!("{:.3}", domain[0]),
            egui::Align2::LEFT_TOP,
        ),
        (
            plot.right_bottom(),
            format!("{:.3}", domain[1]),
            egui::Align2::RIGHT_TOP,
        ),
    ] {
        painter.text(position, align, text, font.clone(), theme::CELL_STROKE);
    }
    for (position, text, align) in [
        (
            plot.left_top(),
            format!("{:.3}", range[1]),
            egui::Align2::RIGHT_TOP,
        ),
        (
            plot.left_bottom(),
            format!("{:.3}", range[0]),
            egui::Align2::RIGHT_BOTTOM,
        ),
    ] {
        painter.text(position, align, text, font.clone(), theme::CELL_STROKE);
    }
    painter.text(
        plot.center_top(),
        egui::Align2::CENTER_BOTTOM,
        quantity.label(),
        font.clone(),
        theme::state_color(theme::State::Live),
    );

    // Zero is where a rate stops changing the channel, so it is worth a line.
    if range[0] < 0.0 && range[1] > 0.0 {
        let t = (0.0 - range[0]) / (range[1] - range[0]);
        let y = plot.bottom() - t * plot.height();
        painter.line_segment(
            [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            Stroke::new(1.0, theme::KERNEL_ANCHOR),
        );
        painter.text(
            egui::pos2(plot.left(), y),
            egui::Align2::RIGHT_CENTER,
            "0",
            font,
            theme::KERNEL_ANCHOR,
        );
    }
}

fn report_non_finite(painter: &egui::Painter, plot: Rect, non_finite: usize) {
    if non_finite == 0 {
        return;
    }
    // Saying how many samples were skipped keeps a gap in the curve from
    // reading as a shape the program produced.
    painter.text(
        plot.right_top(),
        egui::Align2::RIGHT_BOTTOM,
        format!("{non_finite} samples were not finite"),
        egui::FontId::proportional(11.0),
        theme::state_color(theme::State::Invalid),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::growth::typecheck;
    use crate::sim::growth::types::ExternalSymbols;

    fn program(source: &str, kernels: &[&str]) -> TypedProgram {
        typecheck::compile(source, &ExternalSymbols::new(kernels, &[]))
            .expect("the fixture compiles")
    }

    fn signature(kernels: &[&str]) -> Vec<(String, KernelId)> {
        kernels
            .iter()
            .enumerate()
            .map(|(index, name)| ((*name).to_string(), KernelId(index as u32)))
            .collect()
    }

    #[test]
    fn a_program_reading_no_kernel_plots_against_its_own_value() {
        let axes = default_axes(&[], &signature(&["k0", "k1"]));
        assert_eq!(axes, PlotAxes::Curve(PlotSymbol::SelfValue));
    }

    #[test]
    fn one_referenced_kernel_becomes_the_single_axis() {
        let axes = default_axes(&["k1".into()], &signature(&["k0", "k1"]));
        assert_eq!(axes, PlotAxes::Curve(PlotSymbol::Kernel(KernelId(1))));
    }

    #[test]
    fn two_referenced_kernels_become_a_heatmap_in_signature_order() {
        let axes = default_axes(&["k2".into(), "k0".into()], &signature(&["k0", "k1", "k2"]));
        assert_eq!(
            axes,
            PlotAxes::Heatmap(
                PlotSymbol::Kernel(KernelId(0)),
                PlotSymbol::Kernel(KernelId(2))
            ),
            "the axes follow the signature, not the order symbols happen to appear"
        );
    }

    #[test]
    fn a_declared_but_unread_kernel_is_never_made_an_axis() {
        // k0 is declared and ignored; plotting against it would draw a flat
        // surface and claim the program depends on it.
        let axes = default_axes(&["k1".into()], &signature(&["k0", "k1"]));
        assert_eq!(axes, PlotAxes::Curve(PlotSymbol::Kernel(KernelId(1))));
    }

    #[test]
    fn a_curve_samples_the_program_across_the_domain() {
        let program = program("k0 * 2.0", &["k0"]);
        let signature = signature(&["k0"]);
        let pinned = PinnedInputs::default();
        let scene = compute(&PlotInput {
            program: &program,
            signature_kernels: &signature,
            axes: PlotAxes::Curve(PlotSymbol::Kernel(KernelId(0))),
            pinned: &pinned,
            domain: [0.0, 1.0],
        });
        let PlotScene::Curve { samples, range, .. } = scene else {
            panic!("a curve was requested");
        };
        assert_eq!(samples.len(), CURVE_SAMPLES);
        assert!((samples[0].1 - 0.0).abs() < 1e-6);
        assert!((samples[samples.len() - 1].1 - 2.0).abs() < 1e-5);
        assert!(range[0] <= 0.0 && range[1] >= 2.0 - 1e-5);
    }

    #[test]
    fn a_pinned_input_is_held_at_the_value_it_was_pinned_to() {
        let program = program("k0 + k1", &["k0", "k1"]);
        let signature = signature(&["k0", "k1"]);
        let mut pinned = PinnedInputs::default();
        pinned.set_kernel(KernelId(1), 0.25);
        let scene = compute(&PlotInput {
            program: &program,
            signature_kernels: &signature,
            axes: PlotAxes::Curve(PlotSymbol::Kernel(KernelId(0))),
            pinned: &pinned,
            domain: [0.0, 1.0],
        });
        let PlotScene::Curve { samples, .. } = scene else {
            panic!("a curve was requested");
        };
        // At k0 = 0 the whole result is the pinned k1.
        assert!((samples[0].1 - 0.25).abs() < 1e-6);
    }

    #[test]
    fn a_heatmap_varies_both_axes() {
        let program = program("k0 - k1", &["k0", "k1"]);
        let signature = signature(&["k0", "k1"]);
        let pinned = PinnedInputs::default();
        let scene = compute(&PlotInput {
            program: &program,
            signature_kernels: &signature,
            axes: PlotAxes::Heatmap(
                PlotSymbol::Kernel(KernelId(0)),
                PlotSymbol::Kernel(KernelId(1)),
            ),
            pinned: &pinned,
            domain: [0.0, 1.0],
        });
        let PlotScene::Heatmap {
            values,
            size,
            range,
            ..
        } = scene
        else {
            panic!("a heatmap was requested");
        };
        assert_eq!(values.len(), size * size);
        assert!(range[0] < 0.0 && range[1] > 0.0, "k0 - k1 changes sign");
    }

    #[test]
    fn a_constant_response_still_has_a_drawable_range() {
        let program = program("0.5", &["k0"]);
        let signature = signature(&["k0"]);
        let pinned = PinnedInputs::default();
        let scene = compute(&PlotInput {
            program: &program,
            signature_kernels: &signature,
            axes: PlotAxes::Curve(PlotSymbol::SelfValue),
            pinned: &pinned,
            domain: [0.0, 1.0],
        });
        let PlotScene::Curve { range, .. } = scene else {
            panic!("a curve was requested");
        };
        assert!(
            range[1] > range[0],
            "a flat curve must not have zero height"
        );
    }

    #[test]
    fn each_quantity_says_what_it_means_and_follows_the_update_mode() {
        use crate::sim::experiment_model::UpdateMode;
        for quantity in PlotQuantity::ALL {
            assert!(!quantity.hint().is_empty());
        }
        assert_ne!(PlotQuantity::Rate.label(), PlotQuantity::Value.label());
        assert_ne!(PlotQuantity::Rate.hint(), PlotQuantity::Value.hint());
        // The plot never decides for itself what its numbers mean.
        assert_eq!(PlotQuantity::of(UpdateMode::GrowthRate), PlotQuantity::Rate);
        assert_eq!(
            PlotQuantity::of(UpdateMode::DirectUpdate),
            PlotQuantity::Value
        );
    }

    #[test]
    fn the_ramp_separates_the_two_signs_through_a_neutral_middle() {
        let low = ramp(0.0);
        let mid = ramp(0.5);
        let high = ramp(1.0);
        assert_eq!(low, theme::KERNEL_NEGATIVE);
        assert_eq!(mid, theme::KERNEL_ACTIVE_ZERO);
        assert_eq!(high, theme::KERNEL_POSITIVE);
    }
}
