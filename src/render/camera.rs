#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    center: [f32; 2],
    zoom: f32,
}

impl Camera {
    pub fn new(center: [f32; 2], zoom: f32) -> Self {
        assert!(
            zoom.is_finite() && zoom > 0.0,
            "zoom must be finite and positive"
        );
        Self { center, zoom }
    }

    pub fn center(&self) -> [f32; 2] {
        self.center
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn world_to_screen(&self, world: [f32; 2], width: usize, height: usize) -> [f32; 2] {
        [
            (world[0] - self.center[0]) * self.zoom + width as f32 / 2.0,
            (world[1] - self.center[1]) * self.zoom + height as f32 / 2.0,
        ]
    }

    pub fn screen_to_world(&self, screen: [f32; 2], width: usize, height: usize) -> [f32; 2] {
        [
            (screen[0] - width as f32 / 2.0) / self.zoom + self.center[0],
            (screen[1] - height as f32 / 2.0) / self.zoom + self.center[1],
        ]
    }

    pub fn pan_screen(&mut self, delta: [f32; 2]) {
        self.center[0] -= delta[0] / self.zoom;
        self.center[1] -= delta[1] / self.zoom;
    }

    pub fn zoom_at(&mut self, screen: [f32; 2], width: usize, height: usize, factor: f32) {
        let anchored = self.screen_to_world(screen, width, height);
        self.zoom = (self.zoom * factor).clamp(0.1, 128.0);
        self.center[0] = anchored[0] - (screen[0] - width as f32 / 2.0) / self.zoom;
        self.center[1] = anchored[1] - (screen[1] - height as f32 / 2.0) / self.zoom;
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_between_screen_and_world_coordinates() {
        let camera = Camera::new([4.0, 2.0], 2.0);
        assert_eq!(camera.zoom(), 2.0);
        assert_eq!(camera.center(), [4.0, 2.0]);

        let screen = camera.world_to_screen([5.0, 3.0], 20, 12);
        assert_eq!(screen, [12.0, 8.0]);
        assert_eq!(camera.screen_to_world([12.0, 8.0], 20, 12), [5.0, 3.0]);
    }

    #[test]
    fn zoom_keeps_the_screen_point_stable() {
        let mut camera = Camera::new([10.0, 10.0], 2.0);
        let world_point = camera.screen_to_world([15.0, 7.0], 20, 12);
        camera.zoom_at([15.0, 7.0], 20, 12, 1.5);

        assert_eq!(camera.zoom(), 3.0);
        assert_eq!(camera.screen_to_world([15.0, 7.0], 20, 12), world_point);
    }

    #[test]
    fn pan_moves_center_in_screen_pixels() {
        let mut camera = Camera::new([10.0, 10.0], 2.0);
        camera.pan_screen([4.0, -6.0]);
        assert_eq!(camera.center(), [8.0, 13.0]);
    }
}
