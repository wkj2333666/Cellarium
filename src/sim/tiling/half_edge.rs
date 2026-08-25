use super::{GeometryBudget, PeriodicArrangement, PeriodicTilingDraft, TileId, Vec2};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
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
    _tolerance: f64,
) -> Result<Vec<EdgePair>, Vec<String>> {
    let arrangement = PeriodicArrangement::build(draft, GeometryBudget::authoritative()).map_err(
        |diagnostics| {
            diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.message)
                .collect::<Vec<_>>()
        },
    )?;
    Ok(arrangement
        .atomic_edges
        .iter()
        .filter(|edge| edge.id < edge.twin)
        .map(|edge| {
            let twin = &arrangement.atomic_edges[edge.twin.0];
            EdgePair {
                first: EdgeRef {
                    tile: edge.source.basis,
                    edge: edge.source.edge,
                },
                second: EdgeRef {
                    tile: twin.source.basis,
                    edge: twin.source.edge,
                },
                lattice_offset: edge.offset,
            }
        })
        .collect())
}
