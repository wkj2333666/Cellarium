use std::sync::Arc;

use crate::document::persistence::{self, GuiSettings};
use crate::document::recording::{Recording, ReplayState};
use crate::document::{DocumentCommand, DocumentController};
use crate::gui::canvas::channels::{
    ChannelCanvasState, ChannelPreview, ChannelPreviewSource, ChannelView, resolve_preview,
};
use crate::gui::canvas::growth::{GrowthPlotState, PlotInput, PlotScene, compute, default_axes};
use crate::gui::canvas::kernel::{KernelCanvasState, KernelEdit, KernelStencil};
use crate::gui::canvas::tiling::TilingCanvasState;
use crate::gui::canvas::world::WorldCanvasState;
use crate::gui::layout;
use crate::gui::sections::simulation::SimulationControl;
use crate::gui::widgets::decision_dialog::Decision;
use crate::gui::widgets::file_dialog::{FileDialog, FileDialogKind, FileDialogOutcome};
use crate::gui::widgets::numeric_popover::NumericPopover;
use crate::sim::backend_selector::{BackendPolicy, BackendSelector};
use crate::sim::compute_plan::compile_compute_plan;
use crate::sim::experiment_model::{ChannelId, ExperimentSpec, KernelId};
use crate::sim::local_backend::{BackendProbe, initial_cells};
use crate::sim::ruleset::BindingKey;
use crate::sim::tiling::SeamAssessment;
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
    SaveAs,
    Undo,
    Redo,
    ApplyAndRun,
    ToggleRunning,
    Step,
    Reset,
    Backend,
}

impl ShellAction {
    pub const ALL: [ShellAction; 11] = [
        ShellAction::New,
        ShellAction::Open,
        ShellAction::Save,
        ShellAction::SaveAs,
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
            ShellAction::SaveAs => "action_save_as",
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
            ShellAction::SaveAs => "Save as",
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
            ShellAction::SaveAs => "Save the experiment to a new file",
            ShellAction::Undo => "Undo the last draft edit",
            ShellAction::Redo => "Redo the last undone draft edit",
            ShellAction::ApplyAndRun => "Compile the draft and replace the running simulation",
            ShellAction::ToggleRunning => "Run or pause the simulation",
            ShellAction::Step => "Advance the simulation by one tick",
            ShellAction::Reset => "Reset the world to its initial state",
            ShellAction::Backend => "Choose the compute backend",
        }
    }

    /// The key that reaches this action, if it has one.
    ///
    /// Every one of these has a visible control; the key is an accelerator, not
    /// the only way in. That is the promise the Help panel makes, and it is
    /// what keeps the window usable for someone who has never read it.
    pub fn shortcut(self) -> Option<eframe::egui::KeyboardShortcut> {
        use eframe::egui::{Key, KeyboardShortcut, Modifiers};
        let shortcut = match self {
            ShellAction::New => KeyboardShortcut::new(Modifiers::COMMAND, Key::N),
            ShellAction::Open => KeyboardShortcut::new(Modifiers::COMMAND, Key::O),
            ShellAction::Save => KeyboardShortcut::new(Modifiers::COMMAND, Key::S),
            ShellAction::SaveAs => {
                KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S)
            }
            ShellAction::Undo => KeyboardShortcut::new(Modifiers::COMMAND, Key::Z),
            ShellAction::Redo => {
                KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z)
            }
            ShellAction::ApplyAndRun => KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter),
            ShellAction::ToggleRunning => KeyboardShortcut::new(Modifiers::NONE, Key::Space),
            ShellAction::Step => KeyboardShortcut::new(Modifiers::NONE, Key::ArrowRight),
            ShellAction::Reset => KeyboardShortcut::new(Modifiers::COMMAND, Key::R),
            ShellAction::Backend => KeyboardShortcut::new(Modifiers::COMMAND, Key::B),
        };
        Some(shortcut)
    }

    /// How the shortcut reads in a menu or a help list.
    pub fn shortcut_text(self, ctx: &eframe::egui::Context) -> Option<String> {
        self.shortcut()
            .map(|shortcut| ctx.format_shortcut(&shortcut))
    }
}

/// Width and height of the world a new experiment starts with.
pub const DEFAULT_WORLD: u32 = 256;

/// Something the user asked for that would discard unsaved work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PendingIntent {
    New,
    Open,
}

/// A file name derived from the experiment's own name, so the suggested name
/// means something before the user has typed anything.
fn suggested_file_name(spec: &ExperimentSpec) -> String {
    let stem: String = spec
        .name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let stem = stem.trim_matches('-').replace("--", "-");
    let stem = if stem.is_empty() { "experiment" } else { &stem };
    format!("{stem}.ron")
}

/// Whether a message reports a problem or just says what happened.
///
/// Colour is the first thing read in a status bar, so a successful save shown
/// in the failure colour reads as a failure.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NoticeLevel {
    #[default]
    Problem,
    Info,
}

/// How long a status message stays on screen. Long enough to read, short
/// enough that it never describes an action the user has forgotten making.
const NOTICE_SECONDS: f64 = 12.0;

/// How often the recovery snapshot is rewritten while the draft is dirty.
const AUTOSAVE_SECONDS: f64 = 5.0;

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
    tiling_canvas: TilingCanvasState,
    channel_canvas: ChannelCanvasState,
    /// Working RGB for the colour popover, so dragging the fields does not
    /// write a new draft on every pixel of movement.
    channel_colour_draft: [u8; 3],
    /// The channel and colour the draft above was last seeded from.
    ///
    /// The picker has to open on the channel's own colour, but seeding it every
    /// frame overwrites whatever the user is in the middle of typing, which
    /// left the exact-colour fields impossible to change at all.
    channel_colour_seed: Option<(ChannelId, [u8; 3])>,
    kernel_canvas: KernelCanvasState,
    kernel_popover: NumericPopover,
    /// A destructive kernel edit waiting for the user's answer, with the
    /// draft it would produce already computed.
    kernel_decision: Option<(Decision, Box<ExperimentSpec>)>,
    growth_plot: GrowthPlotState,
    /// Smoothed rate the window is actually being drawn at.
    frame_hz: f32,
    /// Where this experiment was opened from or last saved to.
    experiment_path: Option<std::path::PathBuf>,
    settings: GuiSettings,
    /// Directory holding settings and the autosave.
    data_root: Option<std::path::PathBuf>,
    /// The open file dialog, if the user is choosing a file.
    file_dialog: Option<FileDialog>,
    /// What to do once the user has answered about unsaved work.
    pending_intent: Option<PendingIntent>,
    /// An experiment a previous session left behind, offered on startup.
    recovery: Option<Box<ExperimentSpec>>,
    /// Whether the draft has changed since it was last written to disk.
    unsaved_changes: bool,
    /// Captured frames of the run, and the playhead over them.
    recording: Recording,
    notice: Option<String>,
    /// When `notice` was set, on the frame clock, so it can expire.
    notice_at: Option<f64>,
    notice_level: NoticeLevel,
    /// When the recovery snapshot was last written.
    autosaved_at: f64,
    /// Seconds since the window opened, refreshed once per frame.
    now: f64,
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
        Self::with_backend(spec, BackendPolicy::Auto)
    }

    /// Build the GUI and start a worker on a named backend.
    ///
    /// Choosing before starting matters: `Auto` asks the drivers what this
    /// machine has, which creates a device. A caller that already knows it
    /// wants the CPU should not pay for a GPU it is about to discard, and
    /// several such callers at once contend for one card.
    pub fn with_backend(spec: ExperimentSpec, policy: BackendPolicy) -> Self {
        let mut app = Self::for_test(spec);
        app.backend_policy = policy;
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
            tiling_canvas: TilingCanvasState::default(),
            channel_canvas: ChannelCanvasState::default(),
            channel_colour_draft: [236, 240, 246],
            channel_colour_seed: None,
            kernel_canvas: KernelCanvasState::new(),
            kernel_popover: NumericPopover::default(),
            kernel_decision: None,
            growth_plot: GrowthPlotState::default(),
            frame_hz: 0.0,
            experiment_path: None,
            settings: GuiSettings::default(),
            data_root: None,
            file_dialog: None,
            pending_intent: None,
            recovery: None,
            unsaved_changes: false,
            recording: Recording::default(),
            notice: None,
            notice_at: None,
            notice_level: NoticeLevel::default(),
            autosaved_at: 0.0,
            now: 0.0,
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

    pub fn recording(&self) -> &Recording {
        &self.recording
    }

    pub fn recording_mut(&mut self) -> &mut Recording {
        &mut self.recording
    }

    /// Start or stop capturing frames.
    pub fn toggle_recording(&mut self) {
        if self.recording.is_capturing() {
            self.recording.stop();
            let summary = format!("recording stopped — {}", self.recording.summary());
            self.set_info(summary);
        } else {
            self.recording.start();
            self.set_info("recording — frames are kept until you stop");
        }
    }

    pub fn toggle_replay(&mut self) {
        match self.recording.state() {
            ReplayState::Playing => self.recording.pause(),
            _ => self.recording.play(),
        }
    }

    /// The snapshot the canvas should draw.
    ///
    /// While replaying this is a recorded frame, so every readout beside the
    /// canvas describes the frame on screen rather than a live world the user
    /// is not currently looking at.
    pub fn displayed_snapshot(&self) -> Option<Arc<SimulationSnapshot>> {
        if self.recording.is_replaying() {
            return self.recording.current();
        }
        self.snapshot()
    }

    /// Take a frame if recording, and move the playhead if replaying.
    fn drive_recording(&mut self, dt: f64) {
        // Paced rather than every frame: see `tick_capture_clock`.
        if self.recording.tick_capture_clock(dt)
            && let Some(snapshot) = self.snapshot()
        {
            self.recording.capture(snapshot);
        }
        self.recording.advance(dt);
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

    /// Fire any action whose key was pressed this frame.
    ///
    /// Two things are deliberately not shortcuts here. While a dialog is open
    /// it owns the keyboard, because a stray Space behind a modal would run the
    /// simulation the user is being asked a question about. And while a text
    /// field has focus only Command-modified keys are accelerators: Space and
    /// the arrow keys belong to whoever is typing, and Undo belongs to the
    /// editor holding the caret.
    fn consume_shortcuts(&mut self, ctx: &eframe::egui::Context) {
        if self.file_dialog.is_some()
            || self.pending_intent.is_some()
            || self.recovery.is_some()
            || self.kernel_decision.is_some()
        {
            return;
        }
        let typing = ctx.egui_wants_keyboard_input();
        let mut fired = Vec::new();
        for action in ShellAction::ALL {
            let Some(shortcut) = action.shortcut() else {
                continue;
            };
            if typing {
                let editing_key = matches!(action, ShellAction::Undo | ShellAction::Redo);
                if !shortcut.modifiers.command || editing_key {
                    continue;
                }
            }
            if ctx.input_mut(|input| input.consume_shortcut(&shortcut)) {
                fired.push(action);
            }
        }
        for action in fired {
            self.dispatch(action);
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
            ShellAction::ApplyAndRun => {
                self.apply_and_run();
                None
            }
            ShellAction::Save => {
                self.save_experiment();
                None
            }
            ShellAction::SaveAs => {
                self.begin_save_as();
                None
            }
            ShellAction::New => {
                self.request_new();
                None
            }
            ShellAction::Open => {
                self.request_open();
                None
            }
            ShellAction::Undo => {
                // Undo and Redo are document transactions, not simulation
                // commands: they rewind the draft the user is editing.
                match self.document.undo() {
                    Ok(_) => self.set_notice(None),
                    Err(error) => self.set_notice(Some(error.to_string())),
                }
                self.channel_canvas.invalidate();
                None
            }
            ShellAction::Redo => {
                match self.document.redo() {
                    Ok(_) => self.set_notice(None),
                    Err(error) => self.set_notice(Some(error.to_string())),
                }
                self.channel_canvas.invalidate();
                None
            } // Every action is listed. There is deliberately no catch-all arm:
              // New and Open were enabled, documented buttons that did nothing
              // for exactly as long as one silently swallowed them.
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

    pub fn experiment_path(&self) -> Option<&std::path::Path> {
        self.experiment_path.as_deref()
    }

    pub fn settings(&self) -> &GuiSettings {
        &self.settings
    }

    /// Remember where the experiment on screen came from.
    ///
    /// Launching with `--experiment` is opening a file, and the path has to
    /// survive that: without it Save has nowhere to write and can only report
    /// that the experiment is unnamed.
    pub fn set_experiment_path(&mut self, path: impl Into<std::path::PathBuf>) {
        let path = path.into();
        self.settings.remember(&path);
        self.experiment_path = Some(path);
    }

    /// Point the session at a directory for settings and autosave.
    pub fn use_data_root(&mut self, root: impl Into<std::path::PathBuf>) {
        let root = root.into();
        self.settings = persistence::load_settings(&root);
        self.data_root = Some(root);
    }

    /// Reasons the draft cannot be applied, in the model's own words.
    pub fn draft_problems(&self) -> Vec<String> {
        match self.document.draft().clone().normalize_rules() {
            Ok(candidate) => {
                let mut problems: Vec<String> =
                    crate::sim::experiment_model::validate_structure(&candidate)
                        .err()
                        .map(|errors| errors.iter().map(ToString::to_string).collect())
                        .unwrap_or_default();
                // The same questions Apply asks, so this workspace cannot report
                // a draft as ready that Apply would then refuse.
                problems.extend(crate::document::growth::invalid_programs(&candidate));
                problems.extend(crate::document::tiling::coverage_problems(&candidate));
                problems
            }
            Err(errors) => errors.iter().map(ToString::to_string).collect(),
        }
    }

    /// Validate the draft and, only if it is sound, make it the running
    /// experiment.
    ///
    /// A rejected candidate leaves the active experiment and its world exactly
    /// as they were: the draft is proven before anything is replaced, never
    /// after.
    pub fn apply_and_run(&mut self) {
        let candidate = match self.document.prepare_apply() {
            Ok(candidate) => candidate,
            Err(errors) => {
                self.set_notice(Some(errors.join("; ")));
                return;
            }
        };
        let request = candidate.request_id;
        let experiment = candidate.experiment;
        if !self.document.accept_apply(request, experiment) {
            // The draft moved while the candidate was being built, so the
            // candidate describes an experiment nobody asked for.
            self.set_notice(Some("the draft changed while it was being applied".into()));
            return;
        }
        self.restart_simulation();
        if self.startup_notice.is_none() {
            self.set_notice(None);
            self.running = true;
            self.send_simulation(SimulationCommand::SetRunning(true));
        }
        self.channel_canvas.invalidate();
        self.autosave();
    }

    /// Save to the current path, asking for one the first time.
    ///
    /// An experiment that has never been written has nowhere to go, and telling
    /// the user to use a control that does not exist is worse than useless.
    /// Save on an unnamed experiment is Save as.
    pub fn save_experiment(&mut self) {
        match self.experiment_path.clone() {
            Some(path) => self.save_experiment_as(path),
            None => self.begin_save_as(),
        }
    }

    /// Ask where to write this experiment.
    pub fn begin_save_as(&mut self) {
        let suggested = suggested_file_name(self.document.draft());
        self.file_dialog = Some(FileDialog::new(
            FileDialogKind::Save,
            self.experiment_path.as_deref().or_else(|| {
                self.settings
                    .recent
                    .first()
                    .map(std::path::PathBuf::as_path)
            }),
            &suggested,
        ));
    }

    /// Start a new experiment, asking about unsaved work first.
    pub fn request_new(&mut self) {
        if self.unsaved_changes {
            self.pending_intent = Some(PendingIntent::New);
            return;
        }
        self.start_new_experiment();
    }

    /// Open an experiment, asking about unsaved work first.
    pub fn request_open(&mut self) {
        if self.unsaved_changes {
            self.pending_intent = Some(PendingIntent::Open);
            return;
        }
        self.begin_open();
    }

    pub fn begin_open(&mut self) {
        self.file_dialog = Some(FileDialog::new(
            FileDialogKind::Open,
            self.experiment_path.as_deref().or_else(|| {
                self.settings
                    .recent
                    .first()
                    .map(std::path::PathBuf::as_path)
            }),
            "",
        ));
    }

    fn start_new_experiment(&mut self) {
        self.new_experiment(ExperimentSpec::single_channel_lenia(
            DEFAULT_WORLD,
            DEFAULT_WORLD,
        ));
        self.set_info("started a new experiment");
    }

    pub fn file_dialog_open(&self) -> bool {
        self.file_dialog.is_some()
    }

    /// Draw the file dialog and act on the answer.
    pub fn drive_file_dialog(&mut self, ctx: &eframe::egui::Context) {
        let Some(dialog) = self.file_dialog.as_mut() else {
            return;
        };
        let kind = dialog.kind();
        let recent = self.settings.recent.clone();
        match dialog.show(ctx, &recent) {
            FileDialogOutcome::Pending => {}
            FileDialogOutcome::Cancelled => self.file_dialog = None,
            FileDialogOutcome::Chosen(path) => {
                self.file_dialog = None;
                match kind {
                    FileDialogKind::Open => self.open_experiment(path),
                    FileDialogKind::Save => self.save_experiment_as(path),
                }
            }
        }
    }

    /// The question waiting on the user about unsaved work, if any.
    pub fn pending_intent(&self) -> Option<PendingIntent> {
        self.pending_intent
    }

    pub fn resolve_pending_intent(&mut self, proceed: bool) {
        let Some(intent) = self.pending_intent.take() else {
            return;
        };
        if !proceed {
            return;
        }
        match intent {
            PendingIntent::New => self.start_new_experiment(),
            PendingIntent::Open => self.begin_open(),
        }
    }

    /// An experiment a previous session left behind, waiting to be offered.
    pub fn pending_recovery(&self) -> Option<&ExperimentSpec> {
        self.recovery.as_deref()
    }

    /// Look for work a previous session did not save.
    ///
    /// The autosave is written continually and was never read back, so a
    /// session that ended without saving lost everything it had done while a
    /// complete copy sat on disk.
    pub fn offer_recovery(&mut self) {
        if self.experiment_path.is_some() {
            // Opening a named experiment is an explicit choice of what to work
            // on; it should not be interrupted by an older draft.
            return;
        }
        self.recovery = self.recoverable().map(Box::new);
    }

    pub fn accept_recovery(&mut self) {
        if let Some(spec) = self.recovery.take() {
            self.new_experiment(*spec);
            self.unsaved_changes = true;
            self.set_info("restored the experiment from the last session");
        }
    }

    pub fn decline_recovery(&mut self) {
        self.recovery = None;
        self.discard_recovery();
    }

    pub fn has_unsaved_changes(&self) -> bool {
        self.unsaved_changes
    }

    pub fn save_experiment_as(&mut self, path: impl Into<std::path::PathBuf>) {
        let path = path.into();
        match persistence::save_experiment(&path, self.document.draft()) {
            Ok(()) => {
                self.settings.remember(&path);
                self.experiment_path = Some(path.clone());
                self.persist_settings();
                self.unsaved_changes = false;
                // The autosave exists to survive a session that ends without
                // saving. Work that has been saved does not need it.
                self.discard_recovery();
                self.set_info(format!(
                    "saved to {}",
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ));
            }
            Err(error) => self.set_notice(Some(error.to_string())),
        }
    }

    /// Open an experiment, replacing the document and the running world.
    pub fn open_experiment(&mut self, path: impl Into<std::path::PathBuf>) {
        let path = path.into();
        match persistence::load_experiment(&path) {
            Ok(spec) => {
                self.document = DocumentController::new(spec);
                self.settings.remember(&path);
                self.experiment_path = Some(path.clone());
                self.persist_settings();
                self.unsaved_changes = false;
                self.set_info(format!(
                    "opened {}",
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ));
                self.reset_view_state();
                self.restart_simulation();
            }
            Err(error) => self.set_notice(Some(error.to_string())),
        }
    }

    /// Start a fresh experiment, keeping the window open.
    pub fn new_experiment(&mut self, spec: ExperimentSpec) {
        self.document = DocumentController::new(spec);
        self.experiment_path = None;
        self.unsaved_changes = false;
        self.set_notice(None);
        self.reset_view_state();
        self.restart_simulation();
    }

    /// Views hold zoom, selections and cached textures for the experiment that
    /// was open. None of it describes the new one.
    fn reset_view_state(&mut self) {
        self.world_canvas = WorldCanvasState::default();
        self.tiling_canvas = TilingCanvasState::default();
        self.channel_canvas = ChannelCanvasState::default();
        self.kernel_canvas = KernelCanvasState::new();
        self.kernel_popover.close();
        self.kernel_decision = None;
        self.growth_plot = GrowthPlotState::default();
    }

    fn persist_settings(&mut self) {
        if let Some(root) = self.data_root.clone()
            && let Err(error) = persistence::save_settings(&root, &self.settings)
        {
            self.set_notice(Some(error.to_string()));
        }
    }

    /// Write the recovery snapshot if enough time has passed and the draft has
    /// moved since the last one.
    ///
    /// Autosaving only on Apply meant a draft the user had been editing for an
    /// hour without applying was never written at all, which is exactly the
    /// work a recovery file exists to protect.
    fn autosave_if_due(&mut self) {
        if !self.unsaved_changes || self.data_root.is_none() {
            return;
        }
        if self.now - self.autosaved_at < AUTOSAVE_SECONDS {
            return;
        }
        self.autosaved_at = self.now;
        self.autosave();
    }

    /// Write a recovery snapshot. The draft is cloned first, so the copy being
    /// written is never the one the user is still editing.
    pub fn autosave(&mut self) {
        let Some(root) = self.data_root.clone() else {
            return;
        };
        let snapshot = self.document.draft().clone();
        if let Err(error) = persistence::write_autosave(&root, &snapshot) {
            self.set_notice(Some(error.to_string()));
        }
    }

    /// An experiment left behind by a session that did not finish.
    pub fn recoverable(&self) -> Option<ExperimentSpec> {
        self.data_root.as_ref().and_then(persistence::recover)
    }

    pub fn discard_recovery(&mut self) {
        if let Some(root) = self.data_root.clone() {
            persistence::clear_autosave(root);
        }
    }

    pub fn growth_plot(&self) -> &GrowthPlotState {
        &self.growth_plot
    }

    pub fn growth_plot_mut(&mut self) -> &mut GrowthPlotState {
        &mut self.growth_plot
    }

    pub fn growth_signature(&self) -> crate::document::growth::GrowthSignature {
        crate::document::growth::signature_of(self.spec(), self.selected_binding())
    }

    pub fn growth_mode(&self) -> crate::sim::experiment_model::UpdateMode {
        crate::document::growth::mode_of(self.spec(), self.selected_binding())
            .unwrap_or(crate::sim::experiment_model::UpdateMode::GrowthRate)
    }

    pub fn set_growth_mode(&mut self, mode: crate::sim::experiment_model::UpdateMode) {
        let binding = self.selected_binding();
        match crate::document::growth::set_mode(self.spec(), binding, mode) {
            Ok(spec) => self.dispatch_document(DocumentCommand::ReplaceExperiment(Box::new(spec))),
            Err(error) => self.set_notice(Some(error)),
        }
    }

    /// Kernel symbols the program actually reads.
    pub fn growth_referenced(&self) -> Vec<String> {
        crate::document::growth::analyze(self.spec(), self.selected_binding()).unwrap_or_default()
    }

    pub fn growth_diagnostics(&self) -> Vec<crate::document::growth::GrowthDiagnostic> {
        crate::document::growth::analyze(self.spec(), self.selected_binding())
            .err()
            .unwrap_or_default()
    }

    pub fn kernel_symbol(&self, kernel: KernelId) -> String {
        self.kernel_cards()
            .into_iter()
            .find(|card| card.id == kernel)
            .map(|card| card.symbol)
            .unwrap_or_else(|| format!("k{}", kernel.0))
    }

    /// The axes the plot uses: the user's choice if they made one, otherwise
    /// the ones the program's own references imply.
    pub fn plot_axes(&self) -> crate::document::selection::PlotAxes {
        let signature = self.growth_signature();
        // A chosen axis only survives while the kernel it names does. Deleting
        // a kernel the plot was drawn against used to leave the caption naming
        // a symbol no signature contained — "x: k0" for a kernel that had been
        // removed — because the fallback invents a name from the dead id.
        if let Some(chosen) = self
            .growth_plot
            .chosen_axes
            .filter(|axes| axes_are_live(*axes, &signature.kernel_ids))
        {
            return chosen;
        }
        let pairs: Vec<(String, KernelId)> = signature
            .kernel_inputs
            .iter()
            .cloned()
            .zip(signature.kernel_ids.iter().copied())
            .collect();
        default_axes(&self.growth_referenced(), &pairs)
    }

    /// Assign a symbol to one axis, promoting a curve to a heatmap when the
    /// user asks for a second one.
    pub fn set_plot_axis(
        &mut self,
        axis: crate::gui::sections::growth::Axis,
        symbol: crate::document::selection::PlotSymbol,
    ) {
        use crate::document::selection::PlotAxes;
        use crate::gui::sections::growth::Axis;
        let current = self.plot_axes();
        let next = match (axis, current) {
            (Axis::X, PlotAxes::Curve(_)) => PlotAxes::Curve(symbol),
            (Axis::X, PlotAxes::Heatmap(_, y)) => PlotAxes::Heatmap(symbol, y),
            (Axis::Y, PlotAxes::Curve(x)) if x != symbol => PlotAxes::Heatmap(x, symbol),
            // Asking for the same symbol on both axes would plot a diagonal
            // and say nothing, so it collapses back to a curve.
            (Axis::Y, PlotAxes::Curve(x)) => PlotAxes::Curve(x),
            (Axis::Y, PlotAxes::Heatmap(x, _)) if x != symbol => PlotAxes::Heatmap(x, symbol),
            (Axis::Y, PlotAxes::Heatmap(x, _)) => PlotAxes::Curve(x),
        };
        self.growth_plot.chosen_axes = Some(next);
    }

    /// Compute the plot, or nothing when the program does not compile.
    pub fn growth_scene(&self) -> Option<PlotScene> {
        let binding = self.selected_binding();
        let signature = crate::document::growth::signature_of(self.spec(), binding);
        let source = crate::document::growth::source_of(self.spec(), binding)?;
        let program =
            crate::sim::growth::typecheck::compile(&source, &signature.externals()).ok()?;
        let pairs: Vec<(String, KernelId)> = signature
            .kernel_inputs
            .iter()
            .cloned()
            .zip(signature.kernel_ids.iter().copied())
            .collect();
        let mut pinned = self.growth_plot.pinned.clone();
        for (name, value) in &signature.parameters {
            pinned.parameters.entry(name.clone()).or_insert(*value);
        }
        Some(compute(&PlotInput {
            program: &program,
            signature_kernels: &pairs,
            axes: self.plot_axes(),
            pinned: &pinned,
            domain: self.growth_plot.domain,
        }))
    }

    /// What the plot's numbers are, taken from the binding rather than chosen
    /// separately.
    pub fn growth_quantity(&self) -> crate::gui::canvas::growth::PlotQuantity {
        crate::gui::canvas::growth::PlotQuantity::of(self.growth_mode())
    }

    /// Select a kernel and show it, so a symbol in the growth signature is a
    /// route to the thing it names.
    pub fn open_kernel(&mut self, kernel: KernelId) {
        self.select_kernel(kernel);
        self.navigation.select(Section::Kernels);
    }

    /// The growth program of the selected binding.
    pub fn growth_source(&self) -> String {
        let binding = self.selected_binding();
        crate::document::growth::source_of(self.spec(), binding).unwrap_or_default()
    }

    /// Take a keystroke from the source editor.
    ///
    /// Dispatched as a typed edit so a run of keystrokes is one undoable step.
    pub fn set_growth_source(&mut self, source: impl Into<String>) {
        self.dispatch_document(DocumentCommand::TypeGrowthSource(source.into()));
    }

    pub fn kernel_canvas(&self) -> &KernelCanvasState {
        &self.kernel_canvas
    }

    pub fn kernel_canvas_mut(&mut self) -> &mut KernelCanvasState {
        &mut self.kernel_canvas
    }

    pub fn kernel_popover(&self) -> &NumericPopover {
        &self.kernel_popover
    }

    pub fn kernel_popover_mut(&mut self) -> &mut NumericPopover {
        &mut self.kernel_popover
    }

    /// The binding the editors are working on: one basis, one output channel.
    pub fn selected_binding(&self) -> BindingKey {
        let selection = self.document.selection();
        BindingKey {
            basis: selection.basis,
            output: selection.channel,
        }
    }

    pub fn selected_kernel(&self) -> Option<KernelId> {
        self.document.selection().kernel
    }

    pub fn kernel_cards(&self) -> Vec<crate::document::kernels::KernelCardModel> {
        crate::document::kernels::binding_kernels(
            self.spec(),
            self.selected_binding(),
            self.selected_kernel(),
        )
    }

    pub fn select_kernel(&mut self, kernel: KernelId) {
        self.dispatch_document(DocumentCommand::SelectKernel(kernel));
        // A different kernel is a different stencil, so the view refits rather
        // than keeping a zoom that framed the previous one.
        self.kernel_canvas.selected_cell = None;
        self.kernel_canvas.request_fit();
    }

    pub fn add_kernel(&mut self) {
        let binding = self.selected_binding();
        match crate::document::kernels::add_kernel(self.spec(), binding) {
            Ok((spec, id)) => {
                self.dispatch_document(DocumentCommand::ReplaceExperiment(Box::new(spec)));
                if self.notice.is_none() {
                    // A kernel the user just added is the one they want to
                    // edit, so it is selected without a second click.
                    self.select_kernel(id);
                }
            }
            Err(error) => self.set_notice(Some(error)),
        }
    }

    /// Work out what deleting a kernel would do and, if it would rewrite the
    /// growth program, ask before doing it.
    pub fn begin_kernel_removal(&mut self, kernel: KernelId) {
        let binding = self.selected_binding();
        let symbol = self
            .kernel_cards()
            .into_iter()
            .find(|card| card.id == kernel)
            .map(|card| card.symbol)
            .unwrap_or_else(|| format!("k{}", kernel.0));
        match crate::document::kernels::plan_removal(self.spec(), binding, kernel) {
            Ok(plan) => match plan.rewrite {
                Some(rewrite) => {
                    self.kernel_decision = Some((
                        Decision {
                            title: format!("Delete kernel {symbol}"),
                            summary: format!(
                                "The growth program uses {symbol}. Deleting it replaces that reference with 0."
                            ),
                            diff: Some(crate::gui::widgets::decision_dialog::DecisionDiff {
                                caption: "Growth source".into(),
                                before: rewrite.before,
                                after: rewrite.after,
                            }),
                            confirm: "Replace references with 0 and remove".into(),
                            confirm_hint:
                                "Remove the kernel and rewrite the growth program as shown".into(),
                        },
                        Box::new(plan.spec),
                    ));
                }
                None => {
                    self.dispatch_document(DocumentCommand::ReplaceExperiment(Box::new(plan.spec)))
                }
            },
            Err(error) => self.set_notice(Some(error)),
        }
    }

    pub fn kernel_decision(&self) -> Option<&Decision> {
        self.kernel_decision.as_ref().map(|(decision, _)| decision)
    }

    pub fn confirm_kernel_decision(&mut self) {
        if let Some((_, spec)) = self.kernel_decision.take() {
            // The kernel removal and the source rewrite were computed together
            // and are committed together, so the draft is never half-edited.
            self.dispatch_document(DocumentCommand::ReplaceExperiment(spec));
        }
    }

    pub fn cancel_kernel_decision(&mut self) {
        self.kernel_decision = None;
    }

    /// The stencil of the selected kernel, flattened for the canvas.
    pub fn kernel_stencil(&self) -> KernelStencil {
        let binding = self.selected_binding();
        let Some(kernel) = self.selected_kernel() else {
            return KernelStencil::default();
        };
        crate::document::kernels::stencil_of(self.spec(), binding, kernel, binding.basis)
            .unwrap_or_default()
    }

    pub fn apply_kernel_edit(&mut self, edit: KernelEdit) {
        let binding = self.selected_binding();
        let Some(kernel) = self.selected_kernel() else {
            return;
        };
        let result = match edit {
            KernelEdit::Weight { x, y, value } => crate::document::kernels::set_weight(
                self.spec(),
                binding,
                kernel,
                binding.basis,
                x,
                y,
                value,
            ),
            KernelEdit::Active { x, y, active } => crate::document::kernels::set_active(
                self.spec(),
                binding,
                kernel,
                binding.basis,
                x,
                y,
                active,
            ),
        };
        match result {
            Ok(spec) => self.dispatch_document(DocumentCommand::ReplaceExperiment(Box::new(spec))),
            Err(error) => self.set_notice(Some(error)),
        }
    }

    pub fn set_kernel_source(&mut self, kernel: KernelId, source: ChannelId) {
        let binding = self.selected_binding();
        match crate::document::kernels::set_source(self.spec(), binding, kernel, source) {
            Ok(spec) => self.dispatch_document(DocumentCommand::ReplaceExperiment(Box::new(spec))),
            Err(error) => self.set_notice(Some(error)),
        }
    }

    pub fn reset_rule_set(&mut self) {
        let binding = self.selected_binding();
        self.dispatch_document(DocumentCommand::Draft(Box::new(
            crate::document::DraftCommand::ResetRuleSetToDefault { binding },
        )));
    }

    pub fn channel_canvas(&self) -> &ChannelCanvasState {
        &self.channel_canvas
    }

    pub fn channel_canvas_mut(&mut self) -> &mut ChannelCanvasState {
        &mut self.channel_canvas
    }

    pub fn selected_channel(&self) -> ChannelId {
        self.document.selection().channel
    }

    /// The cards the Channels strip is showing.
    pub fn channel_cards(&self) -> Vec<crate::document::channel_cards::ChannelCardModel> {
        crate::document::channel_cards::channel_cards(self.spec(), self.selected_channel())
    }

    pub fn channel_view(&self) -> ChannelView {
        self.channel_canvas.view
    }

    pub fn set_channel_view(&mut self, view: ChannelView) {
        self.channel_canvas.view = view;
        self.channel_canvas.invalidate();
    }

    pub fn channel_preview_source(&self) -> ChannelPreviewSource {
        self.channel_canvas.source
    }

    pub fn set_channel_preview_source(&mut self, source: ChannelPreviewSource) {
        self.channel_canvas.source = source;
        self.channel_canvas.invalidate();
    }

    /// What the preview would honestly draw right now.
    pub fn channel_preview(&self) -> ChannelPreview {
        resolve_preview(
            self.channel_canvas.source,
            self.document.active(),
            self.document.draft(),
            self.snapshot().as_deref(),
        )
    }

    pub fn channel_colour_draft(&self) -> [u8; 3] {
        self.channel_colour_draft
    }

    pub fn set_channel_colour_draft(&mut self, rgb: [u8; 3]) {
        self.channel_colour_draft = rgb;
    }

    /// Open the exact-colour fields on a channel's own colour.
    ///
    /// Only when that colour is not the one they were opened on: re-seeding
    /// every frame would discard each keystroke as it was made.
    pub fn seed_channel_colour_draft(&mut self, channel: ChannelId, rgb: [u8; 3]) {
        if self.channel_colour_seed != Some((channel, rgb)) {
            self.channel_colour_seed = Some((channel, rgb));
            self.channel_colour_draft = rgb;
        }
    }

    pub fn set_selected_channel_rgb(&mut self, red: u8, green: u8, blue: u8) {
        self.channel_colour_draft = [red, green, blue];
        self.channel_colour_seed = None;
        self.dispatch_document(DocumentCommand::SetSelectedChannelColor(
            crate::document::custom_color(red, green, blue),
        ));
        self.channel_canvas.invalidate();
    }

    pub fn set_selected_channel_automatic_colour(&mut self) {
        self.dispatch_document(DocumentCommand::SetSelectedChannelColor(
            crate::sim::experiment_model::DisplayColor::Auto,
        ));
        self.channel_canvas.invalidate();
    }

    pub fn tiling_canvas(&self) -> &TilingCanvasState {
        &self.tiling_canvas
    }

    pub fn tiling_canvas_mut(&mut self) -> &mut TilingCanvasState {
        &mut self.tiling_canvas
    }

    /// Vertices placed in the polygon currently being drawn.
    pub fn construction_vertices(&self) -> usize {
        self.tiling_canvas.construction().len()
    }

    /// Independent polygon sites in the draft unit cell.
    pub fn draft_basis_count(&self) -> usize {
        self.spec()
            .tiling
            .as_ref()
            .map(|tiling| tiling.instances.len())
            .unwrap_or(0)
    }

    /// Periodic copies drawn around the unit cell in the last frame.
    pub fn visible_neighbor_copies(&self) -> usize {
        self.tiling_canvas.neighbor_copies()
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn set_notice(&mut self, notice: Option<String>) {
        self.notice_at = notice.is_some().then_some(self.now);
        self.notice_level = NoticeLevel::Problem;
        self.notice = notice;
    }

    /// Report something that went right, or is merely worth knowing.
    pub fn set_info(&mut self, notice: impl Into<String>) {
        self.notice = Some(notice.into());
        self.notice_at = Some(self.now);
        self.notice_level = NoticeLevel::Info;
    }

    pub fn notice_level(&self) -> NoticeLevel {
        self.notice_level
    }

    /// Seconds a notice has left on screen, for a caller that wants to show it
    /// fading rather than vanishing.
    pub fn notice_age(&self) -> Option<f64> {
        self.notice_at.map(|set_at| self.now - set_at)
    }

    /// What the assistant currently says about the drawn tiling.
    ///
    /// Recomputed on demand rather than stored: the drawing changes under the
    /// pointer, and a hint that lags the drawing by a frame points at where an
    /// edge used to be.
    pub fn seam_assessment(&self) -> Option<SeamAssessment> {
        let draft = self.spec().tiling.as_ref()?;
        crate::sim::tiling::assess_seams(draft).ok()
    }

    /// Move the drawing so the closeable seams actually meet, then hold them.
    ///
    /// The flow this replaces proposed pairs, recorded them as constraints,
    /// and left the geometry exactly where it was — a control labelled "Solve
    /// seams" that solved nothing the user could see. The solver had always
    /// returned the corrected drawing; the interface discarded it.
    pub fn close_seams(&mut self) {
        let Some(draft) = self.spec().tiling.clone() else {
            self.set_notice(Some("there is no tiling to close yet".into()));
            return;
        };
        let Some(assessment) = self.seam_assessment() else {
            self.set_notice(Some("the tiling could not be assessed".into()));
            return;
        };
        let constraints = assessment
            .acceptable()
            .map(|candidate| candidate.constraint)
            .collect::<Vec<_>>();
        if constraints.is_empty() {
            // Never a bare refusal: say what is in the way.
            self.set_notice(Some(match assessment.orphans.first() {
                Some(orphan) => orphan.describe(),
                None => format!(
                    "nothing is close enough to close yet — {}",
                    assessment.summary()
                ),
            }));
            return;
        }
        match crate::sim::tiling::solve_edge_constraints(&draft, &constraints, None) {
            Ok(solved) => {
                let moved = solved.max_displacement;
                let held = constraints.len();
                self.tiling_canvas.seams = constraints;
                self.dispatch_document(DocumentCommand::SetTilingDraft(Box::new(solved.draft)));
                // `set_info`, not `set_notice`: the status bar paints a notice
                // in the problem colour, and reporting a success in red is a
                // small lie told every time the feature works.
                self.set_info(format!(
                    "closed {held} {}, moving the drawing by at most {moved:.4}",
                    if held == 1 { "seam" } else { "seams" }
                ));
            }
            Err(reason) => self.set_notice(Some(reason.0)),
        }
    }

    /// Stop holding the seams, so vertices move one at a time again.
    pub fn release_seams(&mut self) {
        self.tiling_canvas.seams.clear();
        self.tiling_canvas.broken.clear();
        self.set_notice(None);
    }

    /// Run one document command, reporting a rejection instead of applying it.
    pub fn dispatch_document(&mut self, command: DocumentCommand) {
        match self.document.execute(command) {
            Ok(_) => {
                self.set_notice(None);
                self.unsaved_changes = true;
            }
            Err(error) => self.set_notice(Some(error.to_string())),
        }
    }

    /// Close the construction polygon into the draft.
    pub fn finish_tiling_polygon(&mut self) {
        let vertices = self.tiling_canvas.construction().to_vec();
        let target = self.tiling_canvas.target;
        self.dispatch_document(DocumentCommand::FinishTilingPolygon { vertices, target });
        if self.notice.is_none() {
            self.tiling_canvas.cancel();
            self.tiling_canvas.selected_prototype = self.document.selection().prototype;
            self.tiling_canvas.selected_basis = Some(self.document.selection().basis);
            // A committed polygon changes what the view should frame.
            self.tiling_canvas.request_fit();
        }
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

    /// Record how fast frames are arriving.
    ///
    /// egui already measures the interval between frames and smooths out the
    /// occasional long one; taking its number keeps the status bar agreeing
    /// with the thing it is reporting on.
    fn observe_frame(&mut self, ctx: &eframe::egui::Context) {
        self.now = ctx.input(|input| input.time);
        // A message about one action must not still be on screen many actions
        // later. Expiry is measured from when it was set, so a notice the user
        // has had time to read makes way for the next one.
        if let Some(set_at) = self.notice_at
            && self.now - set_at > NOTICE_SECONDS
        {
            self.notice = None;
            self.notice_at = None;
        }
        let dt = ctx.input(|input| input.stable_dt);
        if dt > 0.0 && dt.is_finite() {
            let instant = 1.0 / dt;
            // A little smoothing, so the number is readable rather than
            // flickering through every value between two frames.
            self.frame_hz = if self.frame_hz > 0.0 {
                self.frame_hz * 0.9 + instant * 0.1
            } else {
                instant
            };
        }
    }

    pub fn status(&self) -> StatusLine {
        let snapshot = self.snapshot();
        let backend = snapshot
            .as_ref()
            .map(|snapshot| snapshot.backend.summary())
            .unwrap_or_else(|| "no backend".into());
        // What the user just did comes first. A refused Apply or a failed Save
        // has to be readable from the workspace the user was standing in, not
        // only from the one section that happens to render `notice` inline.
        let notice = self
            .notice
            .clone()
            .or_else(|| self.startup_notice.clone())
            .or_else(|| self.fallback_notice.clone())
            .or_else(|| {
                snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.error.as_ref())
                    .map(|error| error.message.clone())
            });
        let replay = self.recording.is_replaying().then(|| ReplayStatus {
            frame: self.recording.playhead() + 1,
            frames: self.recording.frames(),
            tick: self.recording.current_tick().unwrap_or(0),
        });
        StatusLine {
            backend,
            notice_level: self.notice_level,
            replay,
            tick: snapshot.as_ref().map(|snapshot| snapshot.tick).unwrap_or(0),
            simulation_hz: snapshot
                .as_ref()
                .map(|snapshot| step_rate(snapshot))
                .unwrap_or(0.0),
            frame_hz: self.frame_hz,
            draft_clean: self.document.status() == crate::document::DraftStatus::Clean,
            notice,
        }
    }
}

/// Steps per second implied by the newest step, or zero when nothing has run.
///
/// A paused simulation reports zero rather than the rate of whatever it last
/// did. After a single Step the old stats are still the newest ones, and
/// reporting them leaves the bar claiming a speed the simulation is not moving
/// at — for as long as the user leaves it paused.
fn step_rate(snapshot: &SimulationSnapshot) -> f32 {
    if !snapshot.running {
        return 0.0;
    }
    if snapshot.step_stats.elapsed_micros == 0 || snapshot.step_stats.steps == 0 {
        return 0.0;
    }
    snapshot.step_stats.steps as f32 * 1_000_000.0 / snapshot.step_stats.elapsed_micros as f32
}

/// Where the playhead is, for a bar that has to describe a recorded frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReplayStatus {
    pub frame: usize,
    pub frames: usize,
    pub tick: u64,
}

/// The bottom status bar contents. Values are placeholders until the simulation
/// worker publishes real snapshots in Task 4.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusLine {
    pub backend: String,
    pub notice_level: NoticeLevel,
    pub tick: u64,
    /// Set while a recorded frame is on screen instead of the live world.
    pub replay: Option<ReplayStatus>,
    pub simulation_hz: f32,
    pub frame_hz: f32,
    pub draft_clean: bool,
    pub notice: Option<String>,
}

/// Whether every symbol an axis names still exists.
///
/// `self` is always there; a kernel is only there while the signature lists it.
fn axes_are_live(
    axes: crate::document::selection::PlotAxes,
    kernels: &[crate::sim::experiment_model::KernelId],
) -> bool {
    use crate::document::selection::{PlotAxes, PlotSymbol};
    let live = |symbol: PlotSymbol| match symbol {
        PlotSymbol::SelfValue => true,
        PlotSymbol::Kernel(id) => kernels.contains(&id),
    };
    match axes {
        PlotAxes::Curve(x) => live(x),
        PlotAxes::Heatmap(x, y) => live(x) && live(y),
    }
}

impl eframe::App for CellariumGui {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        // The status bar reports what the window is doing, so it has to measure
        // it. A hardcoded zero beside a visibly animating canvas is worse than
        // no number at all.
        self.observe_frame(ui.ctx());
        // While the simulation runs, ask for a repaint at the display cadence
        // instead of polling the worker or spinning at full speed. The local
        // intent counts as running too: the frame that presses Run still sees
        // the old paused snapshot, and without this the display would freeze
        // until some unrelated input arrived.
        let dt = ui.ctx().input(|input| input.stable_dt).clamp(0.0, 0.25) as f64;
        self.drive_recording(dt);
        if self.running || self.running() || self.recording.state() == ReplayState::Playing {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
        }
        self.consume_shortcuts(ui.ctx());
        layout::draw(self, ui);
        // Modals last, so they sit above the workspace they interrupt.
        self.drive_file_dialog(ui.ctx());
        layout::modals(self, ui.ctx());
        self.autosave_if_due();
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
        let _one = crate::test_backend_guard::one_backend_at_a_time();
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
        let _one = crate::test_backend_guard::one_backend_at_a_time();
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
        let _one = crate::test_backend_guard::one_backend_at_a_time();
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
