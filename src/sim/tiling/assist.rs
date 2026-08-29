//! The seam assistant.
//!
//! The job here is to be useful about a drawing that is *wrong*. A validator
//! answers "do these edges meet"; an assistant answers "which edges were you
//! trying to meet, how far apart are they, and which way does this one go".
//!
//! Every boundary edge therefore leaves this module accounted for. It is
//! either half of a candidate pair — held, ready, or merely near — or it is an
//! orphan carrying the reason nothing could be paired with it. Silence is not
//! one of the outcomes: an assistant that says nothing when the drawing is
//! rough is an assistant that only speaks when it is not needed.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    EdgeRef, PeriodicTilingDraft, SeamConstraint, Vec2, polygon::prototype_vertices,
    snap::world_edge,
};

/// How near two endpoints must be, relative to the tiling's characteristic
/// edge length, before the pair counts as already closed.
const HELD_FRACTION: f64 = 1e-6;

/// How near two endpoints must be, relative to the same length, before closing
/// them counts as fixing a drawing inaccuracy rather than moving the design.
/// A quarter of an edge is deliberately generous: a pointer is not precise,
/// and a candidate offered wrongly costs one glance, while a candidate withheld
/// costs the user the whole feature.
const READY_FRACTION: f64 = 0.25;

/// Two edges must point within this much of opposite to be the same seam.
/// `1.0` is exactly opposite, `0.0` is perpendicular.
const OPPOSITION_FLOOR: f64 = 0.5;

/// The shorter edge must be at least this fraction of the longer one.
const LENGTH_FLOOR: f64 = 0.5;

/// Where a candidate pair stands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SeamBucket {
    /// Already closed to solver tolerance.
    Held,
    /// Close enough that accepting it repairs a drawing inaccuracy.
    Ready,
    /// A plausible partner that needs a deliberate move. Offered, not hidden.
    Near,
}

impl SeamBucket {
    /// A column heading: one word, for a table of counts.
    pub fn label(self) -> &'static str {
        match self {
            SeamBucket::Held => "held",
            SeamBucket::Ready => "ready",
            SeamBucket::Near => "near",
        }
    }

    /// The same state as part of a sentence. "2 ready" reads as jargon in a
    /// toolbar; "2 ready to close" says what pressing the button will do.
    pub fn phrase(self) -> &'static str {
        match self {
            SeamBucket::Held => "already closed",
            SeamBucket::Ready => "ready to close",
            SeamBucket::Near => "far apart",
        }
    }
}

/// The four signals the geometry spec names, each reported rather than folded
/// away, so a ranking can be checked instead of trusted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeamScore {
    /// World distance the further of the two endpoint pairs must close.
    pub endpoint_gap: f64,
    /// `1.0` when the edges point exactly against each other.
    pub opposition: f64,
    /// Shorter edge over longer edge; `1.0` when they match.
    pub length_ratio: f64,
    /// How far the pair sits from a whole number of lattice steps, in steps.
    /// `0.0` is consistent, `0.5` is as inconsistent as an offset can be.
    pub offset_error: f64,
    /// The three signals above combined into `0.0..=1.0`, higher is better.
    pub confidence: f64,
}

/// One proposed seam, with what it would take to close it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeamCandidate {
    pub constraint: SeamConstraint,
    pub bucket: SeamBucket,
    pub score: SeamScore,
    /// Vector from the first endpoint of the left edge to the partner it must
    /// meet. This is the hint: its length is how far, its heading is which way.
    pub gap_start: Vec2,
    /// The same for the second endpoint.
    pub gap_end: Vec2,
}

impl SeamCandidate {
    /// The single move that best describes closing this seam.
    pub fn hint(self) -> Vec2 {
        (self.gap_start + self.gap_end) * 0.5
    }
}

/// Why an edge could not be paired with anything.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OrphanReason {
    /// There is no second edge to pair with at all.
    NothingToPairWith,
    /// Nothing points back at this edge.
    NoOpposingEdge,
    /// The best opposing edge is a different length.
    LengthMismatch {
        nearest: EdgeRef,
        nearest_length: f64,
    },
    /// A partner exists, but it scored better against another edge.
    PartnerTaken { nearest: EdgeRef },
}

/// A boundary edge left without a seam.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrphanEdge {
    pub edge: EdgeRef,
    pub length: f64,
    pub reason: OrphanReason,
}

impl OrphanEdge {
    /// A sentence naming what is wrong and what would fix it.
    pub fn describe(self) -> String {
        let edge = describe_edge(self.edge);
        match self.reason {
            OrphanReason::NothingToPairWith => {
                format!("{edge} is the only edge here; a seam needs two")
            }
            OrphanReason::NoOpposingEdge => format!(
                "{edge} has no edge pointing back at it; a seam joins two edges drawn in \
                 opposite directions"
            ),
            OrphanReason::LengthMismatch {
                nearest,
                nearest_length,
            } => format!(
                "{edge} is {:.3} long and the closest edge facing it, {}, is {nearest_length:.3}; \
                 make them the same length",
                self.length,
                describe_edge(nearest)
            ),
            OrphanReason::PartnerTaken { nearest } => format!(
                "{edge} would pair with {}, but that edge matched another better; give {edge} its \
                 own partner",
                describe_edge(nearest)
            ),
        }
    }
}

/// How far a seam is from closing in a given drawing, in world units.
///
/// A held constraint records *which* edges belong together, not where they
/// are. After the geometry moves the pairing may no longer close, and the user
/// is owed the number rather than a refusal.
pub fn constraint_gap(draft: &PeriodicTilingDraft, constraint: SeamConstraint) -> Option<f64> {
    let edges = boundary_edges(draft).ok()?;
    let locate = |reference: EdgeRef| {
        edges
            .iter()
            .find(|edge| edge.reference == reference)
            .map(|edge| (edge.start, edge.end))
    };
    let (lhs_start, lhs_end) = locate(constraint.lhs)?;
    let (rhs_start, rhs_end) = locate(constraint.rhs)?;
    let shift = draft.translation_a * f64::from(constraint.periodic_offset[0])
        + draft.translation_b * f64::from(constraint.periodic_offset[1]);
    let gap = ((rhs_end + shift) - lhs_start)
        .length()
        .max(((rhs_start + shift) - lhs_end).length());
    gap.is_finite().then_some(gap)
}

/// Whether a seam still closes, judged against the drawing's own scale so the
/// answer does not change when the whole tiling is scaled up or down.
pub fn constraint_closes(draft: &PeriodicTilingDraft, constraint: SeamConstraint) -> bool {
    let Some(gap) = constraint_gap(draft, constraint) else {
        return false;
    };
    let scale = boundary_edges(draft)
        .map(|edges| characteristic_length(&edges))
        .unwrap_or(1.0);
    gap <= HELD_FRACTION * scale
}

fn describe_edge(edge: EdgeRef) -> String {
    format!("edge {} of basis {}", edge.edge, edge.tile.0)
}

/// Everything the assistant has to say about a draft.
#[derive(Clone, Debug, PartialEq)]
pub struct SeamAssessment {
    /// Best first.
    pub candidates: Vec<SeamCandidate>,
    pub orphans: Vec<OrphanEdge>,
    /// Every boundary edge is in exactly one of the two lists above; this is
    /// the total they must account for.
    pub edge_count: usize,
    /// Characteristic edge length, the scale the buckets are relative to.
    pub scale: f64,
}

impl SeamAssessment {
    pub fn count(&self, bucket: SeamBucket) -> usize {
        self.candidates
            .iter()
            .filter(|candidate| candidate.bucket == bucket)
            .count()
    }

    /// The pairs worth accepting: already closed, or closeable as an inaccuracy.
    pub fn acceptable(&self) -> impl Iterator<Item = &SeamCandidate> {
        self.candidates
            .iter()
            .filter(|candidate| candidate.bucket != SeamBucket::Near)
    }

    /// True when every edge is paired and every pair already closes.
    pub fn is_closed(&self) -> bool {
        self.orphans.is_empty()
            && !self.candidates.is_empty()
            && self
                .candidates
                .iter()
                .all(|candidate| candidate.bucket == SeamBucket::Held)
    }

    /// One line for the toolbar: what is done, what is offered, what is stuck.
    pub fn summary(&self) -> String {
        if self.edge_count == 0 {
            return "no polygon has been drawn yet".to_string();
        }
        if self.is_closed() {
            return format!("every seam closes: {} pairs holding", self.candidates.len());
        }
        let mut parts = Vec::new();
        for bucket in [SeamBucket::Held, SeamBucket::Ready, SeamBucket::Near] {
            let count = self.count(bucket);
            if count > 0 {
                parts.push(format!("{count} {}", bucket.phrase()));
            }
        }
        if !self.orphans.is_empty() {
            parts.push(format!("{} with no partner at all", self.orphans.len()));
        }
        parts.join(", ")
    }
}

/// Score every boundary edge against every other and account for all of them.
pub fn assess_seams(draft: &PeriodicTilingDraft) -> Result<SeamAssessment, String> {
    let determinant = draft.translation_a.cross(draft.translation_b);
    if !determinant.is_finite() || determinant.abs() <= f64::MIN_POSITIVE {
        return Err("translation vectors must span a finite non-zero period".into());
    }

    let edges = boundary_edges(draft)?;
    if edges.len() > 4_096 || edges.len().saturating_mul(edges.len()) > 1_000_000 {
        return Err("seam assessment exceeds the edge-pair budget".into());
    }
    let scale = characteristic_length(&edges);

    if edges.len() < 2 {
        return Ok(SeamAssessment {
            candidates: Vec::new(),
            orphans: edges
                .iter()
                .map(|edge| OrphanEdge {
                    edge: edge.reference,
                    length: edge.length(),
                    reason: OrphanReason::NothingToPairWith,
                })
                .collect(),
            edge_count: edges.len(),
            scale,
        });
    }

    // Every pair that clears the direction and length floors, plus a record of
    // the best rejection for each edge so an orphan can say what went wrong.
    let mut scored = Vec::new();
    let mut best_opposing: BTreeMap<EdgeRef, (f64, EdgeRef, f64)> = BTreeMap::new();
    for left in 0..edges.len() {
        for right in left + 1..edges.len() {
            let lhs = &edges[left];
            let rhs = &edges[right];
            let opposition = opposition_of(lhs, rhs);
            let length_ratio = length_ratio_of(lhs, rhs);
            if opposition >= OPPOSITION_FLOOR {
                remember_opposing(&mut best_opposing, lhs, rhs, opposition);
                remember_opposing(&mut best_opposing, rhs, lhs, opposition);
            }
            if opposition < OPPOSITION_FLOOR || length_ratio < LENGTH_FLOOR {
                continue;
            }
            let Some(candidate) = pair_candidate(
                draft,
                determinant,
                lhs,
                rhs,
                opposition,
                length_ratio,
                scale,
            ) else {
                continue;
            };
            scored.push(candidate);
        }
    }

    scored.sort_by(|left, right| {
        right
            .score
            .confidence
            .total_cmp(&left.score.confidence)
            .then_with(|| edge_key(left.constraint).cmp(&edge_key(right.constraint)))
    });

    let mut used = BTreeSet::new();
    let mut candidates = Vec::new();
    let mut runner_up: BTreeMap<EdgeRef, EdgeRef> = BTreeMap::new();
    for candidate in scored {
        let (lhs, rhs) = (candidate.constraint.lhs, candidate.constraint.rhs);
        if used.contains(&lhs) || used.contains(&rhs) {
            runner_up.entry(lhs).or_insert(rhs);
            runner_up.entry(rhs).or_insert(lhs);
            continue;
        }
        used.insert(lhs);
        used.insert(rhs);
        candidates.push(candidate);
    }

    let orphans = edges
        .iter()
        .filter(|edge| !used.contains(&edge.reference))
        .map(|edge| OrphanEdge {
            edge: edge.reference,
            length: edge.length(),
            reason: orphan_reason(edge, &best_opposing, &runner_up),
        })
        .collect::<Vec<_>>();

    Ok(SeamAssessment {
        edge_count: edges.len(),
        candidates,
        orphans,
        scale,
    })
}

fn orphan_reason(
    edge: &BoundaryEdge,
    best_opposing: &BTreeMap<EdgeRef, (f64, EdgeRef, f64)>,
    runner_up: &BTreeMap<EdgeRef, EdgeRef>,
) -> OrphanReason {
    if let Some(nearest) = runner_up.get(&edge.reference) {
        return OrphanReason::PartnerTaken { nearest: *nearest };
    }
    match best_opposing.get(&edge.reference) {
        Some((_, nearest, nearest_length)) => OrphanReason::LengthMismatch {
            nearest: *nearest,
            nearest_length: *nearest_length,
        },
        None => OrphanReason::NoOpposingEdge,
    }
}

fn remember_opposing(
    best: &mut BTreeMap<EdgeRef, (f64, EdgeRef, f64)>,
    edge: &BoundaryEdge,
    other: &BoundaryEdge,
    opposition: f64,
) {
    let entry =
        best.entry(edge.reference)
            .or_insert((f64::NEG_INFINITY, other.reference, other.length()));
    if opposition > entry.0 {
        *entry = (opposition, other.reference, other.length());
    }
}

struct BoundaryEdge {
    reference: EdgeRef,
    start: Vec2,
    end: Vec2,
}

impl BoundaryEdge {
    fn direction(&self) -> Vec2 {
        self.end - self.start
    }

    fn length(&self) -> f64 {
        self.direction().length()
    }
}

fn boundary_edges(draft: &PeriodicTilingDraft) -> Result<Vec<BoundaryEdge>, String> {
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
            edges.push(BoundaryEdge {
                reference: EdgeRef {
                    tile: instance.id,
                    edge: edge as u16,
                },
                start,
                end,
            });
        }
    }
    Ok(edges)
}

fn characteristic_length(edges: &[BoundaryEdge]) -> f64 {
    if edges.is_empty() {
        return 1.0;
    }
    let total = edges.iter().map(BoundaryEdge::length).sum::<f64>();
    let mean = total / edges.len() as f64;
    if mean.is_finite() && mean > 0.0 {
        mean
    } else {
        1.0
    }
}

fn opposition_of(lhs: &BoundaryEdge, rhs: &BoundaryEdge) -> f64 {
    let (left, right) = (lhs.direction(), rhs.direction());
    let (left_length, right_length) = (left.length(), right.length());
    if left_length <= 0.0 || right_length <= 0.0 {
        return -1.0;
    }
    -left.dot(right) / (left_length * right_length)
}

fn length_ratio_of(lhs: &BoundaryEdge, rhs: &BoundaryEdge) -> f64 {
    let (left, right) = (lhs.length(), rhs.length());
    let (low, high) = if left <= right {
        (left, right)
    } else {
        (right, left)
    };
    if high <= 0.0 { 0.0 } else { low / high }
}

fn pair_candidate(
    draft: &PeriodicTilingDraft,
    determinant: f64,
    lhs: &BoundaryEdge,
    rhs: &BoundaryEdge,
    opposition: f64,
    length_ratio: f64,
    scale: f64,
) -> Option<SeamCandidate> {
    // A seam joins the head of one edge to the tail of the other, so the
    // periodic offset is read off the vector between those two endpoints.
    let delta = lhs.start - rhs.end;
    let coordinates = [
        delta.cross(draft.translation_b) / determinant,
        draft.translation_a.cross(delta) / determinant,
    ];
    if coordinates.iter().any(|value| {
        !value.is_finite() || *value < f64::from(i32::MIN) || *value > f64::from(i32::MAX)
    }) {
        return None;
    }
    let offset = [coordinates[0].round() as i32, coordinates[1].round() as i32];
    let offset_error = coordinates
        .iter()
        .map(|value| (value - value.round()).abs())
        .fold(0.0_f64, f64::max);
    let shift =
        draft.translation_a * f64::from(offset[0]) + draft.translation_b * f64::from(offset[1]);

    let gap_start = (rhs.end + shift) - lhs.start;
    let gap_end = (rhs.start + shift) - lhs.end;
    let endpoint_gap = gap_start.length().max(gap_end.length());
    if !endpoint_gap.is_finite() {
        return None;
    }

    let proximity = 1.0 / (1.0 + endpoint_gap / scale);
    let confidence = proximity * length_ratio * ((opposition + 1.0) * 0.5);
    let bucket = if endpoint_gap <= HELD_FRACTION * scale {
        SeamBucket::Held
    } else if endpoint_gap <= READY_FRACTION * scale {
        SeamBucket::Ready
    } else {
        SeamBucket::Near
    };

    Some(SeamCandidate {
        constraint: SeamConstraint {
            lhs: lhs.reference,
            rhs: rhs.reference,
            periodic_offset: offset,
        },
        bucket,
        score: SeamScore {
            endpoint_gap,
            opposition,
            length_ratio,
            offset_error,
            confidence,
        },
        gap_start,
        gap_end,
    })
}

fn edge_key(constraint: SeamConstraint) -> (EdgeRef, EdgeRef, [i32; 2]) {
    (constraint.lhs, constraint.rhs, constraint.periodic_offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::tiling::{PrototypeShape, TilingPreset, build_preset};

    /// Freeze a preset into an editable outline, the way "Redraw selected" does,
    /// so a single vertex can be moved the way a pointer would move it.
    fn as_drawn(preset: TilingPreset) -> PeriodicTilingDraft {
        let mut draft = build_preset(preset, 1.0);
        for prototype in &mut draft.prototypes {
            let vertices = prototype_vertices(&prototype.shape).unwrap();
            prototype.shape = PrototypeShape::SimplePolygon { vertices };
        }
        draft
    }

    fn nudged(preset: TilingPreset, vertex: usize, by: Vec2) -> PeriodicTilingDraft {
        let mut draft = as_drawn(preset);
        if let PrototypeShape::SimplePolygon { vertices } = &mut draft.prototypes[0].shape {
            vertices[vertex] = vertices[vertex] + by;
        }
        draft
    }

    #[test]
    fn an_exact_preset_reports_every_seam_as_held() {
        for preset in TilingPreset::ALL {
            let assessment = assess_seams(&as_drawn(preset)).unwrap();
            assert!(
                assessment.is_closed(),
                "{preset:?} is exact and must read as closed, got {}",
                assessment.summary()
            );
            assert!(assessment.orphans.is_empty(), "{preset:?} left an orphan");
        }
    }

    /// The whole point. A drawing that is off by a pointer's worth of accuracy
    /// used to lose every seam; it must now keep them, and say how far off.
    #[test]
    fn a_hand_drawn_square_keeps_its_seams_and_says_how_far_off_they_are() {
        let draft = nudged(TilingPreset::Square, 0, Vec2::new(0.05, -0.03));
        let assessment = assess_seams(&draft).unwrap();

        assert_eq!(assessment.candidates.len(), 2, "both pairs must survive");
        assert!(assessment.orphans.is_empty());
        assert!(
            !assessment.is_closed(),
            "the drawing is wrong and must not read as finished"
        );
        assert!(
            assessment.acceptable().count() > 0,
            "a pointer-sized error must be offered as repairable, got {}",
            assessment.summary()
        );
        assert!(
            assessment
                .candidates
                .iter()
                .any(|candidate| candidate.score.endpoint_gap > 0.0),
            "a wrong drawing must report a non-zero gap"
        );
    }

    /// Errors far larger than a slip still get an answer, because the user is
    /// exploring and a refusal teaches them nothing.
    #[test]
    fn even_a_badly_drawn_outline_is_answered_rather_than_refused() {
        for slip in [0.05, 0.2, 0.5] {
            let draft = nudged(TilingPreset::Square, 0, Vec2::new(slip, slip * 0.4));
            let assessment = assess_seams(&draft).unwrap();
            assert!(
                !assessment.candidates.is_empty(),
                "a slip of {slip} left the user with nothing: {}",
                assessment.summary()
            );
        }
    }

    /// The hint is only useful if following it actually closes the seam.
    #[test]
    fn following_the_hint_lands_the_endpoint_on_its_partner() {
        let draft = nudged(TilingPreset::Square, 0, Vec2::new(0.05, -0.03));
        let assessment = assess_seams(&draft).unwrap();
        let edges = boundary_edges(&draft).unwrap();
        let locate = |reference: EdgeRef| {
            edges
                .iter()
                .find(|edge| edge.reference == reference)
                .expect("the report may only name edges that exist")
        };

        for candidate in &assessment.candidates {
            let lhs = locate(candidate.constraint.lhs);
            let rhs = locate(candidate.constraint.rhs);
            let offset = candidate.constraint.periodic_offset;
            let shift = draft.translation_a * f64::from(offset[0])
                + draft.translation_b * f64::from(offset[1]);

            let moved_start = lhs.start + candidate.gap_start;
            let moved_end = lhs.end + candidate.gap_end;
            assert!(
                (moved_start - (rhs.end + shift)).length() < 1e-12,
                "following gap_start must land on the partner endpoint"
            );
            assert!(
                (moved_end - (rhs.start + shift)).length() < 1e-12,
                "following gap_end must land on the partner endpoint"
            );
        }
    }

    /// The guarantee that replaces silence: nothing is dropped without a word.
    #[test]
    fn every_boundary_edge_is_accounted_for() {
        let drafts = [
            as_drawn(TilingPreset::Square),
            nudged(TilingPreset::Square, 0, Vec2::new(0.05, -0.03)),
            nudged(TilingPreset::RegularHexagon, 2, Vec2::new(-0.2, 0.1)),
            nudged(TilingPreset::EquilateralTriangles, 1, Vec2::new(0.4, 0.4)),
            nudged(TilingPreset::OctagonSquare, 0, Vec2::new(0.15, 0.0)),
        ];
        for draft in drafts {
            let assessment = assess_seams(&draft).unwrap();
            assert_eq!(
                assessment.candidates.len() * 2 + assessment.orphans.len(),
                assessment.edge_count,
                "every edge must be paired or explained, not dropped: {}",
                assessment.summary()
            );
        }
    }

    #[test]
    fn an_unpairable_edge_is_named_with_a_reason_a_user_can_act_on() {
        // Five edges cannot pair off evenly, so at least one must be explained.
        let mut draft = as_drawn(TilingPreset::Square);
        if let PrototypeShape::SimplePolygon { vertices } = &mut draft.prototypes[0].shape {
            let last = *vertices.last().unwrap();
            let first = vertices[0];
            vertices.push((last + first) * 0.5 + Vec2::new(0.3, 0.0));
        }
        let assessment = assess_seams(&draft).unwrap();
        assert_eq!(assessment.edge_count, 5);
        assert!(
            !assessment.orphans.is_empty(),
            "an odd edge count must leave something unpaired"
        );
        for orphan in &assessment.orphans {
            let sentence = orphan.describe();
            assert!(
                sentence.contains(&format!("edge {}", orphan.edge.edge)),
                "the reason must name the edge: {sentence}"
            );
            assert!(
                sentence.len() > 30,
                "the reason must be a sentence, not a code: {sentence}"
            );
        }
    }

    #[test]
    fn a_draft_with_one_edge_says_so_instead_of_reporting_nothing() {
        let mut draft = as_drawn(TilingPreset::Square);
        draft.instances.clear();
        draft.prototypes.clear();
        let assessment = assess_seams(&draft).unwrap();
        assert_eq!(assessment.edge_count, 0);
        assert_eq!(assessment.summary(), "no polygon has been drawn yet");
    }

    #[test]
    fn the_summary_never_claims_a_wrong_drawing_is_finished() {
        let draft = nudged(TilingPreset::RegularHexagon, 0, Vec2::new(0.3, 0.2));
        let assessment = assess_seams(&draft).unwrap();
        assert!(!assessment.is_closed());
        assert!(
            !assessment.summary().contains("every seam closes"),
            "summary must not read as finished: {}",
            assessment.summary()
        );
    }

    #[test]
    fn a_held_seam_reports_the_gap_that_opened_under_it() {
        let exact = as_drawn(TilingPreset::Square);
        let assessment = assess_seams(&exact).unwrap();
        let held = assessment.candidates[0].constraint;
        assert!(
            constraint_closes(&exact, held),
            "an exact drawing must read as closed"
        );
        assert!(constraint_gap(&exact, held).unwrap() < 1e-9);

        // Move a vertex out from under the seam and the same constraint must
        // report how far it has been pulled apart, not simply fail.
        let mut torn = exact.clone();
        if let PrototypeShape::SimplePolygon { vertices } = &mut torn.prototypes[0].shape {
            vertices[0] = vertices[0] + Vec2::new(0.4, 0.25);
        }
        let gap = constraint_gap(&torn, held).expect("the gap must still be measurable");
        assert!(
            gap > 0.1,
            "a torn seam must report a real distance, got {gap}"
        );
        assert!(!constraint_closes(&torn, held));
    }

    #[test]
    fn a_constraint_naming_a_missing_edge_reports_nothing_rather_than_guessing() {
        let draft = as_drawn(TilingPreset::Square);
        let bogus = SeamConstraint {
            lhs: EdgeRef {
                tile: crate::sim::tiling::BasisId(99),
                edge: 0,
            },
            rhs: EdgeRef {
                tile: crate::sim::tiling::BasisId(99),
                edge: 1,
            },
            periodic_offset: [0, 0],
        };
        assert!(constraint_gap(&draft, bogus).is_none());
        assert!(!constraint_closes(&draft, bogus));
    }
}
