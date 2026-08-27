use std::sync::Arc;

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
use crate::gui::widgets::numeric_popover::NumericPopover;
use crate::sim::backend_selector::{BackendPolicy, BackendSelector};
use crate::sim::compute_plan::compile_compute_plan;
use crate::sim::experiment_model::{ChannelId, ExperimentSpec, KernelId};
use crate::sim::local_backend::{BackendProbe, initial_cells};
use crate::sim::ruleset::BindingKey;
use crate::sim::tiling::SeamProposal;
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
    tiling_canvas: TilingCanvasState,
    channel_canvas: ChannelCanvasState,
    /// Working RGB for the colour popover, so dragging the fields does not
    /// write a new draft on every pixel of movement.
    channel_colour_draft: [u8; 3],
    kernel_canvas: KernelCanvasState,
    kernel_popover: NumericPopover,
    /// A destructive kernel edit waiting for the user's answer, with the
    /// draft it would produce already computed.
    kernel_decision: Option<(Decision, Box<ExperimentSpec>)>,
    growth_plot: GrowthPlotState,
    seam_proposals: Option<Vec<SeamProposal>>,
    notice: Option<String>,
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
            tiling_canvas: TilingCanvasState::default(),
            channel_canvas: ChannelCanvasState::default(),
            channel_colour_draft: [236, 240, 246],
            kernel_canvas: KernelCanvasState::new(),
            kernel_popover: NumericPopover::default(),
            kernel_decision: None,
            growth_plot: GrowthPlotState::default(),
            seam_proposals: None,
            notice: None,
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
            ShellAction::Undo => {
                // Undo and Redo are document transactions, not simulation
                // commands: they rewind the draft the user is editing.
                match self.document.undo() {
                    Ok(_) => self.notice = None,
                    Err(error) => self.notice = Some(error.to_string()),
                }
                self.channel_canvas.invalidate();
                None
            }
            ShellAction::Redo => {
                match self.document.redo() {
                    Ok(_) => self.notice = None,
                    Err(error) => self.notice = Some(error.to_string()),
                }
                self.channel_canvas.invalidate();
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
            Err(error) => self.notice = Some(error),
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
        if let Some(chosen) = self.growth_plot.chosen_axes {
            return chosen;
        }
        let signature = self.growth_signature();
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

    pub fn set_growth_source(&mut self, source: impl Into<String>) {
        let binding = self.selected_binding();
        match crate::document::growth::set_source(self.spec(), binding, &source.into()) {
            Ok(spec) => self.dispatch_document(DocumentCommand::ReplaceExperiment(Box::new(spec))),
            Err(error) => self.notice = Some(error),
        }
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
            Err(error) => self.notice = Some(error),
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
            Err(error) => self.notice = Some(error),
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
            Err(error) => self.notice = Some(error),
        }
    }

    pub fn set_kernel_source(&mut self, kernel: KernelId, source: ChannelId) {
        let binding = self.selected_binding();
        match crate::document::kernels::set_source(self.spec(), binding, kernel, source) {
            Ok(spec) => self.dispatch_document(DocumentCommand::ReplaceExperiment(Box::new(spec))),
            Err(error) => self.notice = Some(error),
        }
    }

    pub fn reset_rule_set(&mut self) {
        let binding = self.selected_binding();
        self.dispatch_document(DocumentCommand::Draft(Box::new(
            crate::workbench::DraftCommand::ResetRuleSetToDefault { binding },
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
    pub fn channel_cards(&self) -> Vec<crate::workbench::channel_editor::ChannelCardModel> {
        crate::workbench::channel_editor::channel_cards(self.spec(), self.selected_channel())
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

    pub fn set_selected_channel_rgb(&mut self, red: u8, green: u8, blue: u8) {
        self.channel_colour_draft = [red, green, blue];
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
        self.notice = notice;
    }

    pub fn seam_proposals(&self) -> Option<&[SeamProposal]> {
        self.seam_proposals.as_deref()
    }

    pub fn set_seam_proposals(&mut self, proposals: Vec<SeamProposal>) {
        self.notice = (proposals.is_empty())
            .then(|| "no full-edge pairs are close enough to glue".to_string());
        self.seam_proposals = Some(proposals);
    }

    pub fn clear_seam_proposals(&mut self) {
        self.seam_proposals = None;
        self.notice = None;
    }

    /// Hold the proposed seams. Subsequent vertex drags move whole equivalence
    /// classes rather than tearing the tiling apart.
    pub fn accept_seam_proposals(&mut self) {
        if let Some(proposals) = self.seam_proposals.take() {
            self.tiling_canvas.seams = proposals
                .into_iter()
                .map(|proposal| proposal.constraint)
                .collect();
        }
        self.notice = None;
    }

    /// Run one document command, reporting a rejection instead of applying it.
    pub fn dispatch_document(&mut self, command: DocumentCommand) {
        match self.document.execute(command) {
            Ok(_) => self.notice = None,
            Err(error) => self.notice = Some(error.to_string()),
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
