use crate::render::workbench_graphics::{GraphicsFrame, GraphicsScene};
use crate::sim::tiling::{
    PeriodicTilingDraft, PrototypeId, PrototypeShape, Vec2,
    polygon::{prototype_vertices, transform_vertices, validate_polygon},
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
    SelectVertex { prototype: PrototypeId, vertex: usize },
    AddVertex { point: Vec2 },
    MoveVertex { prototype: PrototypeId, vertex: usize, to: Vec2 },
    RemoveVertex { prototype: PrototypeId, vertex: usize },
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
        let selected = self.selected_prototype.into_iter().chain(
            self.draft
                .prototypes
                .iter()
                .map(|prototype| prototype.id),
        );
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
            TilingGesture::MoveVertex { prototype, vertex, to } => {
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
}

impl GraphicsScene for TilingScene {
    fn render_rgba(&self, width: u32, height: u32) -> GraphicsFrame {
        let width = width.max(1);
        let height = height.max(1);
        let mut rgba = vec![0_u8; width as usize * height as usize * 4];
        let translations = [
            Vec2::ZERO,
            self.draft.translation_a,
            self.draft.translation_a * -1.0,
            self.draft.translation_b,
            self.draft.translation_b * -1.0,
            self.draft.translation_a + self.draft.translation_b,
            self.draft.translation_a - self.draft.translation_b,
            self.draft.translation_b - self.draft.translation_a,
        ];
        for prototype in &self.draft.prototypes {
            let Ok(base) = prototype_vertices(&prototype.shape) else {
                continue;
            };
            for instance in self
                .draft
                .instances
                .iter()
                .filter(|instance| instance.prototype == prototype.id)
            {
                let transformed = transform_vertices(&base, instance.transform);
                for translation in translations {
                    let polygon = transformed
                        .iter()
                        .map(|vertex| *vertex + translation)
                        .collect::<Vec<_>>();
                    let valid = validate_polygon(&polygon).is_empty();
                    let selected = self.selected_prototype == Some(prototype.id);
                    let edge = if !valid {
                        [255, 70, 80, 255]
                    } else if selected {
                        [255, 238, 170, 255]
                    } else {
                        [80, 160, 230, 220]
                    };
                    for index in 0..polygon.len() {
                        let next = (index + 1) % polygon.len();
                        draw_line(
                            &mut rgba,
                            width,
                            height,
                            self.world_to_pixel(polygon[index], width, height),
                            self.world_to_pixel(polygon[next], width, height),
                            edge,
                        );
                    }
                    if selected && translation == Vec2::ZERO {
                        for (index, vertex) in polygon.iter().enumerate() {
                            draw_disc(
                                &mut rgba,
                                width,
                                height,
                                self.world_to_pixel(*vertex, width, height),
                                if self.selected_vertex == Some(index) { 4 } else { 2 },
                                if self.selected_vertex == Some(index) {
                                    [255, 255, 255, 255]
                                } else {
                                    [255, 210, 80, 255]
                                },
                            );
                        }
                    }
                }
            }
        }
        GraphicsFrame::new(width, height, rgba, 0).expect("raster dimensions are valid")
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
            if (x - center.0) * (x - center.0) + (y - center.1) * (y - center.1)
                <= radius * radius
            {
                blend_pixel(rgba, width, height, x, y, color);
            }
        }
    }
}

fn blend_pixel(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    color: [u8; 4],
) {
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
        assert_eq!(scene.hit_test_vertex(x as u32, y as u32, 320, 240, 5), Some((PrototypeId(1), 0)));
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
        assert!(frame.rgba.chunks_exact(4).any(|pixel| pixel[0] > 0 || pixel[1] > 0 || pixel[2] > 0));
    }
}
