use super::growth_editor::editor_for;
use super::{ChannelView, DraftCommand, GrowthEditorState, History, HistoryError};
use crate::sim::experiment_model::{
    ChannelId, DisplayColor, ExperimentSpec, KernelId, KernelSlot, RgbColor,
};
use crate::sim::tiling::{PrototypeId, PrototypeShape, TilingPreset, build_preset};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AppMode {
    #[default]
    Simulation,
    Workbench,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WorkbenchSection {
    #[default]
    World,
    Tiling,
    Channels,
    Kernels,
    Growth,
    Experiment,
}
impl WorkbenchSection {
    pub const ALL: [Self; 6] = [
        Self::World,
        Self::Tiling,
        Self::Channels,
        Self::Kernels,
        Self::Growth,
        Self::Experiment,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::World => "World",
            Self::Tiling => "Tiling",
            Self::Channels => "Channels",
            Self::Kernels => "Kernels",
            Self::Growth => "Growth",
            Self::Experiment => "Experiment",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WorkbenchFocus {
    #[default]
    Outline,
    Canvas,
    Inspector,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DraftStatus {
    #[default]
    Clean,
    Dirty,
    Invalid,
}

#[derive(Clone, Debug)]
pub struct WorkbenchState {
    authoritative: ExperimentSpec,
    draft: ExperimentSpec,
    history: History,
    section: WorkbenchSection,
    focus: WorkbenchFocus,
    status: DraftStatus,
    selected_channel: ChannelId,
    channel_view: ChannelView,
    growth_editor: GrowthEditorState,
    growth_editing: bool,
    selected_prototype: Option<PrototypeId>,
}
impl WorkbenchState {
    pub fn new(spec: ExperimentSpec) -> Self {
        let selected_channel = spec.channels.first().map_or(ChannelId(0), |c| c.id);
        let growth_editor = editor_for(&spec, selected_channel);
        let selected_prototype = spec
            .tiling
            .as_ref()
            .and_then(|tiling| tiling.prototypes.first().map(|prototype| prototype.id));
        Self {
            authoritative: spec.clone(),
            draft: spec,
            history: History::default(),
            section: WorkbenchSection::World,
            focus: WorkbenchFocus::Outline,
            status: DraftStatus::Clean,
            selected_channel,
            channel_view: ChannelView::Composite,
            growth_editor,
            growth_editing: false,
            selected_prototype,
        }
    }
    pub fn draft(&self) -> &ExperimentSpec {
        &self.draft
    }
    pub fn authoritative(&self) -> &ExperimentSpec {
        &self.authoritative
    }
    pub fn section(&self) -> WorkbenchSection {
        self.section
    }
    pub fn focus(&self) -> WorkbenchFocus {
        self.focus
    }
    pub fn status(&self) -> DraftStatus {
        self.status
    }
    pub fn selected_channel(&self) -> ChannelId {
        self.selected_channel
    }
    pub fn channel_view(&self) -> ChannelView {
        self.channel_view
    }
    pub fn growth_editor(&self) -> &GrowthEditorState {
        &self.growth_editor
    }
    pub fn growth_editor_mut(&mut self) -> &mut GrowthEditorState {
        &mut self.growth_editor
    }
    pub fn growth_editing(&self) -> bool {
        self.growth_editing
    }
    pub fn toggle_growth_editing(&mut self) {
        self.growth_editing = !self.growth_editing;
    }
    pub fn stop_growth_editing(&mut self) {
        self.growth_editing = false;
        self.selected_prototype = self
            .draft
            .tiling
            .as_ref()
            .and_then(|tiling| tiling.prototypes.first().map(|prototype| prototype.id));
    }
    pub fn sync_growth_source(&mut self) {
        let source = self.growth_editor.buffer().as_str().to_string();
        if let Some(growth) = self
            .draft
            .growth
            .iter_mut()
            .find(|growth| growth.target == self.selected_channel)
        {
            growth.source = source;
            self.status = if self.growth_editor.diagnostics().is_empty() {
                DraftStatus::Dirty
            } else {
                DraftStatus::Invalid
            };
        }
    }
    pub fn execute(&mut self, command: DraftCommand) -> Result<(), HistoryError> {
        self.history.execute(&mut self.draft, command)?;
        self.status = DraftStatus::Dirty;
        Ok(())
    }
    pub fn undo(&mut self) -> Result<(), HistoryError> {
        self.history.undo(&mut self.draft)?;
        self.status = if self.draft == self.authoritative {
            DraftStatus::Clean
        } else {
            DraftStatus::Dirty
        };
        Ok(())
    }
    pub fn redo(&mut self) -> Result<(), HistoryError> {
        self.history.redo(&mut self.draft)?;
        self.status = DraftStatus::Dirty;
        Ok(())
    }
    pub fn revert(&mut self) {
        self.draft = self.authoritative.clone();
        self.growth_editor = editor_for(&self.draft, self.selected_channel);
        self.growth_editing = false;
        self.selected_prototype = self
            .draft
            .tiling
            .as_ref()
            .and_then(|tiling| tiling.prototypes.first().map(|prototype| prototype.id));
        self.history.clear();
        self.status = DraftStatus::Clean;
    }
    pub fn accept(&mut self, normalized: ExperimentSpec) {
        self.authoritative = normalized.clone();
        self.draft = normalized;
        self.selected_channel = self.draft.channels.first().map_or(ChannelId(0), |c| c.id);
        self.growth_editor = editor_for(&self.draft, self.selected_channel);
        self.growth_editing = false;
        self.history.clear();
        self.status = DraftStatus::Clean;
    }
    pub fn select_section(&mut self, section: WorkbenchSection) {
        self.section = section;
    }
    pub fn section_next(&mut self) {
        let index = WorkbenchSection::ALL
            .iter()
            .position(|value| *value == self.section)
            .unwrap_or(0);
        self.section = WorkbenchSection::ALL[(index + 1) % WorkbenchSection::ALL.len()];
    }
    pub fn focus_next(&mut self) {
        self.focus = match self.focus {
            WorkbenchFocus::Outline => WorkbenchFocus::Canvas,
            WorkbenchFocus::Canvas => WorkbenchFocus::Inspector,
            WorkbenchFocus::Inspector => WorkbenchFocus::Outline,
        };
    }
    pub fn focus_previous(&mut self) {
        self.focus = match self.focus {
            WorkbenchFocus::Outline => WorkbenchFocus::Inspector,
            WorkbenchFocus::Canvas => WorkbenchFocus::Outline,
            WorkbenchFocus::Inspector => WorkbenchFocus::Canvas,
        };
    }
    pub fn set_selected_channel(&mut self, channel: ChannelId) -> Result<(), String> {
        if self.draft.channels.iter().any(|entry| entry.id == channel) {
            self.selected_channel = channel;
            Ok(())
        } else {
            Err("unknown channel".into())
        }
    }
    pub fn set_channel_view(&mut self, view: ChannelView) {
        self.channel_view = view;
    }

    fn replace_draft(&mut self, next: ExperimentSpec) -> Result<(), HistoryError> {
        self.execute(DraftCommand::ReplaceDraft(Box::new(next)))?;
        if self.draft.tiling.as_ref().is_some_and(|tiling| {
            self.selected_prototype
                .is_some_and(|id| !tiling.prototypes.iter().any(|prototype| prototype.id == id))
        }) {
            self.selected_prototype = self
                .draft
                .tiling
                .as_ref()
                .and_then(|tiling| tiling.prototypes.first().map(|prototype| prototype.id));
        } else if self.draft.tiling.is_some() && self.selected_prototype.is_none() {
            self.selected_prototype = self
                .draft
                .tiling
                .as_ref()
                .and_then(|tiling| tiling.prototypes.first().map(|prototype| prototype.id));
        }
        self.growth_editor = editor_for(&self.draft, self.selected_channel);
        Ok(())
    }

    pub fn add_channel(&mut self) -> Result<(), HistoryError> {
        let mut next = self.draft.clone();
        let name = format!("channel_{}", next.channels.len() + 1);
        let id = next.add_channel(name, false);
        self.selected_channel = id;
        self.replace_draft(next)
    }

    pub fn remove_selected_channel(&mut self) -> Result<(), String> {
        if self.draft.channels.len() <= 1 {
            return Err("an experiment must retain at least one channel".into());
        }
        let removed = self.selected_channel;
        let mut next = self.draft.clone();
        next.channels.retain(|channel| channel.id != removed);
        next.kernels
            .retain(|kernel| kernel.source != removed && kernel.target != removed);
        next.growth.retain(|growth| growth.target != removed);
        for growth in &mut next.growth {
            growth.kernel_inputs.retain(|id| {
                next.kernels
                    .iter()
                    .any(|kernel| kernel.id == *id && kernel.target == growth.target)
            });
        }
        self.selected_channel = next.channels[0].id;
        self.replace_draft(next).map_err(|error| error.to_string())
    }

    pub fn select_next_channel(&mut self) {
        let index = self
            .draft
            .channels
            .iter()
            .position(|channel| channel.id == self.selected_channel)
            .unwrap_or(0);
        self.selected_channel = self.draft.channels[(index + 1) % self.draft.channels.len()].id;
        self.growth_editor = editor_for(&self.draft, self.selected_channel);
    }

    pub fn cycle_channel_view(&mut self) {
        self.channel_view = match self.channel_view {
            ChannelView::Composite => ChannelView::Solo,
            ChannelView::Solo => ChannelView::Grid,
            ChannelView::Grid => ChannelView::Composite,
        };
    }

    pub fn cycle_selected_color(&mut self) -> Result<(), HistoryError> {
        let channel = self.selected_channel;
        let current = self
            .draft
            .channels
            .iter()
            .find(|entry| entry.id == channel)
            .map(|entry| entry.display.color.clone())
            .unwrap_or(DisplayColor::Auto);
        let color = match current {
            DisplayColor::Auto => DisplayColor::Custom(RgbColor {
                red: 255,
                green: 0,
                blue: 0,
            }),
            DisplayColor::Custom(RgbColor {
                red: 255,
                green: 0,
                blue: 0,
            }) => DisplayColor::Custom(RgbColor {
                red: 0,
                green: 255,
                blue: 0,
            }),
            DisplayColor::Custom(RgbColor {
                red: 0,
                green: 255,
                blue: 0,
            }) => DisplayColor::Custom(RgbColor {
                red: 0,
                green: 0,
                blue: 255,
            }),
            _ => DisplayColor::Auto,
        };
        self.execute(DraftCommand::SetChannelColor { channel, color })
    }

    pub fn toggle_selected_visibility(&mut self) -> Result<(), HistoryError> {
        let channel = self.selected_channel;
        let visible = !self
            .draft
            .channels
            .iter()
            .find(|entry| entry.id == channel)
            .is_none_or(|entry| entry.display.visible);
        self.execute(DraftCommand::SetChannelVisible { channel, visible })
    }

    pub fn toggle_selected_frozen(&mut self) -> Result<(), HistoryError> {
        let target = self.selected_channel;
        let mut next = self.draft.clone();
        let Some(channel) = next
            .channels
            .iter_mut()
            .find(|channel| channel.id == target)
        else {
            return Ok(());
        };
        channel.frozen = !channel.frozen;
        if channel.frozen {
            next.kernels.retain(|kernel| kernel.target != target);
            next.growth.retain(|growth| growth.target != target);
        } else if !next.growth.iter().any(|growth| growth.target == target) {
            next.growth
                .push(crate::sim::experiment_model::GrowthSource {
                    target,
                    kernel_inputs: Vec::new(),
                    parameters: Default::default(),
                    source: "self".into(),
                    mode: crate::sim::experiment_model::UpdateMode::DirectUpdate,
                });
        }
        self.replace_draft(next)
    }

    pub fn add_kernel_for_selected(&mut self) -> Result<(), HistoryError> {
        let target = self.selected_channel;
        let mut next = self.draft.clone();
        let id = KernelId(
            next.kernels
                .iter()
                .map(|kernel| kernel.id.0.saturating_add(1))
                .max()
                .unwrap_or(0),
        );
        let symbol = format!("k{}", id.0);
        next.kernels
            .push(KernelSlot::identity(id, symbol, target, target));
        if let Some(growth) = next
            .growth
            .iter_mut()
            .find(|growth| growth.target == target)
        {
            growth.kernel_inputs.push(id);
            growth.kernel_inputs.sort_unstable();
        }
        self.replace_draft(next)
    }

    pub fn remove_last_kernel_for_selected(&mut self) -> Result<(), String> {
        let target = self.selected_channel;
        let mut next = self.draft.clone();
        let Some(position) = next
            .kernels
            .iter()
            .rposition(|kernel| kernel.target == target)
        else {
            return Err("selected channel has no kernel".into());
        };
        let removed = next.kernels.remove(position).id;
        if let Some(growth) = next
            .growth
            .iter_mut()
            .find(|growth| growth.target == target)
        {
            growth.kernel_inputs.retain(|id| *id != removed);
        }
        self.replace_draft(next).map_err(|error| error.to_string())
    }

    pub fn cycle_tiling_preset(&mut self) -> Result<(), HistoryError> {
        let mut next = self.draft.clone();
        let preset = match next.tiling.as_ref().map(|tiling| tiling.prototypes.len()) {
            Some(1) => TilingPreset::OctagonSquare,
            _ => TilingPreset::Square,
        };
        next.tiling = Some(build_preset(preset, 1.0));
        self.replace_draft(next)
    }

    pub fn tiling_prototype(&self) -> Option<PrototypeId> {
        self.selected_prototype
    }

    pub fn select_next_prototype(&mut self) {
        let Some(tiling) = &self.draft.tiling else {
            return;
        };
        if tiling.prototypes.is_empty() {
            return;
        }
        let index = tiling
            .prototypes
            .iter()
            .position(|prototype| Some(prototype.id) == self.selected_prototype)
            .unwrap_or(0);
        self.selected_prototype = Some(tiling.prototypes[(index + 1) % tiling.prototypes.len()].id);
    }

    pub fn adjust_prototype_sides(&mut self, delta: i16) -> Result<(), String> {
        let Some(selected) = self.selected_prototype else {
            return Err("choose a tiling prototype first".into());
        };
        let mut next = self.draft.clone();
        let Some(prototype) = next.tiling.as_mut().and_then(|tiling| {
            tiling
                .prototypes
                .iter_mut()
                .find(|prototype| prototype.id == selected)
        }) else {
            return Err("tiling prototype not found".into());
        };
        let PrototypeShape::RegularPolygon { sides, .. } = &mut prototype.shape else {
            return Err("custom polygon vertices are edited by loading a draft or preset".into());
        };
        *sides = (*sides as i16).saturating_add(delta).clamp(3, 64) as u16;
        self.replace_draft(next).map_err(|error| error.to_string())
    }

    pub fn import_draft(&mut self, draft: ExperimentSpec) -> Result<(), HistoryError> {
        self.replace_draft(draft)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adding_kernel_updates_growth_arity_atomically() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        state.add_kernel_for_selected().unwrap();
        assert_eq!(state.draft().kernels.len(), 2);
        assert_eq!(state.draft().growth[0].kernel_inputs.len(), 2);
        crate::sim::experiment_model::validate_structure(state.draft()).unwrap();
        state.undo().unwrap();
        assert_eq!(state.draft().kernels.len(), 1);
    }

    #[test]
    fn channel_and_tiling_actions_are_reversible() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        state.add_channel().unwrap();
        assert_eq!(state.draft().channels.len(), 2);
        state.cycle_tiling_preset().unwrap();
        assert_eq!(state.draft().tiling.as_ref().unwrap().prototypes.len(), 1);
        state.cycle_tiling_preset().unwrap();
        assert_eq!(state.draft().tiling.as_ref().unwrap().prototypes.len(), 2);
    }
}
