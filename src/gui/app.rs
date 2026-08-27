use std::sync::Arc;

use crate::document::DocumentController;
use crate::gui::canvas::world::WorldCanvasState;
use crate::gui::layout;
use crate::gui::sections::simulation::SimulationControl;
use crate::sim::backend_selector::{BackendPolicy, BackendSelector};
use crate::sim::compute_plan::compile_compute_plan;
use crate::sim::experiment_model::ExperimentSpec;
use crate::sim::local_backend::{BackendProbe, initial_cells};
use crate::sim::worker::{
    BackendFallback, SimulationCommand, SimulationController, SimulationSnapshot,
};

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
    backend_policy: BackendPolicy,
    probes: Vec<BackendProbe>,
    backend_open: bool,
    fallback_notice: Option<String>,
    world_canvas: WorldCanvasState,
    randomize_seed: u64,
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
        app.restart_simulation();
        app
    }

    /// Rebuild the worker under the current policy, keeping the window open and
    /// reporting the reason if nothing can run.
    pub fn restart_simulation(&mut self) {
        match self.start_simulation() {
            Ok(controller) => {
                self.simulation = Some(controller);
                self.startup_notice = None;
            }
            Err(reason) => {
                self.simulation = None;
                self.startup_notice = Some(reason);
            }
        }
    }

    pub fn backend_policy(&self) -> &BackendPolicy {
        &self.backend_policy
    }

    pub fn probes(&self) -> &[BackendProbe] {
        &self.probes
    }

    pub fn backend_panel_open(&self) -> bool {
        self.backend_open
    }

    /// Choose a policy and rebuild on it. A policy with nothing to run on
    /// leaves the reason in the status bar rather than silently using another.
    pub fn select_backend(&mut self, policy: BackendPolicy) {
        self.backend_policy = policy;
        self.restart_simulation();
    }

    /// Construct the model without creating a window, event loop, GPU device or
    /// worker thread.
    pub fn for_test(spec: ExperimentSpec) -> Self {
        Self {
            document: DocumentController::new(spec),
            simulation: None,
            startup_notice: None,
            backend_policy: BackendPolicy::Auto,
            probes: Vec::new(),
            backend_open: false,
            fallback_notice: None,
            world_canvas: WorldCanvasState::default(),
            randomize_seed: 0x2545_F491_4F6C_DD1D,
            navigation: Navigation::default(),
            inspector_tab: InspectorTab::default(),
            last_action: None,
            running: false,
        }
    }

    fn start_simulation(&mut self) -> Result<SimulationController, String> {
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
        let probes = BackendSelector::probe_all(&plan);
        let candidates = BackendSelector::candidates(self.backend_policy.clone(), probes.clone());
        self.probes = probes;
        if candidates.is_empty() {
            return Err(format!(
                "no backend satisfies the selected policy: {}",
                crate::gui::widgets::backend_picker::unavailable_reason(
                    &self.backend_policy,
                    &self.probes
                )
                .unwrap_or_else(|| "nothing available".into())
            ));
        }
        let plan = std::sync::Arc::new(plan);
        let (backend, rejected) = BackendSelector::build(&candidates, &plan, &cells)
            .map_err(|reasons| reasons.join("; "))?;
        let notice = crate::sim::backend_selector::fallback_notice(&rejected, backend.descriptor());
        let fallback = BackendFallback::new(
            std::sync::Arc::clone(&plan),
            candidates,
            self.backend_policy.clone(),
        );
        let controller = SimulationController::spawn_with_fallback(backend, Some(fallback))
            .map_err(|error| error.to_string())?;
        self.fallback_notice = notice;
        Ok(controller)
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
            ShellAction::Backend => {
                self.backend_open = !self.backend_open;
                None
            }
            _ => None,
        };
        if let (Some(command), Some(simulation)) = (command, self.simulation.as_ref()) {
            // A dropped command means the worker already stopped; the status bar
            // reports that through the next snapshot rather than blocking here.
            let _ = simulation.send(command);
        }
        self.last_action = Some(action);
    }

    /// Wait for a published snapshot to satisfy `predicate`. Test support: the
    /// GUI itself never waits on the worker.
    pub fn wait_for_simulation(
        &self,
        predicate: impl Fn(&SimulationSnapshot) -> bool,
    ) -> Arc<SimulationSnapshot> {
        self.simulation
            .as_ref()
            .expect("a worker must be running")
            .wait_for(predicate)
    }

    pub fn world_canvas(&self) -> &WorldCanvasState {
        &self.world_canvas
    }

    pub fn world_canvas_mut(&mut self) -> &mut WorldCanvasState {
        &mut self.world_canvas
    }

    /// Send a command to the worker, ignoring a stopped worker: the status bar
    /// reports that through the next snapshot rather than blocking here.
    pub fn send_simulation(&self, command: SimulationCommand) {
        if let Some(simulation) = self.simulation.as_ref() {
            let _ = simulation.send(command);
        }
    }

    /// Handle a click on one of the Simulation canvas controls.
    pub fn dispatch_simulation(&mut self, control: SimulationControl) {
        match control {
            SimulationControl::RunPause => self.dispatch(ShellAction::ToggleRunning),
            SimulationControl::Step => self.send_simulation(SimulationCommand::Step(1)),
            SimulationControl::Reset => self.send_simulation(SimulationCommand::Reset),
            SimulationControl::Randomize => {
                self.randomize_seed = self
                    .randomize_seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1);
                self.send_simulation(SimulationCommand::Randomize {
                    seed: self.randomize_seed,
                });
            }
            SimulationControl::Clear => self.send_simulation(SimulationCommand::Clear),
            SimulationControl::Fit => self.world_canvas.request_fit(),
        }
    }

    pub fn status(&self) -> StatusLine {
        let snapshot = self.snapshot();
        let backend = snapshot
            .as_ref()
            .map(|snapshot| snapshot.backend.summary())
            .unwrap_or_else(|| "no backend".into());
        let notice = self
            .startup_notice
            .clone()
            .or_else(|| self.fallback_notice.clone())
            .or_else(|| {
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
        // Auto picks the best backend this machine offers, so assert that the
        // status names a probed backend rather than assuming which one won.
        let backend = model.status().backend;
        assert!(
            model
                .probes()
                .iter()
                .any(|probe| probe.available && backend.starts_with(probe.kind.label())),
            "status reported {backend}, which no available probe matches"
        );
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
    fn requiring_the_cpu_runs_on_the_cpu() {
        let mut model = CellariumGui::new(ExperimentSpec::single_channel_lenia(8, 8));
        model.select_backend(BackendPolicy::RequireCpu);
        assert!(model.status().backend.starts_with("CPU"));
        assert_eq!(*model.backend_policy(), BackendPolicy::RequireCpu);
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
