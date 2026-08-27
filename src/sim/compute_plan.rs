//! Backend-neutral compiled experiment.
//!
//! Every backend — the CPU reference, CUDA and the portable wgpu compute path —
//! consumes this one plan, so ordering, dense indices and growth semantics are
//! fixed before a backend is chosen. The plan keeps the stable model identity of
//! each basis, channel and kernel next to its dense index so diagnostics and the
//! GUI can name what failed.

use std::collections::BTreeMap;

use crate::sim::basis_runtime::{SparseBasisWeight, StateLayout};
use crate::sim::experiment_model::{ChannelId, ExperimentSpec, GeometrySpec, KernelId, UpdateMode};
use crate::sim::growth::types::TypedProgram;
use crate::sim::ruleset::KernelSpatialDefinition;
use crate::sim::runtime::CompiledBoundary;
use crate::sim::tiling::BasisId;
use crate::sim::topology::BoundarySpec;

/// A compilation failure tied to the document object that caused it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    /// Document path such as `channels[1].kernels[0]`, used to navigate the GUI
    /// to the offending object.
    pub path: String,
    pub message: String,
}

impl Diagnostic {
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.path, self.message)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledChannel {
    pub id: ChannelId,
    /// Dense index into the state layout.
    pub index: usize,
    pub name: String,
    pub frozen: bool,
    pub boundary_constant: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledKernel {
    pub id: KernelId,
    pub symbol: String,
    pub source_channel: ChannelId,
    pub source_channel_index: usize,
    pub boundary_constant: f32,
    /// Flattened offsets already resolved against the periodic topology.
    pub weights: Vec<SparseBasisWeight>,
}

/// One `(basis, output channel)` binding with its own growth program.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledBinding {
    pub basis: BasisId,
    pub basis_index: usize,
    pub output: ChannelId,
    pub output_index: usize,
    pub frozen: bool,
    pub mode: UpdateMode,
    pub kernels: Vec<CompiledKernel>,
    pub parameters: BTreeMap<String, f32>,
    pub program: Option<TypedProgram>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComputePlan {
    pub width: u32,
    pub height: u32,
    pub bases: Vec<BasisId>,
    pub channels: Vec<CompiledChannel>,
    pub bindings: Vec<CompiledBinding>,
    pub boundary: CompiledBoundary,
    pub dt: f32,
    pub layout: StateLayout,
}

impl ComputePlan {
    pub fn binding(&self, basis: BasisId, output: ChannelId) -> Option<&CompiledBinding> {
        self.bindings
            .iter()
            .find(|binding| binding.basis == basis && binding.output == output)
    }

    pub fn channel(&self, id: ChannelId) -> Option<&CompiledChannel> {
        self.channels.iter().find(|channel| channel.id == id)
    }

    /// Effective kernel count across every binding.
    pub fn kernel_count(&self) -> usize {
        self.bindings
            .iter()
            .map(|binding| binding.kernels.len())
            .sum()
    }

    pub fn summary(&self) -> ComputePlanSummary {
        ComputePlanSummary {
            width: self.width,
            height: self.height,
            bases: self.bases.len(),
            channels: self.channels.len(),
            active_channels: self.channels.iter().filter(|c| !c.frozen).count(),
            bindings: self.bindings.len(),
            kernels: self.kernel_count(),
            state_scalars: self.state_scalars(),
            estimated_state_bytes: self.state_scalars().saturating_mul(size_of::<f32>()),
        }
    }

    /// Scalars in one full state buffer.
    pub fn state_scalars(&self) -> usize {
        (self.width as usize)
            .saturating_mul(self.height as usize)
            .saturating_mul(self.bases.len())
            .saturating_mul(self.channels.len())
    }
}

/// Serialization-free audit of a plan, for the Experiment summary and tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComputePlanSummary {
    pub width: u32,
    pub height: u32,
    pub bases: usize,
    pub channels: usize,
    pub active_channels: usize,
    pub bindings: usize,
    pub kernels: usize,
    pub state_scalars: usize,
    pub estimated_state_bytes: usize,
}

pub fn compile_compute_plan(spec: &ExperimentSpec) -> Result<ComputePlan, Vec<Diagnostic>> {
    if spec.rules.is_empty() {
        return Err(vec![Diagnostic::new(
            "rules",
            "compute planning requires normalized rules",
        )]);
    }
    let GeometrySpec::RasterGrid(grid) = &spec.geometry;
    let boundary = match grid.boundary {
        BoundarySpec::Open => CompiledBoundary::Open,
        BoundarySpec::Constant(_) => CompiledBoundary::Constant,
        BoundarySpec::Periodic => CompiledBoundary::Periodic,
        BoundarySpec::Clamp => CompiledBoundary::Clamp,
        BoundarySpec::Reflect => CompiledBoundary::Reflect,
    };
    if !spec.simulation_dt.is_finite() || spec.simulation_dt <= 0.0 {
        return Err(vec![Diagnostic::new(
            "simulation_dt",
            "simulation dt must be finite and positive",
        )]);
    }

    let layout = StateLayout::new(
        grid.width as usize,
        grid.height as usize,
        spec.basis_ids(),
        spec.channels.len(),
    )
    .map_err(|error| vec![Diagnostic::new("geometry", error.to_string())])?;

    spec.rules
        .validate(&layout.bases, &spec.channels)
        .map_err(|errors| {
            errors
                .into_iter()
                .map(|error| Diagnostic::new("rules", error.to_string()))
                .collect::<Vec<_>>()
        })?;

    let channel_indices = spec
        .channels
        .iter()
        .enumerate()
        .map(|(index, channel)| (channel.id, index))
        .collect::<BTreeMap<_, _>>();
    let basis_indices = layout
        .bases
        .iter()
        .enumerate()
        .map(|(index, basis)| (*basis, index))
        .collect::<BTreeMap<_, _>>();

    let channels = spec
        .channels
        .iter()
        .enumerate()
        .map(|(index, channel)| CompiledChannel {
            id: channel.id,
            index,
            name: channel.name.clone(),
            frozen: channel.frozen,
            boundary_constant: channel.boundary_constant,
        })
        .collect::<Vec<_>>();

    let mut bindings = Vec::with_capacity(channels.len() * layout.bases.len());
    let mut errors = Vec::new();
    for (output_index, channel) in spec.channels.iter().enumerate() {
        for (basis_index, basis) in layout.bases.iter().copied().enumerate() {
            let path = format!("channels[{output_index}].bindings[{basis_index}]");
            if channel.frozen {
                bindings.push(CompiledBinding {
                    basis,
                    basis_index,
                    output: channel.id,
                    output_index,
                    frozen: true,
                    mode: UpdateMode::DirectUpdate,
                    kernels: Vec::new(),
                    parameters: BTreeMap::new(),
                    program: None,
                });
                continue;
            }
            let Some(binding) = spec.rules.binding(basis, channel.id) else {
                errors.push(Diagnostic::new(
                    &path,
                    format!("missing binding for {basis:?}/{:?}", channel.id),
                ));
                continue;
            };
            let Some(rule) = spec.rules.get(binding.rule_set) else {
                errors.push(Diagnostic::new(
                    &path,
                    format!("missing rule-set {:?}", binding.rule_set),
                ));
                continue;
            };

            let mut kernels = Vec::with_capacity(rule.kernels.len());
            for (kernel_index, kernel) in rule.kernels.iter().enumerate() {
                let kernel_path = format!("{path}.kernels[{kernel_index}]");
                let Some(source_channel_index) =
                    channel_indices.get(&kernel.source_channel).copied()
                else {
                    errors.push(Diagnostic::new(
                        &kernel_path,
                        format!("missing source channel {:?}", kernel.source_channel),
                    ));
                    continue;
                };
                let weights = match flatten_weights(kernel, &basis_indices, layout.bases.len()) {
                    Ok(weights) => weights,
                    Err(message) => {
                        errors.push(Diagnostic::new(&kernel_path, message));
                        continue;
                    }
                };
                kernels.push(CompiledKernel {
                    id: kernel.id,
                    symbol: kernel.symbol.clone(),
                    source_channel: kernel.source_channel,
                    source_channel_index,
                    boundary_constant: spec
                        .channels
                        .iter()
                        .find(|entry| entry.id == kernel.source_channel)
                        .map(|entry| entry.boundary_constant)
                        .unwrap_or(0.0),
                    weights,
                });
            }

            let program = match crate::sim::growth::typecheck::compile(
                &rule.growth.source,
                &crate::sim::growth::types::ExternalSymbols {
                    kernel_inputs: kernels.iter().map(|kernel| kernel.symbol.clone()).collect(),
                    parameters: rule.growth.parameters.keys().cloned().collect(),
                },
            ) {
                Ok(program) => Some(program),
                Err(growth_errors) => {
                    for error in growth_errors {
                        errors.push(Diagnostic::new(
                            format!("{path}.growth"),
                            format!("{} at {}..{}", error.code, error.span.start, error.span.end),
                        ));
                    }
                    None
                }
            };

            bindings.push(CompiledBinding {
                basis,
                basis_index,
                output: channel.id,
                output_index,
                frozen: false,
                mode: rule.growth.mode,
                kernels,
                parameters: rule.growth.parameters.clone(),
                program,
            });
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(ComputePlan {
        width: grid.width,
        height: grid.height,
        bases: layout.bases.clone(),
        channels,
        bindings,
        boundary,
        dt: spec.simulation_dt,
        layout,
    })
}

/// Resolve a kernel's spatial definition into sorted sparse offsets. Raster
/// kernels apply to every source basis; periodic kernels name their planes.
fn flatten_weights(
    kernel: &crate::sim::ruleset::RuleKernel,
    basis_indices: &BTreeMap<BasisId, usize>,
    basis_count: usize,
) -> Result<Vec<SparseBasisWeight>, String> {
    let mut weights = Vec::new();
    match &kernel.spatial {
        KernelSpatialDefinition::Periodic(definition) => {
            definition.validate().map_err(|error| error.to_string())?;
            for (source_basis, plane) in &definition.planes {
                let source_basis = *basis_indices
                    .get(source_basis)
                    .ok_or("periodic kernel references an unknown source basis")?;
                for y in 0..definition.height {
                    for x in 0..definition.width {
                        let index = y * definition.width + x;
                        if plane.mask.as_ref().is_some_and(|mask| !mask[index]) {
                            continue;
                        }
                        let weight = plane.values[index];
                        if !weight.is_finite() {
                            return Err("kernel weight must be finite".into());
                        }
                        if weight != 0.0 {
                            weights.push(SparseBasisWeight {
                                dx: x as i16 - definition.anchor_x as i16,
                                dy: y as i16 - definition.anchor_y as i16,
                                source_basis,
                                weight,
                            });
                        }
                    }
                }
            }
        }
        KernelSpatialDefinition::Raster(definition) => {
            let built = definition.build().map_err(|error| error.to_string())?;
            for source_basis in 0..basis_count {
                for y in 0..built.height {
                    for x in 0..built.width {
                        let index = y * built.width + x;
                        if built.mask.as_ref().is_some_and(|mask| !mask[index]) {
                            continue;
                        }
                        let weight = built.values[index];
                        if !weight.is_finite() {
                            return Err("kernel weight must be finite".into());
                        }
                        if weight != 0.0 {
                            weights.push(SparseBasisWeight {
                                dx: x as i16 - built.anchor_x as i16,
                                dy: y as i16 - built.anchor_y as i16,
                                source_basis,
                                weight,
                            });
                        }
                    }
                }
            }
        }
    }
    weights.sort_by_key(|weight| (weight.dy, weight.dx, weight.source_basis));
    Ok(weights)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::basis_kernel::{BasisWeightPlane, PeriodicKernelDefinition};
    use crate::sim::ruleset::{BindingKey, RuleKernel};
    use crate::sim::tiling::{TilingPreset, build_preset};

    fn two_basis_three_channel_multi_kernel_spec() -> ExperimentSpec {
        let mut spec = ExperimentSpec::single_channel_lenia(4, 4);
        spec.tiling = Some(build_preset(TilingPreset::EquilateralTriangles, 1.0));
        for _ in 0..2 {
            spec = crate::document::channels::add_channel(&spec).unwrap().spec;
        }
        let mut spec = spec.normalize_rules().unwrap();

        // Give one binding three kernels so kernel count is provably per binding
        // rather than per channel.
        let binding = BindingKey {
            basis: BasisId(1),
            output: ChannelId(2),
        };
        let rule_set = spec.rules.detach(binding).unwrap();
        let rule = spec.rules.get_mut(rule_set).unwrap();
        let template = rule.kernels[0].clone();
        for extra in 1..3u32 {
            let mut kernel = RuleKernel {
                id: KernelId(template.id.0 + extra),
                symbol: format!("k{}", template.id.0 + extra),
                ..template.clone()
            };
            kernel.spatial = KernelSpatialDefinition::Periodic(PeriodicKernelDefinition {
                width: 1,
                height: 1,
                anchor_x: 0,
                anchor_y: 0,
                planes: [(
                    BasisId(0),
                    BasisWeightPlane {
                        values: vec![1.0],
                        mask: None,
                    },
                )]
                .into_iter()
                .collect(),
            });
            rule.kernels.push(kernel);
        }
        // The model requires the growth signature to name every kernel of its
        // rule set, so the added kernels join the inputs.
        rule.growth.kernel_inputs = rule.kernels.iter().map(|kernel| kernel.id).collect();
        rule.growth.source = "self".into();
        spec
    }

    #[test]
    fn plan_preserves_basis_channel_binding_and_kernel_order() {
        let spec = two_basis_three_channel_multi_kernel_spec();
        let plan = compile_compute_plan(&spec).unwrap();
        assert_eq!(plan.bases.len(), 2);
        assert_eq!(plan.channels.len(), 3);
        assert_eq!(plan.bindings.len(), 6);
        assert_eq!(
            plan.binding(BasisId(1), ChannelId(2))
                .unwrap()
                .kernels
                .len(),
            3
        );
    }

    #[test]
    fn every_binding_keeps_its_stable_identity_and_dense_index() {
        let spec = two_basis_three_channel_multi_kernel_spec();
        let plan = compile_compute_plan(&spec).unwrap();
        for binding in &plan.bindings {
            assert_eq!(plan.bases[binding.basis_index], binding.basis);
            assert_eq!(plan.channels[binding.output_index].id, binding.output);
            for kernel in &binding.kernels {
                assert_eq!(
                    plan.channels[kernel.source_channel_index].id,
                    kernel.source_channel
                );
            }
        }
    }

    #[test]
    fn a_plan_reports_dimensions_and_effective_kernel_counts() {
        let spec = two_basis_three_channel_multi_kernel_spec();
        let plan = compile_compute_plan(&spec).unwrap();
        let summary = plan.summary();
        assert_eq!(summary.bindings, 6);
        assert_eq!(summary.bases, 2);
        assert_eq!(summary.channels, 3);
        assert_eq!(summary.kernels, plan.kernel_count());
        assert_eq!(summary.state_scalars, 4 * 4 * 2 * 3);
        assert_eq!(summary.estimated_state_bytes, summary.state_scalars * 4);
    }

    #[test]
    fn a_legacy_experiment_without_normalized_rules_is_rejected_with_a_path() {
        let spec = ExperimentSpec::single_channel_lenia(4, 4);
        let errors = compile_compute_plan(&spec).unwrap_err();
        assert_eq!(errors[0].path, "rules");
    }

    #[test]
    fn a_non_positive_dt_is_rejected_before_any_kernel_work() {
        let mut spec = ExperimentSpec::single_channel_lenia(4, 4)
            .normalize_rules()
            .unwrap();
        spec.simulation_dt = 0.0;
        let errors = compile_compute_plan(&spec).unwrap_err();
        assert_eq!(errors[0].path, "simulation_dt");
    }

    #[test]
    fn an_invalid_growth_program_names_the_binding_that_failed() {
        let mut spec = ExperimentSpec::single_channel_lenia(4, 4)
            .normalize_rules()
            .unwrap();
        let binding = BindingKey {
            basis: spec.basis_ids()[0],
            output: ChannelId(0),
        };
        let rule_set = spec.rules.detach(binding).unwrap();
        spec.rules.get_mut(rule_set).unwrap().growth.source = "unknown_symbol".into();
        let errors = compile_compute_plan(&spec).unwrap_err();
        assert!(errors.iter().any(|error| error.path.ends_with(".growth")));
    }
}
