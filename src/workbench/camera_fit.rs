use crate::render::camera::Camera;
use crate::sim::experiment_model::{ExperimentSpec, GeometrySpec};
use crate::sim::tiling::{Vec2, polygon::instance_polygon};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldBounds {
    pub min: [f64; 2],
    pub max: [f64; 2],
}

impl WorldBounds {
    fn include(&mut self, point: Vec2) {
        self.min[0] = self.min[0].min(point.x);
        self.min[1] = self.min[1].min(point.y);
        self.max[0] = self.max[0].max(point.x);
        self.max[1] = self.max[1].max(point.y);
    }

    fn finite_non_degenerate(self) -> bool {
        self.min.into_iter().chain(self.max).all(f64::is_finite)
            && self.max[0] > self.min[0]
            && self.max[1] > self.min[1]
    }
}

pub fn fit_camera(bounds: WorldBounds, pixel_size: [u32; 2], margin: f64) -> Option<Camera> {
    fit_camera_in_basis(
        bounds,
        pixel_size,
        margin,
        Vec2::new(1.0, 0.0),
        Vec2::new(0.0, 1.0),
    )
}

fn fit_camera_in_basis(
    bounds: WorldBounds,
    pixel_size: [u32; 2],
    margin: f64,
    translation_a: Vec2,
    translation_b: Vec2,
) -> Option<Camera> {
    if !bounds.finite_non_degenerate()
        || pixel_size[0] == 0
        || pixel_size[1] == 0
        || !margin.is_finite()
        || !(0.0..0.5).contains(&margin)
    {
        return None;
    }
    let determinant = translation_a.cross(translation_b);
    if !determinant.is_finite() || determinant.abs() <= 1.0e-12 {
        return None;
    }
    let span_x = bounds.max[0] - bounds.min[0];
    let span_y = bounds.max[1] - bounds.min[1];
    let usable = 1.0 - margin * 2.0;
    let pixels_per_world = (f64::from(pixel_size[0]) * usable / span_x)
        .min(f64::from(pixel_size[1]) * usable / span_y);
    let nominal = determinant.abs().sqrt();
    let zoom = pixels_per_world * nominal;
    if !zoom.is_finite() || zoom <= 0.0 {
        return None;
    }
    let center_world = Vec2::new(
        (bounds.min[0] + bounds.max[0]) * 0.5,
        (bounds.min[1] + bounds.max[1]) * 0.5,
    );
    let center = [
        ((center_world.x * translation_b.y - center_world.y * translation_b.x) / determinant)
            as f32,
        ((translation_a.x * center_world.y - translation_a.y * center_world.x) / determinant)
            as f32,
    ];
    let zoom = zoom.clamp(0.1, 128.0) as f32;
    center
        .into_iter()
        .chain([zoom])
        .all(f32::is_finite)
        .then(|| Camera::new(center, zoom))
}

pub fn fit_experiment_camera(
    experiment: &ExperimentSpec,
    pixel_size: [u32; 2],
    margin: f64,
) -> Option<Camera> {
    let GeometrySpec::RasterGrid(grid) = &experiment.geometry;
    if grid.width == 0 || grid.height == 0 {
        return None;
    }
    let Some(tiling) = experiment.tiling.as_ref() else {
        return fit_camera(
            WorldBounds {
                min: [0.0, 0.0],
                max: [f64::from(grid.width), f64::from(grid.height)],
            },
            pixel_size,
            margin,
        );
    };
    let mut bounds = WorldBounds {
        min: [f64::INFINITY; 2],
        max: [f64::NEG_INFINITY; 2],
    };
    let corners = [
        (0_u32, 0_u32),
        (grid.width - 1, 0),
        (0, grid.height - 1),
        (grid.width - 1, grid.height - 1),
    ];
    for index in 0..tiling.instances.len() {
        let polygon = instance_polygon(tiling, index).ok()?;
        for (x, y) in corners {
            let shift = tiling.translation_a * f64::from(x) + tiling.translation_b * f64::from(y);
            for vertex in &polygon.vertices {
                bounds.include(*vertex + shift);
            }
        }
    }
    fit_camera_in_basis(
        bounds,
        pixel_size,
        margin,
        tiling.translation_a,
        tiling.translation_b,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_domain_fills_the_limiting_axis_with_margin() {
        let camera = fit_camera(
            WorldBounds {
                min: [0.0, 0.0],
                max: [256.0, 256.0],
            },
            [1360, 900],
            0.05,
        )
        .unwrap();
        assert_eq!(camera.center(), [128.0, 128.0]);
        assert!((camera.zoom() - 3.164_062_5).abs() < 1.0e-5);
    }

    #[test]
    fn oblique_hexagonal_domain_fits_all_physical_vertices() {
        let mut experiment = ExperimentSpec::single_channel_lenia(16, 8);
        experiment.tiling = Some(crate::sim::tiling::build_preset(
            crate::sim::tiling::TilingPreset::RegularHexagon,
            1.0,
        ));
        let camera = fit_experiment_camera(&experiment, [800, 600], 0.05).unwrap();
        assert!(camera.zoom().is_finite() && camera.zoom() > 1.0);
        assert!(camera.center().into_iter().all(f32::is_finite));
    }

    #[test]
    fn rejects_degenerate_or_non_finite_bounds() {
        assert!(
            fit_camera(
                WorldBounds {
                    min: [0.0, 0.0],
                    max: [0.0, 1.0],
                },
                [100, 100],
                0.05,
            )
            .is_none()
        );
        assert!(
            fit_camera(
                WorldBounds {
                    min: [f64::NAN, 0.0],
                    max: [1.0, 1.0],
                },
                [100, 100],
                0.05,
            )
            .is_none()
        );
    }
}
