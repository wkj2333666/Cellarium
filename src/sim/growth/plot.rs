use super::eval::{EvalTrace, ScalarInputs, evaluate_with_trace};
use super::types::TypedProgram;
use std::collections::BTreeMap;

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
}

#[derive(Clone, Debug, PartialEq)]
pub struct CurveSample {
    pub input: f32,
    pub value: Option<f32>,
    pub trace: Option<EvalTrace>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CurveData {
    pub axis: String,
    pub samples: Vec<CurveSample>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlotData {
    Curve(CurveData),
}

impl PlotData {
    pub fn invalid_sample_count(&self) -> usize {
        match self {
            Self::Curve(curve) => curve
                .samples
                .iter()
                .filter(|sample| sample.value.is_none())
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
            let mut result = Vec::with_capacity(samples);
            for index in 0..samples {
                let t = if samples == 1 {
                    0.0
                } else {
                    index as f32 / (samples - 1) as f32
                };
                let input = start + (end - start) * t;
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
                    }),
                    Err(_) => result.push(CurveSample {
                        input,
                        value: None,
                        trace: None,
                    }),
                }
            }
            Ok(PlotData::Curve(CurveData {
                axis,
                samples: result,
            }))
        }
    }
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
        let PlotData::Curve(curve) = data;
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
}
