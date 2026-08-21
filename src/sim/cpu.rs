use std::collections::BTreeMap;

use crate::sim::expression::{ExpressionContext, KernelExpressionError, evaluate};
use crate::sim::program::RuleProgram;
use crate::sim::rule::{Rule, SimulationSpec};
use crate::sim::world::{ChannelWorld, World};

pub use crate::sim::runtime::CpuExperimentBackend;

pub struct CpuBackend {
    spec: SimulationSpec,
    tick: u64,
}

impl CpuBackend {
    pub fn new(spec: SimulationSpec) -> Self {
        Self { spec, tick: 0 }
    }

    pub fn spec(&self) -> &SimulationSpec {
        &self.spec
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn step(&mut self, world: &mut World) -> Result<(), KernelExpressionError> {
        match &self.spec.rule {
            Rule::Conway => self.step_conway(world),
            Rule::Lenia { mu, sigma } => self.step_lenia(world, *mu, *sigma)?,
            Rule::Program(program) => self.step_program(world, program)?,
        }
        self.tick += 1;
        Ok(())
    }

    pub fn step_channels(&mut self, world: &mut ChannelWorld) -> Result<(), KernelExpressionError> {
        let Rule::Program(program) = &self.spec.rule else {
            return Err(KernelExpressionError::NonFinite);
        };
        let mut values = BTreeMap::new();
        for y in 0..world.height() as isize {
            for x in 0..world.width() as isize {
                program.populate_channel_inputs(world, x, y, &mut values);
                let update = evaluate(
                    &program.update,
                    &ExpressionContext {
                        x: 0.0,
                        y: 0.0,
                        radius: 0.0,
                        distance: 0.0,
                        parameters: &values,
                    },
                )?;
                let current = world.get(0, x, y);
                world.set_next(0, x, y, (current + self.spec.dt * update).clamp(0.0, 1.0));
            }
        }
        world.swap_buffers();
        self.tick += 1;
        Ok(())
    }

    fn step_program(
        &self,
        world: &mut World,
        program: &RuleProgram,
    ) -> Result<(), KernelExpressionError> {
        let mut values = BTreeMap::new();
        for y in 0..world.height() as isize {
            for x in 0..world.width() as isize {
                program.populate_inputs(world, x, y, &mut values);
                let update = evaluate(
                    &program.update,
                    &ExpressionContext {
                        x: 0.0,
                        y: 0.0,
                        radius: 0.0,
                        distance: 0.0,
                        parameters: &values,
                    },
                )?;
                world.set_next(
                    x,
                    y,
                    (world.get(x, y) + self.spec.dt * update).clamp(0.0, 1.0),
                );
            }
        }
        world.swap_buffers();
        Ok(())
    }

    fn step_conway(&self, world: &mut World) {
        for y in 0..world.height() as isize {
            for x in 0..world.width() as isize {
                let mut neighbors = 0_u32;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if dx != 0 || dy != 0 {
                            neighbors += u32::from(world.get(x + dx, y + dy) > 0.5);
                        }
                    }
                }
                let alive = world.get(x, y) > 0.5;
                let survives = alive && (neighbors == 2 || neighbors == 3);
                let born = !alive && neighbors == 3;
                world.set_next(x, y, f32::from(survives || born));
            }
        }
        world.swap_buffers();
    }

    fn step_lenia(
        &self,
        world: &mut World,
        mu: f32,
        sigma: f32,
    ) -> Result<(), KernelExpressionError> {
        let growth_expression = self
            .spec
            .growth_expression()
            .expect("continuous rules always have a validated growth expression");
        let mut parameters = BTreeMap::from([
            ("mu".to_string(), mu),
            ("potential".to_string(), 0.0),
            ("sigma".to_string(), sigma),
        ]);
        for y in 0..world.height() as isize {
            for x in 0..world.width() as isize {
                let mut potential = 0.0;
                for kernel_y in 0..self.spec.kernel.height {
                    for kernel_x in 0..self.spec.kernel.width {
                        let kernel_index = kernel_y * self.spec.kernel.width + kernel_x;
                        if self
                            .spec
                            .kernel
                            .mask
                            .as_ref()
                            .is_none_or(|mask| mask[kernel_index])
                        {
                            let offset_x = kernel_x as isize - self.spec.kernel.anchor_x as isize;
                            let offset_y = kernel_y as isize - self.spec.kernel.anchor_y as isize;
                            potential += self.spec.kernel.values[kernel_index]
                                * world.get(x + offset_x, y + offset_y);
                        }
                    }
                }
                *parameters
                    .get_mut("potential")
                    .expect("the potential input was initialized") = potential;
                let growth = evaluate(
                    growth_expression,
                    &ExpressionContext {
                        x: 0.0,
                        y: 0.0,
                        radius: 0.0,
                        distance: 0.0,
                        parameters: &parameters,
                    },
                )?;
                let next = (world.get(x, y) + self.spec.dt * growth).clamp(0.0, 1.0);
                world.set_next(x, y, next);
            }
        }
        world.swap_buffers();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::kernel::{KernelDefinition, KernelValues, Normalization};
    use crate::sim::world::World;
    use std::collections::BTreeMap;

    fn kernel_spec(definition: KernelDefinition) -> SimulationSpec {
        SimulationSpec {
            rule: Rule::Lenia {
                mu: 0.5,
                sigma: 1.0,
            },
            kernel: definition.build().expect("test kernel is valid"),
            dt: 0.1,
            growth: SimulationSpec::lenia_orbium().growth,
        }
    }

    fn centered_square_kernel() -> KernelDefinition {
        KernelDefinition {
            name: "centered-square".to_string(),
            width: 3,
            height: 3,
            anchor_x: 1,
            anchor_y: 1,
            mask: None,
            normalization: Normalization::Sum,
            parameters: BTreeMap::new(),
            values: KernelValues::Explicit(vec![1.0; 9]),
        }
    }

    fn non_square_kernel() -> KernelDefinition {
        KernelDefinition {
            name: "non-square".to_string(),
            width: 5,
            height: 3,
            anchor_x: 2,
            anchor_y: 1,
            mask: None,
            normalization: Normalization::Sum,
            parameters: BTreeMap::new(),
            values: KernelValues::Explicit(vec![1.0; 15]),
        }
    }

    fn asymmetric_masked_kernel() -> KernelDefinition {
        KernelDefinition {
            name: "asymmetric-masked".to_string(),
            width: 3,
            height: 2,
            anchor_x: 2,
            anchor_y: 0,
            mask: Some(vec![false, true, true, false, true, false]),
            normalization: Normalization::Sum,
            parameters: BTreeMap::new(),
            values: KernelValues::Explicit(vec![0.0, 0.5, 1.0, 0.0, 2.0, 0.0]),
        }
    }

    fn unnormalized_kernel() -> KernelDefinition {
        KernelDefinition {
            name: "unnormalized".to_string(),
            width: 3,
            height: 1,
            anchor_x: 1,
            anchor_y: 0,
            mask: None,
            normalization: Normalization::None,
            parameters: BTreeMap::new(),
            values: KernelValues::Explicit(vec![1.0, 2.0, 3.0]),
        }
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 1e-6,
            "actual {actual} differed from expected {expected}"
        );
    }

    #[test]
    fn conway_blinker_oscillates_and_counts_ticks() {
        let mut world = World::new(5, 5);
        world.set(2, 1, 1.0);
        world.set(2, 2, 1.0);
        world.set(2, 3, 1.0);
        let mut backend = CpuBackend::new(SimulationSpec::conway());

        backend.step(&mut world).unwrap();
        assert_eq!(world.get(1, 2), 1.0);
        assert_eq!(world.get(2, 2), 1.0);
        assert_eq!(world.get(3, 2), 1.0);
        assert_eq!(backend.tick(), 1);

        backend.step(&mut world).unwrap();
        assert_eq!(world.get(2, 1), 1.0);
        assert_eq!(world.get(2, 2), 1.0);
        assert_eq!(world.get(2, 3), 1.0);
    }

    #[test]
    fn conway_survival_and_birth_follow_eight_neighbors() {
        let mut world = World::new(7, 7);
        world.set(3, 3, 1.0);
        world.set(2, 3, 1.0);
        world.set(4, 3, 1.0);
        let mut backend = CpuBackend::new(SimulationSpec::conway());
        backend.step(&mut world).unwrap();

        assert_eq!(world.get(3, 3), 1.0);
        assert_eq!(world.get(3, 4), 1.0);
        assert_eq!(world.get(2, 3), 0.0);
    }

    #[test]
    fn lenia_step_keeps_state_continuous_and_bounded() {
        let mut world = World::new(32, 32);
        world.randomize(123, 0.25);
        let mut backend = CpuBackend::new(SimulationSpec::lenia_orbium());

        backend.step(&mut world).unwrap();
        assert_eq!(backend.tick(), 1);
        assert!(
            world
                .cells()
                .iter()
                .all(|value| (0.0..=1.0).contains(value))
        );
        assert!(world.cells().iter().any(|value| *value > 0.0));
    }

    #[test]
    fn edited_growth_expression_is_evaluated_by_the_cpu_backend() {
        let spec = SimulationSpec::lenia_orbium()
            .with_growth_expression("0.5")
            .unwrap();
        let mut world = World::new(3, 2);
        world.replace_cells(&[0.2; 6]);
        let mut backend = CpuBackend::new(spec);

        backend.step(&mut world).unwrap();

        assert!(
            world
                .cells()
                .iter()
                .all(|value| (*value - 0.25).abs() < 1e-6)
        );
        assert_eq!(backend.tick(), 1);
    }

    #[test]
    fn invalid_runtime_growth_does_not_commit_the_step() {
        let spec = SimulationSpec::lenia_orbium()
            .with_growth_expression("1 / (potential - potential)")
            .unwrap();
        let mut world = World::new(3, 2);
        world.replace_cells(&[0.2; 6]);
        let before = world.cells().to_vec();
        let mut backend = CpuBackend::new(spec);

        assert!(backend.step(&mut world).is_err());

        assert_eq!(world.cells(), before);
        assert_eq!(backend.tick(), 0);
    }

    #[test]
    fn rule_is_available_without_exposing_backend_internals() {
        let spec = SimulationSpec::conway();
        let backend = CpuBackend::new(spec.clone());
        assert!(matches!(backend.spec().rule, Rule::Conway));
    }

    #[test]
    fn centered_square_kernel_uses_anchor_offsets() {
        let mut world = World::new(6, 5);
        world.set(2, 1, 1.0);
        let mut backend = CpuBackend::new(kernel_spec(centered_square_kernel()));

        backend.step(&mut world).unwrap();

        assert_close(world.get(2, 1), 1.0);
        assert_close(world.get(1, 1), 0.071_929_2);
        assert_close(world.get(3, 1), 0.071_929_2);
        assert_close(world.get(2, 0), 0.071_929_2);
        assert_close(world.get(2, 2), 0.071_929_2);
        assert_close(world.get(0, 0), 0.055_760_2);
    }

    #[test]
    fn non_square_kernel_traverses_actual_width_and_height() {
        let mut world = World::new(7, 5);
        world.set(2, 1, 1.0);
        let mut backend = CpuBackend::new(kernel_spec(non_square_kernel()));

        backend.step(&mut world).unwrap();

        assert_close(world.get(2, 1), 1.0);
        assert_close(world.get(0, 1), 0.065_759_8);
        assert_close(world.get(4, 1), 0.065_759_8);
        assert_close(world.get(2, 0), 0.065_759_8);
        assert_close(world.get(2, 2), 0.065_759_8);
        assert_close(world.get(2, 4), 0.055_760_2);
    }

    #[test]
    fn asymmetric_masked_kernel_skips_masked_offsets() {
        let mut world = World::new(6, 4);
        world.set(2, 1, 1.0);
        let mut backend = CpuBackend::new(kernel_spec(asymmetric_masked_kernel()));

        backend.step(&mut world).unwrap();

        assert_close(world.get(3, 1), 0.076_049_7);
        assert_close(world.get(2, 1), 1.0);
        assert_close(world.get(3, 0), 0.098_982_2);
        assert_close(world.get(2, 2), 0.055_760_2);
        assert_close(world.get(1, 1), 0.055_760_2);
    }

    #[test]
    fn unnormalized_kernel_preserves_explicit_values() {
        let mut world = World::new(6, 4);
        world.set(2, 1, 1.0);
        let mut backend = CpuBackend::new(kernel_spec(unnormalized_kernel()));

        backend.step(&mut world).unwrap();

        assert_close(world.get(1, 1), 0.0);
        assert_close(world.get(2, 1), 0.921_079_9);
        assert_close(world.get(3, 1), 0.055_760_2);
        assert_close(world.get(3, 0), 0.055_760_2);
    }
}
