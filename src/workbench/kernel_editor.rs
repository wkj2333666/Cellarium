use crate::render::workbench_graphics::{GraphicsFrame, GraphicsScene};
use crate::sim::kernel::{KernelDefinition, KernelValues};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KernelPoint {
    pub x: usize,
    pub y: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KernelView {
    /// View center in normalized kernel coordinates.
    pub center: [f64; 2],
    pub zoom: f64,
}

impl Default for KernelView {
    fn default() -> Self {
        Self {
            center: [0.5, 0.5],
            zoom: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KernelGesture {
    SetValue { x: usize, y: usize, value: f32 },
    ToggleMask { x: usize, y: usize },
    SetAnchor { x: usize, y: usize },
    Resize { width: usize, height: usize },
}

#[derive(Clone, Debug)]
pub struct KernelScene {
    pub definition: KernelDefinition,
    pub selected: Option<KernelPoint>,
    pub cell_size: u32,
    pub view: KernelView,
}

impl KernelScene {
    pub fn new(definition: KernelDefinition) -> Self {
        Self {
            definition,
            selected: None,
            cell_size: 32,
            view: KernelView::default(),
        }
    }

    pub fn with_view(mut self, view: KernelView) -> Self {
        self.view = view;
        self
    }

    pub fn with_selected(mut self, selected: Option<KernelPoint>) -> Self {
        self.selected = selected;
        self
    }

    pub fn cell_at_pixel(&self, px: u32, py: u32) -> Option<KernelPoint> {
        let x = (px / self.cell_size) as usize;
        let y = (py / self.cell_size) as usize;
        (x < self.definition.width && y < self.definition.height).then_some(KernelPoint { x, y })
    }

    pub fn cell_at_pixel_in(
        &self,
        px: u32,
        py: u32,
        width: u32,
        height: u32,
    ) -> Option<KernelPoint> {
        let [x, y] = self.cell_coordinates_at_pixel(px, py, width, height)?;
        Some(KernelPoint { x, y })
    }

    pub fn pixel_for_cell(&self, point: KernelPoint) -> (u32, u32) {
        (
            point.x as u32 * self.cell_size,
            point.y as u32 * self.cell_size,
        )
    }

    pub fn zoom_at(&mut self, px: u32, py: u32, width: u32, height: u32, factor: f64) {
        let before = self.continuous_cell_at_pixel(px, py, width, height);
        self.view.zoom = (self.view.zoom * factor).clamp(1.0, 64.0);
        let after = self.continuous_cell_at_pixel(px, py, width, height);
        let kernel = [self.definition.width as f64, self.definition.height as f64];
        self.view.center[0] += (before[0] - after[0]) / kernel[0];
        self.view.center[1] += (before[1] - after[1]) / kernel[1];
        self.clamp_view();
    }

    pub fn pan_pixels(&mut self, dx: f64, dy: f64, width: u32, height: u32) {
        let scale = self.pixel_scale(width, height);
        self.view.center[0] -= dx / scale / self.definition.width as f64;
        self.view.center[1] -= dy / scale / self.definition.height as f64;
        self.clamp_view();
    }

    pub fn apply_gesture(&mut self, gesture: KernelGesture) -> Result<(), String> {
        match gesture {
            KernelGesture::SetValue { x, y, value } => {
                if !value.is_finite() {
                    return Err("kernel value must be finite".into());
                }
                let index = self.index(x, y)?;
                if matches!(self.definition.values, KernelValues::Expression(_)) {
                    let values = self
                        .definition
                        .build()
                        .map_err(|error| error.to_string())?
                        .values;
                    self.definition.values = KernelValues::Explicit(values);
                }
                let values = match &mut self.definition.values {
                    KernelValues::Explicit(values) => values,
                    KernelValues::Expression(_) => unreachable!("expression was materialized"),
                };
                values[index] = value;
                self.selected = Some(KernelPoint { x, y });
                Ok(())
            }
            KernelGesture::ToggleMask { x, y } => {
                let index = self.index(x, y)?;
                let mask = self.definition.mask.get_or_insert_with(|| {
                    vec![true; self.definition.width * self.definition.height]
                });
                mask[index] = !mask[index];
                self.selected = Some(KernelPoint { x, y });
                Ok(())
            }
            KernelGesture::SetAnchor { x, y } => {
                self.index(x, y)?;
                self.definition.anchor_x = x;
                self.definition.anchor_y = y;
                self.selected = Some(KernelPoint { x, y });
                Ok(())
            }
            KernelGesture::Resize { width, height } => {
                if width == 0 || height == 0 || width > 129 || height > 129 {
                    return Err("kernel dimensions must be between 1 and 129".into());
                }
                let old_values = self.values();
                let old_mask = self.definition.mask.clone();
                self.definition.width = width;
                self.definition.height = height;
                self.definition.anchor_x = self.definition.anchor_x.min(width - 1);
                self.definition.anchor_y = self.definition.anchor_y.min(height - 1);
                let mut values = vec![0.0; width * height];
                let mut mask = old_mask.as_ref().map(|_| vec![true; width * height]);
                for y in 0..height {
                    for x in 0..width {
                        if x < old_values.0 && y < old_values.1 {
                            let old_index = y * old_values.0 + x;
                            values[y * width + x] = old_values.2[old_index];
                            if let (Some(old), Some(next)) = (&old_mask, &mut mask) {
                                next[y * width + x] = old[old_index];
                            }
                        }
                    }
                }
                self.definition.values = KernelValues::Explicit(values);
                self.definition.mask = mask;
                Ok(())
            }
        }
    }

    fn index(&self, x: usize, y: usize) -> Result<usize, String> {
        (x < self.definition.width && y < self.definition.height)
            .then_some(y * self.definition.width + x)
            .ok_or_else(|| "kernel cell is outside the grid".into())
    }

    fn values(&self) -> (usize, usize, Vec<f32>) {
        let values = match &self.definition.values {
            KernelValues::Explicit(values) => values.clone(),
            KernelValues::Expression(_) => self
                .definition
                .build()
                .map(|kernel| kernel.values)
                .unwrap_or_else(|_| vec![0.0; self.definition.width * self.definition.height]),
        };
        (self.definition.width, self.definition.height, values)
    }

    fn pixel_scale(&self, width: u32, height: u32) -> f64 {
        let fit = (f64::from(width.max(1)) / self.definition.width.max(1) as f64)
            .min(f64::from(height.max(1)) / self.definition.height.max(1) as f64);
        fit * self.view.zoom
    }

    fn continuous_cell_at_pixel(&self, px: u32, py: u32, width: u32, height: u32) -> [f64; 2] {
        let scale = self.pixel_scale(width, height);
        let center = [
            self.view.center[0] * self.definition.width as f64,
            self.view.center[1] * self.definition.height as f64,
        ];
        [
            center[0] + (f64::from(px) + 0.5 - f64::from(width) * 0.5) / scale,
            center[1] + (f64::from(py) + 0.5 - f64::from(height) * 0.5) / scale,
        ]
    }

    fn cell_coordinates_at_pixel(
        &self,
        px: u32,
        py: u32,
        width: u32,
        height: u32,
    ) -> Option<[usize; 2]> {
        if (self.view.zoom - 1.0).abs() <= f64::EPSILON && self.view.center == [0.5, 0.5] {
            let (origin_x, origin_y, draw_width, draw_height) = self.fitted_layout(width, height);
            let local_x = px.checked_sub(origin_x)?;
            let local_y = py.checked_sub(origin_y)?;
            if local_x >= draw_width || local_y >= draw_height {
                return None;
            }
            return Some([
                map_pixel_to_cell(local_x, draw_width, self.definition.width),
                map_pixel_to_cell(local_y, draw_height, self.definition.height),
            ]);
        }
        let [x, y] = self.continuous_cell_at_pixel(px, py, width, height);
        if x < 0.0
            || y < 0.0
            || x >= self.definition.width as f64
            || y >= self.definition.height as f64
        {
            return None;
        }
        Some([x.floor() as usize, y.floor() as usize])
    }

    fn clamp_view(&mut self) {
        for center in &mut self.view.center {
            *center = center.clamp(0.0, 1.0);
        }
    }

    fn fitted_layout(&self, width: u32, height: u32) -> (u32, u32, u32, u32) {
        let kernel_width = self.definition.width.max(1) as f64;
        let kernel_height = self.definition.height.max(1) as f64;
        let scale =
            (f64::from(width.max(1)) / kernel_width).min(f64::from(height.max(1)) / kernel_height);
        let grid_width = (kernel_width * scale).round().max(1.0) as u32;
        let grid_height = (kernel_height * scale).round().max(1.0) as u32;
        (
            width.saturating_sub(grid_width) / 2,
            height.saturating_sub(grid_height) / 2,
            grid_width.min(width.max(1)),
            grid_height.min(height.max(1)),
        )
    }
}

fn map_pixel_to_cell(pixel: u32, pixels: u32, cells: usize) -> usize {
    if pixels <= 1 || cells <= 1 {
        return 0;
    }
    if pixel == 0 {
        return 0;
    }
    if pixel + 1 >= pixels {
        return cells - 1;
    }
    (((u64::from(pixel) * 2 + 1) * cells as u64) / (u64::from(pixels) * 2)).min((cells - 1) as u64)
        as usize
}

impl GraphicsScene for KernelScene {
    fn render_rgba(&self, width: u32, height: u32) -> GraphicsFrame {
        let mut rgba = vec![8_u8; width.max(1) as usize * height.max(1) as usize * 4];
        let width = width.max(1);
        let height = height.max(1);
        for pixel in rgba.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        let (_, _, values) = self.values();
        // Keep normalized kernels legible after one cell is painted to a much
        // larger value: scale to the 90th percentile and clip rare outliers.
        let mut magnitudes = values
            .iter()
            .copied()
            .map(f32::abs)
            .filter(|value| value.is_finite() && *value > f32::EPSILON)
            .collect::<Vec<_>>();
        magnitudes.sort_by(f32::total_cmp);
        let max_value = magnitudes
            .get(magnitudes.len().saturating_sub(1) * 9 / 10)
            .copied()
            .unwrap_or(1.0)
            .max(f32::EPSILON);
        let mask = self.definition.mask.as_deref();
        for py in 0..height {
            for px in 0..width {
                let Some([x, y]) = self.cell_coordinates_at_pixel(px, py, width, height) else {
                    continue;
                };
                let i = y * self.definition.width + x;
                let active = mask.is_none_or(|m| m[i]);
                let value = values.get(i).copied().unwrap_or(0.0);
                let intensity = ((value / max_value).abs().min(1.0) * 255.0) as u8;
                let color = if !active {
                    [24, 24, 24, 255]
                } else if value > 0.0 {
                    [0, intensity, intensity, 255]
                } else if value < 0.0 {
                    [intensity, intensity / 5, 0, 255]
                } else {
                    [8, 8, 8, 255]
                };
                let idx = (py * width + px) as usize * 4;
                let selected = self.selected == Some(KernelPoint { x, y });
                rgba[idx..idx + 4].copy_from_slice(if selected {
                    &[255, 255, 255, 255]
                } else {
                    &color
                });
            }
        }
        GraphicsFrame::new(width, height, rgba, 0).expect("valid kernel frame")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::kernel::{render_definition, ring_definition};

    #[test]
    fn pixel_mapping_and_value_edit_work() {
        let mut scene = KernelScene::new(render_definition(3, 3));
        assert_eq!(scene.cell_at_pixel(35, 2), Some(KernelPoint { x: 1, y: 0 }));
        scene
            .apply_gesture(KernelGesture::SetValue {
                x: 1,
                y: 0,
                value: 0.75,
            })
            .unwrap();
        let KernelValues::Explicit(values) = &scene.definition.values else {
            panic!("expected explicit values")
        };
        assert_eq!(values[1], 0.75);
    }

    #[test]
    fn anchor_mask_and_resize_are_validated() {
        let mut scene = KernelScene::new(ring_definition(2, 0.5, 0.5));
        scene
            .apply_gesture(KernelGesture::SetAnchor { x: 0, y: 0 })
            .unwrap();
        scene
            .apply_gesture(KernelGesture::ToggleMask { x: 0, y: 0 })
            .unwrap();
        assert_eq!(scene.definition.mask.as_ref().unwrap()[0], true);
        assert!(
            scene
                .apply_gesture(KernelGesture::Resize {
                    width: 0,
                    height: 2
                })
                .is_err()
        );
    }

    #[test]
    fn kernel_scene_renders_colored_cells() {
        let scene = KernelScene::new(ring_definition(2, 0.5, 0.5));
        let frame = scene.render_rgba(128, 128);
        assert!(
            frame
                .rgba
                .chunks_exact(4)
                .any(|p| p[0] != 8 || p[1] != 8 || p[2] != 8)
        );
    }

    #[test]
    fn zero_kernel_values_are_dark_and_positive_values_are_bright() {
        let mut scene = KernelScene::new(render_definition(2, 1));
        scene
            .apply_gesture(KernelGesture::SetValue {
                x: 0,
                y: 0,
                value: 0.0,
            })
            .unwrap();
        scene
            .apply_gesture(KernelGesture::SetValue {
                x: 1,
                y: 0,
                value: 1.0,
            })
            .unwrap();
        let frame = scene.render_rgba(64, 32);
        let pixel = |x: usize| &frame.rgba[(16 * 64 + x) * 4..(16 * 64 + x) * 4 + 4];
        assert!(pixel(16)[0..3].iter().all(|channel| *channel < 32));
        assert!(pixel(48)[1] > 200 && pixel(48)[2] > 200);
    }

    #[test]
    fn expression_kernel_preview_shows_evaluated_spatial_values() {
        let scene = KernelScene::new(ring_definition(5, 0.5, 0.15));
        let frame = scene.render_rgba(192, 192);
        let colors = frame
            .rgba
            .chunks_exact(4)
            .map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect::<std::collections::HashSet<_>>();
        assert!(
            colors.len() > 4,
            "expression preview must not be a flat color"
        );
    }

    #[test]
    fn large_kernel_preview_fits_the_whole_symmetric_kernel() {
        let scene = KernelScene::new(ring_definition(13, 0.5, 0.15));
        let frame = scene.render_rgba(459, 459);
        for y in 0..459_usize {
            for x in 0..459_usize {
                let left = (y * 459 + x) * 4;
                let right = (y * 459 + (458 - x)) * 4;
                assert_eq!(
                    &frame.rgba[left..left + 4],
                    &frame.rgba[right..right + 4],
                    "preview cropped or shifted at ({x}, {y})",
                );
            }
        }
    }

    #[test]
    fn maximum_kernel_fits_and_every_edge_remains_mouse_reachable_on_small_canvas() {
        let scene = KernelScene::new(ring_definition(64, 0.5, 0.15));
        let frame = scene.render_rgba(58, 64);
        assert_eq!(frame.width, 58);
        assert_eq!(frame.height, 64);
        assert_eq!(
            scene.cell_at_pixel_in(0, 32, 58, 64),
            Some(KernelPoint { x: 0, y: 65 })
        );
        assert_eq!(
            scene.cell_at_pixel_in(57, 32, 58, 64),
            Some(KernelPoint { x: 128, y: 65 })
        );
        assert_eq!(
            scene.cell_at_pixel_in(29, 3, 58, 64),
            Some(KernelPoint { x: 65, y: 0 })
        );
        assert_eq!(
            scene.cell_at_pixel_in(29, 60, 58, 64),
            Some(KernelPoint { x: 65, y: 128 })
        );
    }

    #[test]
    fn painting_an_expression_kernel_materializes_editable_values() {
        let mut scene = KernelScene::new(ring_definition(5, 0.5, 0.15));
        scene
            .apply_gesture(KernelGesture::SetValue {
                x: 5,
                y: 5,
                value: 0.75,
            })
            .unwrap();
        let KernelValues::Explicit(values) = &scene.definition.values else {
            panic!("paint must materialize an explicit kernel");
        };
        assert_eq!(values[5 * scene.definition.width + 5], 0.75);
    }

    #[test]
    fn zoom_and_pan_make_every_maximum_kernel_cell_mouse_reachable() {
        let definition = ring_definition(64, 0.5, 0.15);
        for y in 0..129 {
            for x in 0..129 {
                let view = KernelView {
                    center: [(x as f64 + 0.5) / 129.0, (y as f64 + 0.5) / 129.0],
                    zoom: 4.0,
                };
                let scene = KernelScene::new(definition.clone()).with_view(view);
                assert_eq!(
                    scene.cell_at_pixel_in(28, 31, 58, 64),
                    Some(KernelPoint { x, y }),
                    "cell ({x}, {y}) cannot be centered after zoom/pan",
                );
            }
        }
    }

    #[test]
    fn one_large_mouse_edit_does_not_make_the_rest_of_the_kernel_invisible() {
        let mut scene = KernelScene::new(ring_definition(13, 0.5, 0.15));
        scene
            .apply_gesture(KernelGesture::SetValue {
                x: 3,
                y: 3,
                value: 1.0,
            })
            .unwrap();
        let frame = scene.render_rgba(464, 512);
        let clearly_visible = frame
            .rgba
            .chunks_exact(4)
            .filter(|pixel| pixel[1] > 64 || pixel[0] > 64)
            .count();
        assert!(
            clearly_visible > 10_000,
            "an edited outlier must not flatten the whole preview; visible={clearly_visible}"
        );
    }
}
