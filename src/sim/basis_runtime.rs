use std::collections::BTreeMap;

use crate::sim::experiment_model::{ExperimentSpec, UpdateMode};
use crate::sim::runtime::{CompiledBoundary, RuntimeError};
use crate::sim::tiling::BasisId;
use crate::sim::world::ChannelWorld;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateLayout {
    pub width: usize,
    pub height: usize,
    pub bases: Vec<BasisId>,
    pub channels: usize,
    basis_indices: BTreeMap<BasisId, usize>,
    channel_len: usize,
    total_len: usize,
}

impl StateLayout {
    pub fn new(
        width: usize,
        height: usize,
        mut bases: Vec<BasisId>,
        channels: usize,
    ) -> Result<Self, RuntimeError> {
        bases.sort_unstable();
        bases.dedup();
        if width == 0 || height == 0 || bases.is_empty() || channels == 0 {
            return Err(RuntimeError::Model(
                "basis state dimensions must be positive".into(),
            ));
        }
        let channel_len = width
            .checked_mul(height)
            .and_then(|cells| cells.checked_mul(bases.len()))
            .ok_or_else(|| RuntimeError::Model("basis state dimensions overflow".into()))?;
        let total_len = channel_len
            .checked_mul(channels)
            .ok_or_else(|| RuntimeError::Model("basis state dimensions overflow".into()))?;
        let basis_indices = bases
            .iter()
            .enumerate()
            .map(|(index, basis)| (*basis, index))
            .collect();
        Ok(Self {
            width,
            height,
            bases,
            channels,
            basis_indices,
            channel_len,
            total_len,
        })
    }

    pub fn index(&self, channel: usize, x: usize, y: usize, basis: BasisId) -> Option<usize> {
        let basis = *self.basis_indices.get(&basis)?;
        self.index_by_position(channel, x, y, basis)
    }

    pub(crate) fn index_by_position(
        &self,
        channel: usize,
        x: usize,
        y: usize,
        basis: usize,
    ) -> Option<usize> {
        if channel >= self.channels
            || x >= self.width
            || y >= self.height
            || basis >= self.bases.len()
        {
            return None;
        }
        Some(channel * self.channel_len + (y * self.width + x) * self.bases.len() + basis)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SparseBasisWeight {
    pub dx: i16,
    pub dy: i16,
    pub source_basis: usize,
    pub weight: f32,
}

#[derive(Clone, Debug)]
pub struct CompiledBasisKernel {
    pub symbol: String,
    pub source_channel: usize,
    pub boundary_constant: f32,
    pub weights: Vec<SparseBasisWeight>,
}

#[derive(Clone, Debug)]
pub struct CompiledBasisRule {
    pub target_channel: usize,
    pub target_basis: usize,
    pub frozen: bool,
    pub mode: UpdateMode,
    pub kernels: Vec<CompiledBasisKernel>,
    pub parameters: BTreeMap<String, f32>,
    pub program: Option<crate::sim::growth::types::TypedProgram>,
}

#[derive(Clone, Debug)]
pub struct CompiledBasisExperiment {
    pub layout: StateLayout,
    pub boundary: CompiledBoundary,
    pub simulation_dt: f32,
    pub rules: Vec<CompiledBasisRule>,
}

/// Build the CPU reference runtime from the shared [`ComputePlan`].
///
/// Ordering, dense indices and growth typing all come from the plan, so the CPU
/// path cannot drift from the CUDA and wgpu backends.
pub fn compile_basis_experiment(
    spec: &ExperimentSpec,
) -> Result<CompiledBasisExperiment, RuntimeError> {
    let plan = crate::sim::compute_plan::compile_compute_plan(spec).map_err(|diagnostics| {
        RuntimeError::Model(
            diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.message)
                .collect::<Vec<_>>()
                .join("; "),
        )
    })?;
    Ok(CompiledBasisExperiment::from_plan(&plan))
}

impl CompiledBasisExperiment {
    pub fn from_plan(plan: &crate::sim::compute_plan::ComputePlan) -> Self {
        let rules = plan
            .bindings
            .iter()
            .map(|binding| CompiledBasisRule {
                target_channel: binding.output_index,
                target_basis: binding.basis_index,
                frozen: binding.frozen,
                mode: binding.mode,
                kernels: binding
                    .kernels
                    .iter()
                    .map(|kernel| CompiledBasisKernel {
                        symbol: kernel.symbol.clone(),
                        source_channel: kernel.source_channel_index,
                        boundary_constant: kernel.boundary_constant,
                        weights: kernel.weights.clone(),
                    })
                    .collect(),
                parameters: binding.parameters.clone(),
                program: binding.program.clone(),
            })
            .collect();
        Self {
            layout: plan.layout.clone(),
            boundary: plan.boundary,
            simulation_dt: plan.dt,
            rules,
        }
    }
}

impl CompiledBasisExperiment {
    pub fn step(&self, world: &mut ChannelWorld) -> Result<(), RuntimeError> {
        if world.width() != self.layout.width
            || world.height() != self.layout.height
            || world.channels() != self.layout.channels
            || world.bases() != self.layout.bases.len()
        {
            return Err(RuntimeError::WorldShape);
        }
        let old = world.cells().to_vec();
        if old.len() != self.layout.total_len {
            return Err(RuntimeError::WorldShape);
        }
        let mut next = old.clone();
        for rule in &self.rules {
            if rule.frozen {
                continue;
            }
            let program = rule.program.as_ref().ok_or_else(|| {
                RuntimeError::Model("active basis rule has no growth program".into())
            })?;
            for y in 0..self.layout.height {
                for x in 0..self.layout.width {
                    let target = self
                        .layout
                        .index_by_position(rule.target_channel, x, y, rule.target_basis)
                        .ok_or(RuntimeError::WorldShape)?;
                    let current = old[target];
                    let mut potentials = Vec::with_capacity(rule.kernels.len());
                    for kernel in &rule.kernels {
                        let mut value = 0.0_f32;
                        for weight in &kernel.weights {
                            value += weight.weight
                                * sample_basis(
                                    &old,
                                    &self.layout,
                                    kernel.source_channel,
                                    x as isize + isize::from(weight.dx),
                                    y as isize + isize::from(weight.dy),
                                    weight.source_basis,
                                    kernel.boundary_constant,
                                    self.boundary,
                                );
                        }
                        if !value.is_finite() {
                            return Err(RuntimeError::NonFiniteState);
                        }
                        potentials.push(value);
                    }
                    let result = crate::sim::growth::eval::evaluate(
                        program,
                        &crate::sim::growth::eval::ScalarInputs {
                            kernel_inputs: potentials,
                            self_value: current,
                            parameters: rule.parameters.clone(),
                        },
                    )
                    .map_err(|error| RuntimeError::Model(error.to_string()))?;
                    let value = match rule.mode {
                        UpdateMode::GrowthRate => {
                            (current + self.simulation_dt * result).clamp(0.0, 1.0)
                        }
                        UpdateMode::DirectUpdate => result.clamp(0.0, 1.0),
                    };
                    if !value.is_finite() {
                        return Err(RuntimeError::NonFiniteState);
                    }
                    next[target] = value;
                }
            }
        }
        world.replace_all(&next)?;
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn sample_basis(
    cells: &[f32],
    layout: &StateLayout,
    channel: usize,
    x: isize,
    y: isize,
    basis: usize,
    boundary_constant: f32,
    boundary: CompiledBoundary,
) -> f32 {
    let (x, y) = match boundary {
        CompiledBoundary::Open => {
            if x < 0 || y < 0 || x >= layout.width as isize || y >= layout.height as isize {
                return 0.0;
            }
            (x as usize, y as usize)
        }
        CompiledBoundary::Constant => {
            if x < 0 || y < 0 || x >= layout.width as isize || y >= layout.height as isize {
                return boundary_constant;
            }
            (x as usize, y as usize)
        }
        CompiledBoundary::Periodic => (
            x.rem_euclid(layout.width as isize) as usize,
            y.rem_euclid(layout.height as isize) as usize,
        ),
        CompiledBoundary::Clamp => (
            x.clamp(0, layout.width as isize - 1) as usize,
            y.clamp(0, layout.height as isize - 1) as usize,
        ),
        CompiledBoundary::Reflect => (reflect(x, layout.width), reflect(y, layout.height)),
    };
    let index = layout
        .index_by_position(channel, x, y, basis)
        .expect("validated basis sample index");
    cells[index]
}

fn reflect(value: isize, size: usize) -> usize {
    if size <= 1 {
        return 0;
    }
    let span = size as isize - 1;
    let period = span * 2;
    let folded = value.rem_euclid(period);
    if folded <= span {
        folded as usize
    } else {
        (period - folded) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::basis_kernel::{BasisWeightPlane, PeriodicKernelDefinition};
    use crate::sim::experiment_model::{ChannelId, ExperimentSpec, UpdateMode};
    use crate::sim::ruleset::{BindingKey, KernelSpatialDefinition};
    use crate::sim::tiling::{BasisId, TilingPreset, build_preset};
    use crate::sim::world::ChannelWorld;

    #[test]
    fn state_is_channel_major_and_basis_contiguous() {
        let layout = StateLayout::new(2, 2, vec![BasisId(4), BasisId(9)], 3).unwrap();
        assert_eq!(layout.index(0, 0, 0, BasisId(4)), Some(0));
        assert_eq!(layout.index(0, 0, 0, BasisId(9)), Some(1));
        assert_eq!(layout.index(1, 0, 0, BasisId(4)), Some(8));
        assert_eq!(layout.index(2, 1, 1, BasisId(9)), Some(23));
        assert_eq!(layout.index(0, 0, 0, BasisId(7)), None);
    }

    #[test]
    fn two_basis_sparse_step_uses_independent_rulesets_and_source_basis_weights() {
        let mut spec = ExperimentSpec::single_channel_lenia(1, 1);
        spec.tiling = Some(build_preset(TilingPreset::EquilateralTriangles, 1.0));
        let mut spec = spec.normalize_rules().unwrap();
        spec.channels[0].initial = vec![0.25, 0.75];
        let output = ChannelId(0);
        let shared = spec.rules.defaults[&output];
        let rule = spec.rules.get_mut(shared).unwrap();
        rule.growth.mode = UpdateMode::DirectUpdate;
        rule.growth.source = "potential".into();
        rule.kernels[0].spatial = KernelSpatialDefinition::Periodic(PeriodicKernelDefinition {
            width: 1,
            height: 1,
            anchor_x: 0,
            anchor_y: 0,
            planes: [
                (
                    BasisId(0),
                    BasisWeightPlane {
                        values: vec![0.0],
                        mask: None,
                    },
                ),
                (
                    BasisId(1),
                    BasisWeightPlane {
                        values: vec![1.0],
                        mask: None,
                    },
                ),
            ]
            .into(),
        });
        let second = spec
            .rules
            .detach(BindingKey {
                basis: BasisId(1),
                output,
            })
            .unwrap();
        let rule = spec.rules.get_mut(second).unwrap();
        rule.growth.source = "potential * 0.5".into();
        let KernelSpatialDefinition::Periodic(definition) = &mut rule.kernels[0].spatial else {
            unreachable!();
        };
        definition.planes.get_mut(&BasisId(0)).unwrap().values[0] = 1.0;
        definition.planes.get_mut(&BasisId(1)).unwrap().values[0] = 0.0;

        let compiled = compile_basis_experiment(&spec).unwrap();
        let mut world = ChannelWorld::from_basis_channels(1, 1, 2, &[vec![0.25, 0.75]]).unwrap();
        compiled.step(&mut world).unwrap();

        assert!((world.get_basis(0, 0, 0, 0) - 0.75).abs() < 1e-6);
        assert!((world.get_basis(0, 0, 0, 1) - 0.125).abs() < 1e-6);
    }
}
