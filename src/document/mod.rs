//! GUI-independent experiment document.
//!
//! The controller owns the active experiment, its editable draft, undo history
//! and the stable editor selection. It never imports a UI toolkit: the terminal
//! Workbench and the egui GUI both drive it through typed [`DocumentCommand`]
//! transactions.

pub mod channels;
pub mod growth;
pub mod kernels;
pub mod selection;
pub mod tiling;

use crate::sim::experiment_model::{
    ChannelId, DisplayColor, ExperimentSpec, KernelId, RgbColor, UpdateMode, validate_structure,
};
use crate::sim::ruleset::{BindingKey, RuleSetId};
use crate::sim::tiling::{PeriodicTilingDraft, TilingPreset, Vec2 as TilingVec2};
use crate::workbench::{DraftCommand, History, HistoryError};

pub use selection::{EditorSelection, PlotAxes, PlotSymbol};

/// Whether the draft still matches the active experiment.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DraftStatus {
    #[default]
    Clean,
    Dirty,
    Invalid,
}

/// What a successful command touched, so a view can refresh the minimum.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Affected {
    Selection,
    Channels,
    Kernels,
    Growth,
    Tiling,
    Experiment,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentChange {
    pub generation: u64,
    pub affected: Vec<Affected>,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum DocumentError {
    #[error("{0}")]
    Rejected(String),
    #[error("nothing to undo")]
    NothingToUndo,
    #[error("nothing to redo")]
    NothingToRedo,
}

impl From<HistoryError> for DocumentError {
    fn from(error: HistoryError) -> Self {
        match error {
            HistoryError::NothingToUndo => Self::NothingToUndo,
            HistoryError::NothingToRedo => Self::NothingToRedo,
            HistoryError::Edit(message) => Self::Rejected(message),
        }
    }
}

/// Typed model edits. Every variant is undoable as one transaction.
#[derive(Clone, Debug, PartialEq)]
pub enum DocumentCommand {
    SelectChannel(ChannelId),
    SelectKernel(KernelId),
    AddChannel,
    DeleteSelectedChannel,
    RenameSelectedChannel(String),
    SetSelectedChannelColor(DisplayColor),
    SetSelectedChannelVisible(bool),
    SetSelectedChannelFrozen(bool),
    SetSelectedGrowthMode(UpdateMode),
    SetSelectedGrowthSource(String),
    SetSimulationDt(f32),
    /// Close a construction path into a basis polygon.
    FinishTilingPolygon {
        vertices: Vec<TilingVec2>,
        target: tiling::ConstructionTarget,
    },
    /// Replace the whole tiling with a preset unit cell.
    ApplyTilingPreset {
        preset: TilingPreset,
        scale: f64,
    },
    /// Store a tiling the canvas produced directly, such as a dragged vertex or
    /// an accepted seam solve.
    SetTilingDraft(Box<PeriodicTilingDraft>),
    /// Commit a whole experiment computed by a pure transform. The transform
    /// has already worked out every consequence, so this lands as one
    /// undoable step rather than a sequence a failure could interrupt.
    ReplaceExperiment(Box<ExperimentSpec>),
    /// Escape hatch for the existing kernel and value level edits.
    Draft(Box<DraftCommand>),
}

/// A validated, compiled-ready copy of the draft. Building one never mutates
/// the active experiment.
#[derive(Clone, Debug, PartialEq)]
pub struct ApplyCandidate {
    pub request_id: u64,
    pub experiment: ExperimentSpec,
}

/// Comparable summary used by tests to prove a rejected command changed nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentAudit {
    pub draft: ExperimentSpec,
    pub active: ExperimentSpec,
    pub selection: EditorSelection,
    pub status: DraftStatus,
    pub active_revision: u64,
    pub generation: u64,
    pub undo_depth: usize,
    pub redo_depth: usize,
}

#[derive(Clone, Debug)]
pub struct DocumentController {
    active: ExperimentSpec,
    draft: ExperimentSpec,
    active_revision: u64,
    generation: u64,
    status: DraftStatus,
    selection: EditorSelection,
    history: History,
    selection_undo: Vec<EditorSelection>,
    selection_redo: Vec<EditorSelection>,
    next_request_id: u64,
}

impl DocumentController {
    pub fn new(spec: ExperimentSpec) -> Self {
        let selection = EditorSelection::initial(&spec);
        Self {
            active: spec.clone(),
            draft: spec,
            active_revision: 0,
            generation: 0,
            status: DraftStatus::Clean,
            selection,
            history: History::default(),
            selection_undo: Vec::new(),
            selection_redo: Vec::new(),
            next_request_id: 1,
        }
    }

    pub fn active(&self) -> &ExperimentSpec {
        &self.active
    }

    pub fn draft(&self) -> &ExperimentSpec {
        &self.draft
    }

    pub fn selection(&self) -> &EditorSelection {
        &self.selection
    }

    pub fn status(&self) -> DraftStatus {
        self.status
    }

    pub fn active_revision(&self) -> u64 {
        self.active_revision
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn audit_snapshot(&self) -> DocumentAudit {
        DocumentAudit {
            draft: self.draft.clone(),
            active: self.active.clone(),
            selection: self.selection.clone(),
            status: self.status,
            active_revision: self.active_revision,
            generation: self.generation,
            undo_depth: self.history.undo_depth(),
            redo_depth: self.history.redo_depth(),
        }
    }

    pub fn select_channel(&mut self, channel: ChannelId) -> Result<DocumentChange, DocumentError> {
        self.execute(DocumentCommand::SelectChannel(channel))
    }

    /// Run one command as a transaction. A rejected command leaves the draft,
    /// history and selection exactly as they were.
    pub fn execute(&mut self, command: DocumentCommand) -> Result<DocumentChange, DocumentError> {
        match command {
            DocumentCommand::SelectChannel(channel) => {
                if !self.draft.channels.iter().any(|entry| entry.id == channel) {
                    return Err(DocumentError::Rejected("unknown channel".into()));
                }
                self.selection.channel = channel;
                self.selection.normalize(&self.draft);
                Ok(self.record(vec![Affected::Selection]))
            }
            DocumentCommand::SelectKernel(kernel) => {
                if !self
                    .selection
                    .available_kernels(&self.draft)
                    .contains(&kernel)
                {
                    return Err(DocumentError::Rejected(
                        "kernel is not part of the selected binding".into(),
                    ));
                }
                self.selection.kernel = Some(kernel);
                Ok(self.record(vec![Affected::Selection]))
            }
            DocumentCommand::AddChannel => {
                let added = channels::add_channel(&self.draft).map_err(DocumentError::Rejected)?;
                let channel = added.channel;
                let kernel = added.selected_kernel;
                self.transact(added.spec, |selection| {
                    selection.channel = channel;
                    selection.kernel = kernel;
                })?;
                Ok(self.record(vec![Affected::Channels, Affected::Selection]))
            }
            DocumentCommand::DeleteSelectedChannel => {
                let (next, nearest) = channels::remove_channel(&self.draft, self.selection.channel)
                    .map_err(DocumentError::Rejected)?;
                self.transact(next, |selection| selection.channel = nearest)?;
                Ok(self.record(vec![Affected::Channels, Affected::Selection]))
            }
            DocumentCommand::RenameSelectedChannel(name) => {
                let channel = self.selection.channel;
                self.draft_command(DraftCommand::RenameChannel { channel, name })?;
                Ok(self.record(vec![Affected::Channels]))
            }
            DocumentCommand::SetSelectedChannelColor(color) => {
                let channel = self.selection.channel;
                self.draft_command(DraftCommand::SetChannelColor { channel, color })?;
                Ok(self.record(vec![Affected::Channels]))
            }
            DocumentCommand::SetSelectedChannelVisible(visible) => {
                let channel = self.selection.channel;
                self.draft_command(DraftCommand::SetChannelVisible { channel, visible })?;
                Ok(self.record(vec![Affected::Channels]))
            }
            DocumentCommand::SetSelectedChannelFrozen(frozen) => {
                // Freezing also has to remove the channel's bindings, so this
                // cannot be a plain field write on the draft.
                let next =
                    channels::set_channel_frozen(&self.draft, self.selection.channel, frozen)
                        .map_err(DocumentError::Rejected)?;
                self.transact(next, |_| {})?;
                Ok(self.record(vec![Affected::Channels]))
            }
            DocumentCommand::SetSelectedGrowthMode(mode) => {
                let next = channels::set_growth_mode(&self.draft, self.binding(), mode)
                    .map_err(DocumentError::Rejected)?;
                self.transact(next, |_| {})?;
                Ok(self.record(vec![Affected::Growth]))
            }
            DocumentCommand::SetSelectedGrowthSource(source) => {
                let next = channels::set_growth_source(&self.draft, self.binding(), &source)
                    .map_err(DocumentError::Rejected)?;
                self.transact(next, |_| {})?;
                Ok(self.record(vec![Affected::Growth]))
            }
            DocumentCommand::SetSimulationDt(dt) => {
                if !dt.is_finite() || dt <= 0.0 || dt > 10.0 {
                    return Err(DocumentError::Rejected(
                        "simulation dt must be finite and in (0, 10]".into(),
                    ));
                }
                let mut next = self.draft.clone();
                next.simulation_dt = dt;
                self.transact(next, |_| {})?;
                Ok(self.record(vec![Affected::Experiment]))
            }
            DocumentCommand::FinishTilingPolygon { vertices, target } => {
                let commit = tiling::finish_polygon(&self.draft, &vertices, target)
                    .map_err(DocumentError::Rejected)?;
                let prototype = commit.prototype;
                let basis = commit.basis;
                self.transact(commit.spec, |selection| {
                    selection.prototype = Some(prototype);
                    if let Some(basis) = basis {
                        selection.basis = basis;
                    }
                })?;
                Ok(self.record(vec![Affected::Tiling, Affected::Selection]))
            }
            DocumentCommand::ApplyTilingPreset { preset, scale } => {
                if !scale.is_finite() || scale <= 0.0 {
                    return Err(DocumentError::Rejected(
                        "preset scale must be finite and positive".into(),
                    ));
                }
                let next = tiling::apply_preset(&self.draft, preset, scale)
                    .map_err(DocumentError::Rejected)?;
                let first = next
                    .tiling
                    .as_ref()
                    .and_then(|entry| entry.instances.first())
                    .map(|instance| (instance.prototype, instance.id));
                self.transact(next, |selection| {
                    if let Some((prototype, basis)) = first {
                        selection.prototype = Some(prototype);
                        selection.basis = basis;
                    }
                })?;
                Ok(self.record(vec![Affected::Tiling, Affected::Selection]))
            }
            DocumentCommand::SetTilingDraft(draft) => {
                let mut next = self.draft.clone();
                next.tiling = Some(*draft);
                self.transact(next, |_| {})?;
                Ok(self.record(vec![Affected::Tiling]))
            }
            DocumentCommand::ReplaceExperiment(spec) => {
                self.transact(*spec, |_| {})?;
                Ok(self.record(vec![Affected::Kernels, Affected::Selection]))
            }
            DocumentCommand::Draft(command) => {
                self.draft_command(*command)?;
                Ok(self.record(vec![Affected::Kernels]))
            }
        }
    }

    pub fn undo(&mut self) -> Result<DocumentChange, DocumentError> {
        let current = self.selection.clone();
        self.history.undo(&mut self.draft)?;
        let restored = self.selection_undo.pop().unwrap_or_else(|| current.clone());
        self.selection_redo.push(current);
        self.selection = restored;
        self.selection.normalize(&self.draft);
        self.status = self.derive_status();
        Ok(self.record(vec![Affected::Selection]))
    }

    pub fn redo(&mut self) -> Result<DocumentChange, DocumentError> {
        let current = self.selection.clone();
        self.history.redo(&mut self.draft)?;
        let restored = self.selection_redo.pop().unwrap_or_else(|| current.clone());
        self.selection_undo.push(current);
        self.selection = restored;
        self.selection.normalize(&self.draft);
        self.status = self.derive_status();
        Ok(self.record(vec![Affected::Selection]))
    }

    /// Validate and normalize the draft without touching the active experiment
    /// or the history.
    pub fn prepare_apply(&mut self) -> Result<ApplyCandidate, Vec<String>> {
        let candidate = self
            .draft
            .clone()
            .normalize_rules()
            .map_err(|errors| errors.iter().map(ToString::to_string).collect::<Vec<_>>())?;
        validate_structure(&candidate)
            .map_err(|errors| errors.iter().map(ToString::to_string).collect::<Vec<_>>())?;
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        Ok(ApplyCandidate {
            request_id,
            experiment: candidate,
        })
    }

    /// Install a prepared candidate. Stale replies for superseded requests are
    /// ignored so a late worker answer cannot overwrite a newer active state.
    pub fn accept_apply(&mut self, request_id: u64, experiment: ExperimentSpec) -> bool {
        if request_id + 1 != self.next_request_id {
            return false;
        }
        self.active = experiment.clone();
        self.draft = experiment;
        self.active_revision += 1;
        self.history.clear();
        self.selection_undo.clear();
        self.selection_redo.clear();
        self.selection.normalize(&self.draft);
        self.status = DraftStatus::Clean;
        self.generation += 1;
        true
    }

    fn binding(&self) -> BindingKey {
        BindingKey {
            basis: self.selection.basis,
            output: self.selection.channel,
        }
    }

    fn draft_command(&mut self, command: DraftCommand) -> Result<(), DocumentError> {
        let selection = self.selection.clone();
        self.history.execute(&mut self.draft, command)?;
        self.selection_undo.push(selection);
        self.selection_redo.clear();
        self.selection.normalize(&self.draft);
        self.status = self.derive_status();
        Ok(())
    }

    fn transact(
        &mut self,
        next: ExperimentSpec,
        adjust: impl FnOnce(&mut EditorSelection),
    ) -> Result<(), DocumentError> {
        self.draft_command(DraftCommand::ReplaceDraft(Box::new(next)))?;
        adjust(&mut self.selection);
        self.selection.normalize(&self.draft);
        Ok(())
    }

    fn derive_status(&self) -> DraftStatus {
        if self.draft == self.active {
            DraftStatus::Clean
        } else {
            DraftStatus::Dirty
        }
    }

    fn record(&mut self, affected: Vec<Affected>) -> DocumentChange {
        self.generation += 1;
        DocumentChange {
            generation: self.generation,
            affected,
        }
    }
}

/// Kept so a caller can name a rule set without importing the sim module.
pub fn rule_set_of(spec: &ExperimentSpec, binding: BindingKey) -> Option<RuleSetId> {
    spec.rules
        .binding(binding.basis, binding.output)
        .map(|entry| entry.rule_set)
}

/// Convenience for tests and fixtures.
pub fn custom_color(red: u8, green: u8, blue: u8) -> DisplayColor {
    DisplayColor::Custom(RgbColor { red, green, blue })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_channel_spec() -> ExperimentSpec {
        ExperimentSpec::single_channel_lenia(8, 8)
    }

    fn three_channel_spec() -> ExperimentSpec {
        let mut spec = one_channel_spec();
        for _ in 0..2 {
            spec = channels::add_channel(&spec).unwrap().spec;
        }
        spec
    }

    #[test]
    fn delete_add_undo_redo_preserves_stable_channel_selection() {
        let mut doc = DocumentController::new(three_channel_spec());
        doc.select_channel(ChannelId(1)).unwrap();
        doc.execute(DocumentCommand::DeleteSelectedChannel).unwrap();
        assert_eq!(doc.selection().channel, ChannelId(2));
        doc.undo().unwrap();
        assert_eq!(doc.selection().channel, ChannelId(1));
        doc.redo().unwrap();
        assert_eq!(doc.selection().channel, ChannelId(2));
    }

    #[test]
    fn failed_command_changes_neither_draft_history_nor_selection() {
        let mut doc = DocumentController::new(one_channel_spec());
        let before = doc.audit_snapshot();
        assert!(doc.execute(DocumentCommand::DeleteSelectedChannel).is_err());
        assert_eq!(doc.audit_snapshot(), before);
    }

    #[test]
    fn adding_a_channel_selects_it_and_undo_restores_the_previous_selection() {
        let mut doc = DocumentController::new(one_channel_spec());
        assert_eq!(doc.selection().channel, ChannelId(0));
        doc.execute(DocumentCommand::AddChannel).unwrap();
        let added = doc.selection().channel;
        assert_ne!(added, ChannelId(0));
        assert_eq!(doc.draft().channels.len(), 2);
        doc.undo().unwrap();
        assert_eq!(doc.draft().channels.len(), 1);
        assert_eq!(doc.selection().channel, ChannelId(0));
    }

    #[test]
    fn a_rejected_selection_leaves_the_document_untouched() {
        let mut doc = DocumentController::new(one_channel_spec());
        let before = doc.audit_snapshot();
        assert!(doc.select_channel(ChannelId(41)).is_err());
        assert_eq!(doc.audit_snapshot(), before);
    }

    #[test]
    fn an_edit_marks_the_draft_dirty_and_undo_returns_it_to_clean() {
        let mut doc = DocumentController::new(one_channel_spec());
        assert_eq!(doc.status(), DraftStatus::Clean);
        doc.execute(DocumentCommand::RenameSelectedChannel("surface".into()))
            .unwrap();
        assert_eq!(doc.status(), DraftStatus::Dirty);
        assert_eq!(doc.draft().channels[0].name, "surface");
        assert_eq!(doc.active().channels[0].name, "state");
        doc.undo().unwrap();
        assert_eq!(doc.status(), DraftStatus::Clean);
    }

    #[test]
    fn rejecting_a_non_positive_dt_keeps_the_previous_value() {
        let mut doc = DocumentController::new(one_channel_spec());
        let before = doc.audit_snapshot();
        assert!(doc.execute(DocumentCommand::SetSimulationDt(0.0)).is_err());
        assert!(
            doc.execute(DocumentCommand::SetSimulationDt(f32::NAN))
                .is_err()
        );
        assert_eq!(doc.audit_snapshot(), before);
        doc.execute(DocumentCommand::SetSimulationDt(0.25)).unwrap();
        assert_eq!(doc.draft().simulation_dt, 0.25);
    }

    #[test]
    fn prepare_apply_does_not_touch_the_active_experiment_or_history() {
        let mut doc = DocumentController::new(one_channel_spec());
        doc.execute(DocumentCommand::AddChannel).unwrap();
        let before = doc.audit_snapshot();
        let candidate = doc.prepare_apply().unwrap();
        assert_eq!(candidate.experiment.channels.len(), 2);
        let after = doc.audit_snapshot();
        assert_eq!(after.active, before.active);
        assert_eq!(after.active_revision, before.active_revision);
        assert_eq!(after.undo_depth, before.undo_depth);
    }

    #[test]
    fn only_the_latest_apply_request_installs_a_candidate() {
        let mut doc = DocumentController::new(one_channel_spec());
        doc.execute(DocumentCommand::AddChannel).unwrap();
        let stale = doc.prepare_apply().unwrap();
        doc.execute(DocumentCommand::AddChannel).unwrap();
        let latest = doc.prepare_apply().unwrap();

        assert!(!doc.accept_apply(stale.request_id, stale.experiment));
        assert_eq!(doc.active_revision(), 0);
        assert_eq!(doc.status(), DraftStatus::Dirty);

        assert!(doc.accept_apply(latest.request_id, latest.experiment));
        assert_eq!(doc.active_revision(), 1);
        assert_eq!(doc.status(), DraftStatus::Clean);
        assert_eq!(doc.active().channels.len(), 3);
    }

    #[test]
    fn every_successful_command_advances_the_generation() {
        let mut doc = DocumentController::new(one_channel_spec());
        let first = doc.execute(DocumentCommand::AddChannel).unwrap();
        let second = doc
            .execute(DocumentCommand::SetSelectedChannelVisible(false))
            .unwrap();
        assert!(second.generation > first.generation);
        assert!(second.affected.contains(&Affected::Channels));
    }
}
