use std::collections::{BTreeMap, BTreeSet};

use crate::sim::experiment_model::{ExperimentSpec, KernelId, UpdateMode, validate_structure};
use crate::sim::expression::{
    ExpressionContext, KernelExpression, KernelExpressionError, evaluate,
};
use crate::sim::kernel::{Kernel, KernelError};
use crate::sim::parser::{ParseError, parse_and_validate};
use crate::sim::topology::BoundarySpec;
use crate::sim::world::{ChannelWorld, ChannelWorldError};

#[derive(Clone, Debug)]
pub struct CompiledKernelInput {
    pub id: KernelId,
    pub symbol: String,
    pub source: usize,
    pub boundary_constant: f32,
    pub kernel: Kernel,
}

#[derive(Clone, Debug)]
pub struct CompiledChannelRule {
    pub target: usize,
    pub frozen: bool,
    pub mode: UpdateMode,
    pub inputs: Vec<CompiledKernelInput>,
    pub parameters: BTreeMap<String, f32>,
    pub update: KernelExpression,
    pub program: Option<crate::sim::growth::types::TypedProgram>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompiledBoundary {
    Open,
    Constant,
    Periodic,
    Clamp,
    Reflect,
}

#[derive(Clone, Debug)]
pub struct CompiledExperiment {
    pub width: usize,
    pub height: usize,
    pub simulation_dt: f32,
    pub boundary: CompiledBoundary,
    pub rules: Vec<CompiledChannelRule>,
    pub basis: Option<Box<crate::sim::basis_runtime::CompiledBasisExperiment>>,
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("invalid experiment model: {0}")]
    Model(String),
    #[error("invalid kernel: {0}")]
    Kernel(#[from] KernelError),
    #[error("invalid growth expression: {0}")]
    Parse(#[from] ParseError),
    #[error("growth expression failed during execution: {0}")]
    Expression(#[from] KernelExpressionError),
    #[error("invalid channel-world state: {0}")]
    World(#[from] ChannelWorldError),
    #[error("channel-world dimensions do not match compiled experiment")]
    WorldShape,
    #[error("compiled experiment produced a non-finite state")]
    NonFiniteState,
}

pub fn compile_experiment(spec: &ExperimentSpec) -> Result<CompiledExperiment, RuntimeError> {
    validate_structure(spec).map_err(|errors| {
        RuntimeError::Model(
            errors
                .into_iter()
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        )
    })?;

    let (width, height, boundary) = match &spec.geometry {
        crate::sim::experiment_model::GeometrySpec::RasterGrid(grid) => (
            grid.width as usize,
            grid.height as usize,
            match grid.boundary {
                BoundarySpec::Open => CompiledBoundary::Open,
                BoundarySpec::Constant(_) => CompiledBoundary::Constant,
                BoundarySpec::Periodic => CompiledBoundary::Periodic,
                BoundarySpec::Clamp => CompiledBoundary::Clamp,
                BoundarySpec::Reflect => CompiledBoundary::Reflect,
            },
        ),
    };
    let channel_indices = spec
        .channels
        .iter()
        .enumerate()
        .map(|(index, channel)| (channel.id, index))
        .collect::<BTreeMap<_, _>>();
    if !spec.rules.is_empty() {
        let basis = crate::sim::basis_runtime::compile_basis_experiment(spec)?;
        return Ok(CompiledExperiment {
            width,
            height,
            simulation_dt: spec.simulation_dt,
            boundary,
            rules: Vec::new(),
            basis: Some(Box::new(basis)),
        });
    }
    let channel_constants = spec
        .channels
        .iter()
        .map(|channel| (channel.id, channel.boundary_constant))
        .collect::<BTreeMap<_, _>>();
    let kernels = spec
        .kernels
        .iter()
        .map(|slot| {
            slot.definition
                .build()
                .map(|kernel| (slot.id, (slot, kernel)))
        })
        .collect::<Result<BTreeMap<_, _>, KernelError>>()?;
    let growth = spec
        .growth
        .iter()
        .map(|growth| (growth.target, growth))
        .collect::<BTreeMap<_, _>>();

    let mut rules = Vec::with_capacity(spec.channels.len());
    for (target, channel) in spec.channels.iter().enumerate() {
        if channel.frozen {
            rules.push(CompiledChannelRule {
                target,
                frozen: true,
                mode: UpdateMode::DirectUpdate,
                inputs: Vec::new(),
                parameters: BTreeMap::new(),
                update: KernelExpression::Constant(0.0),
                program: None,
            });
            continue;
        }
        let Some(growth) = growth.get(&channel.id) else {
            return Err(RuntimeError::Model(format!(
                "missing growth program for channel {:?}",
                channel.id
            )));
        };
        let mut inputs = Vec::with_capacity(growth.kernel_inputs.len());
        let mut symbols = BTreeSet::from(["self".to_string()]);
        for kernel_id in &growth.kernel_inputs {
            let Some((slot, kernel)) = kernels.get(kernel_id) else {
                return Err(RuntimeError::Model(format!(
                    "growth program for {:?} references missing kernel {:?}",
                    channel.id, kernel_id
                )));
            };
            let source = *channel_indices.get(&slot.source).ok_or_else(|| {
                RuntimeError::Model(format!("missing source channel {:?}", slot.source))
            })?;
            let boundary_constant = *channel_constants.get(&slot.source).unwrap_or(&0.0);
            symbols.insert(slot.symbol.clone());
            inputs.push(CompiledKernelInput {
                id: *kernel_id,
                symbol: slot.symbol.clone(),
                source,
                boundary_constant,
                kernel: kernel.clone(),
            });
        }
        symbols.extend(growth.parameters.keys().cloned());
        let typed_program = crate::sim::growth::typecheck::compile(
            &growth.source,
            &crate::sim::growth::types::ExternalSymbols {
                kernel_inputs: inputs.iter().map(|input| input.symbol.clone()).collect(),
                parameters: growth.parameters.keys().cloned().collect(),
            },
        )
        .map_err(|errors| {
            RuntimeError::Model(
                errors
                    .into_iter()
                    .map(|error| {
                        format!("{} at {}..{}", error.code, error.span.start, error.span.end)
                    })
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        })?;
        let update =
            parse_and_validate(&growth.source, &symbols).unwrap_or(KernelExpression::Constant(0.0));
        rules.push(CompiledChannelRule {
            target,
            frozen: channel.frozen,
            mode: growth.mode,
            inputs,
            parameters: growth.parameters.clone(),
            update,
            program: Some(typed_program),
        });
    }

    Ok(CompiledExperiment {
        width,
        height,
        simulation_dt: spec.simulation_dt,
        boundary,
        rules,
        basis: None,
    })
}

pub struct CpuExperimentBackend {
    compiled: CompiledExperiment,
    tick: u64,
}

impl CpuExperimentBackend {
    pub fn new(compiled: CompiledExperiment) -> Self {
        Self { compiled, tick: 0 }
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn compiled(&self) -> &CompiledExperiment {
        &self.compiled
    }

    pub fn step(&mut self, world: &mut ChannelWorld) -> Result<(), RuntimeError> {
        if let Some(basis) = &self.compiled.basis {
            basis.step(world)?;
            self.tick += 1;
            return Ok(());
        }
        if world.width() != self.compiled.width
            || world.height() != self.compiled.height
            || world.channels() != self.compiled.rules.len()
        {
            return Err(RuntimeError::WorldShape);
        }
        let old = world.cells().to_vec();
        let mut values = BTreeMap::new();
        {
            let next = world.next_cells_mut();
            for rule in &self.compiled.rules {
                let target_start = rule.target * self.compiled.width * self.compiled.height;
                if rule.frozen {
                    next[target_start..target_start + self.compiled.width * self.compiled.height]
                        .copy_from_slice(
                            &old[target_start
                                ..target_start + self.compiled.width * self.compiled.height],
                        );
                    continue;
                }
                for y in 0..self.compiled.height as isize {
                    for x in 0..self.compiled.width as isize {
                        values.clear();
                        values.extend(
                            rule.parameters
                                .iter()
                                .map(|(name, value)| (name.clone(), *value)),
                        );
                        let current =
                            old[target_start + y as usize * self.compiled.width + x as usize];
                        values.insert("self".to_string(), current);
                        for input in &rule.inputs {
                            let value = convolve(
                                &old,
                                self.compiled.width,
                                self.compiled.height,
                                x,
                                y,
                                input,
                                self.compiled.boundary,
                            );
                            values.insert(input.symbol.clone(), value);
                        }
                        let result: Result<f32, RuntimeError> = if let Some(program) = &rule.program
                        {
                            let inputs = crate::sim::growth::eval::ScalarInputs {
                                kernel_inputs: rule
                                    .inputs
                                    .iter()
                                    .map(|input| values.get(&input.symbol).copied().unwrap_or(0.0))
                                    .collect(),
                                self_value: current,
                                parameters: rule.parameters.clone(),
                            };
                            crate::sim::growth::eval::evaluate(program, &inputs)
                                .map_err(|error| RuntimeError::Model(error.to_string()))
                        } else {
                            evaluate(
                                &rule.update,
                                &ExpressionContext {
                                    x: 0.0,
                                    y: 0.0,
                                    radius: 0.0,
                                    distance: 0.0,
                                    parameters: &values,
                                },
                            )
                            .map_err(RuntimeError::Expression)
                        };
                        let result = match result {
                            Ok(result) => result,
                            Err(error) => {
                                world.discard_next();
                                return Err(error);
                            }
                        };
                        let next_value = match rule.mode {
                            UpdateMode::GrowthRate => {
                                (current + self.compiled.simulation_dt * result).clamp(0.0, 1.0)
                            }
                            UpdateMode::DirectUpdate => result.clamp(0.0, 1.0),
                        };
                        if !next_value.is_finite() {
                            world.discard_next();
                            return Err(RuntimeError::NonFiniteState);
                        }
                        next[target_start + y as usize * self.compiled.width + x as usize] =
                            next_value;
                    }
                }
            }
        }
        world.swap_buffers();
        self.tick += 1;
        Ok(())
    }
}

fn convolve(
    old: &[f32],
    width: usize,
    height: usize,
    x: isize,
    y: isize,
    input: &CompiledKernelInput,
    boundary: CompiledBoundary,
) -> f32 {
    let mut result = 0.0;
    for kernel_y in 0..input.kernel.height {
        for kernel_x in 0..input.kernel.width {
            let index = kernel_y * input.kernel.width + kernel_x;
            let offset_x = kernel_x as isize - input.kernel.anchor_x as isize;
            let offset_y = kernel_y as isize - input.kernel.anchor_y as isize;
            let sample = sample(
                old,
                width,
                height,
                input.source,
                x + offset_x,
                y + offset_y,
                input.boundary_constant,
                boundary,
            );
            result += input.kernel.values[index] * sample;
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn sample(
    old: &[f32],
    width: usize,
    height: usize,
    channel: usize,
    x: isize,
    y: isize,
    boundary_constant: f32,
    boundary: CompiledBoundary,
) -> f32 {
    let (x, y) = match boundary {
        CompiledBoundary::Open => {
            if x < 0 || y < 0 || x >= width as isize || y >= height as isize {
                return 0.0;
            }
            (x as usize, y as usize)
        }
        CompiledBoundary::Constant => {
            if x < 0 || y < 0 || x >= width as isize || y >= height as isize {
                return boundary_constant;
            }
            (x as usize, y as usize)
        }
        CompiledBoundary::Periodic => (
            ((x % width as isize + width as isize) % width as isize) as usize,
            ((y % height as isize + height as isize) % height as isize) as usize,
        ),
        CompiledBoundary::Clamp => (
            x.clamp(0, width as isize - 1) as usize,
            y.clamp(0, height as isize - 1) as usize,
        ),
        CompiledBoundary::Reflect => (reflect(x, width) as usize, reflect(y, height) as usize),
    };
    old[channel * width * height + y * width + x]
}

fn reflect(value: isize, size: usize) -> isize {
    if size <= 1 {
        return 0;
    }
    let span = size as isize - 1;
    let period = span * 2;
    let folded = (value % period + period) % period;
    if folded <= span {
        folded
    } else {
        period - folded
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::CpuExperimentBackend;
    use super::*;
    use crate::sim::experiment_model::{
        ExperimentSpec, GrowthSource, KernelId, KernelSlot, UpdateMode,
    };
    use crate::sim::kernel::{KernelDefinition, KernelValues, Normalization};
    use crate::sim::topology::BoundarySpec;
    use crate::sim::world::ChannelWorld;

    fn routed_two_channel_fixture() -> ExperimentSpec {
        let mut spec = ExperimentSpec::single_channel_lenia(1, 1);
        spec.kernels.clear();
        spec.growth.clear();
        let first = spec.channels[0].id;
        let second = spec.add_channel("second", false);
        let from_second = KernelId(10);
        let from_first = KernelId(20);
        spec.kernels = vec![
            KernelSlot::identity(from_second, "from_second", second, first),
            KernelSlot::identity(from_first, "from_first", first, second),
        ];
        spec.growth = vec![
            GrowthSource {
                target: first,
                kernel_inputs: vec![from_second],
                parameters: BTreeMap::new(),
                source: "from_second".to_string(),
                mode: UpdateMode::DirectUpdate,
            },
            GrowthSource {
                target: second,
                kernel_inputs: vec![from_first],
                parameters: BTreeMap::new(),
                source: "from_first".to_string(),
                mode: UpdateMode::DirectUpdate,
            },
        ];
        spec
    }

    #[test]
    fn two_targets_receive_only_their_routed_kernel_inputs() {
        let spec = routed_two_channel_fixture();
        let compiled = compile_experiment(&spec).unwrap();
        let mut world = ChannelWorld::from_channels(1, 1, &[vec![0.25], vec![0.75]]).unwrap();
        CpuExperimentBackend::new(compiled)
            .step(&mut world)
            .unwrap();
        assert!((world.get(0, 0, 0) - 0.75).abs() < 1e-6);
        assert!((world.get(1, 0, 0) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn frozen_channel_is_copied_without_update() {
        let mut spec = ExperimentSpec::single_channel_lenia(1, 1);
        spec.kernels.clear();
        spec.growth.clear();
        let active = spec.channels[0].id;
        let frozen = spec.add_channel("environment", true);
        let signal = KernelId(8);
        spec.kernels = vec![KernelSlot::identity(signal, "signal", frozen, active)];
        spec.growth = vec![GrowthSource {
            target: active,
            kernel_inputs: vec![signal],
            parameters: BTreeMap::new(),
            source: "signal".to_string(),
            mode: UpdateMode::DirectUpdate,
        }];
        let compiled = compile_experiment(&spec).unwrap();
        let mut world = ChannelWorld::from_channels(1, 1, &[vec![0.0], vec![0.6]]).unwrap();
        let before = world.channel_cells(1).to_vec();

        CpuExperimentBackend::new(compiled)
            .step(&mut world)
            .unwrap();

        assert_eq!(world.channel_cells(1), before);
        assert_eq!(world.channel_cells(0), &[0.6]);
    }

    #[test]
    fn constant_boundary_samples_each_source_channels_own_constant() {
        let mut spec = ExperimentSpec::single_channel_lenia(1, 1);
        spec.kernels.clear();
        spec.growth.clear();
        spec.geometry = crate::sim::experiment_model::GeometrySpec::RasterGrid(
            crate::sim::experiment_model::GridGeometry {
                width: 1,
                height: 1,
                boundary: BoundarySpec::Constant(0.0),
            },
        );
        spec.channels[0].boundary_constant = 0.25;
        let first = spec.channels[0].id;
        let second = spec.add_channel("second", false);
        spec.channels[1].boundary_constant = 0.75;
        let left_only = |name: &str| KernelDefinition {
            name: name.to_string(),
            width: 2,
            height: 1,
            anchor_x: 1,
            anchor_y: 0,
            mask: None,
            normalization: Normalization::None,
            parameters: BTreeMap::new(),
            values: KernelValues::Explicit(vec![1.0, 0.0]),
        };
        spec.kernels = vec![
            KernelSlot {
                id: KernelId(1),
                symbol: "left_first".to_string(),
                name: "left first".to_string(),
                source: first,
                target: first,
                definition: left_only("left first"),
            },
            KernelSlot {
                id: KernelId(2),
                symbol: "left_second".to_string(),
                name: "left second".to_string(),
                source: second,
                target: second,
                definition: left_only("left second"),
            },
        ];
        spec.growth = vec![
            GrowthSource {
                target: first,
                kernel_inputs: vec![KernelId(1)],
                parameters: BTreeMap::new(),
                source: "left_first".to_string(),
                mode: UpdateMode::DirectUpdate,
            },
            GrowthSource {
                target: second,
                kernel_inputs: vec![KernelId(2)],
                parameters: BTreeMap::new(),
                source: "left_second".to_string(),
                mode: UpdateMode::DirectUpdate,
            },
        ];
        let compiled = compile_experiment(&spec).unwrap();
        let mut world = ChannelWorld::from_channels(1, 1, &[vec![0.0], vec![0.0]]).unwrap();

        CpuExperimentBackend::new(compiled)
            .step(&mut world)
            .unwrap();

        assert_eq!(world.get(0, 0, 0), 0.25);
        assert_eq!(world.get(1, 0, 0), 0.75);
    }

    #[test]
    fn failed_expression_does_not_leave_partial_next_state() {
        let mut spec = ExperimentSpec::single_channel_lenia(2, 1);
        spec.kernels.clear();
        spec.growth[0].kernel_inputs.clear();
        spec.growth[0].parameters.clear();
        spec.growth[0].source = "1 / (self - self)".to_string();
        spec.growth[0].mode = UpdateMode::DirectUpdate;
        let compiled = compile_experiment(&spec).unwrap();
        let mut world = ChannelWorld::from_channels(2, 1, &[vec![0.2, 0.4]]).unwrap();
        let before = world.cells().to_vec();

        assert!(
            CpuExperimentBackend::new(compiled)
                .step(&mut world)
                .is_err()
        );
        assert_eq!(world.cells(), before.as_slice());
        assert!(world.next_cells_mut().iter().all(|value| *value == 0.0));
    }

    #[test]
    fn structured_growth_program_runs_on_the_channel_runtime() {
        let mut spec = ExperimentSpec::single_channel_lenia(1, 1);
        spec.growth[0].source =
            "let doubled = self * 2.0; if doubled > 0.5 { doubled } else { 0.0 }".to_string();
        spec.growth[0].kernel_inputs.clear();
        spec.kernels.clear();
        spec.growth[0].mode = UpdateMode::DirectUpdate;
        let compiled = compile_experiment(&spec).unwrap();
        let mut world = ChannelWorld::from_channels(1, 1, &[vec![0.3]]).unwrap();
        CpuExperimentBackend::new(compiled)
            .step(&mut world)
            .unwrap();
        assert!((world.get(0, 0, 0) - 0.6).abs() < 1e-6);
    }
}
