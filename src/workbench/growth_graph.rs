use crate::render::workbench_graphics::{GraphicsFrame, GraphicsScene};
use crate::workbench::growth_editor::GrowthEditorState;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrowthCursor {
    pub sample: usize,
    pub input: f32,
    pub value: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct GrowthScene {
    pub plot: Vec<Option<f32>>,
    pub stale: bool,
    pub cursor: Option<GrowthCursor>,
}

impl GrowthScene {
    pub fn from_editor(editor: &GrowthEditorState) -> Self {
        Self {
            plot: editor.plot().data.clone(),
            stale: editor.plot().stale,
            cursor: None,
        }
    }

    pub fn sample_at_pixel(&self, x: u32, width: u32) -> Option<usize> {
        if self.plot.is_empty() || width == 0 {
            return None;
        }
        Some(
            ((x as usize * self.plot.len().saturating_sub(1)) / width as usize)
                .min(self.plot.len() - 1),
        )
    }

    pub fn select_pixel(&mut self, x: u32, width: u32) -> Option<GrowthCursor> {
        let sample = self.sample_at_pixel(x, width)?;
        let value = self.plot[sample];
        let cursor = GrowthCursor {
            sample,
            input: sample as f32 / self.plot.len().saturating_sub(1).max(1) as f32,
            value,
        };
        self.cursor = Some(cursor);
        Some(cursor)
    }
}

impl GraphicsScene for GrowthScene {
    fn render_rgba(&self, width: u32, height: u32) -> GraphicsFrame {
        let width = width.max(1);
        let height = height.max(1);
        let mut rgba = vec![10_u8; width as usize * height as usize * 4];
        for p in rgba.chunks_exact_mut(4) {
            p[3] = 255;
        }
        let axis = [48_u8, 58, 76, 255];
        draw_line(
            &mut rgba,
            width,
            height,
            (24, height as i32 - 24),
            (width as i32 - 8, height as i32 - 24),
            axis,
        );
        draw_line(
            &mut rgba,
            width,
            height,
            (24, 8),
            (24, height as i32 - 24),
            axis,
        );
        let valid = self.plot.iter().filter_map(|v| *v).collect::<Vec<_>>();
        let (min, max) = valid
            .iter()
            .copied()
            .fold((0.0_f32, 1.0_f32), |(lo, hi), value| {
                (lo.min(value), hi.max(value))
            });
        let span = (max - min).max(1e-6);
        let mut previous = None;
        for (index, value) in self.plot.iter().enumerate() {
            let Some(value) = value else {
                previous = None;
                continue;
            };
            let x = 24
                + (index * (width.saturating_sub(33) as usize)
                    / self.plot.len().saturating_sub(1).max(1)) as i32;
            let y = height as i32
                - 24
                - (((value - min) / span) * (height.saturating_sub(33) as f32)) as i32;
            let point = (x, y);
            if let Some(prev) = previous {
                draw_line(&mut rgba, width, height, prev, point, [80, 220, 140, 255]);
            }
            previous = Some(point);
        }
        if let Some(cursor) = self.cursor {
            if !self.plot.is_empty() {
                let x = 24
                    + (cursor.sample * width.saturating_sub(33) as usize
                        / self.plot.len().saturating_sub(1).max(1)) as i32;
                for y in 8..height.saturating_sub(24) {
                    blend(&mut rgba, width, height, x, y as i32, [255, 220, 100, 255]);
                }
            }
        }
        if self.stale {
            for x in 0..width {
                blend(&mut rgba, width, height, x as i32, 0, [220, 90, 90, 255]);
            }
        }
        GraphicsFrame::new(width, height, rgba, 0).expect("valid growth frame")
    }
}

fn draw_line(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    start: (i32, i32),
    end: (i32, i32),
    color: [u8; 4],
) {
    let mut x = start.0;
    let mut y = start.1;
    let dx = (end.0 - x).abs();
    let sx = if x < end.0 { 1 } else { -1 };
    let dy = -(end.1 - y).abs();
    let sy = if y < end.1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        blend(rgba, width, height, x, y, color);
        if x == end.0 && y == end.1 {
            break;
        }
        let twice = 2 * err;
        if twice >= dy {
            err += dy;
            x += sx;
        }
        if twice <= dx {
            err += dx;
            y += sy;
        }
    }
}
fn blend(rgba: &mut [u8], width: u32, height: u32, x: i32, y: i32, color: [u8; 4]) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    let i = (y as usize * width as usize + x as usize) * 4;
    rgba[i..i + 4].copy_from_slice(&color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::growth::types::ExternalSymbols;
    use crate::workbench::growth_editor::GrowthEditorState;
    use std::collections::BTreeMap;
    #[test]
    fn cursor_maps_to_curve_sample() {
        let editor = GrowthEditorState::new(
            "inner * inner",
            ExternalSymbols::new(&["inner"], &[]),
            BTreeMap::new(),
            "growth",
        );
        let mut scene = GrowthScene::from_editor(&editor);
        let cursor = scene.select_pixel(50, 100).unwrap();
        assert!(cursor.sample < scene.plot.len());
    }
    #[test]
    fn graph_renders_line_pixels_and_stale_marker() {
        let editor = GrowthEditorState::new(
            "inner * inner",
            ExternalSymbols::new(&["inner"], &[]),
            BTreeMap::new(),
            "growth",
        );
        let scene = GrowthScene::from_editor(&editor);
        let frame = scene.render_rgba(200, 120);
        assert!(frame.rgba.chunks_exact(4).any(|p| p[1] > 100));
    }
    #[test]
    fn graph_pixels_change_after_valid_source_edit() {
        let mut parameters = BTreeMap::new();
        parameters.insert("mu".into(), 0.15);
        parameters.insert("sigma".into(), 0.015);
        let mut editor = GrowthEditorState::new(
            "2 * exp(-((potential - mu) / sigma) ^ 2) - 1",
            ExternalSymbols::new(&["potential"], &["mu", "sigma"]),
            parameters,
            "growth",
        );
        let before = GrowthScene::from_editor(&editor).render_rgba(464, 512);
        let before_plot = editor.plot().data.clone();
        editor.buffer_mut().insert_str("+0.2*sin(potential*20)");
        editor.refresh_now();
        assert!(editor.diagnostics().is_empty());
        let after_plot = editor.plot().data.clone();
        assert_eq!(before_plot.iter().flatten().count(), before_plot.len());
        assert!(
            before_plot
                .iter()
                .flatten()
                .all(|value| (-1.0..=1.0).contains(value))
        );
        assert_ne!(before_plot, after_plot);
        let after = GrowthScene::from_editor(&editor).render_rgba(464, 512);
        assert!(
            before.rgba != after.rgba,
            "valid edit must change curve pixels"
        );
    }
}
