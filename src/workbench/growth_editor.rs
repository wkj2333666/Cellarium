use super::TextBuffer;
use crate::sim::growth::{
    plot::{PinnedInputs, PlotData, PlotRequest, sample_plot},
    typecheck::compile,
    types::ExternalSymbols,
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GrowthPlot {
    pub data: Vec<Option<f32>>,
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
    pub fn replace_source(&mut self, source: impl Into<String>) {
        self.buffer.replace(source);
        self.generation = self.generation.wrapping_add(1);
    }
    pub fn refresh_now(&mut self) {
        match compile(self.buffer.as_str(), &self.symbols) {
            Ok(program) => {
                self.diagnostics.clear();
                if let Ok(PlotData::Curve(curve)) = sample_plot(
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
pub fn editor_for(
    spec: &crate::sim::experiment_model::ExperimentSpec,
    target: crate::sim::experiment_model::ChannelId,
) -> GrowthEditorState {
    let growth = spec.growth.iter().find(|growth| growth.target == target);
    let inputs: Vec<String> = growth
        .map(|g| {
            g.kernel_inputs
                .iter()
                .filter_map(|id| {
                    spec.kernels
                        .iter()
                        .find(|k| k.id == *id)
                        .map(|k| k.symbol.clone())
                })
                .collect()
        })
        .unwrap_or_default();
    let parameters = growth.map(|g| g.parameters.clone()).unwrap_or_default();
    let target_name = spec
        .channels
        .iter()
        .find(|c| c.id == target)
        .map_or("channel", |c| c.name.as_str());
    let source = growth.map_or("self", |g| g.source.as_str());
    let signature = format!("growth_{target_name}({}; self) -> rate", inputs.join(", "));
    GrowthEditorState::new(
        source,
        ExternalSymbols {
            kernel_inputs: inputs,
            parameters: parameters.keys().cloned().collect(),
        },
        parameters,
        signature,
    )
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
}
