use rand::{Rng, SeedableRng, rngs::StdRng};

pub struct World {
    width: usize,
    height: usize,
    current: Vec<f32>,
    next: Vec<f32>,
}

pub struct ChannelWorld {
    width: usize,
    height: usize,
    channels: usize,
    current: Vec<f32>,
    next: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ChannelWorldError {
    #[error("channel-world dimensions and channel count must be positive")]
    InvalidDimensions,
    #[error("channel {channel} has {actual} cells; expected {expected}")]
    InvalidChannelLength {
        channel: usize,
        expected: usize,
        actual: usize,
    },
    #[error("replacement has {actual} cells; expected {expected}")]
    InvalidReplacementLength { expected: usize, actual: usize },
    #[error("channel-world state contains a non-finite value")]
    NonFiniteState,
}

impl ChannelWorld {
    pub fn new(width: usize, height: usize, channels: usize) -> Self {
        assert!(
            width > 0 && height > 0 && channels > 0,
            "world dimensions must be positive"
        );
        let len = width * height * channels;
        Self {
            width,
            height,
            channels,
            current: vec![0.0; len],
            next: vec![0.0; len],
        }
    }

    pub fn from_channels(
        width: usize,
        height: usize,
        channels: &[Vec<f32>],
    ) -> Result<Self, ChannelWorldError> {
        if width == 0 || height == 0 || channels.is_empty() {
            return Err(ChannelWorldError::InvalidDimensions);
        }
        let channel_len = width
            .checked_mul(height)
            .ok_or(ChannelWorldError::InvalidDimensions)?;
        let total_len = channel_len
            .checked_mul(channels.len())
            .ok_or(ChannelWorldError::InvalidDimensions)?;
        let mut current = Vec::with_capacity(total_len);
        for (channel, values) in channels.iter().enumerate() {
            if values.len() != channel_len {
                return Err(ChannelWorldError::InvalidChannelLength {
                    channel,
                    expected: channel_len,
                    actual: values.len(),
                });
            }
            if values.iter().any(|value| !value.is_finite()) {
                return Err(ChannelWorldError::NonFiniteState);
            }
            current.extend_from_slice(values);
        }
        Ok(Self {
            width,
            height,
            channels: channels.len(),
            current,
            next: vec![0.0; total_len],
        })
    }

    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }
    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn get(&self, channel: usize, x: isize, y: isize) -> f32 {
        self.current[self.index(channel, x, y)]
    }

    pub fn set(&mut self, channel: usize, x: isize, y: isize, value: f32) {
        let index = self.index(channel, x, y);
        self.current[index] = value;
        self.next[index] = 0.0;
    }

    pub fn set_next(&mut self, channel: usize, x: isize, y: isize, value: f32) {
        let index = self.index(channel, x, y);
        self.next[index] = value;
    }

    pub fn swap_buffers(&mut self) {
        std::mem::swap(&mut self.current, &mut self.next);
    }

    pub fn channel_cells(&self, channel: usize) -> &[f32] {
        let range = self.channel_range(channel);
        &self.current[range]
    }

    pub fn replace_channel(&mut self, channel: usize, values: &[f32]) {
        assert_eq!(values.len(), self.width * self.height);
        let range = self.channel_range(channel);
        self.current[range.clone()].copy_from_slice(values);
        self.next[range].fill(0.0);
    }

    pub fn cells(&self) -> &[f32] {
        &self.current
    }

    pub fn next_cells_mut(&mut self) -> &mut [f32] {
        &mut self.next
    }

    pub fn replace_all(&mut self, values: &[f32]) -> Result<(), ChannelWorldError> {
        if values.len() != self.current.len() {
            return Err(ChannelWorldError::InvalidReplacementLength {
                expected: self.current.len(),
                actual: values.len(),
            });
        }
        if values.iter().any(|value| !value.is_finite()) {
            return Err(ChannelWorldError::NonFiniteState);
        }
        self.current.copy_from_slice(values);
        self.next.fill(0.0);
        Ok(())
    }

    pub fn discard_next(&mut self) {
        self.next.fill(0.0);
    }

    fn index(&self, channel: usize, x: isize, y: isize) -> usize {
        assert!(channel < self.channels, "channel index is out of range");
        let width = self.width as isize;
        let height = self.height as isize;
        let wrapped_x = ((x % width) + width) % width;
        let wrapped_y = ((y % height) + height) % height;
        channel * self.width * self.height + (wrapped_y * width + wrapped_x) as usize
    }

    fn channel_range(&self, channel: usize) -> std::ops::Range<usize> {
        assert!(channel < self.channels, "channel index is out of range");
        let start = channel * self.width * self.height;
        start..start + self.width * self.height
    }
}

// Multi-channel simulation state lives alongside the legacy scalar world.
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

    #[test]
    fn channel_world_keeps_independent_periodic_buffers() {
        let mut world = ChannelWorld::new(3, 2, 2);
        world.set(1, -1, 2, 0.75);
        assert_eq!(world.get(1, 2, 0), 0.75);
        assert_eq!(world.get(0, 2, 0), 0.0);
        world.set_next(0, 0, 0, 0.4);
        world.set_next(1, -1, 2, 0.75);
        world.swap_buffers();
        assert_eq!(world.get(0, 0, 0), 0.4);
        assert_eq!(world.get(1, 2, 0), 0.75);
    }
}
