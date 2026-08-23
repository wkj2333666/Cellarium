use ratatui::layout::Rect;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneCamera {
    pub center: [f64; 2],
    pub pixels_per_unit: f64,
}

impl SceneCamera {
    pub fn new(center: [f64; 2], pixels_per_unit: f64) -> Self {
        Self {
            center,
            pixels_per_unit,
        }
    }

    fn is_valid(self) -> bool {
        self.center.into_iter().all(f64::is_finite)
            && self.pixels_per_unit.is_finite()
            && self.pixels_per_unit > 0.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneTransformError {
    EmptyPlacement,
    InvalidCamera,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneTransform {
    pub generation: u64,
    pub terminal_rect: Rect,
    pub pixel_size: [u32; 2],
    pub camera: SceneCamera,
}

impl SceneTransform {
    pub fn new(
        terminal_rect: Rect,
        pixel_size: [u32; 2],
        camera: SceneCamera,
        generation: u64,
    ) -> Result<Self, SceneTransformError> {
        if terminal_rect.is_empty() || pixel_size[0] == 0 || pixel_size[1] == 0 {
            return Err(SceneTransformError::EmptyPlacement);
        }
        if !camera.is_valid() {
            return Err(SceneTransformError::InvalidCamera);
        }
        Ok(Self {
            generation,
            terminal_rect,
            pixel_size,
            camera,
        })
    }

    pub fn accepts_generation(&self, generation: u64) -> bool {
        generation == self.generation
    }

    pub fn terminal_to_pixel(&self, terminal: [u16; 2]) -> Option<[f64; 2]> {
        if terminal[0] < self.terminal_rect.left()
            || terminal[0] >= self.terminal_rect.right()
            || terminal[1] < self.terminal_rect.top()
            || terminal[1] >= self.terminal_rect.bottom()
        {
            return None;
        }
        let x = f64::from(terminal[0] - self.terminal_rect.x) + 0.5;
        let y = f64::from(terminal[1] - self.terminal_rect.y) + 0.5;
        Some([
            x * f64::from(self.pixel_size[0]) / f64::from(self.terminal_rect.width),
            y * f64::from(self.pixel_size[1]) / f64::from(self.terminal_rect.height),
        ])
    }

    pub fn pixel_to_world(&self, pixel: [f64; 2]) -> Option<[f64; 2]> {
        if !self.contains_pixel(pixel) {
            return None;
        }
        Some([
            (pixel[0] - f64::from(self.pixel_size[0]) / 2.0) / self.camera.pixels_per_unit
                + self.camera.center[0],
            (pixel[1] - f64::from(self.pixel_size[1]) / 2.0) / self.camera.pixels_per_unit
                + self.camera.center[1],
        ])
    }

    pub fn world_to_pixel(&self, world: [f64; 2]) -> Option<[f64; 2]> {
        if !world.into_iter().all(f64::is_finite) {
            return None;
        }
        let pixel = [
            (world[0] - self.camera.center[0]) * self.camera.pixels_per_unit
                + f64::from(self.pixel_size[0]) / 2.0,
            (world[1] - self.camera.center[1]) * self.camera.pixels_per_unit
                + f64::from(self.pixel_size[1]) / 2.0,
        ];
        self.contains_pixel(pixel).then_some(pixel)
    }

    pub fn world_to_terminal(&self, world: [f64; 2]) -> Option<[u16; 2]> {
        let pixel = self.world_to_pixel(world)?;
        let column = (pixel[0] * f64::from(self.terminal_rect.width)
            / f64::from(self.pixel_size[0]))
        .floor() as u16;
        let row = (pixel[1] * f64::from(self.terminal_rect.height) / f64::from(self.pixel_size[1]))
            .floor() as u16;
        Some([
            self.terminal_rect.x.checked_add(column)?,
            self.terminal_rect.y.checked_add(row)?,
        ])
    }

    fn contains_pixel(&self, pixel: [f64; 2]) -> bool {
        pixel.into_iter().all(f64::is_finite)
            && pixel[0] >= 0.0
            && pixel[1] >= 0.0
            && pixel[0] < f64::from(self.pixel_size[0])
            && pixel[1] < f64::from(self.pixel_size[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    #[test]
    fn terminal_pixel_world_round_trip_survives_resize_and_pan() {
        let transform = SceneTransform::new(
            Rect::new(3, 2, 80, 40),
            [1280, 640],
            SceneCamera::new([2.0, -1.0], 3.5),
            7,
        )
        .unwrap();
        let terminal = [41, 19];
        let pixel = transform.terminal_to_pixel(terminal).unwrap();
        let world = transform.pixel_to_world(pixel).unwrap();
        assert_eq!(transform.world_to_terminal(world), Some(terminal));
    }

    #[test]
    fn transform_rejects_outside_and_stale_events() {
        let transform = SceneTransform::new(
            Rect::new(3, 2, 80, 40),
            [1280, 640],
            SceneCamera::new([0.0, 0.0], 1.0),
            7,
        )
        .unwrap();
        assert_eq!(transform.terminal_to_pixel([2, 2]), None);
        assert_eq!(transform.terminal_to_pixel([83, 42]), None);
        assert!(!transform.accepts_generation(6));
        assert!(transform.accepts_generation(7));
    }
}
