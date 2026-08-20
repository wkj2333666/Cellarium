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
    let width = frame.width;
    let height = frame.height;
    for y in 0..height {
        for x in 0..width {
            let world_position =
                camera.screen_to_world([x as f32 + 0.5, y as f32 + 0.5], width, height);
            let sample_x = world_position[0].floor().rem_euclid(world.width() as f32) as usize;
            let sample_y = world_position[1].floor().rem_euclid(world.height() as f32) as usize;
            let value = world.get(sample_x as isize, sample_y as isize);
            frame.set(x, y, value_to_rgb(value));
        }
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
    fn color_map_covers_the_full_state_range() {
        assert_eq!(value_to_rgb(0.0), Rgb8::new(8, 12, 24));
        assert_eq!(value_to_rgb(1.0), Rgb8::new(255, 238, 170));
        assert_ne!(value_to_rgb(0.5), value_to_rgb(0.0));
    }
}
