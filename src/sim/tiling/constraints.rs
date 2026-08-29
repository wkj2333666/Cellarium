use super::{EdgeRef, PeriodicTilingDraft};

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

/// The pairs that already hold to within `tolerance`.
///
/// This is the exact-geometry view, used to build a constraint set the solver
/// can satisfy as written. It is a filter over [`assess_seams`], not a second
/// pairing algorithm: an assistant and a validator that disagree about which
/// edges are partners would be worse than either alone.
pub fn propose_full_edge_seams(
    draft: &PeriodicTilingDraft,
    tolerance: f64,
) -> Result<Vec<SeamProposal>, String> {
    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Err("seam tolerance must be finite and positive".into());
    }
    let assessment = super::assist::assess_seams(draft)?;
    Ok(assessment
        .candidates
        .iter()
        .filter(|candidate| candidate.score.endpoint_gap <= tolerance)
        .map(|candidate| SeamProposal {
            constraint: candidate.constraint,
            residual: candidate.score.endpoint_gap,
        })
        .collect())
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
