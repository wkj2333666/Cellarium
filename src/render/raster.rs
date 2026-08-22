use crate::render::camera::Camera;
use crate::sim::world::World;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb8 {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb8 {
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }
}

pub struct Framebuffer {
    width: usize,
    height: usize,
    pixels: Vec<Rgb8>,
}

impl Framebuffer {
    pub fn new(width: usize, height: usize) -> Self {
        assert!(
            width > 0 && height > 0,
            "framebuffer dimensions must be positive"
        );
        Self {
            width,
            height,
            pixels: vec![Rgb8::new(0, 0, 0); width * height],
        }
    }

    /// Reconfigure the framebuffer while retaining its pixel allocation when
    /// the output dimensions are unchanged. Rasterization overwrites every
    /// pixel, so an existing same-sized buffer does not need to be cleared.
    pub fn ensure_size(&mut self, width: usize, height: usize) {
        assert!(
            width > 0 && height > 0,
            "framebuffer dimensions must be positive"
        );
        if self.width == width && self.height == height {
            return;
        }
        self.width = width;
        self.height = height;
        self.pixels.resize(width * height, Rgb8::new(0, 0, 0));
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn get(&self, x: usize, y: usize) -> Rgb8 {
        self.pixels[y * self.width + x]
    }

    pub fn set(&mut self, x: usize, y: usize, color: Rgb8) {
        if x < self.width && y < self.height {
            self.pixels[y * self.width + x] = color;
        }
    }
}

pub fn rasterize_world(world: &World, camera: &Camera, width: usize, height: usize) -> Framebuffer {
    let mut frame = Framebuffer::new(width, height);
    rasterize_world_into(world, camera, &mut frame);
    frame
}

pub fn rasterize_world_into(world: &World, camera: &Camera, frame: &mut Framebuffer) {
    let _ = rasterize_world_into_while(world, camera, frame, || true);
}

pub fn rasterize_world_into_while(
    world: &World,
    camera: &Camera,
    frame: &mut Framebuffer,
    mut keep_rendering: impl FnMut() -> bool,
) -> bool {
    let width = frame.width;
    let height = frame.height;
    // Integer zooms keep crisp cell edges; fractional zooms need the whole
    // output-pixel footprint to avoid nearest-neighbor shape aliasing.
    let use_coverage = (camera.zoom() - camera.zoom().round()).abs() > 1.0e-6;
    for y in 0..height {
        if !keep_rendering() {
            return false;
        }
        for x in 0..width {
            let screen = [x as f32 + 0.5, y as f32 + 0.5];
            let value = if use_coverage {
                sample_world_coverage(world, camera, screen, width, height)
            } else {
                let world_position = camera.screen_to_world(screen, width, height);
                let sample_x = world_position[0].floor().rem_euclid(world.width() as f32) as usize;
                let sample_y = world_position[1].floor().rem_euclid(world.height() as f32) as usize;
                world.get(sample_x as isize, sample_y as isize)
            };
            frame.set(x, y, value_to_rgb(value));
        }
    }
    true
}

fn sample_world_coverage(
    world: &World,
    camera: &Camera,
    screen: [f32; 2],
    width: usize,
    height: usize,
) -> f32 {
    let center = camera.screen_to_world(screen, width, height);
    let half_extent = 0.5 / camera.zoom();
    let left = center[0] - half_extent;
    let right = center[0] + half_extent;
    let top = center[1] - half_extent;
    let bottom = center[1] + half_extent;
    let x_start = left.floor() as isize;
    let x_end = right.ceil() as isize;
    let y_start = top.floor() as isize;
    let y_end = bottom.ceil() as isize;

    let mut weighted = 0.0;
    let mut covered = 0.0;
    for y in y_start..y_end {
        let y_overlap = (bottom.min(y as f32 + 1.0) - top.max(y as f32)).max(0.0);
        if y_overlap == 0.0 {
            continue;
        }
        for x in x_start..x_end {
            let x_overlap = (right.min(x as f32 + 1.0) - left.max(x as f32)).max(0.0);
            if x_overlap == 0.0 {
                continue;
            }
            let weight = x_overlap * y_overlap;
            weighted += world.get(x, y) * weight;
            covered += weight;
        }
    }

    if covered > 0.0 {
        weighted / covered
    } else {
        world.get(center[0].floor() as isize, center[1].floor() as isize)
    }
}

pub fn value_to_rgb(value: f32) -> Rgb8 {
    let value = value.clamp(0.0, 1.0);
    let (low, high, phase) = if value < 0.5 {
        (Rgb8::new(8, 12, 24), Rgb8::new(58, 92, 168), value / 0.5)
    } else {
        (
            Rgb8::new(58, 92, 168),
            Rgb8::new(255, 238, 170),
            (value - 0.5) / 0.5,
        )
    };
    Rgb8::new(
        interpolate(low.red, high.red, phase),
        interpolate(low.green, high.green, phase),
        interpolate(low.blue, high.blue, phase),
    )
}

fn interpolate(low: u8, high: u8, phase: f32) -> u8 {
    (low as f32 + phase * (high as f32 - low as f32)).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::camera::Camera;

    #[test]
    fn rasterizes_world_to_requested_framebuffer_size() {
        let mut world = World::new(4, 4);
        world.set(2, 1, 1.0);
        let camera = Camera::new([1.5, 1.5], 1.0);

        let frame = rasterize_world(&world, &camera, 4, 6);
        assert_eq!(frame.width(), 4);
        assert_eq!(frame.height(), 6);
        assert_ne!(frame.get(2, 2), frame.get(0, 0));
        assert_eq!(frame.get(2, 2), value_to_rgb(1.0));
    }

    #[test]
    fn rasterizes_into_reusable_framebuffer_storage() {
        let mut world = World::new(2, 2);
        world.set(0, 0, 1.0);
        let camera = Camera::new([0.5, 0.5], 1.0);
        let mut frame = Framebuffer::new(2, 2);

        rasterize_world_into(&world, &camera, &mut frame);

        assert_eq!(frame.get(0, 0), value_to_rgb(1.0));
    }

    #[test]
    fn ensure_size_reuses_storage_for_same_dimensions() {
        let mut frame = Framebuffer::new(2, 2);
        let allocation = frame.pixels.as_ptr();

        frame.ensure_size(2, 2);

        assert_eq!(frame.pixels.as_ptr(), allocation);
        assert_eq!(frame.width(), 2);
        assert_eq!(frame.height(), 2);
    }

    #[test]
    fn cancellable_rasterization_stops_before_obsolete_rows() {
        let world = World::new(2, 2);
        let camera = Camera::new([1.0, 1.0], 1.0);
        let mut frame = Framebuffer::new(4, 4);
        let mut rows = 0;

        let completed = rasterize_world_into_while(&world, &camera, &mut frame, || {
            rows += 1;
            rows <= 2
        });

        assert!(!completed);
        assert_eq!(rows, 3);
    }

    #[test]
    fn color_map_covers_the_full_state_range() {
        assert_eq!(value_to_rgb(0.0), Rgb8::new(8, 12, 24));
        assert_eq!(value_to_rgb(1.0), Rgb8::new(255, 238, 170));
        assert_ne!(value_to_rgb(0.5), value_to_rgb(0.0));
    }

    #[test]
    fn fractional_zoom_blends_cell_boundaries_instead_of_aliasing() {
        let mut world = World::new(2, 2);
        world.set(0, 0, 1.0);
        let camera = Camera::new([1.0, 1.0], 1.2);

        let frame = rasterize_world(&world, &camera, 4, 4);
        let empty = value_to_rgb(0.0);
        let full = value_to_rgb(1.0);

        assert!(
            (0..frame.height()).any(|y| {
                (0..frame.width()).any(|x| {
                    let color = frame.get(x, y);
                    color != empty && color != full
                })
            }),
            "fractional zoom must preserve partial cell coverage instead of selecting only one cell"
        );
    }
}
