//! The one screen/world mapping every canvas uses.
//!
//! Rendering and hit testing must consume the same transform instance for the
//! same frame. Keeping two mappings in step by hand is what produced the
//! historical drift between where a cell was drawn and where a click landed.

use eframe::egui::{Pos2, Rect, Vec2, pos2};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasTransform {
    pub viewport: Rect,
    /// World point at the centre of the viewport.
    pub center_world: [f64; 2],
    /// Screen pixels per world unit. Always finite and positive.
    pub pixels_per_world: f64,
}

impl CanvasTransform {
    pub fn new(viewport: Rect, center_world: [f64; 2], pixels_per_world: f64) -> Self {
        assert!(
            pixels_per_world.is_finite() && pixels_per_world > 0.0,
            "scale must be finite and positive"
        );
        Self {
            viewport,
            center_world,
            pixels_per_world,
        }
    }

    /// Fit `world` inside `viewport` with a margin, so the whole domain is
    /// visible without the user hunting for it.
    pub fn fit(viewport: Rect, world: [f64; 2], margin: f64) -> Self {
        let usable_x = (viewport.width() as f64 - margin).max(1.0);
        let usable_y = (viewport.height() as f64 - margin).max(1.0);
        let scale = (usable_x / world[0].max(1e-9))
            .min(usable_y / world[1].max(1e-9))
            .max(1e-9);
        Self::new(viewport, [world[0] / 2.0, world[1] / 2.0], scale)
    }

    pub fn world_to_screen(&self, world: [f64; 2]) -> Pos2 {
        let center = self.viewport.center();
        pos2(
            (center.x as f64 + (world[0] - self.center_world[0]) * self.pixels_per_world) as f32,
            (center.y as f64 + (world[1] - self.center_world[1]) * self.pixels_per_world) as f32,
        )
    }

    pub fn screen_to_world(&self, screen: Pos2) -> [f64; 2] {
        let center = self.viewport.center();
        [
            self.center_world[0] + (screen.x as f64 - center.x as f64) / self.pixels_per_world,
            self.center_world[1] + (screen.y as f64 - center.y as f64) / self.pixels_per_world,
        ]
    }

    /// Zoom about a screen point, keeping the world point under it fixed.
    pub fn zoom_at(&mut self, pointer: Pos2, factor: f64) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let anchor = self.screen_to_world(pointer);
        let next = (self.pixels_per_world * factor).clamp(MIN_SCALE, MAX_SCALE);
        if next == self.pixels_per_world {
            return;
        }
        self.pixels_per_world = next;
        // Move the centre so `anchor` lands back under the pointer.
        let center = self.viewport.center();
        self.center_world = [
            anchor[0] - (pointer.x as f64 - center.x as f64) / self.pixels_per_world,
            anchor[1] - (pointer.y as f64 - center.y as f64) / self.pixels_per_world,
        ];
    }

    /// Pan by a screen-space delta.
    pub fn pan_screen(&mut self, delta: Vec2) {
        self.center_world = [
            self.center_world[0] - delta.x as f64 / self.pixels_per_world,
            self.center_world[1] - delta.y as f64 / self.pixels_per_world,
        ];
    }

    /// World rectangle currently visible, as `[min_x, min_y, max_x, max_y]`.
    pub fn visible_world(&self) -> [f64; 4] {
        let min = self.screen_to_world(self.viewport.min);
        let max = self.screen_to_world(self.viewport.max);
        [min[0], min[1], max[0], max[1]]
    }
}

const MIN_SCALE: f64 = 1e-3;
const MAX_SCALE: f64 = 1e6;

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::vec2;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(width, height))
    }

    fn assert_close(left: [f64; 2], right: [f64; 2], tolerance: f64) {
        assert!(
            (left[0] - right[0]).abs() <= tolerance && (left[1] - right[1]).abs() <= tolerance,
            "{left:?} differs from {right:?} by more than {tolerance}"
        );
    }

    fn assert_close_pos(left: Pos2, right: Pos2, tolerance: f32) {
        assert!(
            (left.x - right.x).abs() <= tolerance && (left.y - right.y).abs() <= tolerance,
            "{left:?} differs from {right:?} by more than {tolerance}"
        );
    }

    #[test]
    fn screen_world_round_trip_and_zoom_anchor_are_stable() {
        let mut t = CanvasTransform::new(rect(37.0, 19.0, 901.0, 613.0), [128.0, 128.0], 2.7);
        let pointer = pos2(411.25, 287.75);
        let before = t.screen_to_world(pointer);
        t.zoom_at(pointer, 1.25);
        assert_close(t.screen_to_world(pointer), before, 1e-9);
        assert_close_pos(t.world_to_screen(before), pointer, 1e-4);
    }

    #[test]
    fn the_mapping_is_its_own_inverse_across_the_viewport() {
        let t = CanvasTransform::new(rect(11.0, 7.0, 640.0, 480.0), [3.5, -2.25], 17.0);
        for x in [11.0_f32, 200.0, 331.5, 650.0] {
            for y in [7.0_f32, 100.0, 244.75, 486.0] {
                let screen = pos2(x, y);
                let round_trip = t.world_to_screen(t.screen_to_world(screen));
                assert_close_pos(round_trip, screen, 1e-3);
            }
        }
    }

    #[test]
    fn repeated_zoom_never_drifts_the_anchor() {
        let mut t = CanvasTransform::new(rect(0.0, 0.0, 800.0, 600.0), [64.0, 64.0], 4.0);
        let pointer = pos2(123.0, 457.0);
        let anchor = t.screen_to_world(pointer);
        for _ in 0..64 {
            t.zoom_at(pointer, 1.1);
            t.zoom_at(pointer, 1.0 / 1.1);
        }
        assert_close(t.screen_to_world(pointer), anchor, 1e-6);
    }

    #[test]
    fn panning_moves_the_world_with_the_pointer() {
        let mut t = CanvasTransform::new(rect(0.0, 0.0, 400.0, 400.0), [10.0, 10.0], 5.0);
        let before = t.screen_to_world(pos2(200.0, 200.0));
        t.pan_screen(vec2(50.0, -25.0));
        let after = t.screen_to_world(pos2(250.0, 175.0));
        assert_close(after, before, 1e-9);
    }

    #[test]
    fn zoom_is_clamped_instead_of_collapsing_the_scale() {
        let mut t = CanvasTransform::new(rect(0.0, 0.0, 400.0, 400.0), [0.0, 0.0], 1.0);
        for _ in 0..200 {
            t.zoom_at(pos2(200.0, 200.0), 0.5);
        }
        assert!(t.pixels_per_world >= MIN_SCALE);
        for _ in 0..400 {
            t.zoom_at(pos2(200.0, 200.0), 2.0);
        }
        assert!(t.pixels_per_world <= MAX_SCALE);
    }

    #[test]
    fn a_nonsense_zoom_factor_is_ignored() {
        let mut t = CanvasTransform::new(rect(0.0, 0.0, 400.0, 400.0), [0.0, 0.0], 3.0);
        let before = t;
        t.zoom_at(pos2(10.0, 10.0), 0.0);
        t.zoom_at(pos2(10.0, 10.0), f64::NAN);
        t.zoom_at(pos2(10.0, 10.0), -2.0);
        assert_eq!(t, before);
    }

    #[test]
    fn fitting_shows_the_whole_world_centred() {
        let viewport = rect(0.0, 0.0, 800.0, 400.0);
        let t = CanvasTransform::fit(viewport, [256.0, 256.0], 16.0);
        assert_eq!(t.center_world, [128.0, 128.0]);
        let visible = t.visible_world();
        assert!(visible[0] <= 0.0 && visible[1] <= 0.0);
        assert!(visible[2] >= 256.0 && visible[3] >= 256.0);
    }
}
