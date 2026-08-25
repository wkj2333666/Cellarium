use std::collections::BTreeMap;

use super::{
    GeometryBudget, PeriodicTilingDraft, TileId, Vec2, polygon::instance_polygon,
    validate_periodic_tiling,
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
    pub neighbor_contact_lengths: Vec<f64>,
}

pub fn compile_tiling(draft: &PeriodicTilingDraft) -> Result<CompiledTiling, Vec<String>> {
    let report =
        validate_periodic_tiling(draft, GeometryBudget::authoritative()).map_err(|errors| {
            errors
                .into_iter()
                .map(|error| match error.path {
                    Some(path) => format!("{} at {path}: {}", error.code, error.message),
                    None => format!("{}: {}", error.code, error.message),
                })
                .collect::<Vec<_>>()
        })?;
    let mut rows = Vec::new();
    for (index, instance) in draft.instances.iter().enumerate() {
        let polygon = instance_polygon(draft, index).map_err(|issues| {
            issues
                .into_iter()
                .map(|issue| issue.message)
                .collect::<Vec<_>>()
        })?;
        let area = polygon.unsigned_area();
        let center = polygon
            .vertices
            .iter()
            .fold(Vec2::ZERO, |sum, point| sum + *point)
            * (1.0 / polygon.vertices.len() as f64);
        rows.push((instance.id, center, area));
    }
    rows.sort_by_key(|row| row.0);
    let ids = rows.iter().map(|row| row.0).collect::<Vec<_>>();
    let dense_ids = ids
        .iter()
        .enumerate()
        .map(|(index, id)| (*id, index))
        .collect::<BTreeMap<_, _>>();
    let side_counts = report
        .arrangement
        .faces
        .iter()
        .map(|face| (face.basis, face.boundary.len()))
        .collect::<BTreeMap<_, _>>();

    let mut adjacency = vec![Vec::<(TileId, usize, [i32; 2], f64)>::new(); rows.len()];
    for edge in &report.arrangement.atomic_edges {
        let twin = &report.arrangement.atomic_edges[edge.twin.0];
        let source = *dense_ids
            .get(&edge.source.basis)
            .ok_or_else(|| vec!["unknown source basis in validated arrangement".into()])?;
        let target = *dense_ids
            .get(&twin.source.basis)
            .ok_or_else(|| vec!["unknown target basis in validated arrangement".into()])?;
        adjacency[source].push((
            twin.source.basis,
            target,
            edge.offset,
            (edge.geometry[1] - edge.geometry[0]).length(),
        ));
    }
    for entries in &mut adjacency {
        entries.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.2.cmp(&right.2))
                .then_with(|| left.1.cmp(&right.1))
        });
    }

    let mut offsets = vec![0];
    let mut neighbors = Vec::new();
    let mut neighbor_offsets = Vec::new();
    let mut neighbor_contact_lengths = Vec::new();
    for entries in adjacency {
        for (_, neighbor, offset, contact_length) in entries {
            neighbors.push(neighbor);
            neighbor_offsets.push(offset);
            neighbor_contact_lengths.push(contact_length);
        }
        offsets.push(neighbors.len());
    }
    let face_side_counts = ids
        .iter()
        .map(|id| {
            side_counts
                .get(id)
                .copied()
                .and_then(|count| u16::try_from(count).ok())
                .ok_or_else(|| vec![format!("basis {:?} has too many atomic sides", id)])
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CompiledTiling {
        tile_ids: ids,
        centers: rows.iter().map(|row| row.1).collect(),
        areas: rows.iter().map(|row| row.2).collect(),
        face_side_counts,
        triangles: Vec::new(),
        offsets,
        neighbors,
        neighbor_offsets,
        neighbor_contact_lengths,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::tiling::{TilingPreset, build_preset, validate_coverage};

    #[test]
    fn validated_atomic_arrangement_compiles_deterministically_to_csr() {
        let draft = build_preset(TilingPreset::OctagonSquare, 1.0);
        let report = validate_coverage(&draft).unwrap();
        let first = compile_tiling(&draft).unwrap();
        let second = compile_tiling(&draft).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.neighbors.len(), report.atomic_edges);
        assert_eq!(first.neighbors.len(), first.neighbor_offsets.len());
        assert_eq!(first.neighbors.len(), first.neighbor_contact_lengths.len());
        assert!(
            first
                .neighbor_contact_lengths
                .iter()
                .all(|length| length.is_finite() && *length > 0.0)
        );

        let source = first
            .tile_ids
            .iter()
            .position(|id| *id == TileId(0))
            .unwrap();
        let targets = first.neighbors[first.offsets[source]..first.offsets[source + 1]]
            .iter()
            .map(|target| first.tile_ids[*target])
            .collect::<std::collections::BTreeSet<_>>();
        assert!(targets.contains(&TileId(1)));
    }
}
