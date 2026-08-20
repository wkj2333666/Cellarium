use rand::{Rng, SeedableRng, rngs::StdRng};

pub struct World {
    width: usize,
    height: usize,
    current: Vec<f32>,
    next: Vec<f32>,
}

impl World {
    pub fn new(width: usize, height: usize) -> Self {
        assert!(width > 0 && height > 0, "world dimensions must be positive");
        Self {
            width,
            height,
            current: vec![0.0; width * height],
            next: vec![0.0; width * height],
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn get(&self, x: isize, y: isize) -> f32 {
        self.current[self.wrap_index(x, y)]
    }

    pub fn set(&mut self, x: isize, y: isize, value: f32) {
        let index = self.wrap_index(x, y);
        self.current[index] = value;
        self.next[index] = 0.0;
    }

    pub fn set_next(&mut self, x: isize, y: isize, value: f32) {
        let index = self.wrap_index(x, y);
        self.next[index] = value;
    }

    pub fn swap_buffers(&mut self) {
        std::mem::swap(&mut self.current, &mut self.next);
    }

    pub fn cells(&self) -> &[f32] {
        &self.current
    }

    pub fn replace_cells(&mut self, values: &[f32]) {
        assert_eq!(
            values.len(),
            self.current.len(),
            "world replacement must contain every cell"
        );
        self.current.copy_from_slice(values);
        self.next.fill(0.0);
    }

    pub fn clear(&mut self) {
        self.current.fill(0.0);
        self.next.fill(0.0);
    }

    pub fn randomize(&mut self, seed: u64, density: f64) {
        let mut rng = StdRng::seed_from_u64(seed);
        for cell in self.current.iter_mut() {
            *cell = f32::from(rng.random_bool(density));
        }
        self.next.fill(0.0);
    }

    fn wrap_index(&self, x: isize, y: isize) -> usize {
        let width = self.width as isize;
        let height = self.height as isize;
        let wrapped_x = ((x % width) + width) % width;
        let wrapped_y = ((y % height) + height) % height;
        (wrapped_y * width + wrapped_x) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_continuous_state_with_periodic_access() {
        let mut world = World::new(3, 3);
        assert_eq!(world.width(), 3);
        assert_eq!(world.height(), 3);
        assert_eq!(world.get(0, 0), 0.0);

        world.set(3, 4, 0.75);
        assert_eq!(world.get(0, 1), 0.75);
        assert_eq!(world.get(-3, -2), 0.75);
    }

    #[test]
    fn randomize_is_deterministic_and_extreme_densities_are_stable() {
        let mut first = World::new(8, 8);
        let mut second = World::new(8, 8);

        first.randomize(42, 0.5);
        second.randomize(42, 0.5);
        assert_eq!(first.cells(), second.cells());

        first.randomize(7, 0.0);
        assert!(first.cells().iter().all(|value| *value == 0.0));
        first.randomize(7, 1.0);
        assert!(first.cells().iter().all(|value| *value == 1.0));
    }

    #[test]
    fn clear_resets_every_cell() {
        let mut world = World::new(4, 4);
        world.randomize(9, 1.0);
        world.clear();
        assert!(world.cells().iter().all(|value| *value == 0.0));
    }
}
