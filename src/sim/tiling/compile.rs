use super::{
    PeriodicTilingDraft, TileId, Vec2,
    half_edge::canonical_half_edges,
    polygon::{instance_polygon, signed_area},
};

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledTiling {
    pub tile_ids: Vec<TileId>,
    pub centers: Vec<Vec2>,
    pub areas: Vec<f64>,
    pub face_side_counts: Vec<u16>,
    pub triangles: Vec<[[f32; 2]; 3]>,
    pub offsets: Vec<usize>,
    pub neighbors: Vec<usize>,
    pub neighbor_offsets: Vec<[i32; 2]>,
}

pub fn compile_tiling(draft: &PeriodicTilingDraft) -> Result<CompiledTiling, Vec<String>> {
    let mut rows = Vec::new();
    for (index, instance) in draft.instances.iter().enumerate() {
        let polygon = instance_polygon(draft, index)
            .map_err(|issues| issues.into_iter().map(|i| i.message).collect::<Vec<_>>())?;
        let area = signed_area(&polygon.vertices).abs();
        let center = polygon.vertices.iter().fold(Vec2::ZERO, |sum, p| sum + *p)
            * (1.0 / polygon.vertices.len() as f64);
        rows.push((instance.id, center, area, polygon.vertices.len()));
    }
    rows.sort_by_key(|r| r.0);
    let pairs = canonical_half_edges(draft, 1e-9).map_err(|e| e.into_iter().collect::<Vec<_>>())?;
    let ids: Vec<TileId> = rows.iter().map(|r| r.0).collect();
    let mut entries: Vec<Vec<(usize, [i32; 2])>> = vec![Vec::new(); rows.len()];
    for pair in pairs {
        let a = ids
            .iter()
            .position(|id| *id == pair.first.tile)
            .ok_or_else(|| vec!["unknown source tile".into()])?;
        let b = ids
            .iter()
            .position(|id| *id == pair.second.tile)
            .ok_or_else(|| vec!["unknown target tile".into()])?;
        entries[a].push((b, pair.lattice_offset));
        entries[b].push((a, [-pair.lattice_offset[0], -pair.lattice_offset[1]]));
    }
    for row in &mut entries {
        row.sort_unstable();
    }
    let mut offsets = vec![0];
    let mut neighbors = Vec::new();
    let mut neighbor_offsets = Vec::new();
    for row in entries {
        for (neighbor, offset) in row {
            neighbors.push(neighbor);
            neighbor_offsets.push(offset);
        }
        offsets.push(neighbors.len());
    }
    Ok(CompiledTiling {
        tile_ids: ids,
        centers: rows.iter().map(|r| r.1).collect(),
        areas: rows.iter().map(|r| r.2).collect(),
        face_side_counts: rows.iter().map(|r| r.3 as u16).collect(),
        triangles: Vec::new(),
        offsets,
        neighbors,
        neighbor_offsets,
    })
}
