use crate::render::workbench_graphics::{GraphicsFrame, GraphicsScene};
use crate::sim::basis_kernel::PeriodicKernelDefinition;
use crate::sim::kernel::{KernelDefinition, KernelValues};
use crate::sim::tiling::{
    BasisId, PeriodicTilingDraft, Vec2,
    polygon::{prototype_vertices, transform_vertices},
};

const MAX_FITTED_KERNEL_CELL_PIXELS: f64 = 96.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KernelPoint {
    pub x: usize,
    pub y: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KernelSelection {
    pub offset: [i16; 2],
    pub source_basis: BasisId,
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
        fit.min(MAX_FITTED_KERNEL_CELL_PIXELS) * self.view.zoom
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
        let scale = (f64::from(width.max(1)) / kernel_width)
            .min(f64::from(height.max(1)) / kernel_height)
            .min(MAX_FITTED_KERNEL_CELL_PIXELS);
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
        for pixel in rgba.as_chunks_mut::<4>().0 {
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
        let mut selected_bounds: Option<[u32; 4]> = None;
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
                if selected {
                    match &mut selected_bounds {
                        Some(bounds) => {
                            bounds[0] = bounds[0].min(px);
                            bounds[1] = bounds[1].min(py);
                            bounds[2] = bounds[2].max(px);
                            bounds[3] = bounds[3].max(py);
                        }
                        None => selected_bounds = Some([px, py, px, py]),
                    }
                }
                rgba[idx..idx + 4].copy_from_slice(&color);
            }
        }
        if let Some([left, top, right, bottom]) = selected_bounds {
            let edge = [255, 255, 255, 255];
            for inset in 0..2_u32 {
                let x0 = left.saturating_add(inset).min(right);
                let x1 = right.saturating_sub(inset).max(left);
                let y0 = top.saturating_add(inset).min(bottom);
                let y1 = bottom.saturating_sub(inset).max(top);
                for x in x0..=x1 {
                    set_pixel(&mut rgba, width, height, x as i32, y0 as i32, edge);
                    set_pixel(&mut rgba, width, height, x as i32, y1 as i32, edge);
                }
                for y in y0..=y1 {
                    set_pixel(&mut rgba, width, height, x0 as i32, y as i32, edge);
                    set_pixel(&mut rgba, width, height, x1 as i32, y as i32, edge);
                }
            }
        }
        GraphicsFrame::new(width, height, rgba, 0).expect("valid kernel frame")
    }
}

#[derive(Clone, Debug)]
pub struct PeriodicKernelScene {
    pub tiling: PeriodicTilingDraft,
    pub definition: PeriodicKernelDefinition,
    pub target_basis: BasisId,
    pub selected: Option<KernelSelection>,
    pub view: KernelView,
}

type PeriodicKernelCell = (KernelSelection, Vec<Vec2>, f32, bool);

#[derive(Clone, Copy, Debug)]
struct PeriodicPixelTransform {
    width: f64,
    height: f64,
    center: Vec2,
    scale: f64,
}

impl PeriodicPixelTransform {
    fn world_to_pixel(self, point: Vec2) -> (i32, i32) {
        (
            (self.width * 0.5 + (point.x - self.center.x) * self.scale).round() as i32,
            (self.height * 0.5 + (point.y - self.center.y) * self.scale).round() as i32,
        )
    }
}

impl PeriodicKernelScene {
    pub fn new(
        tiling: PeriodicTilingDraft,
        definition: PeriodicKernelDefinition,
        target_basis: BasisId,
    ) -> Self {
        Self {
            tiling,
            definition,
            target_basis,
            selected: None,
            view: KernelView::default(),
        }
    }

    pub fn with_view(mut self, view: KernelView) -> Self {
        self.view = view;
        self
    }

    pub fn with_selected(mut self, selected: Option<KernelSelection>) -> Self {
        self.selected = selected;
        self
    }

    pub fn selection_at_pixel(
        &self,
        px: u32,
        py: u32,
        width: u32,
        height: u32,
    ) -> Option<KernelSelection> {
        let point = self.pixel_to_world(px, py, width, height);
        self.cells()
            .into_iter()
            .rev()
            .find_map(|(selection, polygon, _, _)| {
                point_in_polygon(point, &polygon).then_some(selection)
            })
    }

    pub fn pixel_for_selection(
        &self,
        selection: KernelSelection,
        width: u32,
        height: u32,
    ) -> Option<(u32, u32)> {
        let (_, polygon, _, _) = self
            .cells()
            .into_iter()
            .find(|(candidate, _, _, _)| *candidate == selection)?;
        let center = polygon.iter().fold(Vec2::ZERO, |sum, point| sum + *point)
            * (1.0 / polygon.len() as f64);
        let (x, y) = self.world_to_pixel(center, width, height);
        (x >= 0 && y >= 0 && x < width as i32 && y < height as i32).then_some((x as u32, y as u32))
    }

    pub fn zoom_at(&mut self, px: u32, py: u32, width: u32, height: u32, factor: f64) {
        let before = self.pixel_to_world(px, py, width, height);
        self.view.zoom = (self.view.zoom * factor).clamp(1.0, 64.0);
        let after = self.pixel_to_world(px, py, width, height);
        let bounds = self.world_bounds();
        let span = [
            (bounds.1.x - bounds.0.x).max(1e-9),
            (bounds.1.y - bounds.0.y).max(1e-9),
        ];
        self.view.center[0] += (before.x - after.x) / span[0];
        self.view.center[1] += (before.y - after.y) / span[1];
        self.clamp_view();
    }

    pub fn pan_pixels(&mut self, dx: f64, dy: f64, width: u32, height: u32) {
        let bounds = self.world_bounds();
        let scale = self.pixel_scale(width, height, bounds);
        let span = [
            (bounds.1.x - bounds.0.x).max(1e-9),
            (bounds.1.y - bounds.0.y).max(1e-9),
        ];
        self.view.center[0] -= dx / scale / span[0];
        self.view.center[1] -= dy / scale / span[1];
        self.clamp_view();
    }

    fn clamp_view(&mut self) {
        for center in &mut self.view.center {
            *center = center.clamp(0.0, 1.0);
        }
    }

    fn cells(&self) -> Vec<PeriodicKernelCell> {
        let mut cells = Vec::new();
        for y in 0..self.definition.height {
            for x in 0..self.definition.width {
                let offset = [
                    x as i16 - self.definition.anchor_x as i16,
                    y as i16 - self.definition.anchor_y as i16,
                ];
                let translation = self.tiling.translation_a * f64::from(offset[0])
                    + self.tiling.translation_b * f64::from(offset[1]);
                let index = y * self.definition.width + x;
                for (source_basis, plane) in &self.definition.planes {
                    let Some(polygon) = self.basis_polygon(*source_basis, translation) else {
                        continue;
                    };
                    let active = plane.mask.as_ref().is_none_or(|mask| mask[index]);
                    cells.push((
                        KernelSelection {
                            offset,
                            source_basis: *source_basis,
                        },
                        polygon,
                        plane.values[index],
                        active,
                    ));
                }
            }
        }
        cells
    }

    fn basis_polygon(&self, basis: BasisId, translation: Vec2) -> Option<Vec<Vec2>> {
        let instance = self
            .tiling
            .instances
            .iter()
            .find(|entry| entry.id == basis)?;
        let prototype = self
            .tiling
            .prototypes
            .iter()
            .find(|entry| entry.id == instance.prototype)?;
        let base = prototype_vertices(&prototype.shape).ok()?;
        Some(
            transform_vertices(&base, instance.transform)
                .into_iter()
                .map(|point| point + translation)
                .collect(),
        )
    }

    fn world_bounds(&self) -> (Vec2, Vec2) {
        let cells = self.cells();
        world_bounds_for_cells(&cells)
    }

    fn pixel_scale(&self, width: u32, height: u32, bounds: (Vec2, Vec2)) -> f64 {
        let span_x = (bounds.1.x - bounds.0.x).max(1e-9);
        let span_y = (bounds.1.y - bounds.0.y).max(1e-9);
        (f64::from(width.max(1)) / span_x).min(f64::from(height.max(1)) / span_y) * self.view.zoom
    }

    fn view_center(&self, bounds: (Vec2, Vec2)) -> Vec2 {
        Vec2::new(
            bounds.0.x + self.view.center[0] * (bounds.1.x - bounds.0.x),
            bounds.0.y + self.view.center[1] * (bounds.1.y - bounds.0.y),
        )
    }

    fn pixel_transform(
        &self,
        width: u32,
        height: u32,
        bounds: (Vec2, Vec2),
    ) -> PeriodicPixelTransform {
        PeriodicPixelTransform {
            width: f64::from(width),
            height: f64::from(height),
            center: self.view_center(bounds),
            scale: self.pixel_scale(width, height, bounds),
        }
    }

    fn world_to_pixel(&self, point: Vec2, width: u32, height: u32) -> (i32, i32) {
        self.pixel_transform(width, height, self.world_bounds())
            .world_to_pixel(point)
    }

    fn pixel_to_world(&self, px: u32, py: u32, width: u32, height: u32) -> Vec2 {
        let bounds = self.world_bounds();
        let transform = self.pixel_transform(width, height, bounds);
        Vec2::new(
            (f64::from(px) - transform.width * 0.5) / transform.scale + transform.center.x,
            (f64::from(py) - transform.height * 0.5) / transform.scale + transform.center.y,
        )
    }
}

fn world_bounds_for_cells(cells: &[PeriodicKernelCell]) -> (Vec2, Vec2) {
    let mut minimum = Vec2::new(f64::INFINITY, f64::INFINITY);
    let mut maximum = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (_, polygon, _, _) in cells {
        for point in polygon {
            minimum.x = minimum.x.min(point.x);
            minimum.y = minimum.y.min(point.y);
            maximum.x = maximum.x.max(point.x);
            maximum.y = maximum.y.max(point.y);
        }
    }
    if !minimum.x.is_finite() {
        return (Vec2::new(-1.0, -1.0), Vec2::new(1.0, 1.0));
    }
    let margin = ((maximum.x - minimum.x).max(maximum.y - minimum.y) * 0.06).max(0.05);
    (
        Vec2::new(minimum.x - margin, minimum.y - margin),
        Vec2::new(maximum.x + margin, maximum.y + margin),
    )
}

impl GraphicsScene for PeriodicKernelScene {
    fn render_rgba(&self, width: u32, height: u32) -> GraphicsFrame {
        let width = width.max(1);
        let height = height.max(1);
        let mut rgba = vec![0_u8; width as usize * height as usize * 4];
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&[8, 8, 8, 255]);
        }
        let cells = self.cells();
        let mut magnitudes = cells
            .iter()
            .map(|(_, _, value, _)| value.abs())
            .filter(|value| value.is_finite() && *value > f32::EPSILON)
            .collect::<Vec<_>>();
        magnitudes.sort_by(f32::total_cmp);
        let scale = magnitudes
            .get(magnitudes.len().saturating_sub(1) * 9 / 10)
            .copied()
            .unwrap_or(1.0)
            .max(f32::EPSILON);
        let transform = self.pixel_transform(width, height, world_bounds_for_cells(&cells));
        for (selection, polygon, value, active) in cells {
            let intensity = ((value.abs() / scale).min(1.0) * 230.0).round() as u8;
            let color = if !active {
                [26, 26, 30, 255]
            } else if value > 0.0 {
                [12, intensity, intensity, 255]
            } else if value < 0.0 {
                [intensity, 24, 12, 255]
            } else {
                [8, 8, 8, 255]
            };
            draw_world_polygon(&mut rgba, width, height, transform, &polygon, color);
            let edge = if self.selected == Some(selection) {
                [255, 255, 255, 255]
            } else if selection.offset == [0, 0] && selection.source_basis == self.target_basis {
                [255, 220, 105, 255]
            } else {
                [70, 100, 135, 255]
            };
            draw_world_outline(&mut rgba, width, height, transform, &polygon, edge);
        }
        GraphicsFrame::new(width, height, rgba, 0).expect("valid periodic kernel frame")
    }
}

fn point_in_polygon(point: Vec2, polygon: &[Vec2]) -> bool {
    let mut inside = false;
    for index in 0..polygon.len() {
        let a = polygon[index];
        let b = polygon[(index + 1) % polygon.len()];
        if (a.y > point.y) != (b.y > point.y)
            && point.x < (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x
        {
            inside = !inside;
        }
    }
    inside
}

fn draw_world_polygon(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    transform: PeriodicPixelTransform,
    polygon: &[Vec2],
    color: [u8; 4],
) {
    let points = polygon
        .iter()
        .map(|point| transform.world_to_pixel(*point))
        .collect::<Vec<_>>();
    let minimum_y = points.iter().map(|point| point.1).min().unwrap_or(0).max(0);
    let maximum_y = points
        .iter()
        .map(|point| point.1)
        .max()
        .unwrap_or(-1)
        .min(height as i32 - 1);
    for y in minimum_y..=maximum_y {
        let scan_y = f64::from(y) + 0.5;
        let mut intersections = Vec::with_capacity(points.len());
        for index in 0..points.len() {
            let (x0, y0) = points[index];
            let (x1, y1) = points[(index + 1) % points.len()];
            let (y0, y1) = (f64::from(y0), f64::from(y1));
            if (y0 <= scan_y && scan_y < y1) || (y1 <= scan_y && scan_y < y0) {
                let ratio = (scan_y - y0) / (y1 - y0);
                intersections.push(f64::from(x0) + ratio * f64::from(x1 - x0));
            }
        }
        intersections.sort_by(f64::total_cmp);
        for pair in intersections.as_chunks::<2>().0 {
            let start = pair[0].ceil().max(0.0) as i32;
            let end = pair[1].floor().min(f64::from(width.saturating_sub(1))) as i32;
            for x in start..=end {
                set_pixel(rgba, width, height, x, y, color);
            }
        }
    }
}

fn draw_world_outline(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    transform: PeriodicPixelTransform,
    polygon: &[Vec2],
    color: [u8; 4],
) {
    for index in 0..polygon.len() {
        draw_pixel_line(
            rgba,
            width,
            height,
            transform.world_to_pixel(polygon[index]),
            transform.world_to_pixel(polygon[(index + 1) % polygon.len()]),
            color,
        );
    }
}

fn draw_pixel_line(
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
    let mut error = dx + dy;
    let limit = width.saturating_add(height).saturating_mul(4).max(1);
    for _ in 0..limit {
        set_pixel(rgba, width, height, x, y, color);
        if x == end.0 && y == end.1 {
            break;
        }
        let twice = error * 2;
        if twice >= dy {
            error += dy;
            x += sx;
        }
        if twice <= dx {
            error += dx;
            y += sy;
        }
    }
}

fn set_pixel(rgba: &mut [u8], width: u32, height: u32, x: i32, y: i32, color: [u8; 4]) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    let index = (y as usize * width as usize + x as usize) * 4;
    rgba[index..index + 4].copy_from_slice(&color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::basis_kernel::{BasisWeightPlane, PeriodicKernelDefinition};
    use crate::sim::kernel::{render_definition, ring_definition};
    use crate::sim::tiling::{BasisId, TilingPreset, build_preset};

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
        assert!(scene.definition.mask.as_ref().unwrap()[0]);
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
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[0] != 8 || p[1] != 8 || p[2] != 8)
        );
    }

    #[test]
    fn one_by_one_kernel_is_a_centered_cell_instead_of_a_solid_canvas() {
        let scene = KernelScene::new(render_definition(1, 1));
        let frame = scene.render_rgba(256, 256);
        let pixel = |x: usize, y: usize| {
            &frame.rgba[(y * frame.width as usize + x) * 4..(y * frame.width as usize + x) * 4 + 4]
        };

        assert_eq!(pixel(0, 0), &[8, 8, 8, 255]);
        assert!(pixel(128, 128)[1] > 200);
        assert_eq!(
            scene.cell_at_pixel_in(128, 128, 256, 256),
            Some(KernelPoint { x: 0, y: 0 })
        );
        assert_eq!(scene.cell_at_pixel_in(0, 0, 256, 256), None);
    }

    #[test]
    fn selection_uses_a_border_without_hiding_the_kernel_value_color() {
        let mut definition = render_definition(1, 1);
        definition.values = KernelValues::Explicit(vec![-0.2]);
        let scene = KernelScene::new(definition).with_selected(Some(KernelPoint { x: 0, y: 0 }));
        let frame = scene.render_rgba(256, 256);
        let pixel = |x: usize, y: usize| {
            &frame.rgba[(y * frame.width as usize + x) * 4..(y * frame.width as usize + x) * 4 + 4]
        };

        assert_eq!(pixel(128, 128), &[255, 51, 0, 255]);
        assert_eq!(pixel(80, 128), &[255, 255, 255, 255]);
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
            .as_chunks::<4>()
            .0
            .iter()
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
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[1] > 64 || pixel[0] > 64)
            .count();
        assert!(
            clearly_visible > 10_000,
            "an edited outlier must not flatten the whole preview; visible={clearly_visible}"
        );
    }

    #[test]
    fn periodic_heatmap_uses_actual_basis_polygons_and_round_trips_hits() {
        let tiling = build_preset(TilingPreset::OctagonSquare, 1.0);
        let definition = PeriodicKernelDefinition {
            width: 1,
            height: 1,
            anchor_x: 0,
            anchor_y: 0,
            planes: [
                (
                    BasisId(0),
                    BasisWeightPlane {
                        values: vec![0.75],
                        mask: None,
                    },
                ),
                (
                    BasisId(1),
                    BasisWeightPlane {
                        values: vec![-0.5],
                        mask: None,
                    },
                ),
            ]
            .into(),
        };
        let scene = PeriodicKernelScene::new(tiling, definition, BasisId(0));
        let frame = scene.render_rgba(640, 480);

        assert!(
            frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[1] > 120 && pixel[2] > 120)
        );
        assert!(
            frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[0] > 120 && pixel[1] < 100)
        );
        for source_basis in [BasisId(0), BasisId(1)] {
            let selection = KernelSelection {
                offset: [0, 0],
                source_basis,
            };
            let (x, y) = scene
                .pixel_for_selection(selection, 640, 480)
                .expect("basis polygon must be visible");
            assert_eq!(
                scene.selection_at_pixel(x, y, 640, 480),
                Some(selection),
                "rendered basis polygon and mouse hit-test must share geometry",
            );
        }
    }

    #[test]
    fn periodic_27x27_kernel_renders_within_interactive_budget() {
        let tiling = build_preset(TilingPreset::Square, 1.0);
        let built = ring_definition(13, 0.5, 0.15).build().unwrap();
        let definition = PeriodicKernelDefinition {
            width: built.width,
            height: built.height,
            anchor_x: built.anchor_x,
            anchor_y: built.anchor_y,
            planes: [(
                BasisId(0),
                BasisWeightPlane {
                    values: built.values,
                    mask: built.mask,
                },
            )]
            .into(),
        };
        let scene = PeriodicKernelScene::new(tiling, definition, BasisId(0));

        let started = std::time::Instant::now();
        let frame = scene.render_rgba(1280, 1024);
        let elapsed = started.elapsed();

        assert_eq!((frame.width, frame.height), (1280, 1024));
        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "27x27 periodic kernel took {elapsed:?}; graphics editing must remain interactive",
        );
    }

    #[test]
    fn periodic_selection_addresses_lattice_offset_and_source_basis() {
        let tiling = build_preset(TilingPreset::RegularHexagon, 1.0);
        let mut definition = PeriodicKernelDefinition::identity(BasisId(0));
        definition.width = 3;
        definition.height = 3;
        definition.anchor_x = 1;
        definition.anchor_y = 1;
        definition.planes.get_mut(&BasisId(0)).unwrap().values = vec![0.0; 9];
        let scene = PeriodicKernelScene::new(tiling, definition, BasisId(0));
        let selection = KernelSelection {
            offset: [1, -1],
            source_basis: BasisId(0),
        };
        let (x, y) = scene
            .pixel_for_selection(selection, 720, 540)
            .expect("non-central lattice cell must be reachable");
        assert_eq!(scene.selection_at_pixel(x, y, 720, 540), Some(selection));
    }
}
