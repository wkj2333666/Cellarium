use crate::render::workbench_graphics::{GraphicsFrame, GraphicsScene};
use crate::sim::kernel::{KernelDefinition, KernelValues};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KernelPoint {
    pub x: usize,
    pub y: usize,
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
}

impl KernelScene {
    pub fn new(definition: KernelDefinition) -> Self {
        Self { definition, selected: None, cell_size: 32 }
    }

    pub fn cell_at_pixel(&self, px: u32, py: u32) -> Option<KernelPoint> {
        let x = (px / self.cell_size) as usize;
        let y = (py / self.cell_size) as usize;
        (x < self.definition.width && y < self.definition.height)
            .then_some(KernelPoint { x, y })
    }

    pub fn pixel_for_cell(&self, point: KernelPoint) -> (u32, u32) {
        (point.x as u32 * self.cell_size, point.y as u32 * self.cell_size)
    }

    pub fn apply_gesture(&mut self, gesture: KernelGesture) -> Result<(), String> {
        match gesture {
            KernelGesture::SetValue { x, y, value } => {
                if !value.is_finite() { return Err("kernel value must be finite".into()); }
                let index = self.index(x, y)?;
                let values = match &mut self.definition.values {
                    KernelValues::Explicit(values) => values,
                    KernelValues::Expression(_) => return Err("expression kernels must be edited as expressions".into()),
                };
                values[index] = value;
                self.selected = Some(KernelPoint { x, y });
                Ok(())
            }
            KernelGesture::ToggleMask { x, y } => {
                let index = self.index(x, y)?;
                let mask = self.definition.mask.get_or_insert_with(|| vec![true; self.definition.width * self.definition.height]);
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
            KernelValues::Expression(_) => vec![0.0; self.definition.width * self.definition.height],
        };
        (self.definition.width, self.definition.height, values)
    }
}

impl GraphicsScene for KernelScene {
    fn render_rgba(&self, width: u32, height: u32) -> GraphicsFrame {
        let mut rgba = vec![8_u8; width.max(1) as usize * height.max(1) as usize * 4];
        let width = width.max(1);
        let height = height.max(1);
        for pixel in rgba.chunks_exact_mut(4) { pixel[3] = 255; }
        let values: &[f32] = match &self.definition.values {
            KernelValues::Explicit(values) => values.as_slice(),
            KernelValues::Expression(_) => &[],
        };
        let max_value = values.iter().copied().map(f32::abs).fold(0.01, f32::max);
        let mask = self.definition.mask.as_deref();
        for y in 0..self.definition.height {
            for x in 0..self.definition.width {
                let i = y * self.definition.width + x;
                let active = mask.is_none_or(|m| m[i]);
                let value = values.get(i).copied().unwrap_or(0.0);
                let intensity = ((value / max_value).abs().min(1.0) * 220.0) as u8;
                let color = if !active { [24, 24, 24, 255] } else if value >= 0.0 { [intensity, 180, 255, 255] } else { [255, 100, intensity, 255] };
                let x0 = (x as u32 * self.cell_size).min(width);
                let y0 = (y as u32 * self.cell_size).min(height);
                for py in y0..(y0 + self.cell_size).min(height) {
                    for px in x0..(x0 + self.cell_size).min(width) {
                        let idx = (py * width + px) as usize * 4;
                        rgba[idx..idx+4].copy_from_slice(&color);
                    }
                }
                if self.selected == Some(KernelPoint { x, y }) {
                    for px in x0..(x0 + self.cell_size).min(width) {
                        let idx = (y0 * width + px) as usize * 4;
                        rgba[idx..idx+4].copy_from_slice(&[255,255,255,255]);
                    }
                }
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
        scene.apply_gesture(KernelGesture::SetValue { x: 1, y: 0, value: 0.75 }).unwrap();
        let KernelValues::Explicit(values) = &scene.definition.values else { panic!("expected explicit values") };
        assert_eq!(values[1], 0.75);
    }

    #[test]
    fn anchor_mask_and_resize_are_validated() {
        let mut scene = KernelScene::new(ring_definition(2, 0.5, 0.5));
        scene.apply_gesture(KernelGesture::SetAnchor { x: 0, y: 0 }).unwrap();
        scene.apply_gesture(KernelGesture::ToggleMask { x: 0, y: 0 }).unwrap();
        assert_eq!(scene.definition.mask.as_ref().unwrap()[0], true);
        assert!(scene.apply_gesture(KernelGesture::Resize { width: 0, height: 2 }).is_err());
    }

    #[test]
    fn kernel_scene_renders_colored_cells() {
        let scene = KernelScene::new(ring_definition(2, 0.5, 0.5));
        let frame = scene.render_rgba(128, 128);
        assert!(frame.rgba.chunks_exact(4).any(|p| p[0] != 8 || p[1] != 8 || p[2] != 8));
    }
}
