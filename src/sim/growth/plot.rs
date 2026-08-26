use super::ast::{BinaryOp, UnaryOp};
use super::eval::{EvalTrace, ScalarInputs, evaluate_with_trace};
use super::types::{SymbolId, TypedBinding, TypedExpr, TypedExprKind, TypedProgram};
use std::collections::BTreeMap;

/// Conservative interval for a raw (unnormalized) weighted sum.
///
/// Each positive weight reaches its minimum at the source minimum and each
/// negative weight reaches its minimum at the source maximum; maxima use the
/// opposite corners. This intentionally does not normalize kernel weights.
pub fn potential_interval(weights: &[f32], source_interval: [f32; 2]) -> [f32; 2] {
    let [source_min, source_max] = source_interval;
    weights
        .iter()
        .fold([0.0, 0.0], |[minimum, maximum], weight| {
            if *weight >= 0.0 {
                [minimum + weight * source_min, maximum + weight * source_max]
            } else {
                [minimum + weight * source_max, maximum + weight * source_min]
            }
        })
}

#[cfg(test)]
mod interval_tests {
    use super::potential_interval;

    #[test]
    fn six_raw_unit_weights_span_zero_to_six() {
        assert_eq!(potential_interval(&[1.0; 6], [0.0, 1.0]), [0.0, 6.0]);
    }

    #[test]
    fn signed_weights_choose_conservative_corners() {
        assert_eq!(potential_interval(&[2.0, -3.0], [0.0, 1.0]), [-3.0, 2.0]);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PinnedInputs(pub BTreeMap<String, f32>);

#[derive(Clone, Debug, PartialEq)]
pub enum PlotRequest {
    Curve {
        axis: String,
        start: f32,
        end: f32,
        samples: usize,
        pinned: PinnedInputs,
        trace: bool,
    },
    Heatmap {
        x_axis: String,
        y_axis: String,
        x_start: f32,
        x_end: f32,
        y_start: f32,
        y_end: f32,
        width: usize,
        height: usize,
        pinned: PinnedInputs,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct CurveSample {
    pub input: f32,
    pub value: Option<f32>,
    pub trace: Option<EvalTrace>,
    pub kind: CurveSampleKind,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CurveSampleKind {
    #[default]
    Uniform,
    BelowThreshold,
    ExactThreshold,
    AboveThreshold,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlotDiagnostic {
    pub code: &'static str,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CurveData {
    pub axis: String,
    pub samples: Vec<CurveSample>,
    pub diagnostics: Vec<PlotDiagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HeatmapData {
    pub x_axis: String,
    pub y_axis: String,
    pub width: usize,
    pub height: usize,
    pub samples: Vec<Option<f32>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlotData {
    Curve(CurveData),
    Heatmap(HeatmapData),
}

impl PlotData {
    pub fn invalid_sample_count(&self) -> usize {
        match self {
            Self::Curve(curve) => curve
                .samples
                .iter()
                .filter(|sample| sample.value.is_none())
                .count(),
            Self::Heatmap(heatmap) => heatmap
                .samples
                .iter()
                .filter(|sample| sample.is_none())
                .count(),
        }
    }
}

pub fn sample_plot(program: &TypedProgram, request: PlotRequest) -> Result<PlotData, &'static str> {
    match request {
        PlotRequest::Curve {
            axis,
            start,
            end,
            samples,
            pinned,
            trace,
        } => {
            if samples == 0 || samples > 4096 || !start.is_finite() || !end.is_finite() {
                return Err("invalid_curve_request");
            }
            if !program.externals.ordered().iter().any(|name| name == &axis) {
                return Err("unknown_plot_axis");
            }
            let axis_id = program
                .externals
                .ordered()
                .iter()
                .position(|name| name == &axis)
                .map(|index| SymbolId(index as u32))
                .ok_or("unknown_plot_axis")?;
            let mut thresholds = critical_thresholds(program, axis_id)
                .into_iter()
                .filter(|value| {
                    value.is_finite() && *value >= start.min(end) && *value <= start.max(end)
                })
                .collect::<Vec<_>>();
            thresholds.sort_by(f32::total_cmp);
            thresholds.dedup_by(|left, right| left.to_bits() == right.to_bits());
            thresholds.truncate((4096_usize.saturating_sub(samples.min(2048))) / 3);
            let mut points = Vec::with_capacity(samples.min(4096) + thresholds.len() * 3);
            let uniform_samples = samples.min(4096_usize.saturating_sub(thresholds.len() * 3));
            for index in 0..uniform_samples {
                let t = if uniform_samples == 1 {
                    0.0
                } else {
                    index as f32 / (uniform_samples - 1) as f32
                };
                let input = start + (end - start) * t;
                points.push((input, CurveSampleKind::Uniform));
            }
            for threshold in thresholds.iter().copied() {
                let below = next_down(threshold);
                let above = next_up(threshold);
                if below >= start.min(end) && below <= start.max(end) {
                    points.push((below, CurveSampleKind::BelowThreshold));
                }
                points.push((threshold, CurveSampleKind::ExactThreshold));
                if above >= start.min(end) && above <= start.max(end) {
                    points.push((above, CurveSampleKind::AboveThreshold));
                }
            }
            points.sort_by(|left, right| left.0.total_cmp(&right.0));
            let mut deduplicated: Vec<(f32, CurveSampleKind)> = Vec::with_capacity(points.len());
            for point in points {
                if let Some(previous) = deduplicated.last_mut()
                    && previous.0.to_bits() == point.0.to_bits()
                {
                    if sample_kind_priority(point.1) > sample_kind_priority(previous.1) {
                        previous.1 = point.1;
                    }
                    continue;
                }
                deduplicated.push(point);
            }
            let mut result = Vec::with_capacity(deduplicated.len());
            for (input, kind) in deduplicated {
                let mut parameters = pinned.0.clone();
                let axis_is_kernel = program
                    .externals
                    .kernel_inputs
                    .iter()
                    .position(|name| name == &axis);
                let mut kernel_inputs = program
                    .externals
                    .kernel_inputs
                    .iter()
                    .map(|name| *parameters.get(name).unwrap_or(&0.0))
                    .collect::<Vec<_>>();
                if let Some(position) = axis_is_kernel {
                    if position >= kernel_inputs.len() {
                        kernel_inputs.resize(position + 1, 0.0);
                    }
                    kernel_inputs[position] = input;
                }
                let self_value = if axis == "self" {
                    input
                } else {
                    *parameters.get("self").unwrap_or(&0.0)
                };
                if axis != "self" {
                    parameters.insert(axis.clone(), input);
                }
                let evaluated = evaluate_with_trace(
                    program,
                    &ScalarInputs {
                        kernel_inputs,
                        self_value,
                        parameters,
                    },
                );
                match evaluated {
                    Ok(trace_value) => result.push(CurveSample {
                        input,
                        value: Some(trace_value.result),
                        trace: trace.then_some(trace_value),
                        kind,
                    }),
                    Err(_) => result.push(CurveSample {
                        input,
                        value: None,
                        trace: None,
                        kind,
                    }),
                }
            }
            Ok(PlotData::Curve(CurveData {
                axis,
                samples: result,
                diagnostics: (!thresholds.is_empty())
                    .then(|| PlotDiagnostic {
                        code: "critical_thresholds",
                        message: format!(
                            "{} comparison threshold(s) sampled exactly",
                            thresholds.len()
                        ),
                    })
                    .into_iter()
                    .collect(),
            }))
        }
        PlotRequest::Heatmap {
            x_axis,
            y_axis,
            x_start,
            x_end,
            y_start,
            y_end,
            width,
            height,
            pinned,
        } => {
            let sample_count = width.checked_mul(height).ok_or("invalid_heatmap_request")?;
            if width == 0
                || height == 0
                || width > 512
                || height > 512
                || sample_count > 262_144
                || x_axis == y_axis
                || !x_start.is_finite()
                || !x_end.is_finite()
                || !y_start.is_finite()
                || !y_end.is_finite()
            {
                return Err("invalid_heatmap_request");
            }
            let externals = program.externals.ordered();
            if !externals.iter().any(|name| name == &x_axis)
                || !externals.iter().any(|name| name == &y_axis)
            {
                return Err("unknown_plot_axis");
            }
            let mut samples = Vec::with_capacity(sample_count);
            for y in 0..height {
                let y_t = if height == 1 {
                    0.0
                } else {
                    y as f32 / (height - 1) as f32
                };
                let y_value = y_start + (y_end - y_start) * y_t;
                for x in 0..width {
                    let x_t = if width == 1 {
                        0.0
                    } else {
                        x as f32 / (width - 1) as f32
                    };
                    let x_value = x_start + (x_end - x_start) * x_t;
                    samples.push(evaluate_point(
                        program,
                        &pinned,
                        &[(x_axis.as_str(), x_value), (y_axis.as_str(), y_value)],
                    ));
                }
            }
            Ok(PlotData::Heatmap(HeatmapData {
                x_axis,
                y_axis,
                width,
                height,
                samples,
            }))
        }
    }
}

fn sample_kind_priority(kind: CurveSampleKind) -> u8 {
    match kind {
        CurveSampleKind::Uniform => 0,
        CurveSampleKind::BelowThreshold | CurveSampleKind::AboveThreshold => 1,
        CurveSampleKind::ExactThreshold => 2,
    }
}

fn next_down(value: f32) -> f32 {
    if value.is_nan() || value == f32::NEG_INFINITY {
        return value;
    }
    if value == 0.0 {
        return -f32::from_bits(1);
    }
    let bits = value.to_bits();
    f32::from_bits(if value > 0.0 { bits - 1 } else { bits + 1 })
}

fn next_up(value: f32) -> f32 {
    if value.is_nan() || value == f32::INFINITY {
        return value;
    }
    if value == 0.0 {
        return f32::from_bits(1);
    }
    let bits = value.to_bits();
    f32::from_bits(if value > 0.0 { bits + 1 } else { bits - 1 })
}

fn critical_thresholds(program: &TypedProgram, axis: SymbolId) -> Vec<f32> {
    let mut thresholds = Vec::new();
    visit_bindings(&program.bindings, axis, &mut thresholds);
    visit_expression(&program.result, axis, &mut thresholds);
    thresholds
}

fn visit_bindings(bindings: &[TypedBinding], axis: SymbolId, thresholds: &mut Vec<f32>) {
    for binding in bindings {
        visit_expression(&binding.value, axis, thresholds);
    }
}

fn visit_expression(expression: &TypedExpr, axis: SymbolId, thresholds: &mut Vec<f32>) {
    match &expression.kind {
        TypedExprKind::Unary { operand, .. } => visit_expression(operand, axis, thresholds),
        TypedExprKind::Binary { op, lhs, rhs } => {
            if matches!(
                op,
                BinaryOp::Equal
                    | BinaryOp::NotEqual
                    | BinaryOp::Less
                    | BinaryOp::LessEqual
                    | BinaryOp::Greater
                    | BinaryOp::GreaterEqual
            ) {
                if matches!(lhs.kind, TypedExprKind::Symbol(symbol) if symbol == axis)
                    && let Some(value) = constant_scalar(rhs)
                {
                    thresholds.push(value);
                } else if matches!(rhs.kind, TypedExprKind::Symbol(symbol) if symbol == axis)
                    && let Some(value) = constant_scalar(lhs)
                {
                    thresholds.push(value);
                }
            }
            visit_expression(lhs, axis, thresholds);
            visit_expression(rhs, axis, thresholds);
        }
        TypedExprKind::Call { arguments, .. } => {
            for argument in arguments {
                visit_expression(argument, axis, thresholds);
            }
        }
        TypedExprKind::If {
            condition,
            then_branch,
            then_result,
            else_branch,
            else_result,
        } => {
            visit_expression(condition, axis, thresholds);
            visit_bindings(then_branch, axis, thresholds);
            visit_expression(then_result, axis, thresholds);
            visit_bindings(else_branch, axis, thresholds);
            visit_expression(else_result, axis, thresholds);
        }
        TypedExprKind::Constant(_) | TypedExprKind::Bool(_) | TypedExprKind::Symbol(_) => {}
    }
}

fn constant_scalar(expression: &TypedExpr) -> Option<f32> {
    let value = match &expression.kind {
        TypedExprKind::Constant(value) => *value,
        TypedExprKind::Unary {
            op: UnaryOp::Neg,
            operand,
        } => -constant_scalar(operand)?,
        TypedExprKind::Binary { op, lhs, rhs } => {
            let lhs = constant_scalar(lhs)?;
            let rhs = constant_scalar(rhs)?;
            match op {
                BinaryOp::Add => lhs + rhs,
                BinaryOp::Subtract => lhs - rhs,
                BinaryOp::Multiply => lhs * rhs,
                BinaryOp::Divide => lhs / rhs,
                BinaryOp::Power => lhs.powf(rhs),
                _ => return None,
            }
        }
        _ => return None,
    };
    value.is_finite().then_some(value)
}

fn evaluate_point(
    program: &TypedProgram,
    pinned: &PinnedInputs,
    axes: &[(&str, f32)],
) -> Option<f32> {
    let mut parameters = pinned.0.clone();
    let mut kernel_inputs = program
        .externals
        .kernel_inputs
        .iter()
        .map(|name| *parameters.get(name).unwrap_or(&0.0))
        .collect::<Vec<_>>();
    let mut self_value = *parameters.get("self").unwrap_or(&0.0);
    for (axis, value) in axes {
        if *axis == "self" {
            self_value = *value;
        } else if let Some(position) = program
            .externals
            .kernel_inputs
            .iter()
            .position(|name| name == axis)
        {
            kernel_inputs[position] = *value;
        } else {
            parameters.insert((*axis).to_string(), *value);
        }
    }
    evaluate_with_trace(
        program,
        &ScalarInputs {
            kernel_inputs,
            self_value,
            parameters,
        },
    )
    .ok()
    .map(|trace| trace.result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::growth::typecheck::compile;
    use crate::sim::growth::types::ExternalSymbols;
    fn compiled(source: &str) -> TypedProgram {
        compile(source, &ExternalSymbols::new(&["inner"], &[])).unwrap()
    }
    fn request(axis: &str, start: f32, end: f32, samples: usize) -> PlotRequest {
        PlotRequest::Curve {
            axis: axis.to_string(),
            start,
            end,
            samples,
            pinned: PinnedInputs(BTreeMap::new()),
            trace: true,
        }
    }
    #[test]
    fn one_axis_request_returns_ordered_curve_and_trace() {
        let data = sample_plot(
            &compiled("let y = inner * inner; y"),
            request("inner", 0.0, 1.0, 5),
        )
        .unwrap();
        let PlotData::Curve(curve) = data else {
            panic!("curve request must return curve data");
        };
        assert_eq!(curve.samples.len(), 5);
        assert_eq!(curve.samples[2].value, Some(0.25));
        assert_eq!(
            curve.samples[2].trace.as_ref().unwrap().binding("y"),
            Some(0.25)
        );
    }
    #[test]
    fn invalid_samples_are_masked_without_aborting_the_plot() {
        let data = sample_plot(&compiled("sqrt(inner)"), request("inner", -1.0, 1.0, 3)).unwrap();
        assert_eq!(data.invalid_sample_count(), 1);
    }

    #[test]
    fn equality_thresholds_are_sampled_exactly() {
        let data = sample_plot(
            &compiled("if inner == 2/6 || inner == 3/6 { 1 } else { 0 }"),
            request("inner", 0.0, 1.0, 48),
        )
        .unwrap();
        let PlotData::Curve(curve) = data else {
            panic!("curve request must return curve data");
        };
        for threshold in [2.0_f32 / 6.0, 3.0_f32 / 6.0] {
            let sample = curve
                .samples
                .iter()
                .find(|sample| sample.input.to_bits() == threshold.to_bits())
                .unwrap_or_else(|| panic!("missing exact threshold {threshold}"));
            assert_eq!(sample.value, Some(1.0));
        }
    }

    #[test]
    fn two_axis_request_returns_a_nonuniform_heatmap() {
        let program = compile(
            "inner + outer * outer",
            &ExternalSymbols::new(&["inner", "outer"], &[]),
        )
        .unwrap();
        let data = sample_plot(
            &program,
            PlotRequest::Heatmap {
                x_axis: "inner".into(),
                y_axis: "outer".into(),
                x_start: 0.0,
                x_end: 1.0,
                y_start: -1.0,
                y_end: 1.0,
                width: 9,
                height: 7,
                pinned: PinnedInputs(BTreeMap::new()),
            },
        )
        .unwrap();
        let PlotData::Heatmap(heatmap) = data else {
            panic!("two-axis request must produce heatmap data");
        };
        assert_eq!((heatmap.width, heatmap.height), (9, 7));
        assert_eq!(heatmap.samples.len(), 63);
        assert_ne!(heatmap.samples[0], heatmap.samples[31]);
        assert_ne!(heatmap.samples[31], heatmap.samples[62]);
    }
}
