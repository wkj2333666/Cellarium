use std::collections::BTreeSet;

use super::{EdgeRef, PeriodicTilingDraft, Vec2, polygon::prototype_vertices, snap::world_edge};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeamConstraint {
    pub lhs: EdgeRef,
    pub rhs: EdgeRef,
    pub periodic_offset: [i32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeamProposal {
    pub constraint: SeamConstraint,
    pub residual: f64,
}

pub fn propose_full_edge_seams(
    draft: &PeriodicTilingDraft,
    tolerance: f64,
) -> Result<Vec<SeamProposal>, String> {
    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Err("seam tolerance must be finite and positive".into());
    }
    let determinant = draft.translation_a.cross(draft.translation_b);
    if !determinant.is_finite() || determinant.abs() <= f64::MIN_POSITIVE {
        return Err("translation vectors must span a finite non-zero period".into());
    }
    let mut edges = Vec::new();
    for instance in &draft.instances {
        let prototype = draft
            .prototypes
            .iter()
            .find(|prototype| prototype.id == instance.prototype)
            .ok_or_else(|| format!("basis {} references a missing prototype", instance.id.0))?;
        let count = prototype_vertices(&prototype.shape)
            .map_err(|issues| {
                issues
                    .into_iter()
                    .map(|issue| issue.message)
                    .collect::<Vec<_>>()
                    .join("; ")
            })?
            .len();
        for edge in 0..count {
            let (start, end) = world_edge(draft, instance, edge)
                .ok_or_else(|| "could not evaluate a boundary edge".to_string())?;
            edges.push((
                EdgeRef {
                    tile: instance.id,
                    edge: edge as u16,
                },
                start,
                end,
            ));
        }
    }
    if edges.len() > 4_096 || edges.len().saturating_mul(edges.len()) > 1_000_000 {
        return Err("seam proposal exceeds the edge-pair budget".into());
    }

    let mut candidates = Vec::new();
    for left in 0..edges.len() {
        for right in left + 1..edges.len() {
            let (lhs, lhs_start, lhs_end) = edges[left];
            let (rhs, rhs_start, rhs_end) = edges[right];
            let delta = lhs_start - rhs_end;
            let coordinates = [
                delta.cross(draft.translation_b) / determinant,
                draft.translation_a.cross(delta) / determinant,
            ];
            if coordinates.iter().any(|value| {
                !value.is_finite() || *value < f64::from(i32::MIN) || *value > f64::from(i32::MAX)
            }) {
                continue;
            }
            let offset = [coordinates[0].round() as i32, coordinates[1].round() as i32];
            let shift = lattice_shift(draft, offset);
            let residual = (lhs_start - (rhs_end + shift))
                .length()
                .max((lhs_end - (rhs_start + shift)).length())
                .max(((lhs_end - lhs_start) + (rhs_end - rhs_start)).length());
            if residual <= tolerance {
                candidates.push(SeamProposal {
                    constraint: SeamConstraint {
                        lhs,
                        rhs,
                        periodic_offset: offset,
                    },
                    residual,
                });
            }
        }
    }
    candidates.sort_by(|left, right| {
        left.residual
            .total_cmp(&right.residual)
            .then_with(|| edge_key(left.constraint).cmp(&edge_key(right.constraint)))
    });
    let mut used = BTreeSet::new();
    let mut selected = Vec::new();
    for proposal in candidates {
        if used.contains(&proposal.constraint.lhs) || used.contains(&proposal.constraint.rhs) {
            continue;
        }
        used.insert(proposal.constraint.lhs);
        used.insert(proposal.constraint.rhs);
        selected.push(proposal);
    }
    selected.sort_by_key(|proposal| edge_key(proposal.constraint));
    Ok(selected)
}

fn lattice_shift(draft: &PeriodicTilingDraft, offset: [i32; 2]) -> Vec2 {
    draft.translation_a * f64::from(offset[0]) + draft.translation_b * f64::from(offset[1])
}

fn edge_key(constraint: SeamConstraint) -> (EdgeRef, EdgeRef, [i32; 2]) {
    (constraint.lhs, constraint.rhs, constraint.periodic_offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::tiling::{TilingPreset, build_preset};

    #[test]
    fn square_proposals_pair_every_complete_edge_once() {
        let draft = build_preset(TilingPreset::Square, 1.0);
        let proposals = propose_full_edge_seams(&draft, 1e-6).unwrap();
        assert_eq!(proposals.len(), 2);
        assert!(proposals.iter().all(|proposal| proposal.residual <= 1e-9));
    }
}
