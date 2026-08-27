use std::sync::Arc;

use crate::document::DocumentController;
use crate::gui::layout;
use crate::sim::compute_plan::compile_compute_plan;
use crate::sim::experiment_model::ExperimentSpec;
use crate::sim::local_backend::{CpuBackend, initial_cells};
use crate::sim::worker::{SimulationCommand, SimulationController, SimulationSnapshot};

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

/// Composition root of the GUI. It owns transient view state, the document and
/// a handle to the simulation worker. It never runs simulation work itself.
pub struct CellariumGui {
    document: DocumentController,
    simulation: Option<SimulationController>,
    startup_notice: Option<String>,
    navigation: Navigation,
    inspector_tab: InspectorTab,
    last_action: Option<ShellAction>,
    running: bool,
}

impl CellariumGui {
    /// Build the GUI and start a local simulation worker for `spec`.
    ///
    /// A spec that cannot be compiled still opens the window: the reason is
    /// shown in the status bar instead of failing startup, so the user can see
    /// and fix the problem.
    pub fn new(spec: ExperimentSpec) -> Self {
        let mut app = Self::for_test(spec);
        match app.start_simulation() {
            Ok(controller) => app.simulation = Some(controller),
            Err(reason) => app.startup_notice = Some(reason),
        }
        app
    }

    /// Construct the model without creating a window, event loop, GPU device or
    /// worker thread.
    pub fn for_test(spec: ExperimentSpec) -> Self {
        Self {
            document: DocumentController::new(spec),
            simulation: None,
            startup_notice: None,
            navigation: Navigation::default(),
            inspector_tab: InspectorTab::default(),
            last_action: None,
            running: false,
        }
    }

    fn start_simulation(&self) -> Result<SimulationController, String> {
        let normalized = self
            .document
            .draft()
            .clone()
            .normalize_rules()
            .map_err(|errors| {
                errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ")
            })?;
        let plan = compile_compute_plan(&normalized).map_err(|diagnostics| {
            diagnostics
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        })?;
        let cells = initial_cells(&plan, &normalized);
        let backend = CpuBackend::new(&plan, &cells).map_err(|error| error.to_string())?;
        SimulationController::spawn(Box::new(backend)).map_err(|error| error.to_string())
    }

    pub fn document(&self) -> &DocumentController {
        &self.document
    }

    pub fn document_mut(&mut self) -> &mut DocumentController {
        &mut self.document
    }

    pub fn spec(&self) -> &ExperimentSpec {
        self.document.draft()
    }

    /// The newest published snapshot, if a worker is running. Reading it never
    /// blocks on the worker.
    pub fn snapshot(&self) -> Option<Arc<SimulationSnapshot>> {
        self.simulation.as_ref().map(SimulationController::snapshot)
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

    /// What the user last asked for. The published snapshot can still lag this
    /// by a frame, so the repaint schedule uses both.
    pub fn running_intent(&self) -> bool {
        self.running
    }

    pub fn running(&self) -> bool {
        match self.snapshot() {
            Some(snapshot) => snapshot.running,
            None => self.running,
        }
    }

    pub fn dispatch(&mut self, action: ShellAction) {
        let command = match action {
            ShellAction::ToggleRunning => {
                self.running = !self.running();
                Some(SimulationCommand::SetRunning(self.running))
            }
            ShellAction::Step => Some(SimulationCommand::Step(1)),
            ShellAction::Reset => Some(SimulationCommand::Reset),
            _ => None,
        };
        if let (Some(command), Some(simulation)) = (command, self.simulation.as_ref()) {
            // A dropped command means the worker already stopped; the status bar
            // reports that through the next snapshot rather than blocking here.
            let _ = simulation.send(command);
        }
        self.last_action = Some(action);
    }

    pub fn status(&self) -> StatusLine {
        let snapshot = self.snapshot();
        let backend = snapshot
            .as_ref()
            .map(|snapshot| snapshot.backend.summary())
            .unwrap_or_else(|| "no backend".into());
        let notice = self.startup_notice.clone().or_else(|| {
            snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.error.as_ref())
                .map(|error| error.message.clone())
        });
        StatusLine {
            backend,
            tick: snapshot.as_ref().map(|snapshot| snapshot.tick).unwrap_or(0),
            simulation_hz: snapshot
                .as_ref()
                .map(|snapshot| step_rate(snapshot))
                .unwrap_or(0.0),
            frame_hz: 0.0,
            draft_clean: self.document.status() == crate::document::DraftStatus::Clean,
            notice,
        }
    }
}

/// Steps per second implied by the newest step, or zero when nothing has run.
fn step_rate(snapshot: &SimulationSnapshot) -> f32 {
    if snapshot.step_stats.elapsed_micros == 0 || snapshot.step_stats.steps == 0 {
        return 0.0;
    }
    snapshot.step_stats.steps as f32 * 1_000_000.0 / snapshot.step_stats.elapsed_micros as f32
}

/// The bottom status bar contents. Values are placeholders until the simulation
/// worker publishes real snapshots in Task 4.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusLine {
    pub backend: String,
    pub tick: u64,
    pub simulation_hz: f32,
    pub frame_hz: f32,
    pub draft_clean: bool,
    pub notice: Option<String>,
}

impl eframe::App for CellariumGui {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        // While the simulation runs, ask for a repaint at the display cadence
        // instead of polling the worker or spinning at full speed. The local
        // intent counts as running too: the frame that presses Run still sees
        // the old paused snapshot, and without this the display would freeze
        // until some unrelated input arrived.
        if self.running || self.running() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
        }
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
    fn a_started_gui_reports_its_backend_and_reaches_the_worker() {
        let mut model = CellariumGui::new(ExperimentSpec::single_channel_lenia(8, 8));
        assert!(model.snapshot().is_some(), "worker should be running");
        assert!(model.status().backend.starts_with("CPU"));
        assert_eq!(model.status().tick, 0);

        model.dispatch(ShellAction::Step);
        let simulation = model.simulation.as_ref().unwrap();
        let snapshot = simulation.wait_for(|state| state.tick >= 1);
        assert_eq!(snapshot.tick, 1);
        assert!(!snapshot.running);
    }

    #[test]
    fn a_running_simulation_keeps_publishing_new_ticks() {
        let mut model = CellariumGui::new(ExperimentSpec::single_channel_lenia(8, 8));
        model.dispatch(ShellAction::ToggleRunning);
        assert!(model.running_intent(), "pressing Run records the intent");
        let simulation = model.simulation.as_ref().unwrap();
        let first = simulation.wait_for(|state| state.running && state.tick >= 1);
        let later = simulation.wait_for(|state| state.tick > first.tick);
        assert!(later.tick > first.tick);
    }

    #[test]
    fn a_gui_without_a_worker_still_reports_a_readable_status() {
        let model = CellariumGui::for_test(ExperimentSpec::single_channel_lenia(8, 8));
        let status = model.status();
        assert_eq!(status.backend, "no backend");
        assert!(status.draft_clean);
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
