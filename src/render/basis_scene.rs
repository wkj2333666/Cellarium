use crate::render::camera::Camera;
use crate::render::channels::{OUTSIDE_DOMAIN, Rgb8, composite_pixel};
use crate::render::workbench_graphics::{GraphicsFrame, GraphicsScene};
use crate::sim::basis_runtime::StateLayout;
use crate::sim::tiling::{BasisId, PeriodicTilingDraft, Vec2, polygon::instance_polygon};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BasisSceneView {
    Composite,
    Solo(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BasisSceneHit {
    pub x: usize,
    pub y: usize,
    pub basis: BasisId,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum BasisSceneError {
    #[error("basis scene state has {actual} values; expected {expected}")]
    StateLength { expected: usize, actual: usize },
    #[error("basis scene palette/visibility does not match its channels")]
    ChannelMetadata,
    #[error("basis scene tiling is invalid")]
    InvalidTiling,
}

#[derive(Clone, Debug)]
struct ScenePolygon {
    basis: BasisId,
    vertices: Vec<Vec2>,
}

#[derive(Clone, Debug)]
pub struct BasisStateScene {
    layout: StateLayout,
    state: Vec<f32>,
    palette: Vec<Rgb8>,
    visible: Vec<bool>,
    view: BasisSceneView,
    camera: Camera,
    generation: u64,
    translation_a: Vec2,
    translation_b: Vec2,
    polygons: Vec<ScenePolygon>,
}

impl BasisStateScene {
    #[allow(clippy::too_many_arguments)]
    pub fn from_snapshot(
        tiling: Option<&PeriodicTilingDraft>,
        layout: StateLayout,
        state: &[f32],
        palette: &[Rgb8],
        visible: &[bool],
        view: BasisSceneView,
        camera: Camera,
        generation: u64,
    ) -> Result<Self, BasisSceneError> {
        let expected = layout
            .width
            .checked_mul(layout.height)
            .and_then(|n| n.checked_mul(layout.bases.len()))
            .and_then(|n| n.checked_mul(layout.channels))
            .unwrap_or(usize::MAX);
        if state.len() != expected {
            return Err(BasisSceneError::StateLength {
                expected,
                actual: state.len(),
            });
        }
        if palette.len() != layout.channels || visible.len() != layout.channels {
            return Err(BasisSceneError::ChannelMetadata);
        }
        let (translation_a, translation_b, polygons) = if let Some(tiling) = tiling {
            let mut polygons = Vec::with_capacity(tiling.instances.len());
            for (index, instance) in tiling.instances.iter().enumerate() {
                let polygon =
                    instance_polygon(tiling, index).map_err(|_| BasisSceneError::InvalidTiling)?;
                polygons.push(ScenePolygon {
                    basis: instance.id,
                    vertices: polygon.vertices,
                });
            }
            if polygons.is_empty() {
                return Err(BasisSceneError::InvalidTiling);
            }
            (tiling.translation_a, tiling.translation_b, polygons)
        } else {
            (
                Vec2::new(1.0, 0.0),
                Vec2::new(0.0, 1.0),
                vec![ScenePolygon {
                    basis: BasisId(0),
                    vertices: vec![
                        Vec2::new(0.0, 0.0),
                        Vec2::new(1.0, 0.0),
                        Vec2::new(1.0, 1.0),
                        Vec2::new(0.0, 1.0),
                    ],
                }],
            )
        };
        if polygons
            .iter()
            .any(|polygon| !layout.bases.contains(&polygon.basis))
        {
            return Err(BasisSceneError::InvalidTiling);
        }
        Ok(Self {
            layout,
            state: state.to_vec(),
            palette: palette.to_vec(),
            visible: visible.to_vec(),
            view,
            camera,
            generation,
            translation_a,
            translation_b,
            polygons,
        })
    }

    pub fn render_frame(&self, width: u32, height: u32) -> GraphicsFrame {
        self.render_frame_while(width, height, || true)
            .expect("unconditional basis rendering cannot be cancelled")
    }

    pub fn camera(&self) -> Camera {
        self.camera
    }

    /// Resolve a framebuffer pixel through the exact polygon/lattice transform
    /// used by `render_frame_while`. This deliberately does not approximate an
    /// oblique lattice as a rectangular raster.
    pub fn hit_test(
        &self,
        pixel_x: f64,
        pixel_y: f64,
        width: u32,
        height: u32,
    ) -> Option<BasisSceneHit> {
        let width = width.max(1);
        let height = height.max(1);
        let determinant = self.translation_a.cross(self.translation_b);
        if determinant.abs() <= 1.0e-12 {
            return None;
        }
        let nominal = determinant.abs().sqrt().max(1.0e-9);
        let center = self.camera.center();
        let center_world =
            self.translation_a * center[0] as f64 + self.translation_b * center[1] as f64;
        let scale = self.camera.zoom() as f64 / nominal;
        let physical = center_world
            + Vec2::new(
                (pixel_x - width as f64 / 2.0) / scale,
                (pixel_y - height as f64 / 2.0) / scale,
            );
        let lattice_point = lattice_coordinates(
            physical,
            self.translation_a,
            self.translation_b,
            determinant,
        );

        for polygon in &self.polygons {
            let lattice_vertices = polygon
                .vertices
                .iter()
                .map(|point| {
                    lattice_coordinates(*point, self.translation_a, self.translation_b, determinant)
                })
                .collect::<Vec<_>>();
            let (min_x, max_x, min_y, max_y) = vec2_bounds(&lattice_vertices);
            let x_start = (lattice_point.x - max_x).floor() as isize;
            let x_end = (lattice_point.x - min_x).ceil() as isize;
            let y_start = (lattice_point.y - max_y).floor() as isize;
            let y_end = (lattice_point.y - min_y).ceil() as isize;
            for y in y_start..=y_end {
                if !(0..self.layout.height as isize).contains(&y) {
                    continue;
                }
                for x in x_start..=x_end {
                    if !(0..self.layout.width as isize).contains(&x) {
                        continue;
                    }
                    let local =
                        physical - self.translation_a * x as f64 - self.translation_b * y as f64;
                    if point_in_vec2_polygon(local, &polygon.vertices) {
                        return Some(BasisSceneHit {
                            x: x as usize,
                            y: y as usize,
                            basis: polygon.basis,
                        });
                    }
                }
            }
        }
        None
    }

    pub fn render_frame_while(
        &self,
        width: u32,
        height: u32,
        mut keep_rendering: impl FnMut() -> bool,
    ) -> Option<GraphicsFrame> {
        let width = width.max(1);
        let height = height.max(1);
        let mut rgba = vec![0; width as usize * height as usize * 4];
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&[
                OUTSIDE_DOMAIN.red,
                OUTSIDE_DOMAIN.green,
                OUTSIDE_DOMAIN.blue,
                255,
            ]);
        }
        let determinant = self.translation_a.cross(self.translation_b);
        let nominal = determinant.abs().sqrt().max(1.0e-9);
        let center = self.camera.center();
        let center_world =
            self.translation_a * center[0] as f64 + self.translation_b * center[1] as f64;
        let scale = self.camera.zoom() as f64 / nominal;
        let screen_polygons = self
            .polygons
            .iter()
            .map(|polygon| {
                let vertices = polygon
                    .vertices
                    .iter()
                    .map(|point| {
                        let point = *point - center_world;
                        [
                            point.x * scale + width as f64 / 2.0,
                            point.y * scale + height as f64 / 2.0,
                        ]
                    })
                    .collect::<Vec<_>>();
                let bounds = polygon_bounds(&vertices);
                (polygon.basis, vertices, bounds)
            })
            .collect::<Vec<_>>();
        for y in 0..self.layout.height {
            if !keep_rendering() {
                return None;
            }
            for x in 0..self.layout.width {
                let lattice = self.translation_a * x as f64 + self.translation_b * y as f64;
                let shift_x = lattice.x * scale;
                let shift_y = lattice.y * scale;
                for (basis, screen, bounds) in &screen_polygons {
                    let (min_x, max_x, min_y, max_y) = (
                        bounds.0 + shift_x,
                        bounds.1 + shift_x,
                        bounds.2 + shift_y,
                        bounds.3 + shift_y,
                    );
                    if max_x < 0.0 || max_y < 0.0 || min_x >= width as f64 || min_y >= height as f64
                    {
                        continue;
                    }
                    let start_x = min_x.floor().max(0.0) as usize;
                    let end_x = max_x.ceil().min(width as f64) as usize;
                    let start_y = min_y.floor().max(0.0) as usize;
                    let end_y = max_y.ceil().min(height as f64) as usize;
                    let color = self.color_at(x, y, *basis);
                    for py in start_y..end_y {
                        if !keep_rendering() {
                            return None;
                        }
                        for px in start_x..end_x {
                            if point_in_polygon(
                                [px as f64 + 0.5 - shift_x, py as f64 + 0.5 - shift_y],
                                screen,
                            ) {
                                let offset = (py * width as usize + px) * 4;
                                rgba[offset..offset + 4].copy_from_slice(&[
                                    color.red,
                                    color.green,
                                    color.blue,
                                    255,
                                ]);
                            }
                        }
                    }
                }
            }
        }
        Some(
            GraphicsFrame::new(width, height, rgba, self.generation)
                .expect("basis scene always creates a complete RGBA frame"),
        )
    }

    fn color_at(&self, x: usize, y: usize, basis: BasisId) -> Rgb8 {
        let mut values = Vec::with_capacity(self.layout.channels);
        let mut colors = Vec::with_capacity(self.layout.channels);
        for channel in 0..self.layout.channels {
            let included = self.visible[channel]
                && match self.view {
                    BasisSceneView::Composite => true,
                    BasisSceneView::Solo(selected) => channel == selected,
                };
            if !included {
                continue;
            }
            let value = self
                .layout
                .index(channel, x, y, basis)
                .and_then(|index| self.state.get(index))
                .copied()
                .unwrap_or(0.0);
            values.push(value);
            colors.push(self.palette[channel]);
        }
        composite_pixel(&values, &colors)
    }
}

impl GraphicsScene for BasisStateScene {
    fn render_rgba(&self, width: u32, height: u32) -> GraphicsFrame {
        self.render_frame(width, height)
    }
}

fn point_in_polygon(point: [f64; 2], polygon: &[[f64; 2]]) -> bool {
    let mut inside = false;
    let mut previous = polygon.len() - 1;
    for current in 0..polygon.len() {
        let a = polygon[current];
        let b = polygon[previous];
        if ((a[1] > point[1]) != (b[1] > point[1]))
            && point[0] < (b[0] - a[0]) * (point[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

fn polygon_bounds(polygon: &[[f64; 2]]) -> (f64, f64, f64, f64) {
    polygon.iter().fold(
        (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ),
        |(min_x, max_x, min_y, max_y), point| {
            (
                min_x.min(point[0]),
                max_x.max(point[0]),
                min_y.min(point[1]),
                max_y.max(point[1]),
            )
        },
    )
}

fn lattice_coordinates(point: Vec2, a: Vec2, b: Vec2, determinant: f64) -> Vec2 {
    Vec2::new(point.cross(b) / determinant, a.cross(point) / determinant)
}

fn vec2_bounds(polygon: &[Vec2]) -> (f64, f64, f64, f64) {
    polygon.iter().fold(
        (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ),
        |(min_x, max_x, min_y, max_y), point| {
            (
                min_x.min(point.x),
                max_x.max(point.x),
                min_y.min(point.y),
                max_y.max(point.y),
            )
        },
    )
}

fn point_in_vec2_polygon(point: Vec2, polygon: &[Vec2]) -> bool {
    let mut inside = false;
    let mut previous = polygon.len() - 1;
    for current in 0..polygon.len() {
        let a = polygon[current];
        let b = polygon[previous];
        if ((a.y > point.y) != (b.y > point.y))
            && point.x < (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x
        {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::camera::Camera;
    use crate::render::channels::Rgb8;
    use crate::sim::basis_runtime::StateLayout;
    use crate::sim::tiling::{BasisId, TilingPreset, build_preset};

    #[test]
    fn regular_hex_snapshot_rasterizes_true_six_sided_cells() {
        let tiling = build_preset(TilingPreset::RegularHexagon, 1.0);
        let layout = StateLayout::new(3, 3, vec![BasisId(0)], 1).unwrap();
        let mut state = vec![0.0; 9];
        state[4] = 1.0;
        let scene = BasisStateScene::from_snapshot(
            Some(&tiling),
            layout,
            &state,
            &[Rgb8::new(255, 255, 255)],
            &[true],
            BasisSceneView::Composite,
            Camera::new([1.0, 1.0], 12.0),
            1,
        )
        .unwrap();
        let frame = scene.render_rgba(80, 80);
        let lit = |x: usize, y: usize| {
            let offset = (y * 80 + x) * 4;
            frame.rgba[offset] > 200
        };
        assert!(lit(40, 40));
        assert!(lit(40, 35));
        assert!(
            !lit(34, 34),
            "a true hexagon must not fill its bounding-box corner"
        );
    }

    #[test]
    fn hit_test_uses_the_same_oblique_hex_geometry_as_rendering() {
        let tiling = build_preset(TilingPreset::RegularHexagon, 1.0);
        let layout = StateLayout::new(3, 3, vec![BasisId(0)], 1).unwrap();
        let scene = BasisStateScene::from_snapshot(
            Some(&tiling),
            layout,
            &[0.0; 9],
            &[Rgb8::new(255, 255, 255)],
            &[true],
            BasisSceneView::Composite,
            Camera::new([1.0, 1.0], 12.0),
            1,
        )
        .unwrap();

        assert_eq!(
            scene.hit_test(40.5, 40.5, 80, 80),
            Some(BasisSceneHit {
                x: 1,
                y: 1,
                basis: BasisId(0),
            })
        );
        assert_eq!(scene.hit_test(0.5, 79.5, 80, 80), None);
    }

    #[test]
    fn scene_uses_authoritative_snapshot_values() {
        let layout = StateLayout::new(2, 1, vec![BasisId(0)], 1).unwrap();
        let scene = BasisStateScene::from_snapshot(
            None,
            layout,
            &[1.0, 0.0],
            &[Rgb8::new(255, 255, 255)],
            &[true],
            BasisSceneView::Composite,
            Camera::new([1.0, 0.5], 16.0),
            1,
        )
        .unwrap();
        let frame = scene.render_rgba(32, 16);
        assert!(frame.rgba[(8 * 32 + 8) * 4] > 200);
        assert_eq!(frame.rgba[(8 * 32 + 24) * 4], 0);
    }

    #[test]
    fn obsolete_basis_frame_can_be_cancelled_between_rows() {
        let layout = StateLayout::new(64, 64, vec![BasisId(0)], 1).unwrap();
        let scene = BasisStateScene::from_snapshot(
            None,
            layout,
            &vec![0.0; 64 * 64],
            &[Rgb8::new(255, 255, 255)],
            &[true],
            BasisSceneView::Composite,
            Camera::new([32.0, 32.0], 4.0),
            1,
        )
        .unwrap();
        let mut checks = 0;
        let frame = scene.render_frame_while(256, 256, || {
            checks += 1;
            checks < 3
        });
        assert!(frame.is_none());
        assert_eq!(checks, 3);
    }
}
