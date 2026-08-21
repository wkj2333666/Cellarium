use super::{PeriodicTilingDraft, TileId, Vec2, polygon::instance_polygon};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EdgeRef {
    pub tile: TileId,
    pub edge: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HalfEdge {
    pub edge: EdgeRef,
    pub start: Vec2,
    pub end: Vec2,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EdgePair {
    pub first: EdgeRef,
    pub second: EdgeRef,
    pub lattice_offset: [i32; 2],
}

pub fn canonical_half_edges(
    draft: &PeriodicTilingDraft,
    tolerance: f64,
) -> Result<Vec<EdgePair>, Vec<String>> {
    let mut edges = Vec::new();
    for (index, instance) in draft.instances.iter().enumerate() {
        let polygon = instance_polygon(draft, index)
            .map_err(|issues| issues.into_iter().map(|i| i.message).collect::<Vec<_>>())?;
        for edge in 0..polygon.vertices.len() {
            edges.push(HalfEdge {
                edge: EdgeRef {
                    tile: instance.id,
                    edge: edge as u16,
                },
                start: polygon.vertices[edge],
                end: polygon.vertices[(edge + 1) % polygon.vertices.len()],
            });
        }
    }
    let mut pairs = Vec::new();
    let mut used = vec![false; edges.len()];
    for i in 0..edges.len() {
        if used[i] {
            continue;
        }
        let mut matches = Vec::new();
        for j in (i + 1)..edges.len() {
            if used[j]
                || edges[i].edge.tile == edges[j].edge.tile
                    && edges[i].edge.edge == edges[j].edge.edge
            {
                continue;
            }
            for ax in -1..=1 {
                for by in -1..=1 {
                    let shift =
                        draft.translation_a * f64::from(ax) + draft.translation_b * f64::from(by);
                    if close(edges[i].start, edges[j].end + shift, tolerance)
                        && close(edges[i].end, edges[j].start + shift, tolerance)
                    {
                        matches.push((j, [ax, by]));
                    }
                }
            }
        }
        if matches.len() != 1 {
            return Err(vec![format!(
                "edge {:?} has {} candidates",
                edges[i].edge,
                matches.len()
            )]);
        }
        let (j, offset) = matches[0];
        used[i] = true;
        used[j] = true;
        pairs.push(EdgePair {
            first: edges[i].edge,
            second: edges[j].edge,
            lattice_offset: offset,
        });
    }
    Ok(pairs)
}

fn close(a: Vec2, b: Vec2, tolerance: f64) -> bool {
    (a - b).length() <= tolerance.max(1e-10)
}
