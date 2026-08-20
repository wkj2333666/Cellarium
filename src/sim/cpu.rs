use crate::sim::rule::{Rule, SimulationSpec};
use crate::sim::world::World;

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

    pub fn step(&mut self, world: &mut World) {
        match self.spec.rule {
            Rule::Conway => self.step_conway(world),
            Rule::Lenia { mu, sigma } => self.step_lenia(world, mu, sigma),
        }
        self.tick += 1;
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

    fn step_lenia(&self, world: &mut World, mu: f32, sigma: f32) {
        let radius = self.spec.kernel.radius as isize;

        for y in 0..world.height() as isize {
            for x in 0..world.width() as isize {
                let mut potential = 0.0;
                let mut kernel_index = 0;
                for kernel_y in -radius..=radius {
                    for kernel_x in -radius..=radius {
                        potential += self.spec.kernel.values[kernel_index]
                            * world.get(x + kernel_x, y + kernel_y);
                        kernel_index += 1;
                    }
                }
                let ratio = (potential - mu) / sigma;
                let growth = 2.0 * (-(ratio * ratio)).exp() - 1.0;
                let next = (world.get(x, y) + self.spec.dt * growth).clamp(0.0, 1.0);
                world.set_next(x, y, next);
            }
        }
        world.swap_buffers();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::world::World;

    #[test]
    fn conway_blinker_oscillates_and_counts_ticks() {
        let mut world = World::new(5, 5);
        world.set(2, 1, 1.0);
        world.set(2, 2, 1.0);
        world.set(2, 3, 1.0);
        let mut backend = CpuBackend::new(SimulationSpec::conway());

        backend.step(&mut world);
        assert_eq!(world.get(1, 2), 1.0);
        assert_eq!(world.get(2, 2), 1.0);
        assert_eq!(world.get(3, 2), 1.0);
        assert_eq!(backend.tick(), 1);

        backend.step(&mut world);
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
        backend.step(&mut world);

        assert_eq!(world.get(3, 3), 1.0);
        assert_eq!(world.get(3, 4), 1.0);
        assert_eq!(world.get(2, 3), 0.0);
    }

    #[test]
    fn lenia_step_keeps_state_continuous_and_bounded() {
        let mut world = World::new(32, 32);
        world.randomize(123, 0.25);
        let mut backend = CpuBackend::new(SimulationSpec::lenia_orbium());

        backend.step(&mut world);
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
    fn rule_is_available_without_exposing_backend_internals() {
        let spec = SimulationSpec::conway();
        let backend = CpuBackend::new(spec.clone());
        assert!(matches!(backend.spec().rule, Rule::Conway));
    }
}
