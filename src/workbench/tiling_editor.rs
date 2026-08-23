use crate::render::workbench_graphics::{GraphicsFrame, GraphicsScene};
use crate::sim::tiling::{
    PeriodicTilingDraft, PrototypeId, PrototypeShape, Vec2,
    polygon::{prototype_vertices, transform_vertices, validate_polygon},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TilingTool {
    #[default]
    Select,
    DrawPolygon,
    AddNeighbor,
    ConfirmSeam,
    SplitEdge,
    Pan,
}

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
    pub construction: Vec<Vec2>,
}

impl TilingScene {
    pub fn new(draft: PeriodicTilingDraft) -> Self {
        Self {
            selected_prototype: draft.prototypes.first().map(|prototype| prototype.id),
            draft,
            selected_vertex: None,
            camera: TilingCamera::default(),
            construction: Vec::new(),
        }
    }

    pub fn with_selection(mut self, selected: Option<PrototypeId>) -> Self {
        self.selected_prototype = selected;
        self
    }

    pub fn with_construction(mut self, construction: Vec<Vec2>) -> Self {
        self.construction = construction;
        self
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
        let materialized = self
            .draft
            .prototypes
            .iter()
            .find(|entry| entry.id == prototype)
            .and_then(|entry| match &entry.shape {
                PrototypeShape::RegularPolygon { .. } => prototype_vertices(&entry.shape).ok(),
                PrototypeShape::SimplePolygon { .. } => None,
            });
        let shape = self
            .draft
            .prototypes
            .iter_mut()
            .find(|entry| entry.id == prototype)
            .ok_or_else(|| "unknown prototype".to_string())?;
        if let Some(vertices) = materialized {
            shape.shape = PrototypeShape::SimplePolygon { vertices };
        }
        match &mut shape.shape {
            PrototypeShape::SimplePolygon { vertices } => Ok(vertices),
            PrototypeShape::RegularPolygon { .. } => unreachable!("regular polygon was materialized"),
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
}

impl GraphicsScene for TilingScene {
    fn render_rgba(&self, width: u32, height: u32) -> GraphicsFrame {
        let width = width.max(1);
        let height = height.max(1);
        let mut rgba = vec![0_u8; width as usize * height as usize * 4];
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[5, 10, 24, 255]);
        }
        let mut selected_handles = None;
        let mut canonical = Vec::new();
        for instance in &self.draft.instances {
            let Some(prototype) = self
                .draft
                .prototypes
                .iter()
                .find(|prototype| prototype.id == instance.prototype)
            else { continue };
            let Ok(base) = prototype_vertices(&prototype.shape) else { continue };
            canonical.push((
                prototype.id,
                transform_vertices(&base, instance.transform),
            ));
        }

        // The editor intentionally shows one strong canonical cell and only
        // its immediate translated neighbours. Translation vectors may be
        // oblique, so hexagonal and mixed octagon/square tilings retain their
        // true geometry instead of being forced into an axis-aligned grid.
        for lattice_a in -1..=1 {
            for lattice_b in -1..=1 {
                if lattice_a == 0 && lattice_b == 0 { continue; }
                let translation = self.draft.translation_a * f64::from(lattice_a)
                    + self.draft.translation_b * f64::from(lattice_b);
                for (_, polygon) in &canonical {
                    let ghost = polygon.iter().map(|vertex| *vertex + translation).collect::<Vec<_>>();
                    draw_filled_polygon(&mut rgba, width, height, self, &ghost, [28, 66, 108, 72]);
                    draw_polygon(&mut rgba, width, height, self, &ghost, [90, 145, 205, 130]);
                }
            }
        }
        for (prototype_id, polygon) in canonical {
            let selected = self.selected_prototype == Some(prototype_id);
            let valid = validate_polygon(&polygon).is_empty();
            let (fill, edge) = if !valid {
                ([120, 20, 35, 120], [255, 70, 80, 255])
            } else if selected {
                ([150, 116, 28, 150], [255, 238, 170, 255])
            } else {
                ([24, 92, 156, 130], [100, 190, 255, 245])
            };
            draw_filled_polygon(&mut rgba, width, height, self, &polygon, fill);
            draw_polygon(&mut rgba, width, height, self, &polygon, edge);
            if selected && selected_handles.is_none() {
                selected_handles = Some(polygon);
            }
        }
        let origin = self.world_to_pixel(Vec2::ZERO, width, height);
        draw_line(&mut rgba, width, height, origin, self.world_to_pixel(self.draft.translation_a, width, height), [255, 110, 90, 230]);
        draw_line(&mut rgba, width, height, origin, self.world_to_pixel(self.draft.translation_b, width, height), [90, 220, 150, 230]);
        if !self.construction.is_empty() {
            for segment in self.construction.windows(2) {
                draw_line(
                    &mut rgba,
                    width,
                    height,
                    self.world_to_pixel(segment[0], width, height),
                    self.world_to_pixel(segment[1], width, height),
                    [255, 190, 70, 255],
                );
            }
            for point in &self.construction {
                draw_disc(&mut rgba, width, height, self.world_to_pixel(*point, width, height), 4, [255, 245, 190, 255]);
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

fn draw_filled_polygon(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    scene: &TilingScene,
    polygon: &[Vec2],
    color: [u8; 4],
) {
    if polygon.len() < 3 { return; }
    let points = polygon.iter().map(|point| scene.world_to_pixel(*point, width, height)).collect::<Vec<_>>();
    let min_y = points.iter().map(|point| point.1).min().unwrap_or(0).max(0);
    let max_y = points.iter().map(|point| point.1).max().unwrap_or(-1).min(height as i32 - 1);
    for y in min_y..=max_y {
        let scan_y = f64::from(y) + 0.5;
        let mut intersections = Vec::with_capacity(points.len());
        for index in 0..points.len() {
            let (x0, y0) = points[index];
            let (x1, y1) = points[(index + 1) % points.len()];
            let (y0, y1) = (f64::from(y0), f64::from(y1));
            if (y0 <= scan_y && scan_y < y1) || (y1 <= scan_y && scan_y < y0) {
                let t = (scan_y - y0) / (y1 - y0);
                intersections.push(f64::from(x0) + t * f64::from(x1 - x0));
            }
        }
        intersections.sort_by(|a, b| a.total_cmp(b));
        for pair in intersections.chunks_exact(2) {
            let start = pair[0].ceil().max(0.0) as i32;
            let end = pair[1].floor().min(f64::from(width.saturating_sub(1))) as i32;
            for x in start..=end {
                blend_pixel(rgba, width, height, x, y, color);
            }
        }
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
    fn hexagon_preview_has_a_strong_center_and_oblique_ghost_neighbor() {
        let draft = crate::sim::tiling::build_preset(
            crate::sim::tiling::TilingPreset::RegularHexagon,
            1.0,
        );
        let scene = TilingScene::new(draft.clone());
        let frame = scene.render_rgba(640, 480);
        let base = prototype_vertices(&draft.prototypes[0].shape).unwrap();
        let polygon = transform_vertices(&base, draft.instances[0].transform);
        let center = polygon.iter().fold(Vec2::ZERO, |sum, point| sum + *point)
            * (1.0 / polygon.len() as f64);
        let neighbor = center + draft.translation_a;
        let sample = |point: Vec2| {
            let (x, y) = scene.world_to_pixel(point, 640, 480);
            let index = (y as usize * 640 + x as usize) * 4;
            [frame.rgba[index], frame.rgba[index + 1], frame.rgba[index + 2]]
        };
        let center_color = sample(center);
        let ghost_color = sample(neighbor);
        assert_ne!(center_color, [5, 10, 24]);
        assert_ne!(ghost_color, [5, 10, 24]);
        assert_ne!(center_color, ghost_color, "ghost neighbor must be visually subordinate");
        assert!(draft.translation_a.x.abs() > 0.1 && draft.translation_a.y.abs() > 0.1);
    }

    #[test]
    fn octagon_square_preview_renders_both_canonical_basis_shapes() {
        let draft = crate::sim::tiling::build_preset(
            crate::sim::tiling::TilingPreset::OctagonSquare,
            1.0,
        );
        let scene = TilingScene::new(draft.clone());
        let frame = scene.render_rgba(640, 480);
        let mut sampled = Vec::new();
        for instance in &draft.instances {
            let prototype = draft.prototypes.iter().find(|p| p.id == instance.prototype).unwrap();
            let polygon = transform_vertices(&prototype_vertices(&prototype.shape).unwrap(), instance.transform);
            let center = polygon.iter().fold(Vec2::ZERO, |sum, point| sum + *point)
                * (1.0 / polygon.len() as f64);
            let (x, y) = scene.world_to_pixel(center, 640, 480);
            let index = (y as usize * 640 + x as usize) * 4;
            sampled.push([frame.rgba[index], frame.rgba[index + 1], frame.rgba[index + 2]]);
            assert_ne!(sampled.last().copied().unwrap(), [5, 10, 24]);
        }
        assert!(sampled.len() >= 2, "mixed tiling must expose multiple basis polygons");
    }
}
