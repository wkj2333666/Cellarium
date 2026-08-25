use super::TextBuffer;
use crate::sim::experiment_model::UpdateMode;
use crate::sim::growth::{
    plot::{HeatmapData, PinnedInputs, PlotData, PlotRequest, sample_plot},
    typecheck::compile,
    types::ExternalSymbols,
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GrowthPlot {
    pub data: Vec<Option<f32>>,
    pub heatmap: Option<HeatmapData>,
    pub stale: bool,
}
#[derive(Clone, Debug)]
pub struct GrowthEditorState {
    buffer: TextBuffer,
    symbols: ExternalSymbols,
    parameters: BTreeMap<String, f32>,
    diagnostics: Vec<String>,
    plot: GrowthPlot,
    generation: u64,
    signature: String,
    mode: UpdateMode,
}
impl GrowthEditorState {
    pub fn new(
        source: impl Into<String>,
        symbols: ExternalSymbols,
        parameters: BTreeMap<String, f32>,
        signature: impl Into<String>,
    ) -> Self {
        let mut editor = Self {
            buffer: TextBuffer::new(source),
            symbols,
            parameters,
            diagnostics: Vec::new(),
            plot: GrowthPlot::default(),
            generation: 0,
            signature: signature.into(),
            mode: UpdateMode::GrowthRate,
        };
        editor.refresh_now();
        editor
    }
    pub fn buffer(&self) -> &TextBuffer {
        &self.buffer
    }
    pub fn buffer_mut(&mut self) -> &mut TextBuffer {
        self.generation = self.generation.wrapping_add(1);
        &mut self.buffer
    }
    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }
    pub fn plot(&self) -> &GrowthPlot {
        &self.plot
    }
    pub fn signature(&self) -> &str {
        &self.signature
    }
    pub fn mode(&self) -> UpdateMode {
        self.mode
    }
    pub fn with_mode(mut self, mode: UpdateMode) -> Self {
        self.mode = mode;
        self
    }
    pub fn plot_caption(&self) -> String {
        let result = match self.mode {
            UpdateMode::GrowthRate => "rate",
            UpdateMode::DirectUpdate => "value",
        };
        match self.symbols.kernel_inputs.as_slice() {
            [x, y, ..] => {
                format!("plot · x={x} [0,1] · y={y} [0,1] · color={result}")
            }
            [x] => format!("plot · x={x} [0,1] · y={result}"),
            [] => format!("plot · x=self [0,1] · y={result}"),
        }
    }
    pub fn replace_source(&mut self, source: impl Into<String>) {
        self.buffer.replace(source);
        self.generation = self.generation.wrapping_add(1);
    }
    pub fn refresh_now(&mut self) {
        match compile(self.buffer.as_str(), &self.symbols) {
            Ok(program) => {
                self.diagnostics.clear();
                if self.symbols.kernel_inputs.len() >= 2 {
                    let mut pinned = self.parameters.clone();
                    pinned.insert("self".into(), 0.5);
                    if let Ok(PlotData::Heatmap(heatmap)) = sample_plot(
                        &program,
                        PlotRequest::Heatmap {
                            x_axis: self.symbols.kernel_inputs[0].clone(),
                            y_axis: self.symbols.kernel_inputs[1].clone(),
                            x_start: 0.0,
                            x_end: 1.0,
                            y_start: 0.0,
                            y_end: 1.0,
                            width: 96,
                            height: 64,
                            pinned: PinnedInputs(pinned),
                        },
                    ) {
                        self.plot = GrowthPlot {
                            data: Vec::new(),
                            heatmap: Some(heatmap),
                            stale: false,
                        };
                    }
                } else if let Ok(PlotData::Curve(curve)) = sample_plot(
                    &program,
                    PlotRequest::Curve {
                        axis: self
                            .symbols
                            .kernel_inputs
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "self".into()),
                        start: 0.0,
                        end: 1.0,
                        samples: 48,
                        pinned: PinnedInputs(self.parameters.clone()),
                        trace: false,
                    },
                ) {
                    self.plot = GrowthPlot {
                        data: curve
                            .samples
                            .into_iter()
                            .map(|sample| sample.value)
                            .collect(),
                        heatmap: None,
                        stale: false,
                    };
                }
            }
            Err(errors) => {
                self.diagnostics = errors
                    .into_iter()
                    .map(|error| {
                        format!("{} at {}..{}", error.code, error.span.start, error.span.end)
                    })
                    .collect();
                self.plot.stale = true;
            }
        }
    }
}
pub fn editor_for_basis(
    spec: &crate::sim::experiment_model::ExperimentSpec,
    basis: crate::sim::tiling::BasisId,
    target: crate::sim::experiment_model::ChannelId,
) -> GrowthEditorState {
    let normalized = spec
        .rules
        .binding(basis, target)
        .and_then(|binding| spec.rules.get(binding.rule_set));
    let legacy = spec.growth.iter().find(|growth| growth.target == target);
    let inputs: Vec<String> = if let Some(rule) = normalized {
        rule.kernels
            .iter()
            .map(|kernel| kernel.symbol.clone())
            .collect()
    } else {
        legacy
            .map(|growth| {
                growth
                    .kernel_inputs
                    .iter()
                    .filter_map(|id| {
                        spec.kernels
                            .iter()
                            .find(|kernel| kernel.id == *id)
                            .map(|kernel| kernel.symbol.clone())
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let parameters = normalized
        .map(|rule| rule.growth.parameters.clone())
        .or_else(|| legacy.map(|growth| growth.parameters.clone()))
        .unwrap_or_default();
    let source = normalized
        .map(|rule| rule.growth.source.as_str())
        .or_else(|| legacy.map(|growth| growth.source.as_str()))
        .unwrap_or("self");
    let mode = normalized
        .map(|rule| rule.growth.mode)
        .or_else(|| legacy.map(|growth| growth.mode))
        .unwrap_or(UpdateMode::DirectUpdate);
    let mut arguments = vec!["self: Scalar".to_string()];
    arguments.extend(inputs.iter().map(|symbol| format!("{symbol}: Scalar")));
    let result = match mode {
        UpdateMode::GrowthRate => "Rate",
        UpdateMode::DirectUpdate => "Value",
    };
    let signature = format!("fn growth({}) -> {result}", arguments.join(", "));
    GrowthEditorState::new(
        source,
        ExternalSymbols {
            kernel_inputs: inputs,
            parameters: parameters.keys().cloned().collect(),
        },
        parameters,
        signature,
    )
    .with_mode(mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_keeps_last_plot_stale() {
        let symbols = ExternalSymbols::new(&["inner"], &[]);
        let mut editor = GrowthEditorState::new(
            "inner",
            symbols,
            BTreeMap::new(),
            "growth_a(inner; self) -> rate",
        );
        let valid = editor.plot().data.clone();
        editor.replace_source("if inner {");
        editor.refresh_now();
        assert_eq!(editor.plot().data, valid);
        assert!(editor.plot().stale);
        assert!(!editor.diagnostics().is_empty());
    }

    #[test]
    fn normalized_ruleset_supplies_the_basis_specific_signature_and_source() {
        let mut spec = crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(8, 8)
            .normalize_rules()
            .unwrap();
        spec.rules.sets[0].kernels[0].symbol = "nearby".into();
        spec.rules.sets[0].growth.source = "nearby - self".into();
        let editor = editor_for_basis(
            &spec,
            crate::sim::tiling::BasisId(0),
            crate::sim::experiment_model::ChannelId(0),
        );
        assert_eq!(editor.buffer().as_str(), "nearby - self");
        assert_eq!(
            editor.signature(),
            "fn growth(self: Scalar, nearby: Scalar) -> Rate"
        );
        assert!(editor.diagnostics().is_empty());
    }

    #[test]
    fn two_kernel_inputs_select_a_precise_two_dimensional_plot() {
        let editor = GrowthEditorState::new(
            "first + second * second",
            ExternalSymbols::new(&["first", "second"], &[]),
            BTreeMap::new(),
            "fn growth(self: Scalar, first: Scalar, second: Scalar) -> Rate",
        );
        assert_eq!(
            editor.plot_caption(),
            "plot · x=first [0,1] · y=second [0,1] · color=rate"
        );
        let heatmap = editor.plot().heatmap.as_ref().expect("expected 2D heatmap");
        assert_eq!((heatmap.width, heatmap.height), (96, 64));
        assert_eq!(
            (heatmap.x_axis.as_str(), heatmap.y_axis.as_str()),
            ("first", "second")
        );
        assert!(
            heatmap
                .samples
                .iter()
                .flatten()
                .copied()
                .min_by(f32::total_cmp)
                != heatmap
                    .samples
                    .iter()
                    .flatten()
                    .copied()
                    .max_by(f32::total_cmp)
        );
    }
}
