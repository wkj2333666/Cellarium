//! User-defined periodic polygon tilings.
//!
//! The public model intentionally owns plain `f64` values.  Conversion to
//! third-party geometry types is kept in [`polygon`] so persisted experiments
//! do not depend on a geometry crate's wire format.

pub mod arrangement;
pub mod compile;
pub mod constraints;
pub mod copies;
pub mod coverage;
pub mod half_edge;
mod model;
pub mod polygon;
pub mod predicates;
pub mod presets;
pub mod snap;
pub mod solver;

pub use arrangement::{
    ArrangementFace, AtomicEdge, HalfEdgeId, NeighborPlacement, PeriodicArrangement, ShapeEdgeRef,
    VertexId,
};
pub use compile::{CompiledTiling, compile_tiling};
pub use constraints::{SeamConstraint, SeamProposal, propose_full_edge_seams};
pub use copies::{Aabb, GeometryBudget, LatticeCopyBounds};
pub use coverage::{
    CoverageReport, TilingDiagnostic, TilingValidationReport, validate_coverage,
    validate_periodic_tiling,
};
pub use half_edge::{EdgePair, EdgeRef, HalfEdge, canonical_half_edges};
pub use model::{
    BasisId, GeometryIssue, PeriodicTilingDraft, PrototypeId, PrototypeShape, RigidTransform,
    TileId, TileInstance, TilePrototype, TilingMode, Vec2,
};
pub use predicates::{SegmentRelation, segment_relation};
pub use presets::{TilingPreset, build_preset};
pub use snap::{SnapResult, snap_edge};
pub use solver::{DragTarget, SolveDiagnostic, SolvedTiling, solve_edge_constraints};
