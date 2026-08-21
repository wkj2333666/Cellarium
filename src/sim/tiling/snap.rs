use super::{
    EdgeRef, PeriodicTilingDraft, RigidTransform, TileInstance, Vec2, polygon::prototype_vertices,
    polygon::transform_vertices,
};

#[derive(Clone, Debug, PartialEq)]
pub struct SnapResult {
    pub instance: TileInstance,
    pub fixed: EdgeRef,
    pub moving: EdgeRef,
}

pub fn world_edge(
    draft: &PeriodicTilingDraft,
    instance: &TileInstance,
    edge: usize,
) -> Option<(Vec2, Vec2)> {
    let prototype = draft
        .prototypes
        .iter()
        .find(|p| p.id == instance.prototype)?;
    let vertices = transform_vertices(
        &prototype_vertices(&prototype.shape).ok()?,
        instance.transform,
    );
    if vertices.len() < 2 {
        return None;
    }
    Some((
        vertices[edge % vertices.len()],
        vertices[(edge + 1) % vertices.len()],
    ))
}

pub fn snap_edge(
    draft: &PeriodicTilingDraft,
    fixed: &TileInstance,
    fixed_edge: usize,
    moving: &TileInstance,
    moving_edge: usize,
    tolerance: f64,
) -> Option<SnapResult> {
    let (fa, fb) = world_edge(draft, fixed, fixed_edge)?;
    let (ma, mb) = world_edge(draft, moving, moving_edge)?;
    let fv = fb - fa;
    let mv = mb - ma;
    if (fv.length() - mv.length()).abs() > tolerance.max(1e-12) {
        return None;
    }
    let angle = fv.y.atan2(fv.x);
    let moving_angle = mv.y.atan2(mv.x);
    let rotation_delta = angle + std::f64::consts::PI - moving_angle;
    let fixed_mid = (fa + fb) * 0.5;
    let new_rotation = moving.transform.rotation + rotation_delta;
    let prototype = draft.prototypes.iter().find(|p| p.id == moving.prototype)?;
    let local = prototype_vertices(&prototype.shape).ok()?;
    let local_mid =
        (local[moving_edge % local.len()] + local[(moving_edge + 1) % local.len()]) * 0.5;
    let (ns, nc) = new_rotation.sin_cos();
    let rotated_mid = Vec2::new(
        nc * local_mid.x - ns * local_mid.y,
        ns * local_mid.x + nc * local_mid.y,
    );
    let transform = RigidTransform {
        rotation: new_rotation,
        translation: fixed_mid - rotated_mid,
    };
    let changed = TileInstance {
        transform,
        ..moving.clone()
    };
    let (na, nb) = world_edge(draft, &changed, moving_edge)?;
    if !((na - fb).length() <= tolerance.max(1e-9) && (nb - fa).length() <= tolerance.max(1e-9)) {
        return None;
    }
    Some(SnapResult {
        instance: changed,
        fixed: EdgeRef {
            tile: fixed.id,
            edge: fixed_edge as u16,
        },
        moving: EdgeRef {
            tile: moving.id,
            edge: moving_edge as u16,
        },
    })
}
