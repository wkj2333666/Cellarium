use crate::render::workbench_graphics::{GraphicsFrame, GraphicsScene};
use crate::sim::tiling::{
    PeriodicTilingDraft, PrototypeId, PrototypeShape, Vec2,
    polygon::{MAX_POLYGON_VERTICES, prototype_vertices, transform_vertices, validate_polygon},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TilingCamera {
    pub center: Vec2,
    pub scale: f64,
}

impl Default for TilingCamera {
    fn default() -> Self {
        Self {
            center: Vec2::ZERO,
            scale: 64.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TilingGesture {
    SelectVertex {
        prototype: PrototypeId,
        vertex: usize,
    },
    AddVertex {
        point: Vec2,
    },
    MoveVertex {
        prototype: PrototypeId,
        vertex: usize,
        to: Vec2,
    },
    RemoveVertex {
        prototype: PrototypeId,
        vertex: usize,
    },
    FinishPolygon,
}

#[derive(Clone, Debug)]
pub struct TilingScene {
    pub draft: PeriodicTilingDraft,
    pub selected_prototype: Option<PrototypeId>,
    pub selected_vertex: Option<usize>,
    pub camera: TilingCamera,
}

impl TilingScene {
    pub fn new(draft: PeriodicTilingDraft) -> Self {
        Self {
            selected_prototype: draft.prototypes.first().map(|prototype| prototype.id),
            draft,
            selected_vertex: None,
            camera: TilingCamera::default(),
        }
    }

    pub fn world_to_pixel(&self, point: Vec2, width: u32, height: u32) -> (i32, i32) {
        let x = (f64::from(width) * 0.5 + (point.x - self.camera.center.x) * self.camera.scale)
            .round() as i32;
        let y = (f64::from(height) * 0.5 + (point.y - self.camera.center.y) * self.camera.scale)
            .round() as i32;
        (x, y)
    }

    pub fn pixel_to_world(&self, x: u32, y: u32, width: u32, height: u32) -> Vec2 {
        Vec2::new(
            (f64::from(x) - f64::from(width) * 0.5) / self.camera.scale + self.camera.center.x,
            (f64::from(y) - f64::from(height) * 0.5) / self.camera.scale + self.camera.center.y,
        )
    }

    pub fn hit_test_vertex(
        &self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        radius: i32,
    ) -> Option<(PrototypeId, usize)> {
        let radius = f64::from(radius.max(1));
        let selected = self
            .selected_prototype
            .into_iter()
            .chain(self.draft.prototypes.iter().map(|prototype| prototype.id));
        for prototype_id in selected {
            let Some(vertices) = self.prototype_vertices(prototype_id) else {
                continue;
            };
            for (index, vertex) in vertices.iter().enumerate() {
                let (vx, vy) = self.world_to_pixel(*vertex, width, height);
                let dx = f64::from(vx - i32::try_from(x).unwrap_or(i32::MAX));
                let dy = f64::from(vy - i32::try_from(y).unwrap_or(i32::MAX));
                if dx * dx + dy * dy <= radius * radius {
                    return Some((prototype_id, index));
                }
            }
        }
        None
    }

    pub fn apply_gesture(&mut self, gesture: TilingGesture) -> Result<(), String> {
        match gesture {
            TilingGesture::SelectVertex { prototype, vertex } => {
                let vertices = self
                    .prototype_vertices(prototype)
                    .ok_or_else(|| "unknown prototype".to_string())?;
                if vertex >= vertices.len() {
                    return Err("unknown polygon vertex".into());
                }
                self.selected_prototype = Some(prototype);
                self.selected_vertex = Some(vertex);
                Ok(())
            }
            TilingGesture::AddVertex { point } => {
                let prototype = self.selected_prototype.ok_or("no selected prototype")?;
                let shape = self.simple_polygon_mut(prototype)?;
                shape.push(point);
                if let Some(issue) = validate_polygon(shape).into_iter().next() {
                    shape.pop();
                    return Err(issue.message);
                }
                self.selected_vertex = Some(shape.len() - 1);
                Ok(())
            }
            TilingGesture::MoveVertex {
                prototype,
                vertex,
                to,
            } => {
                let shape = self.simple_polygon_mut(prototype)?;
                let Some(previous) = shape.get(vertex).copied() else {
                    return Err("unknown polygon vertex".into());
                };
                shape[vertex] = to;
                if let Some(issue) = validate_polygon(shape).into_iter().next() {
                    shape[vertex] = previous;
                    return Err(issue.message);
                }
                self.selected_prototype = Some(prototype);
                self.selected_vertex = Some(vertex);
                Ok(())
            }
            TilingGesture::RemoveVertex { prototype, vertex } => {
                let shape = self.simple_polygon_mut(prototype)?;
                if shape.len() <= 3 || vertex >= shape.len() {
                    return Err("a polygon needs at least three vertices".into());
                }
                let removed = shape.remove(vertex);
                if let Some(issue) = validate_polygon(shape).into_iter().next() {
                    shape.insert(vertex, removed);
                    return Err(issue.message);
                }
                self.selected_vertex = None;
                Ok(())
            }
            TilingGesture::FinishPolygon => {
                let prototype = self.selected_prototype.ok_or("no selected prototype")?;
                let vertices = self
                    .prototype_vertices(prototype)
                    .ok_or_else(|| "unknown prototype".to_string())?;
                validate_polygon(&vertices)
                    .into_iter()
                    .next()
                    .map_or(Ok(()), |issue| Err(issue.message))
            }
        }
    }

    fn simple_polygon_mut(&mut self, prototype: PrototypeId) -> Result<&mut Vec<Vec2>, String> {
        let shape = self
            .draft
            .prototypes
            .iter_mut()
            .find(|entry| entry.id == prototype)
            .ok_or_else(|| "unknown prototype".to_string())?;
        match &mut shape.shape {
            PrototypeShape::SimplePolygon { vertices } => Ok(vertices),
            PrototypeShape::RegularPolygon { .. } => {
                Err("regular polygons must be converted to a custom polygon before editing".into())
            }
        }
    }

    fn prototype_vertices(&self, prototype: PrototypeId) -> Option<Vec<Vec2>> {
        let shape = self
            .draft
            .prototypes
            .iter()
            .find(|entry| entry.id == prototype)?;
        let base = prototype_vertices(&shape.shape).ok()?;
        let instance = self
            .draft
            .instances
            .iter()
            .find(|instance| instance.prototype == prototype);
        Some(instance.map_or(base.clone(), |instance| {
            transform_vertices(&base, instance.transform)
        }))
    }
    fn visible_lattice_bounds(
        &self,
        polygon: &[Vec2],
        width: u32,
        height: u32,
    ) -> Option<([i32; 2], [i32; 2])> {
        let a = self.draft.translation_a;
        let b = self.draft.translation_b;
        let det = a.cross(b);
        if !det.is_finite() || det.abs() <= 1e-12 {
            return None;
        }
        let viewport = [
            self.pixel_to_world(0, 0, width, height),
            self.pixel_to_world(width, 0, width, height),
            self.pixel_to_world(0, height, width, height),
            self.pixel_to_world(width, height, width, height),
        ];
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for corner in viewport {
            for vertex in polygon {
                let delta = corner - *vertex;
                let coordinates = [delta.cross(b) / det, a.cross(delta) / det];
                for axis in 0..2 {
                    min[axis] = min[axis].min(coordinates[axis]);
                    max[axis] = max[axis].max(coordinates[axis]);
                }
            }
        }
        let integer = |value: f64| value.clamp(-1_000_000.0, 1_000_000.0) as i32;
        Some((
            [integer(min[0].floor()) - 1, integer(min[1].floor()) - 1],
            [integer(max[0].ceil()) + 1, integer(max[1].ceil()) + 1],
        ))
    }
}

const MAX_TILING_EDGE_SEGMENTS: usize = 16_384;

fn sampled_lattice_points(lower: [i32; 2], upper: [i32; 2], max_points: usize) -> Vec<[i32; 2]> {
    if max_points == 0 {
        return Vec::new();
    }
    let count_a = i64::from(upper[0]) - i64::from(lower[0]) + 1;
    let count_b = i64::from(upper[1]) - i64::from(lower[1]) + 1;
    let mut stride = 1_i64;
    while ((count_a + stride - 1) / stride) * ((count_b + stride - 1) / stride) > max_points as i64
    {
        stride *= 2;
    }
    let mut points = Vec::with_capacity(max_points.min(4096));
    for lattice_a in (lower[0]..=upper[0]).step_by(stride as usize) {
        for lattice_b in (lower[1]..=upper[1]).step_by(stride as usize) {
            // The canonical copy is handled separately so it can never be
            // skipped by the adaptive stride.
            if lattice_a == 0 && lattice_b == 0 {
                continue;
            }
            points.push([lattice_a, lattice_b]);
            if points.len() == max_points {
                return points;
            }
        }
    }
    points
}

impl GraphicsScene for TilingScene {
    fn render_rgba(&self, width: u32, height: u32) -> GraphicsFrame {
        let width = width.max(1);
        let height = height.max(1);
        let mut rgba = vec![0_u8; width as usize * height as usize * 4];
        // Budget edge work across the entire scene, not per instance. This
        // keeps tiny legal lattice periods and many-sided prototypes from
        // monopolising the UI thread.
        let mut remaining_edges = MAX_TILING_EDGE_SEGMENTS - MAX_POLYGON_VERTICES;
        let mut selected_edge_reserve = MAX_POLYGON_VERTICES;
        let mut selected_handles = None;
        for prototype in &self.draft.prototypes {
            let Ok(base) = prototype_vertices(&prototype.shape) else {
                continue;
            };
            let selected = self.selected_prototype == Some(prototype.id);
            let mut selected_canonical_drawn = false;
            for instance in self
                .draft
                .instances
                .iter()
                .filter(|instance| instance.prototype == prototype.id)
            {
                let transformed = transform_vertices(&base, instance.transform);
                let Some((lower, upper)) = self.visible_lattice_bounds(&transformed, width, height)
                else {
                    continue;
                };
                let edge_count = transformed.len().max(1);
                let origin_visible =
                    lower[0] <= 0 && upper[0] >= 0 && lower[1] <= 0 && upper[1] >= 0;
                if origin_visible
                    && ((selected
                        && !selected_canonical_drawn
                        && selected_edge_reserve >= edge_count)
                        || (!selected && remaining_edges >= edge_count))
                {
                    let valid = validate_polygon(&transformed).is_empty();
                    let edge = if !valid {
                        [255, 70, 80, 255]
                    } else if selected {
                        [255, 238, 170, 255]
                    } else {
                        [80, 160, 230, 220]
                    };
                    draw_polygon(&mut rgba, width, height, self, &transformed, edge);
                    if selected && !selected_canonical_drawn {
                        selected_edge_reserve -= edge_count;
                        selected_handles = Some(transformed.clone());
                        selected_canonical_drawn = true;
                    } else {
                        remaining_edges = remaining_edges.saturating_sub(edge_count);
                    }
                }
                let max_polygons = remaining_edges / edge_count;
                for [lattice_a, lattice_b] in sampled_lattice_points(lower, upper, max_polygons) {
                    let translation = self.draft.translation_a * f64::from(lattice_a)
                        + self.draft.translation_b * f64::from(lattice_b);
                    let polygon = transformed
                        .iter()
                        .map(|vertex| *vertex + translation)
                        .collect::<Vec<_>>();
                    let valid = validate_polygon(&polygon).is_empty();
                    let edge = if !valid {
                        [255, 70, 80, 255]
                    } else if selected {
                        [255, 238, 170, 255]
                    } else {
                        [80, 160, 230, 220]
                    };
                    draw_polygon(&mut rgba, width, height, self, &polygon, edge);
                    remaining_edges = remaining_edges.saturating_sub(edge_count);
                }
            }
        }
        if let Some(vertices) = selected_handles {
            for (index, vertex) in vertices.iter().enumerate() {
                draw_disc(
                    &mut rgba,
                    width,
                    height,
                    self.world_to_pixel(*vertex, width, height),
                    if self.selected_vertex == Some(index) {
                        4
                    } else {
                        2
                    },
                    if self.selected_vertex == Some(index) {
                        [255, 255, 255, 255]
                    } else {
                        [255, 210, 80, 255]
                    },
                );
            }
        }
        GraphicsFrame::new(width, height, rgba, 0).expect("raster dimensions are valid")
    }
}

fn draw_polygon(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    scene: &TilingScene,
    polygon: &[Vec2],
    edge: [u8; 4],
) {
    for index in 0..polygon.len() {
        let next = (index + 1) % polygon.len();
        draw_line(
            rgba,
            width,
            height,
            scene.world_to_pixel(polygon[index], width, height),
            scene.world_to_pixel(polygon[next], width, height),
            edge,
        );
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
    let Some((start, end)) = clip_line(start, end, width, height) else {
        return;
    };
    let mut x0 = start.0;
    let mut y0 = start.1;
    let dx = (end.0 - x0).abs();
    let sx = if x0 < end.0 { 1 } else { -1 };
    let dy = -(end.1 - y0).abs();
    let sy = if y0 < end.1 { 1 } else { -1 };
    let mut error = dx + dy;
    loop {
        blend_pixel(rgba, width, height, x0, y0, color);
        if x0 == end.0 && y0 == end.1 {
            break;
        }
        let twice = 2 * error;
        if twice >= dy {
            error += dy;
            x0 += sx;
        }
        if twice <= dx {
            error += dx;
            y0 += sy;
        }
    }
}

fn clip_line(
    start: (i32, i32),
    end: (i32, i32),
    width: u32,
    height: u32,
) -> Option<((i32, i32), (i32, i32))> {
    let (x0, y0) = (f64::from(start.0), f64::from(start.1));
    let (dx, dy) = (f64::from(end.0) - x0, f64::from(end.1) - y0);
    let mut enter = 0.0_f64;
    let mut leave = 1.0_f64;
    for (p, q) in [
        (-dx, x0),
        (dx, f64::from(width.saturating_sub(1)) - x0),
        (-dy, y0),
        (dy, f64::from(height.saturating_sub(1)) - y0),
    ] {
        if p.abs() <= f64::EPSILON {
            if q < 0.0 {
                return None;
            }
            continue;
        }
        let ratio = q / p;
        if p < 0.0 {
            enter = enter.max(ratio);
        } else {
            leave = leave.min(ratio);
        }
        if enter > leave {
            return None;
        }
    }
    let point = |t: f64| ((x0 + t * dx).round() as i32, (y0 + t * dy).round() as i32);
    Some((point(enter), point(leave)))
}

fn draw_disc(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    center: (i32, i32),
    radius: i32,
    color: [u8; 4],
) {
    for y in center.1 - radius..=center.1 + radius {
        for x in center.0 - radius..=center.0 + radius {
            if (x - center.0) * (x - center.0) + (y - center.1) * (y - center.1) <= radius * radius
            {
                blend_pixel(rgba, width, height, x, y, color);
            }
        }
    }
}

fn blend_pixel(rgba: &mut [u8], width: u32, height: u32, x: i32, y: i32, color: [u8; 4]) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    let index = (y as usize * width as usize + x as usize) * 4;
    let alpha = f32::from(color[3]) / 255.0;
    for channel in 0..3 {
        rgba[index + channel] = (f32::from(rgba[index + channel]) * (1.0 - alpha)
            + f32::from(color[channel]) * alpha)
            .round() as u8;
    }
    rgba[index + 3] = 255;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::tiling::{RigidTransform, TileId, TileInstance, TilePrototype, TilingMode};

    fn square() -> PeriodicTilingDraft {
        PeriodicTilingDraft {
            translation_a: Vec2::new(4.0, 0.0),
            translation_b: Vec2::new(0.0, 4.0),
            prototypes: vec![TilePrototype {
                id: PrototypeId(1),
                name: "square".into(),
                shape: PrototypeShape::SimplePolygon {
                    vertices: vec![
                        Vec2::new(-1.0, -1.0),
                        Vec2::new(1.0, -1.0),
                        Vec2::new(1.0, 1.0),
                        Vec2::new(-1.0, 1.0),
                    ],
                },
            }],
            instances: vec![TileInstance {
                id: TileId(1),
                prototype: PrototypeId(1),
                transform: RigidTransform::default(),
            }],
            mode: TilingMode::Geometric,
        }
    }

    #[test]
    fn world_pixel_mapping_round_trips() {
        let scene = TilingScene::new(square());
        let point = Vec2::new(0.75, -0.5);
        let (x, y) = scene.world_to_pixel(point, 320, 240);
        let mapped = scene.pixel_to_world(x as u32, y as u32, 320, 240);
        assert!((mapped.x - point.x).abs() < 0.02);
        assert!((mapped.y - point.y).abs() < 0.02);
    }

    #[test]
    fn vertex_hit_testing_finds_the_selected_prototype_vertex() {
        let scene = TilingScene::new(square());
        let (x, y) = scene.world_to_pixel(Vec2::new(-1.0, -1.0), 320, 240);
        assert_eq!(
            scene.hit_test_vertex(x as u32, y as u32, 320, 240, 5),
            Some((PrototypeId(1), 0))
        );
    }

    #[test]
    fn moving_a_vertex_updates_the_polygon_and_rejects_self_intersection() {
        let mut scene = TilingScene::new(square());
        scene
            .apply_gesture(TilingGesture::MoveVertex {
                prototype: PrototypeId(1),
                vertex: 0,
                to: Vec2::new(-0.5, -0.5),
            })
            .unwrap();
        let PrototypeShape::SimplePolygon { vertices } = &scene.draft.prototypes[0].shape else {
            panic!("expected custom polygon");
        };
        assert_eq!(vertices[0], Vec2::new(-0.5, -0.5));

        let error = scene.apply_gesture(TilingGesture::MoveVertex {
            prototype: PrototypeId(1),
            vertex: 0,
            to: Vec2::new(2.0, 2.0),
        });
        assert!(error.is_err());
    }

    #[test]
    fn graphics_scene_renders_geometry_pixels() {
        let scene = TilingScene::new(square());
        let frame = scene.render_rgba(160, 120);
        assert_eq!(frame.rgba.len(), 160 * 120 * 4);
        assert!(
            frame
                .rgba
                .chunks_exact(4)
                .any(|pixel| pixel[0] > 0 || pixel[1] > 0 || pixel[2] > 0)
        );
    }

    #[test]
    fn small_period_preview_still_fills_the_viewport() {
        let scene = TilingScene::new(crate::sim::tiling::build_preset(
            crate::sim::tiling::TilingPreset::Square,
            0.05,
        ));
        let frame = scene.render_rgba(464, 512);
        for (name, x0, y0, x1, y1) in [
            ("top-left", 0, 0, 232, 256),
            ("top-right", 232, 0, 464, 256),
            ("bottom-left", 0, 256, 232, 512),
            ("bottom-right", 232, 256, 464, 512),
        ] {
            let lit = (y0..y1)
                .flat_map(|y| (x0..x1).map(move |x| (y * 464 + x) * 4))
                .filter(|index| {
                    frame.rgba[*index..*index + 3]
                        .iter()
                        .any(|value| *value > 0)
                })
                .count();
            assert!(lit > 500, "{name} preview is empty at minimum scale");
        }
    }

    #[test]
    fn lattice_sampling_obeys_budget_and_keeps_canonical_handle() {
        let points = sampled_lattice_points([-1_000_000, -1_000_000], [1_000_000, 1_000_000], 256);
        assert!(points.len() <= 256);
        assert!(!points.contains(&[0, 0]));

        let mut scene = TilingScene::new(crate::sim::tiling::build_preset(
            crate::sim::tiling::TilingPreset::Square,
            0.05,
        ));
        scene.selected_vertex = Some(0);
        let frame = scene.render_rgba(160, 120);
        assert!(
            frame
                .rgba
                .chunks_exact(4)
                .any(|pixel| pixel[..3] == [255, 255, 255]),
            "the canonical selected handle must survive adaptive sampling"
        );
    }

    #[test]
    fn huge_offscreen_lines_are_clipped_before_rasterization() {
        let mut rgba = vec![0_u8; 64 * 64 * 4];
        draw_line(
            &mut rgba,
            64,
            64,
            (-1_000_000_000, 32),
            (1_000_000_000, 32),
            [255, 255, 255, 255],
        );
        assert_eq!(
            rgba.chunks_exact(4).filter(|pixel| pixel[0] == 255).count(),
            64
        );
    }

    #[test]
    fn periodic_preview_fills_all_canvas_quadrants() {
        let scene = TilingScene::new(crate::sim::tiling::build_preset(
            crate::sim::tiling::TilingPreset::Square,
            1.0,
        ));
        let frame = scene.render_rgba(464, 512);
        let lit = |x0: usize, y0: usize, x1: usize, y1: usize| {
            (y0..y1)
                .flat_map(|y| (x0..x1).map(move |x| (y * 464 + x) * 4))
                .filter(|index| {
                    frame.rgba[*index..*index + 3]
                        .iter()
                        .any(|value| *value > 0)
                })
                .count()
        };
        assert!(lit(0, 0, 232, 256) > 500, "top-left preview is empty");
        assert!(lit(232, 0, 464, 256) > 500, "top-right preview is empty");
        assert!(lit(0, 256, 232, 512) > 500, "bottom-left preview is empty");
        assert!(
            lit(232, 256, 464, 512) > 500,
            "bottom-right preview is empty"
        );
    }
}
