use super::{ChannelView, DraftCommand, History, HistoryError};
use crate::sim::experiment_model::{ChannelId, ExperimentSpec};

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
}
impl WorkbenchState {
    pub fn new(spec: ExperimentSpec) -> Self {
        let selected_channel = spec.channels.first().map_or(ChannelId(0), |c| c.id);
        Self {
            authoritative: spec.clone(),
            draft: spec,
            history: History::default(),
            section: WorkbenchSection::World,
            focus: WorkbenchFocus::Outline,
            status: DraftStatus::Clean,
            selected_channel,
            channel_view: ChannelView::Composite,
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
        self.history.clear();
        self.status = DraftStatus::Clean;
    }
    pub fn accept(&mut self, normalized: ExperimentSpec) {
        self.authoritative = normalized.clone();
        self.draft = normalized;
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
}
