use crate::gui::layout;
use crate::sim::experiment_model::ExperimentSpec;

/// The six top-level workspaces of the application.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Section {
    #[default]
    Simulation,
    Tiling,
    Channels,
    Kernels,
    Growth,
    Experiment,
}

impl Section {
    pub const ALL: [Section; 6] = [
        Section::Simulation,
        Section::Tiling,
        Section::Channels,
        Section::Kernels,
        Section::Growth,
        Section::Experiment,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Section::Simulation => "Simulation",
            Section::Tiling => "Tiling",
            Section::Channels => "Channels",
            Section::Kernels => "Kernels",
            Section::Growth => "Growth",
            Section::Experiment => "Experiment",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Section::Simulation => "Run, paint and inspect the live world",
            Section::Tiling => "Design the periodic unit cell and its basis polygons",
            Section::Channels => "Add, color, hide and freeze scalar channels",
            Section::Kernels => "Edit the kernels of the selected binding",
            Section::Growth => "Write and plot the growth program",
            Section::Experiment => "Review the experiment and apply it",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Navigation {
    selected: Section,
}

impl Navigation {
    pub fn selected(&self) -> Section {
        self.selected
    }

    pub fn select(&mut self, section: Section) {
        self.selected = section;
    }
}

/// Which tab the right-hand Inspector shows.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InspectorTab {
    #[default]
    Properties,
    Help,
}

/// Top-level GUI actions. The shell only records the most recent one until the
/// document controller and simulation worker land in later tasks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellAction {
    New,
    Open,
    Save,
    Undo,
    Redo,
    ApplyAndRun,
    ToggleRunning,
    Step,
    Reset,
    Backend,
}

impl ShellAction {
    pub const ALL: [ShellAction; 10] = [
        ShellAction::New,
        ShellAction::Open,
        ShellAction::Save,
        ShellAction::Undo,
        ShellAction::Redo,
        ShellAction::ApplyAndRun,
        ShellAction::ToggleRunning,
        ShellAction::Step,
        ShellAction::Reset,
        ShellAction::Backend,
    ];

    /// Stable egui widget id. Tests and accessibility tooling address controls
    /// through this value, so it must not change with visual label wording.
    pub fn id(self) -> &'static str {
        match self {
            ShellAction::New => "action_new",
            ShellAction::Open => "action_open",
            ShellAction::Save => "action_save",
            ShellAction::Undo => "action_undo",
            ShellAction::Redo => "action_redo",
            ShellAction::ApplyAndRun => "action_apply_and_run",
            ShellAction::ToggleRunning => "action_toggle_running",
            ShellAction::Step => "action_step",
            ShellAction::Reset => "action_reset",
            ShellAction::Backend => "action_backend",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ShellAction::New => "New",
            ShellAction::Open => "Open",
            ShellAction::Save => "Save",
            ShellAction::Undo => "Undo",
            ShellAction::Redo => "Redo",
            ShellAction::ApplyAndRun => "Apply & Run",
            ShellAction::ToggleRunning => "Run/Pause",
            ShellAction::Step => "Step",
            ShellAction::Reset => "Reset",
            ShellAction::Backend => "Backend",
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            ShellAction::New => "Start a new experiment",
            ShellAction::Open => "Open an experiment from disk",
            ShellAction::Save => "Save the current experiment",
            ShellAction::Undo => "Undo the last draft edit",
            ShellAction::Redo => "Redo the last undone draft edit",
            ShellAction::ApplyAndRun => "Compile the draft and replace the running simulation",
            ShellAction::ToggleRunning => "Run or pause the simulation",
            ShellAction::Step => "Advance the simulation by one tick",
            ShellAction::Reset => "Reset the world to its initial state",
            ShellAction::Backend => "Choose the compute backend",
        }
    }
}

/// Composition root of the GUI. It owns transient view state only; model state
/// moves behind a document controller in Task 2.
pub struct CellariumGui {
    spec: ExperimentSpec,
    navigation: Navigation,
    inspector_tab: InspectorTab,
    last_action: Option<ShellAction>,
    running: bool,
}

impl CellariumGui {
    pub fn new(spec: ExperimentSpec) -> Self {
        Self {
            spec,
            navigation: Navigation::default(),
            inspector_tab: InspectorTab::default(),
            last_action: None,
            running: false,
        }
    }

    /// Construct the model without creating a window, event loop or GPU device.
    pub fn for_test(spec: ExperimentSpec) -> Self {
        Self::new(spec)
    }

    pub fn spec(&self) -> &ExperimentSpec {
        &self.spec
    }

    pub fn navigation(&self) -> &Navigation {
        &self.navigation
    }

    pub fn navigation_mut(&mut self) -> &mut Navigation {
        &mut self.navigation
    }

    pub fn inspector_tab(&self) -> InspectorTab {
        self.inspector_tab
    }

    pub fn set_inspector_tab(&mut self, tab: InspectorTab) {
        self.inspector_tab = tab;
    }

    pub fn last_action(&self) -> Option<ShellAction> {
        self.last_action
    }

    pub fn running(&self) -> bool {
        self.running
    }

    pub fn dispatch(&mut self, action: ShellAction) {
        if action == ShellAction::ToggleRunning {
            self.running = !self.running;
        }
        self.last_action = Some(action);
    }

    pub fn status(&self) -> StatusLine {
        StatusLine {
            backend: "CPU (reference)",
            tick: 0,
            simulation_hz: 0.0,
            frame_hz: 0.0,
            draft_clean: true,
            notice: None,
        }
    }
}

/// The bottom status bar contents. Values are placeholders until the simulation
/// worker publishes real snapshots in Task 4.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusLine {
    pub backend: &'static str,
    pub tick: u64,
    pub simulation_hz: f32,
    pub frame_hz: f32,
    pub draft_clean: bool,
    pub notice: Option<String>,
}

impl eframe::App for CellariumGui {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        layout::draw(self, ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_model_starts_on_simulation_without_a_window() {
        let model = CellariumGui::for_test(ExperimentSpec::single_channel_lenia(8, 8));
        assert_eq!(model.navigation().selected(), Section::Simulation);
        assert_eq!(model.last_action(), None);
        assert!(!model.running());
    }

    #[test]
    fn every_shell_action_has_a_unique_id_label_and_tooltip() {
        for (index, action) in ShellAction::ALL.iter().enumerate() {
            assert!(!action.label().is_empty());
            assert!(!action.tooltip().is_empty());
            for other in &ShellAction::ALL[index + 1..] {
                assert_ne!(action.id(), other.id());
                assert_ne!(action.label(), other.label());
            }
        }
    }

    #[test]
    fn toggling_running_flips_state_and_records_the_action() {
        let mut model = CellariumGui::for_test(ExperimentSpec::single_channel_lenia(8, 8));
        model.dispatch(ShellAction::ToggleRunning);
        assert!(model.running());
        assert_eq!(model.last_action(), Some(ShellAction::ToggleRunning));
        model.dispatch(ShellAction::ToggleRunning);
        assert!(!model.running());
    }

    #[test]
    fn every_section_has_a_unique_label() {
        for (index, section) in Section::ALL.iter().enumerate() {
            assert!(!section.hint().is_empty());
            for other in &Section::ALL[index + 1..] {
                assert_ne!(section.label(), other.label());
            }
        }
    }
}
