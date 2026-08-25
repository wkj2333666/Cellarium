use crate::sim::kernel::{
    Kernel, KernelDefinition, KernelValues, Normalization, render_definition, ring_definition,
};
use std::collections::{BTreeMap, VecDeque};
use std::io::{ErrorKind, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::input::{Command, UiCommand};
use crate::render::camera::Camera;
use crate::render::display::DisplayProtocol;
use crate::render::raster::{Framebuffer, rasterize_world_into};
use crate::render::scene_transform::{SceneCamera, SceneTransform};
use crate::render::workbench_graphics::{GraphicsSurface, PlacementAction, SceneKey};
use crate::sim::backend::{BackendKind, SimulationBackend};
use crate::sim::experiment::{ExperimentError, ExperimentFile, ExperimentMetadata};
use crate::sim::experiment_model::{ExperimentSpec, validate_structure};
use crate::sim::rule::SimulationSpec;
use crate::sim::service::{
    ApplyAccepted, ApplyRejected, ApplyRequest, Diagnostic, DiagnosticPath, ExperimentService,
};
use crate::sim::tiling::PeriodicTilingDraft;
use crate::sim::world::World;
use crate::workbench::{AppMode, WorkbenchFocus, WorkbenchSection, WorkbenchState};
use crossterm::event::{Event, KeyCode, KeyEvent, MouseEvent};
use ratatui::layout::Rect;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Overview,
    Rule,
    Kernel,
    Topology,
    Errors,
}

impl Panel {
    fn next(self) -> Self {
        match self {
            Self::Overview => Self::Rule,
            Self::Rule => Self::Kernel,
            Self::Kernel => Self::Topology,
            Self::Topology => Self::Errors,
            Self::Errors => Self::Overview,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PerformanceStats {
    pub last_step_ms: f64,
    pub average_step_ms: f64,
    pub step_samples: u64,
    pub last_render_ms: f64,
    pub average_render_ms: f64,
    pub render_samples: u64,
}

pub struct App {
    spec: SimulationSpec,
    backend: SimulationBackend,
    world: World,
    camera: Camera,
    paused: bool,
    seed: u64,
    inspected: Option<f32>,
    simulation_rate: f64,
    render_rate: f64,
    viewport: Option<Rect>,
    frame_size: Option<[usize; 2]>,
    backend_error: Option<String>,
    kernel_error: Option<String>,
    kernel_definitions: Vec<KernelDefinition>,
    selected_kernel: usize,
    selected_parameter: Option<String>,
    kernel_preview_enabled: bool,
    active_panel: Panel,
    expression_editing: bool,
    expression_buffer: String,
    help_visible: bool,
    framebuffer: Option<Framebuffer>,
    performance: PerformanceStats,
    remote_tick: Option<u64>,
    remote_backend: Option<String>,
    remote_rule: Option<String>,
    applied_input_sequence: u64,
    snapshot_rate: f64,
    graphics_rate: f64,
    experiment_model: ExperimentSpec,
    experiment_revision: u64,
    workbench_base_revision: u64,
    mode: AppMode,
    workbench: WorkbenchState,
    experiment_service: Option<ExperimentService>,
    workbench_notice: Option<String>,
    workbench_display_needs_clear: bool,
    workbench_area: Rect,
    workbench_graphics_surface: GraphicsSurface,
    workbench_scene_transform: Option<SceneTransform>,
    workbench_transform_generation: u64,
    workbench_placement_generation: u64,
    workbench_draft_scene_generation: u64,
    workbench_frame_generation: u64,
    workspace_persistence: Option<WorkspacePersistence>,
}

struct WorkspacePersistence {
    paths: crate::workbench::WorkspacePaths,
    last_saved_draft: Option<ExperimentSpec>,
    last_save_attempt: Instant,
    restored_pending_remote_rebase: bool,
}

fn graphics_pointer_hit_radius(frame_size: [usize; 2], viewport: Rect) -> i32 {
    let cell_width = frame_size[0] as f64 / f64::from(viewport.width.max(1));
    let cell_height = frame_size[1] as f64 / f64::from(viewport.height.max(1));
    (((cell_width * 0.5).hypot(cell_height * 0.5)).ceil() as i32 + 3).clamp(12, 32)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GraphicsPointerCell {
    center: [u32; 2],
    bounds: [u32; 4],
}

fn graphics_pointer_cell(
    frame_size: [usize; 2],
    viewport: Rect,
    column: u16,
    row: u16,
) -> GraphicsPointerCell {
    let frame_width = frame_size[0].max(1) as u64;
    let frame_height = frame_size[1].max(1) as u64;
    let columns = u64::from(viewport.width.max(1));
    let rows = u64::from(viewport.height.max(1));
    let column = u64::from(column.min(viewport.width.saturating_sub(1)));
    let row = u64::from(row.min(viewport.height.saturating_sub(1)));
    let left = column * frame_width / columns;
    let top = row * frame_height / rows;
    let right = ((column + 1) * frame_width / columns)
        .max(left + 1)
        .min(frame_width);
    let bottom = ((row + 1) * frame_height / rows)
        .max(top + 1)
        .min(frame_height);
    GraphicsPointerCell {
        center: [
            ((left + right - 1) / 2) as u32,
            ((top + bottom - 1) / 2) as u32,
        ],
        bounds: [left as u32, top as u32, right as u32, bottom as u32],
    }
}

impl App {
    pub fn new(spec: SimulationSpec, width: usize, height: usize) -> Self {
        Self::with_backend(spec.clone(), width, height, SimulationBackend::cpu(spec))
    }

    pub fn with_backend(
        spec: SimulationSpec,
        width: usize,
        height: usize,
        backend: SimulationBackend,
    ) -> Self {
        let mut world = World::new(width, height);
        let seed = 1_u64;
        world.randomize(seed, initial_density(&spec));
        let center = [width as f32 / 2.0, height as f32 / 2.0];
        let (kernel_definitions, selected_kernel) = kernel_catalog(&spec);
        let mut experiment_model =
            ExperimentSpec::single_channel_lenia(width as u32, height as u32);
        experiment_model.name = rule_name(&spec).to_string();
        experiment_model.channels[0].initial = world.cells().to_vec();
        let workbench = WorkbenchState::new(experiment_model.clone());
        Self {
            spec: spec.clone(),
            backend,
            world,
            camera: Camera::new(center, 1.0),
            paused: false,
            seed,
            inspected: None,
            simulation_rate: 0.0,
            render_rate: 0.0,
            viewport: None,
            frame_size: None,
            backend_error: None,
            kernel_error: None,
            kernel_definitions,
            selected_kernel,
            selected_parameter: None,
            kernel_preview_enabled: false,
            active_panel: Panel::Overview,
            expression_editing: false,
            help_visible: false,
            expression_buffer: String::new(),
            framebuffer: None,
            performance: PerformanceStats::default(),
            remote_tick: None,
            remote_backend: None,
            remote_rule: None,
            applied_input_sequence: 0,
            snapshot_rate: 0.0,
            graphics_rate: 0.0,
            experiment_model,
            experiment_revision: 0,
            workbench_base_revision: 0,
            mode: AppMode::Simulation,
            workbench,
            experiment_service: None,
            workbench_notice: None,
            workbench_display_needs_clear: false,
            workbench_area: Rect::default(),
            workbench_graphics_surface: GraphicsSurface::new(),
            workbench_scene_transform: None,
            workbench_transform_generation: 0,
            workbench_placement_generation: 0,
            workbench_draft_scene_generation: 0,
            workbench_frame_generation: 0,
            workspace_persistence: None,
        }
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn mode(&self) -> AppMode {
        self.mode
    }
    pub fn workbench(&self) -> &WorkbenchState {
        &self.workbench
    }
    pub fn workbench_mut(&mut self) -> &mut WorkbenchState {
        &mut self.workbench
    }
    pub fn workbench_notice(&self) -> Option<&str> {
        self.workbench_notice.as_deref()
    }
    fn enable_default_workspace(&mut self) -> Result<(), String> {
        self.enable_workspace(crate::workbench::default_workspace_paths()?)
    }
    fn enable_workspace(&mut self, paths: crate::workbench::WorkspacePaths) -> Result<(), String> {
        let restored = if paths.workbench.exists() {
            Some(crate::workbench::load_workspace(&paths.workbench)?)
        } else {
            None
        };
        if let Some(workspace) = &restored {
            self.workbench
                .import_draft(workspace.draft.clone())
                .map_err(|error| error.to_string())?;
            // A restored design is intentionally rebased onto the runtime we
            // just connected to.  A stale revision from a previous server
            // process must not make a portable local design impossible to run.
            self.workbench_base_revision = self.experiment_revision;
            self.workbench_notice = Some(format!(
                "restored workspace from {}",
                paths.workbench.display()
            ));
        }
        let restored_pending_remote_rebase = restored.is_some();
        self.workspace_persistence = Some(WorkspacePersistence {
            paths,
            last_saved_draft: restored.map(|workspace| workspace.draft),
            last_save_attempt: Instant::now(),
            restored_pending_remote_rebase,
        });
        Ok(())
    }
    fn save_default_workspace_now(
        &mut self,
        save_runnable_experiment: bool,
    ) -> Result<crate::workbench::WorkspacePaths, String> {
        let paths = self
            .workspace_persistence
            .as_ref()
            .map(|persistence| persistence.paths.clone())
            .ok_or_else(|| "default workspace persistence is not enabled".to_string())?;
        let draft = self.workbench.draft().clone();
        let workspace = crate::workbench::WorkspaceEnvelope {
            format_version: crate::workbench::WORKSPACE_FORMAT_VERSION,
            active_revision: self.experiment_revision,
            base_revision: self.workbench_base_revision,
            active: self.workbench.authoritative().clone(),
            draft: draft.clone(),
        };
        crate::workbench::save_workspace(&paths.workbench, &workspace)?;
        if save_runnable_experiment {
            if let Some(parent) = paths.experiment.parent() {
                std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            crate::sim::experiment::save_experiment_model(
                &paths.experiment,
                self.workbench.authoritative(),
            )
            .map_err(|error| error.to_string())?;
        }
        if let Some(persistence) = self.workspace_persistence.as_mut() {
            persistence.last_saved_draft = Some(draft);
            persistence.last_save_attempt = Instant::now();
        }
        Ok(paths)
    }
    fn autosave_workspace_if_due(&mut self, now: Instant) {
        let should_save = self
            .workspace_persistence
            .as_ref()
            .is_some_and(|persistence| {
                now.saturating_duration_since(persistence.last_save_attempt)
                    >= Duration::from_millis(500)
                    && persistence.last_saved_draft.as_ref() != Some(self.workbench.draft())
            });
        if !should_save {
            return;
        }
        if let Some(persistence) = self.workspace_persistence.as_mut() {
            persistence.last_save_attempt = now;
        }
        if let Err(error) = self.save_default_workspace_now(false) {
            self.workbench_notice = Some(format!("workspace autosave failed: {error}"));
        }
    }
    pub fn enter_workbench(&mut self) {
        if self.mode != AppMode::Workbench {
            self.mode = AppMode::Workbench;
            self.workbench_display_needs_clear = true;
        }
    }
    pub fn leave_workbench(&mut self) {
        self.mode = AppMode::Simulation;
        self.workbench_display_needs_clear = true;
    }
    pub fn take_workbench_display_clear(&mut self) -> bool {
        let needs_clear = self.workbench_display_needs_clear;
        self.workbench_display_needs_clear = false;
        needs_clear
    }
    pub fn request_workbench_display_clear(&mut self) {
        self.workbench_display_needs_clear = true;
        self.workbench_draft_scene_generation =
            self.workbench_draft_scene_generation.wrapping_add(1);
    }
    pub fn set_workbench_area(&mut self, area: Rect) {
        self.workbench_area = area;
    }
    pub fn prepare_workbench_scene(
        &mut self,
        terminal_rect: Rect,
        pixel_size: [u32; 2],
        display_mode: DisplayProtocol,
    ) -> (PlacementAction, u64) {
        let center = self.camera.center();
        let camera = SceneCamera::new(
            [f64::from(center[0]), f64::from(center[1])],
            f64::from(self.camera.zoom()),
        );
        let placement_changed = self.workbench_scene_transform.is_none_or(|transform| {
            transform.terminal_rect != terminal_rect || transform.pixel_size != pixel_size
        });
        let changed = placement_changed
            || self
                .workbench_scene_transform
                .is_none_or(|transform| transform.camera != camera);
        if changed {
            self.workbench_transform_generation =
                self.workbench_transform_generation.wrapping_add(1);
            self.workbench_scene_transform = SceneTransform::new(
                terminal_rect,
                pixel_size,
                camera,
                self.workbench_transform_generation,
            )
            .ok();
        }
        if placement_changed {
            self.workbench_placement_generation =
                self.workbench_placement_generation.wrapping_add(1);
        }
        let scene = SceneKey {
            section: self.workbench.section(),
            selected_basis: self.workbench.selected_basis(),
            selected_channel: self.workbench.selected_channel(),
            selected_kernel: self.workbench.selected_kernel(),
            display_mode,
            placement_generation: self.workbench_placement_generation,
            transform_generation: self.workbench_transform_generation,
            draft_scene_generation: self.workbench_draft_scene_generation,
        };
        let action = self.workbench_graphics_surface.transition(scene);
        if action != PlacementAction::Keep {
            self.workbench_frame_generation = self.workbench_frame_generation.wrapping_add(1);
        }
        (action, self.workbench_frame_generation)
    }

    pub fn workbench_scene_transform(&self) -> Option<SceneTransform> {
        self.workbench_scene_transform
    }
    /// Handle clicks on the Workbench navigation and inspector panels.
    /// Canvas clicks remain available to the normal paint/inspect path.
    pub fn handle_workbench_panel_mouse(&mut self, mouse: MouseEvent) -> bool {
        let left_down = matches!(
            mouse.kind,
            crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left)
        );
        let left_drag = matches!(
            mouse.kind,
            crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left)
        );
        if self.mode != AppMode::Workbench {
            return false;
        }
        let layout = crate::tui::workbench::workbench_layout(self.workbench_area);
        let point = ratatui::layout::Position::new(mouse.column, mouse.row);
        if self.workbench.section() == WorkbenchSection::Growth
            && layout.inspector.is_some_and(|area| area.contains(point))
        {
            let lines = match mouse.kind {
                crossterm::event::MouseEventKind::ScrollUp => -3,
                crossterm::event::MouseEventKind::ScrollDown => 3,
                _ => 0,
            };
            if lines != 0 {
                self.workbench.set_focus(WorkbenchFocus::Inspector);
                self.workbench.scroll_growth_help(lines);
                return true;
            }
        }
        if !left_down && !left_drag {
            return false;
        }
        if left_down && layout.outline.contains(point) {
            self.workbench.set_focus(WorkbenchFocus::Outline);
            let inner = Rect::new(
                layout.outline.x.saturating_add(1),
                layout.outline.y.saturating_add(1),
                layout.outline.width.saturating_sub(2),
                layout.outline.height.saturating_sub(2),
            );
            if inner.contains(point) {
                let index = usize::from(point.y.saturating_sub(inner.y));
                if let Some(section) = WorkbenchSection::ALL.get(index).copied() {
                    let changed = self.workbench.section() != section;
                    self.workbench.select_section(section);
                    if changed {
                        self.request_workbench_display_clear();
                    }
                    self.workbench_notice = Some(format!("selected {}", section.label()));
                }
            }
            return true;
        }
        if left_down && layout.inspector.is_some_and(|area| area.contains(point)) {
            self.workbench.set_focus(WorkbenchFocus::Inspector);
            return true;
        }
        if layout.canvas.contains(point) {
            self.workbench.set_focus(WorkbenchFocus::Canvas);
            let canvas_content = Rect::new(
                layout.canvas.x.saturating_add(1),
                layout.canvas.y.saturating_add(1),
                layout.canvas.width.saturating_sub(2),
                layout.canvas.height.saturating_sub(2),
            );
            let canvas_header = Rect::new(
                canvas_content.x,
                canvas_content.y,
                canvas_content.width,
                canvas_content.height.min(2),
            );
            if left_down && canvas_header.contains(point) {
                let column = point.x.saturating_sub(canvas_header.x);
                if let Some(action) =
                    crate::tui::workbench::toolbar_action_at(&self.workbench, column)
                {
                    match action {
                        crate::tui::workbench::ToolbarAction::Ui(command) => {
                            if let Err(error) = self.handle_workbench_ui(command) {
                                self.workbench_notice = Some(error);
                            }
                        }
                        crate::tui::workbench::ToolbarAction::EditorKey(code) => {
                            if !self.handle_workbench_editor_key(KeyEvent::new(
                                code,
                                crossterm::event::KeyModifiers::NONE,
                            )) {
                                self.workbench_notice =
                                    Some("select an editable item on the canvas first".into());
                            }
                            self.workbench_draft_scene_generation =
                                self.workbench_draft_scene_generation.wrapping_add(1);
                        }
                        crate::tui::workbench::ToolbarAction::ToggleGrowthEditor => {
                            if self.workbench.growth_editing() {
                                self.workbench.stop_growth_editing();
                                self.workbench_notice =
                                    Some("Growth source editing finished".into());
                            } else {
                                self.workbench.toggle_growth_editing();
                                self.workbench_notice =
                                    Some("Growth source editing · click/type · Esc finish".into());
                            }
                            self.workbench_draft_scene_generation =
                                self.workbench_draft_scene_generation.wrapping_add(1);
                        }
                    }
                    return true;
                }
            }
            if self.workbench.section() == WorkbenchSection::Growth {
                let body = Rect::new(
                    canvas_content.x,
                    canvas_content
                        .y
                        .saturating_add(canvas_content.height.min(2)),
                    canvas_content.width,
                    canvas_content
                        .height
                        .saturating_sub(canvas_content.height.min(2)),
                );
                let source_height = body.height.saturating_mul(48) / 100;
                let source = Rect::new(body.x, body.y, body.width, source_height);
                if source.contains(point) {
                    let line = usize::from(point.y.saturating_sub(source.y)).saturating_sub(2);
                    let column = usize::from(point.x.saturating_sub(source.x)).saturating_sub(2);
                    if left_drag {
                        self.workbench
                            .growth_editor_mut()
                            .buffer_mut()
                            .set_cursor_line_column_extending(line, column);
                    } else {
                        self.workbench
                            .growth_editor_mut()
                            .buffer_mut()
                            .set_cursor_line_column(line, column);
                    }
                    if !self.workbench.growth_editing() {
                        self.workbench.toggle_growth_editing();
                    }
                    self.workbench_notice =
                        Some("Growth cursor placed · type to edit · Esc finish".into());
                    self.workbench_draft_scene_generation =
                        self.workbench_draft_scene_generation.wrapping_add(1);
                    return true;
                }
            }
        }
        false
    }
    pub fn workbench_apply_request(&self, request_id: u64) -> ApplyRequest {
        ApplyRequest {
            request_id,
            base_revision: self.workbench_base_revision,
            draft: self.workbench.draft().clone(),
        }
    }
    pub fn handle_workbench_ui(&mut self, command: UiCommand) -> Result<(), String> {
        let result = match command {
            UiCommand::Undo => self.workbench.undo().map_err(|error| error.to_string()),
            UiCommand::Redo => self.workbench.redo().map_err(|error| error.to_string()),
            UiCommand::RevertDraft => {
                self.workbench.revert();
                Ok(())
            }
            UiCommand::FocusNext => {
                self.workbench.focus_next();
                Ok(())
            }
            UiCommand::FocusPrevious => {
                self.workbench.focus_previous();
                Ok(())
            }
            UiCommand::ContextAdd => match self.workbench.section() {
                crate::workbench::WorkbenchSection::Tiling => {
                    self.workbench.begin_new_basis_polygon();
                    self.workbench_notice = Some(
                        "New basis polygon: click vertices · click first/Enter close · Esc cancel"
                            .into(),
                    );
                    Ok(())
                }
                crate::workbench::WorkbenchSection::Channels => self
                    .workbench
                    .add_channel()
                    .map_err(|error| error.to_string()),
                crate::workbench::WorkbenchSection::Kernels => self
                    .workbench
                    .add_kernel_for_selected()
                    .map_err(|error| error.to_string()),
                _ => Err("Add is available in Tiling, Channels, and Kernels".into()),
            },
            UiCommand::ContextDelete => match self.workbench.section() {
                crate::workbench::WorkbenchSection::Channels => {
                    self.workbench.remove_selected_channel()
                }
                crate::workbench::WorkbenchSection::Kernels => {
                    self.workbench.remove_last_kernel_for_selected()
                }
                _ => Err("Delete is available in Channels and Kernels".into()),
            },
            UiCommand::SelectNext => {
                match self.workbench.section() {
                    crate::workbench::WorkbenchSection::Kernels => {
                        self.workbench.select_next_kernel()
                    }
                    crate::workbench::WorkbenchSection::Tiling => {
                        self.workbench.select_next_prototype()
                    }
                    _ => self.workbench.select_next_channel(),
                }
                Ok(())
            }
            UiCommand::CyclePresentation => {
                self.workbench.cycle_channel_view();
                Ok(())
            }
            UiCommand::CycleColor => self
                .workbench
                .cycle_selected_color()
                .map_err(|error| error.to_string()),
            UiCommand::ToggleVisibility => self
                .workbench
                .toggle_selected_visibility()
                .map_err(|error| error.to_string()),
            UiCommand::ToggleFrozen => self
                .workbench
                .toggle_selected_frozen()
                .map_err(|error| error.to_string()),
            UiCommand::CyclePreset => {
                self.workbench
                    .cycle_tiling_preset()
                    .map_err(|error| error.to_string())?;
                self.workbench_notice =
                    Some("tiling preset loaded · drag vertices · [0] fit view".into());
                Ok(())
            }
            UiCommand::ShapeNext => {
                self.workbench.select_next_prototype();
                Ok(())
            }
            UiCommand::ShapeIncrease => self.workbench.adjust_prototype_sides(1),
            UiCommand::ShapeDecrease => self.workbench.adjust_prototype_sides(-1),
            UiCommand::SaveActive => {
                if self.workspace_persistence.is_some() {
                    let paths = self.save_default_workspace_now(true)?;
                    self.workbench_notice = Some(format!(
                        "saved workspace to {} · runnable experiment to {}",
                        paths.workbench.display(),
                        paths.experiment.display(),
                    ));
                } else {
                    let path = Path::new("cellarium-active.ron");
                    crate::sim::experiment::save_experiment_model(path, &self.active_experiment())
                        .map_err(|error| error.to_string())?;
                    self.workbench_notice = Some(format!("saved active to {}", path.display()));
                }
                Ok(())
            }
            UiCommand::ExportDraft => {
                let path = Path::new("cellarium-draft.ron");
                crate::workbench::export_draft(
                    path,
                    self.workbench_base_revision,
                    self.workbench.draft(),
                )?;
                self.workbench_notice = Some(format!("exported draft to {}", path.display()));
                Ok(())
            }
            UiCommand::LoadDraft => {
                let path = Path::new("cellarium-draft.ron");
                let envelope = crate::workbench::load_draft(path)?;
                self.workbench_base_revision = envelope.base_revision;
                self.workbench
                    .import_draft(envelope.draft)
                    .map_err(|error| error.to_string())?;
                self.workbench_notice = Some(format!(
                    "loaded draft from {} (base revision {})",
                    path.display(),
                    envelope.base_revision
                ));
                Ok(())
            }
            UiCommand::ApplyDraft => {
                let request =
                    self.workbench_apply_request(self.experiment_revision.wrapping_add(1));
                match self.submit_draft(request) {
                    Ok(accepted) => {
                        self.leave_workbench();
                        self.workbench_notice = Some(match self.save_default_workspace_now(true) {
                            Ok(paths) => format!(
                                "running revision {} · saved {}",
                                accepted.revision,
                                paths.experiment.display(),
                            ),
                            Err(error) if self.workspace_persistence.is_some() => format!(
                                "running revision {} · save failed: {error}",
                                accepted.revision,
                            ),
                            Err(_) => format!("running revision {}", accepted.revision),
                        });
                        Ok(())
                    }
                    Err(rejected) => Err(rejected
                        .diagnostics
                        .into_iter()
                        .map(|d| d.message)
                        .collect::<Vec<_>>()
                        .join("; ")),
                }
            }
        };
        if result.is_ok() {
            self.workbench_draft_scene_generation =
                self.workbench_draft_scene_generation.wrapping_add(1);
        }
        result
    }
    pub fn handle_workbench_growth_key(&mut self, key: KeyEvent) -> bool {
        if self.mode != AppMode::Workbench || !self.workbench.growth_editing() {
            return false;
        }
        let shift = key
            .modifiers
            .contains(crossterm::event::KeyModifiers::SHIFT);
        let control = key
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL);
        let changed_source = match key.code {
            KeyCode::Char('a') if control => {
                self.workbench.growth_editor_mut().buffer_mut().select_all();
                false
            }
            KeyCode::Char('u') if control => self
                .workbench
                .growth_editor_mut()
                .buffer_mut()
                .delete_to_line_start(),
            KeyCode::Char(character) if !control => {
                self.workbench
                    .growth_editor_mut()
                    .buffer_mut()
                    .insert_char(character);
                true
            }
            KeyCode::Enter => {
                self.workbench.growth_editor_mut().buffer_mut().newline();
                true
            }
            KeyCode::Backspace => {
                self.workbench.growth_editor_mut().buffer_mut().backspace();
                true
            }
            KeyCode::Delete => {
                self.workbench.growth_editor_mut().buffer_mut().delete();
                true
            }
            KeyCode::Left if control => {
                self.workbench
                    .growth_editor_mut()
                    .buffer_mut()
                    .move_word_left(shift);
                false
            }
            KeyCode::Right if control => {
                self.workbench
                    .growth_editor_mut()
                    .buffer_mut()
                    .move_word_right(shift);
                false
            }
            KeyCode::Left if shift => {
                self.workbench
                    .growth_editor_mut()
                    .buffer_mut()
                    .move_left_extending();
                false
            }
            KeyCode::Right if shift => {
                self.workbench
                    .growth_editor_mut()
                    .buffer_mut()
                    .move_right_extending();
                false
            }
            KeyCode::Left => {
                self.workbench.growth_editor_mut().buffer_mut().move_left();
                false
            }
            KeyCode::Right => {
                self.workbench.growth_editor_mut().buffer_mut().move_right();
                false
            }
            KeyCode::Up => {
                self.workbench
                    .growth_editor_mut()
                    .buffer_mut()
                    .move_vertical(-1);
                false
            }
            KeyCode::Down => {
                self.workbench
                    .growth_editor_mut()
                    .buffer_mut()
                    .move_vertical(1);
                false
            }
            KeyCode::Home => {
                self.workbench.growth_editor_mut().buffer_mut().move_home();
                false
            }
            KeyCode::End => {
                self.workbench.growth_editor_mut().buffer_mut().move_end();
                false
            }
            KeyCode::Esc => {
                self.workbench.stop_growth_editing();
                self.workbench_draft_scene_generation =
                    self.workbench_draft_scene_generation.wrapping_add(1);
                return true;
            }
            _ => return false,
        };
        if changed_source {
            self.workbench.growth_editor_mut().refresh_now();
            self.workbench.sync_growth_source();
        }
        self.workbench_draft_scene_generation =
            self.workbench_draft_scene_generation.wrapping_add(1);
        if changed_source && std::env::var_os("CELLARIUM_E2E_TRACE").is_some() {
            eprintln!(
                "E2E_GROWTH_VALID valid={} source={:?}",
                self.workbench.growth_editor().diagnostics().is_empty(),
                self.workbench.growth_editor().buffer().as_str()
            );
        }
        true
    }

    pub fn handle_workbench_editor_key(&mut self, key: KeyEvent) -> bool {
        if self.mode != AppMode::Workbench {
            return false;
        }
        if self.workbench.kernel_resize_editor().is_some() {
            match key.code {
                KeyCode::Esc => {
                    self.workbench.cancel_kernel_resize_editor();
                    self.workbench_notice = Some("kernel resize cancelled".into());
                }
                KeyCode::Enter => {
                    let source = self
                        .workbench
                        .kernel_resize_editor()
                        .unwrap_or_default()
                        .to_string();
                    let parsed = source
                        .split(',')
                        .map(|part| part.trim().parse::<usize>())
                        .collect::<Result<Vec<_>, _>>();
                    let result = match parsed {
                        Ok(parts) if parts.len() == 4 => {
                            let kernel = self.workbench.selected_rule_kernel().ok_or_else(|| {
                                "selected kernel disappeared while resizing".to_string()
                            });
                            kernel.and_then(|kernel| {
                                let crate::sim::ruleset::KernelSpatialDefinition::Periodic(
                                    definition,
                                ) = &kernel.spatial
                                else {
                                    return Err("selected kernel is not periodic".into());
                                };
                                let mut preview = definition.clone();
                                preview
                                    .resize(parts[0], parts[1], parts[2], parts[3])
                                    .map_err(|error| error.to_string())
                            })
                        }
                        Ok(_) => Err(
                            "enter width,height,anchor_x,anchor_y (four comma-separated integers)"
                                .into(),
                        ),
                        Err(error) => Err(format!("invalid kernel resize number: {error}")),
                    };
                    match result {
                        Ok(report)
                            if !report.discarded_active_nonzero.is_empty()
                                && !self.workbench.kernel_resize_confirmed() =>
                        {
                            self.workbench.confirm_kernel_resize();
                            self.workbench_notice = Some(format!(
                                "resize discards {} active non-zero weights · Enter again to confirm",
                                report.discarded_active_nonzero.len(),
                            ));
                        }
                        Ok(_) => {
                            let parts = source
                                .split(',')
                                .map(|part| part.trim().parse::<usize>().unwrap())
                                .collect::<Vec<_>>();
                            match self.workbench.resize_selected_periodic_kernel(
                                parts[0], parts[1], parts[2], parts[3],
                            ) {
                                Ok(_) => {
                                    self.workbench.cancel_kernel_resize_editor();
                                    self.workbench_notice = Some(format!(
                                        "periodic stencil {}×{} · anchor {},{}",
                                        parts[0], parts[1], parts[2], parts[3],
                                    ));
                                }
                                Err(error) => self.workbench_notice = Some(error.to_string()),
                            }
                        }
                        Err(error) => self.workbench_notice = Some(error),
                    }
                }
                KeyCode::Backspace => {
                    self.workbench.kernel_resize_editor_backspace();
                }
                KeyCode::Char('a')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    self.workbench.kernel_resize_editor_select_all();
                }
                KeyCode::Char(character) => {
                    if !self.workbench.kernel_resize_editor_insert(character) {
                        self.workbench_notice = Some(
                            "resize accepts width,height,anchor_x,anchor_y as integers".into(),
                        );
                    }
                }
                _ => {}
            }
            self.workbench_draft_scene_generation =
                self.workbench_draft_scene_generation.wrapping_add(1);
            return true;
        }
        if self.workbench.color_editor().is_some() {
            match key.code {
                KeyCode::Esc => {
                    self.workbench.cancel_color_editor();
                    self.workbench_notice = Some("color edit cancelled".into());
                }
                KeyCode::Enter => match self.workbench.commit_color_editor() {
                    Ok(color) => {
                        self.workbench_notice = Some(format!(
                            "channel color = #{:02X}{:02X}{:02X}",
                            color.red, color.green, color.blue
                        ));
                    }
                    Err(error) => self.workbench_notice = Some(format!("invalid color: {error}")),
                },
                KeyCode::Backspace => {
                    self.workbench.color_editor_backspace();
                }
                KeyCode::Char('a')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    self.workbench.color_editor_select_all();
                }
                KeyCode::Char(character) => {
                    if !self.workbench.color_editor_insert(character) {
                        self.workbench_notice =
                            Some("color accepts hexadecimal digits in #RRGGBB form".into());
                    }
                }
                _ => return true,
            }
            self.workbench_draft_scene_generation =
                self.workbench_draft_scene_generation.wrapping_add(1);
            return true;
        }
        if self.workbench.section() == WorkbenchSection::Channels && key.code == KeyCode::Char('e')
        {
            self.workbench.begin_selected_color_editor();
            self.workbench_notice = Some("type #RRGGBB · Enter commit · Esc cancel".into());
            self.workbench_draft_scene_generation =
                self.workbench_draft_scene_generation.wrapping_add(1);
            return true;
        }
        if self.workbench.section() == WorkbenchSection::Growth
            && !self.workbench.growth_editing()
            && key.modifiers.is_empty()
            && key.code == KeyCode::Char('m')
        {
            match self.workbench.toggle_selected_growth_mode() {
                Ok(crate::sim::experiment_model::UpdateMode::GrowthRate) => {
                    self.workbench_notice = Some(format!(
                        "Rate mode · next = clamp(self + {} × result, 0, 1)",
                        self.workbench.draft().simulation_dt,
                    ));
                }
                Ok(crate::sim::experiment_model::UpdateMode::DirectUpdate) => {
                    self.workbench_notice = Some("Value mode · next = clamp(result, 0, 1)".into());
                }
                Err(error) => self.workbench_notice = Some(error.to_string()),
            }
            self.workbench_draft_scene_generation =
                self.workbench_draft_scene_generation.wrapping_add(1);
            return true;
        }
        if self.workbench.section() == WorkbenchSection::Kernels
            && key.modifiers.is_empty()
            && key.code == KeyCode::Char('r')
        {
            match self.workbench.begin_selected_kernel_resize_editor() {
                Ok(()) => {
                    self.workbench_notice =
                        Some("resize: width,height,anchor_x,anchor_y · Enter commit".into());
                }
                Err(error) => self.workbench_notice = Some(error),
            }
            self.workbench_draft_scene_generation =
                self.workbench_draft_scene_generation.wrapping_add(1);
            return true;
        }
        if self.workbench.section() == WorkbenchSection::Experiment
            && self.workbench.numeric_editor().is_none()
            && key.modifiers.is_empty()
            && key.code == KeyCode::Char('d')
        {
            self.workbench.begin_simulation_dt_editor();
            self.workbench_notice =
                Some("type simulation dt in (0, 10] · Enter commit · Esc cancel".into());
            self.workbench_draft_scene_generation =
                self.workbench_draft_scene_generation.wrapping_add(1);
            return true;
        }
        if key.code == KeyCode::Char('0')
            && key.modifiers.is_empty()
            && self.workbench.numeric_editor().is_none()
            && matches!(
                self.workbench.section(),
                WorkbenchSection::Tiling | WorkbenchSection::Kernels
            )
        {
            match self.workbench.section() {
                WorkbenchSection::Tiling => {
                    self.workbench.set_tiling_camera(
                        crate::workbench::tiling_editor::TilingCamera::default(),
                    );
                    self.workbench_notice = Some("tiling view fitted".into());
                }
                WorkbenchSection::Kernels => {
                    self.workbench
                        .set_kernel_view(crate::workbench::kernel_editor::KernelView::default());
                    self.workbench_notice = Some("kernel view fitted".into());
                }
                _ => unreachable!("section guard only admits view editors"),
            }
            self.workbench_draft_scene_generation =
                self.workbench_draft_scene_generation.wrapping_add(1);
            return true;
        }
        if self.workbench.section() == WorkbenchSection::Tiling {
            if key.code == KeyCode::Char('z')
                && key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
                && self.workbench.tiling_tool()
                    == crate::workbench::tiling_editor::TilingTool::DrawPolygon
                && !self.workbench.tiling_construction().is_empty()
            {
                let removed = self.workbench.tiling_construction().len();
                self.workbench.pop_tiling_vertex();
                let remaining = self.workbench.tiling_construction().len();
                self.workbench_notice = Some(format!(
                    "removed vertex {removed} · {remaining} {} remains",
                    if remaining == 1 { "vertex" } else { "vertices" }
                ));
                self.workbench_draft_scene_generation =
                    self.workbench_draft_scene_generation.wrapping_add(1);
                return true;
            }
            match key.code {
                KeyCode::Char('d') => {
                    let next = if self.workbench.tiling_tool()
                        == crate::workbench::tiling_editor::TilingTool::DrawPolygon
                    {
                        crate::workbench::tiling_editor::TilingTool::Select
                    } else {
                        crate::workbench::tiling_editor::TilingTool::DrawPolygon
                    };
                    self.workbench.set_tiling_tool(next);
                    self.workbench_notice = Some(match next {
                        crate::workbench::tiling_editor::TilingTool::DrawPolygon => {
                            "Draw polygon: click vertices · click first/Enter close · Esc cancel"
                                .into()
                        }
                        _ => "Select tool".into(),
                    });
                    self.workbench_draft_scene_generation =
                        self.workbench_draft_scene_generation.wrapping_add(1);
                    return true;
                }
                KeyCode::Esc
                    if self.workbench.tiling_tool()
                        == crate::workbench::tiling_editor::TilingTool::DrawPolygon =>
                {
                    self.workbench.cancel_tiling_construction();
                    self.workbench_notice = Some("polygon drawing cancelled".into());
                    self.workbench_draft_scene_generation =
                        self.workbench_draft_scene_generation.wrapping_add(1);
                    return true;
                }
                KeyCode::Enter
                    if self.workbench.tiling_tool()
                        == crate::workbench::tiling_editor::TilingTool::DrawPolygon =>
                {
                    match self.workbench.finish_tiling_construction() {
                        Ok(()) => {
                            self.workbench_notice = Some(
                                if self
                                    .workbench
                                    .draft()
                                    .tiling
                                    .as_ref()
                                    .is_some_and(|tiling| {
                                        crate::sim::tiling::validate_coverage(tiling).is_ok()
                                    })
                                {
                                    "polygon closed · unit cell tiles exactly"
                                } else {
                                    "polygon closed · unit cell incomplete; add polygons or edit lattice"
                                }
                                .into(),
                            )
                        }
                        Err(error) => self.workbench_notice = Some(error),
                    }
                    self.workbench_draft_scene_generation =
                        self.workbench_draft_scene_generation.wrapping_add(1);
                    return true;
                }
                _ => {}
            }
        }
        if self.workbench.section() == WorkbenchSection::Kernels
            && key.modifiers.is_empty()
            && key.code == KeyCode::Char('m')
        {
            self.workbench.cycle_kernel_tool();
            self.workbench_notice = Some(
                match self.workbench.kernel_tool() {
                    crate::workbench::kernel_editor::KernelTool::Weights => {
                        "Weights tool · left/drag paint · right zero · wheel adjust"
                    }
                    crate::workbench::kernel_editor::KernelTool::Support => {
                        "Support tool · left activate · right deactivate"
                    }
                }
                .into(),
            );
            self.workbench_draft_scene_generation =
                self.workbench_draft_scene_generation.wrapping_add(1);
            return true;
        }
        if self.workbench.section() == WorkbenchSection::Kernels
            && key.modifiers.is_empty()
            && matches!(key.code, KeyCode::Char('s') | KeyCode::Char('u'))
        {
            let result = match key.code {
                KeyCode::Char('s') => self
                    .workbench
                    .cycle_selected_kernel_source()
                    .map(|channel| ("source", channel)),
                KeyCode::Char('u') => self
                    .workbench
                    .select_next_kernel_output()
                    .map(|channel| ("output", channel)),
                _ => unreachable!(),
            };
            match result {
                Ok((role, channel)) => {
                    let name = self
                        .workbench
                        .draft()
                        .channels
                        .iter()
                        .find(|entry| entry.id == channel)
                        .map_or("—", |entry| entry.name.as_str());
                    self.workbench_notice =
                        Some(format!("kernel {role} = channel {} ({name})", channel.0));
                }
                Err(error) => self.workbench_notice = Some(error.to_string()),
            }
            self.workbench_draft_scene_generation =
                self.workbench_draft_scene_generation.wrapping_add(1);
            return true;
        }
        if self.workbench.numeric_editor().is_some() {
            match key.code {
                KeyCode::Esc => {
                    self.workbench.take_numeric_editor();
                    self.workbench_notice = Some("numeric edit cancelled".into());
                }
                KeyCode::Enter => {
                    let simulation_dt = self.workbench.simulation_dt_editing();
                    let Some(editor) = self.workbench.take_numeric_editor() else {
                        return true;
                    };
                    match editor.commit() {
                        Ok(value) => {
                            if simulation_dt {
                                if let Err(error) = self.workbench.set_simulation_dt(value as f32) {
                                    self.workbench_notice = Some(error.to_string());
                                } else {
                                    self.workbench_notice = Some(format!(
                                        "simulation dt = {value:.6} · Rate uses self + dt × result"
                                    ));
                                }
                            } else if let Some(selection) =
                                self.workbench.periodic_kernel_selection()
                            {
                                if let Err(error) =
                                    self.set_periodic_kernel_value(selection, value as f32)
                                {
                                    self.workbench_notice = Some(error);
                                } else {
                                    let _ = self.workbench.set_kernel_paint_value(value as f32);
                                    self.workbench_notice = Some(format!("weight = {value:.6}"));
                                }
                            } else if let Some(point) = self.workbench.kernel_selection() {
                                if let Err(error) = self.set_kernel_cell_value(point, value as f32)
                                {
                                    self.workbench_notice = Some(error);
                                } else {
                                    let _ = self.workbench.set_kernel_paint_value(value as f32);
                                    self.workbench_notice = Some(format!("weight = {value:.6}"));
                                }
                            }
                        }
                        Err(error) => {
                            self.workbench_notice = Some(format!("invalid number: {error:?}"));
                            self.workbench.restore_numeric_editor(editor, simulation_dt);
                        }
                    }
                }
                KeyCode::Backspace => {
                    if let Some(editor) = self.workbench.numeric_editor_mut() {
                        editor.backspace();
                    }
                }
                KeyCode::Char(character)
                    if !key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    if let Some(editor) = self.workbench.numeric_editor_mut() {
                        editor.push(character);
                    }
                }
                _ => {}
            }
            self.workbench_draft_scene_generation =
                self.workbench_draft_scene_generation.wrapping_add(1);
            return true;
        }
        if self.workbench.section() == WorkbenchSection::Kernels
            && matches!(
                key.code,
                KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down
            )
        {
            let (dx, dy) = match key.code {
                KeyCode::Left => (-1_i32, 0_i32),
                KeyCode::Right => (1, 0),
                KeyCode::Up => (0, -1),
                KeyCode::Down => (0, 1),
                _ => unreachable!(),
            };
            if let Some(kernel) = self.workbench.selected_rule_kernel()
                && let crate::sim::ruleset::KernelSpatialDefinition::Periodic(definition) =
                    &kernel.spatial
            {
                let source_basis = self
                    .workbench
                    .periodic_kernel_selection()
                    .map(|selection| selection.source_basis)
                    .or_else(|| definition.planes.keys().next().copied())
                    .unwrap_or(self.workbench.selected_basis());
                let current = self.workbench.periodic_kernel_selection().unwrap_or(
                    crate::workbench::kernel_editor::KernelSelection {
                        offset: [0, 0],
                        source_basis,
                    },
                );
                let min_x = -(definition.anchor_x as i32);
                let max_x = definition.width.saturating_sub(definition.anchor_x + 1) as i32;
                let min_y = -(definition.anchor_y as i32);
                let max_y = definition.height.saturating_sub(definition.anchor_y + 1) as i32;
                let selection = crate::workbench::kernel_editor::KernelSelection {
                    offset: [
                        (i32::from(current.offset[0]) + dx).clamp(min_x, max_x) as i16,
                        (i32::from(current.offset[1]) + dy).clamp(min_y, max_y) as i16,
                    ],
                    source_basis,
                };
                self.workbench.select_periodic_kernel(selection);
                self.workbench_notice = self.periodic_kernel_value(selection).map(|value| {
                    format!(
                        "selected offset [{},{}] · source basis {} = {:.6} · E exact",
                        selection.offset[0], selection.offset[1], selection.source_basis.0, value,
                    )
                });
                self.workbench_draft_scene_generation =
                    self.workbench_draft_scene_generation.wrapping_add(1);
                return true;
            }
            if let Some(definition) = self.workbench.selected_raster_kernel_definition() {
                let current = self.workbench.kernel_selection().unwrap_or(
                    crate::workbench::kernel_editor::KernelPoint {
                        x: definition.anchor_x,
                        y: definition.anchor_y,
                    },
                );
                let point = crate::workbench::kernel_editor::KernelPoint {
                    x: (current.x as i32 + dx).clamp(0, definition.width.saturating_sub(1) as i32)
                        as usize,
                    y: (current.y as i32 + dy).clamp(0, definition.height.saturating_sub(1) as i32)
                        as usize,
                };
                self.workbench.select_kernel_point(point);
                self.workbench_notice = self.kernel_cell_value(point).map(|value| {
                    format!(
                        "selected cell {},{} = {value:.6} · E exact",
                        point.x, point.y
                    )
                });
                self.workbench_draft_scene_generation =
                    self.workbench_draft_scene_generation.wrapping_add(1);
                return true;
            }
        }
        if self.workbench.section() == WorkbenchSection::Kernels
            && matches!(key.code, KeyCode::Enter | KeyCode::Char('e'))
        {
            if let Some(selection) = self.workbench.periodic_kernel_selection()
                && let Some(value) = self.periodic_kernel_value(selection)
            {
                self.workbench.begin_numeric_editor(
                    crate::workbench::numeric_editor::NumericEditor::begin(
                        format!(
                            "weight[offset {},{} · basis {}]",
                            selection.offset[0], selection.offset[1], selection.source_basis.0
                        ),
                        f64::from(value),
                        -1.0..=1.0,
                    ),
                );
                self.workbench_notice = Some("type exact weight; Enter commit · Esc cancel".into());
                return true;
            }
            if let Some(point) = self.workbench.kernel_selection()
                && let Some(value) = self.kernel_cell_value(point)
            {
                self.workbench.begin_numeric_editor(
                    crate::workbench::numeric_editor::NumericEditor::begin(
                        format!("weight[{},{}]", point.x, point.y),
                        f64::from(value),
                        -1.0..=1.0,
                    ),
                );
                self.workbench_notice = Some("type exact weight; Enter commit · Esc cancel".into());
                return true;
            }
        }
        self.handle_workbench_growth_key(key)
    }

    fn periodic_kernel_value(
        &self,
        selection: crate::workbench::kernel_editor::KernelSelection,
    ) -> Option<f32> {
        let kernel = self.workbench.selected_rule_kernel()?;
        let crate::sim::ruleset::KernelSpatialDefinition::Periodic(definition) = &kernel.spatial
        else {
            return None;
        };
        definition.weight(selection.offset, selection.source_basis)
    }

    fn set_periodic_kernel_value(
        &mut self,
        selection: crate::workbench::kernel_editor::KernelSelection,
        value: f32,
    ) -> Result<(), String> {
        self.workbench
            .set_selected_kernel_weight(selection.offset, selection.source_basis, value)
            .map_err(|error| error.to_string())?;
        self.workbench.select_periodic_kernel(selection);
        self.workbench_draft_scene_generation =
            self.workbench_draft_scene_generation.wrapping_add(1);
        Ok(())
    }

    fn set_periodic_kernel_active(
        &mut self,
        selection: crate::workbench::kernel_editor::KernelSelection,
        active: bool,
    ) -> Result<(), String> {
        self.workbench
            .set_selected_kernel_active(selection.offset, selection.source_basis, active)
            .map_err(|error| error.to_string())?;
        self.workbench.select_periodic_kernel(selection);
        self.workbench_draft_scene_generation =
            self.workbench_draft_scene_generation.wrapping_add(1);
        Ok(())
    }

    fn kernel_cell_value(
        &self,
        point: crate::workbench::kernel_editor::KernelPoint,
    ) -> Option<f32> {
        let definition = self.workbench.selected_raster_kernel_definition()?;
        if point.x >= definition.width || point.y >= definition.height {
            return None;
        }
        let values = match &definition.values {
            KernelValues::Explicit(values) => values.clone(),
            KernelValues::Expression(_) => definition.build().ok()?.values,
        };
        values.get(point.y * definition.width + point.x).copied()
    }

    fn set_kernel_cell_value(
        &mut self,
        point: crate::workbench::kernel_editor::KernelPoint,
        value: f32,
    ) -> Result<(), String> {
        let Some(definition) = self.workbench.selected_raster_kernel_definition().cloned() else {
            return Err("no selected kernel".into());
        };
        let mut scene = crate::workbench::kernel_editor::KernelScene::new(definition)
            .with_view(self.workbench.kernel_view());
        scene.apply_gesture(crate::workbench::kernel_editor::KernelGesture::SetValue {
            x: point.x,
            y: point.y,
            value,
        })?;
        self.workbench
            .replace_selected_raster_kernel_definition(scene.definition)
            .map_err(|error| error.to_string())?;
        self.workbench.select_kernel_point(point);
        self.workbench_draft_scene_generation =
            self.workbench_draft_scene_generation.wrapping_add(1);
        Ok(())
    }

    pub fn tick(&self) -> u64 {
        self.remote_tick.unwrap_or_else(|| {
            self.experiment_service
                .as_ref()
                .map_or_else(|| self.backend.tick(), ExperimentService::tick)
        })
    }

    pub fn backend_error(&self) -> Option<&str> {
        self.kernel_error
            .as_deref()
            .or(self.backend_error.as_deref())
    }

    pub fn backend_kind(&self) -> BackendKind {
        self.experiment_service
            .as_ref()
            .map_or_else(|| self.backend.kind(), ExperimentService::backend_kind)
    }

    pub fn backend_name(&self) -> &str {
        self.remote_backend.as_deref().unwrap_or_else(|| {
            self.experiment_service.as_ref().map_or_else(
                || self.backend.device_name(),
                ExperimentService::backend_name,
            )
        })
    }

    pub fn is_remote_mirror(&self) -> bool {
        self.remote_tick.is_some()
    }

    pub fn display_rule_name(&self) -> &str {
        self.remote_rule.as_deref().unwrap_or_else(|| {
            if self.experiment_revision > 0 {
                "Experiment"
            } else {
                rule_name(&self.spec)
            }
        })
    }

    pub fn applied_input_sequence(&self) -> u64 {
        self.applied_input_sequence
    }

    /// Monotonic-enough identity for the remote image state. A simulation tick
    /// changes the cells; an acknowledged input sequence covers commands that
    /// mutate cells without advancing the tick (clear, reset, mouse edits).
    pub fn render_generation(&self) -> u64 {
        self.tick()
            .wrapping_mul(1_000_003)
            .wrapping_add(self.applied_input_sequence)
    }

    pub fn set_remote_transport_rates(&mut self, snapshot: f64, graphics: f64) {
        self.snapshot_rate = snapshot;
        self.graphics_rate = graphics;
    }

    pub fn remote_transport_rates(&self) -> (f64, f64) {
        (self.snapshot_rate, self.graphics_rate)
    }

    pub fn active_revision(&self) -> u64 {
        self.experiment_revision
    }

    pub fn channel_count(&self) -> usize {
        self.experiment_model.channels.len()
    }

    pub fn active_experiment(&self) -> ExperimentSpec {
        if let Some(service) = &self.experiment_service {
            return service.snapshot_active_experiment();
        }
        let mut model = self.experiment_model.clone();
        if let Some(channel) = model.channels.first_mut() {
            channel.initial = self.world.cells().to_vec();
        }
        model
    }

    pub fn experiment_model(&self) -> &ExperimentSpec {
        &self.experiment_model
    }

    pub fn tiling_draft(&self) -> Option<&PeriodicTilingDraft> {
        self.experiment_model.tiling.as_ref()
    }

    pub fn set_tiling_draft(
        &mut self,
        tiling: Option<PeriodicTilingDraft>,
    ) -> Result<(), Vec<String>> {
        let mut candidate = self.experiment_model.clone();
        candidate.tiling = tiling;
        validate_structure(&candidate).map_err(|errors| {
            errors
                .into_iter()
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
        })?;
        self.experiment_model = candidate.clone();
        self.workbench.accept(candidate);
        self.kernel_error = None;
        self.backend_error = None;
        Ok(())
    }

    pub fn growth_plot_samples(&self, count: usize) -> Vec<Option<f32>> {
        let source = self
            .active_experiment()
            .growth
            .first()
            .map(|growth| growth.source.clone())
            .or_else(|| {
                self.spec
                    .growth_expression()
                    .map(crate::sim::parser::format_expression)
            });
        let Some(source) = source else {
            return Vec::new();
        };
        let parameters = match self.spec.rule {
            crate::sim::rule::Rule::Lenia { mu, sigma } => std::collections::BTreeMap::from([
                ("mu".to_string(), mu),
                ("sigma".to_string(), sigma),
            ]),
            _ => std::collections::BTreeMap::new(),
        };
        let Ok(program) = crate::sim::growth::typecheck::compile(
            &source,
            &crate::sim::growth::types::ExternalSymbols {
                kernel_inputs: vec!["potential".to_string()],
                parameters: parameters.keys().cloned().collect(),
            },
        ) else {
            return Vec::new();
        };
        let Ok(crate::sim::growth::plot::PlotData::Curve(curve)) =
            crate::sim::growth::plot::sample_plot(
                &program,
                crate::sim::growth::plot::PlotRequest::Curve {
                    axis: "potential".to_string(),
                    start: 0.0,
                    end: 1.0,
                    samples: count.clamp(1, 128),
                    pinned: crate::sim::growth::plot::PinnedInputs(parameters),
                    trace: false,
                },
            )
        else {
            return Vec::new();
        };
        curve
            .samples
            .into_iter()
            .map(|sample| sample.value)
            .collect()
    }

    pub fn submit_draft(&mut self, request: ApplyRequest) -> Result<ApplyAccepted, ApplyRejected> {
        let request_id = request.request_id;
        if request.base_revision != self.experiment_revision {
            return Err(ApplyRejected {
                request_id: request.request_id,
                diagnostics: vec![Diagnostic {
                    code: "revision_conflict".to_string(),
                    message: format!(
                        "draft is based on revision {}, active revision is {}",
                        request.base_revision, self.experiment_revision
                    ),
                    path: DiagnosticPath::field("base_revision"),
                }],
            });
        }
        validate_structure(&request.draft).map_err(|errors| ApplyRejected {
            request_id: request.request_id,
            diagnostics: errors
                .into_iter()
                .map(|error| Diagnostic {
                    code: "invalid_experiment".to_string(),
                    message: error.to_string(),
                    path: DiagnosticPath::field("experiment"),
                })
                .collect(),
        })?;
        let service = ExperimentService::new(request.draft.clone()).map_err(|mut rejected| {
            rejected.request_id = request_id;
            rejected
        })?;
        let normalized = service.active_spec().clone();
        if let Some(first) = normalized.channels.first() {
            let crate::sim::experiment_model::GeometrySpec::RasterGrid(grid) = &normalized.geometry;
            if self.world.width() != grid.width as usize
                || self.world.height() != grid.height as usize
            {
                self.world = World::new(grid.width as usize, grid.height as usize);
                self.camera = Camera::new([grid.width as f32 / 2.0, grid.height as f32 / 2.0], 1.0);
            }
            self.world.replace_cells(&first.initial);
        }
        self.experiment_model = normalized.clone();
        self.workbench.accept(normalized.clone());
        self.experiment_service = Some(service);
        self.paused = false;
        self.experiment_revision =
            self.experiment_revision
                .checked_add(1)
                .ok_or_else(|| ApplyRejected {
                    request_id,
                    diagnostics: vec![Diagnostic {
                        code: "revision_overflow".to_string(),
                        message: "experiment revision overflow".to_string(),
                        path: DiagnosticPath::field("revision"),
                    }],
                })?;
        self.workbench_base_revision = self.experiment_revision;
        Ok(ApplyAccepted {
            request_id,
            revision: self.experiment_revision,
            normalized_experiment: normalized,
        })
    }

    fn accept_remote_apply(&mut self, accepted: ApplyAccepted) {
        self.experiment_revision = accepted.revision;
        self.workbench_base_revision = accepted.revision;
        self.experiment_model = accepted.normalized_experiment.clone();
        self.workbench.accept(accepted.normalized_experiment);
        self.paused = false;
        self.leave_workbench();
        self.workbench_notice = Some(match self.save_default_workspace_now(true) {
            Ok(paths) => format!(
                "running revision {} · saved {}",
                accepted.revision,
                paths.experiment.display(),
            ),
            Err(error) if self.workspace_persistence.is_some() => {
                format!(
                    "running revision {} · save failed: {error}",
                    accepted.revision
                )
            }
            Err(_) => format!("running revision {}", accepted.revision),
        });
    }

    pub fn selected_kernel_name(&self) -> &str {
        self.kernel_definitions
            .get(self.selected_kernel)
            .map_or("", |definition| definition.name.as_str())
    }

    pub fn selected_kernel_dimensions(&self) -> (usize, usize) {
        self.kernel_definitions
            .get(self.selected_kernel)
            .map_or((0, 0), |definition| (definition.width, definition.height))
    }

    pub fn selected_kernel_anchor(&self) -> (usize, usize) {
        self.kernel_definitions
            .get(self.selected_kernel)
            .map_or((0, 0), |definition| {
                (definition.anchor_x, definition.anchor_y)
            })
    }

    pub fn selected_kernel_radius(&self) -> usize {
        self.kernel_definitions
            .get(self.selected_kernel)
            .map_or(0, definition_radius)
    }

    pub fn selected_kernel_normalization(&self) -> Normalization {
        self.kernel_definitions
            .get(self.selected_kernel)
            .map_or(Normalization::None, |definition| definition.normalization)
    }

    pub fn selected_kernel_parameter(&self) -> Option<(String, f32)> {
        let definition = self.kernel_definitions.get(self.selected_kernel)?;
        let name = self.selected_parameter.as_deref()?;
        definition
            .parameters
            .get(name)
            .map(|value| (name.to_string(), *value))
    }

    pub fn kernel_preview_enabled(&self) -> bool {
        self.kernel_preview_enabled
    }

    pub fn active_panel(&self) -> Panel {
        self.active_panel
    }

    pub fn expression_editing(&self) -> bool {
        self.expression_editing
    }

    pub fn expression_buffer(&self) -> &str {
        &self.expression_buffer
    }

    pub fn help_visible(&self) -> bool {
        self.help_visible
    }

    pub fn replace_expression_buffer(&mut self, value: impl Into<String>) {
        self.expression_buffer = value.into();
    }

    pub fn handle_expression_key(&mut self, event: KeyEvent) {
        match event.code {
            KeyCode::Char(character) => self.expression_buffer.push(character),
            KeyCode::Backspace => {
                self.expression_buffer.pop();
            }
            KeyCode::Enter => {
                let expression = self.expression_buffer.clone();
                if self.set_growth_expression(&expression) {
                    self.expression_editing = false;
                }
            }
            KeyCode::Esc => self.expression_editing = false,
            _ => {}
        }
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    pub fn remote_snapshot(&self) -> crate::remote::Snapshot {
        let (simulation_rate, render_rate) = self.rates();
        let performance = self.performance();
        crate::remote::Snapshot {
            width: self.world.width() as u32,
            height: self.world.height() as u32,
            tick: self.tick(),
            paused: self.paused,
            simulation_rate,
            render_rate,
            last_step_ms: performance.last_step_ms,
            average_step_ms: performance.average_step_ms,
            step_samples: performance.step_samples,
            applied_input_sequence: self.applied_input_sequence,
            backend: self.backend_name().to_string(),
            rule: self.display_rule_name().to_string(),
            spec: Box::new(self.spec.clone()),
            selected_kernel: Box::new(
                self.kernel_definitions
                    .get(self.selected_kernel)
                    .cloned()
                    .unwrap_or_else(|| definition_from_kernel(&self.spec.kernel)),
            ),
            selected_parameter: self.selected_parameter.clone(),
            error: self.backend_error().map(str::to_string),
            cells: self.world.cells().to_vec(),
        }
    }

    pub fn apply_remote_snapshot(&mut self, snapshot: &crate::remote::Snapshot) -> bool {
        let dimensions_match = self.world.width() == snapshot.width as usize
            && self.world.height() == snapshot.height as usize
            && snapshot.cells.len() == self.world.width() * self.world.height();
        if !dimensions_match {
            return false;
        }
        if snapshot.selected_kernel.build().is_err()
            || definition_from_kernel(&snapshot.spec.kernel)
                .build()
                .is_err()
        {
            return false;
        }
        self.world.replace_cells(&snapshot.cells);
        self.paused = snapshot.paused;
        self.remote_tick = Some(snapshot.tick);
        self.remote_backend = Some(snapshot.backend.clone());
        self.apply_remote_spec(&snapshot.rule, &snapshot.spec, &snapshot.selected_kernel);
        self.selected_parameter = snapshot.selected_parameter.clone();
        self.applied_input_sequence = snapshot.applied_input_sequence;
        self.simulation_rate = snapshot.simulation_rate;
        self.performance.last_step_ms = snapshot.last_step_ms;
        self.performance.average_step_ms = snapshot.average_step_ms;
        self.performance.step_samples = snapshot.step_samples;
        self.backend_error = snapshot.error.clone();
        true
    }

    fn apply_remote_experiment_state(
        &mut self,
        revision: u64,
        experiment: crate::sim::experiment_model::ExperimentSpec,
    ) {
        self.experiment_revision = revision;
        self.experiment_model = experiment.clone();
        let restored_draft = self.workspace_persistence.as_mut().and_then(|persistence| {
            if !persistence.restored_pending_remote_rebase {
                return None;
            }
            persistence.restored_pending_remote_rebase = false;
            persistence.last_saved_draft.clone()
        });
        if let Some(restored_draft) = restored_draft {
            // The server is authoritative for the active experiment, but a
            // locally restored workspace is an explicit user design. Rebase
            // that draft onto the newly connected server instead of silently
            // replacing it during the initial ExperimentState handshake.
            self.workbench.accept(experiment);
            if let Err(error) = self.workbench.import_draft(restored_draft) {
                self.workbench_notice = Some(format!("workspace restore failed: {error}"));
            }
            self.workbench_base_revision = revision;
            return;
        }
        // The initial authoritative model can arrive after the user has
        // already entered Workbench over a latent SSH connection. Never let
        // that late initialization erase a dirty draft or an active editor.
        if self.mode != AppMode::Workbench
            || (self.workbench.status() == crate::workbench::DraftStatus::Clean
                && !self.workbench.growth_editing())
        {
            self.workbench.accept(experiment);
            self.workbench_base_revision = revision;
        }
    }

    fn apply_remote_spec(
        &mut self,
        name: &str,
        spec: &SimulationSpec,
        selected_kernel: &KernelDefinition,
    ) {
        self.remote_rule = Some(name.to_string());
        self.spec = spec.clone();
        self.kernel_definitions = vec![selected_kernel.clone()];
        self.selected_kernel = 0;
    }

    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    pub fn spec(&self) -> &SimulationSpec {
        &self.spec
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn save_experiment(&self, path: impl AsRef<Path>) -> Result<(), ExperimentError> {
        let file = ExperimentFile::from_parts(
            ExperimentMetadata {
                name: rule_name(&self.spec).to_string(),
                description: "Cellarium experiment".to_string(),
                author: "cellarium".to_string(),
                tags: vec![rule_name(&self.spec).to_ascii_lowercase()],
            },
            self.spec.clone(),
            &self.world,
            self.seed,
        )?;
        crate::sim::experiment::save_experiment(path, &file)
    }

    pub fn inspected(&self) -> Option<f32> {
        self.inspected
    }

    pub fn set_rates(&mut self, simulation: f64, render: f64) {
        self.simulation_rate = simulation;
        self.render_rate = render;
    }

    pub fn rates(&self) -> (f64, f64) {
        (self.simulation_rate, self.render_rate)
    }

    pub fn performance(&self) -> PerformanceStats {
        self.performance
    }

    pub fn record_step_duration(&mut self, duration: Duration) {
        record_duration(
            duration,
            &mut self.performance.last_step_ms,
            &mut self.performance.average_step_ms,
            &mut self.performance.step_samples,
        );
    }

    pub fn record_render_duration(&mut self, duration: Duration) {
        record_duration(
            duration,
            &mut self.performance.last_render_ms,
            &mut self.performance.average_render_ms,
            &mut self.performance.render_samples,
        );
    }

    pub fn render_framebuffer(&mut self, width: usize, height: usize) -> &Framebuffer {
        let needs_resize = self
            .framebuffer
            .as_ref()
            .is_none_or(|frame| frame.width() != width || frame.height() != height);
        if needs_resize {
            self.framebuffer = Some(Framebuffer::new(width, height));
        }
        let camera = self.camera;
        let framebuffer = self
            .framebuffer
            .as_mut()
            .expect("framebuffer is initialized");
        rasterize_world_into(&self.world, &camera, framebuffer);
        framebuffer
    }

    pub fn set_viewport(&mut self, viewport: Rect, frame_size: [usize; 2]) {
        self.viewport = Some(viewport);
        self.frame_size = Some(frame_size);
    }

    pub fn viewport_geometry(&self) -> Option<(Rect, [usize; 2])> {
        Some((self.viewport?, self.frame_size?))
    }

    pub fn step(&mut self) -> bool {
        let started = Instant::now();
        if let Some(service) = &mut self.experiment_service {
            let result = service.step();
            if result.is_ok() {
                self.world.replace_cells(&service.rasterized_channel(0));
            }
            self.record_step_duration(started.elapsed());
            return match result {
                Ok(()) => {
                    self.backend_error = None;
                    true
                }
                Err(error) => {
                    self.backend_error = Some(error.to_string());
                    false
                }
            };
        }
        let result = self.backend.step(&mut self.world);
        self.record_step_duration(started.elapsed());
        match result {
            Ok(()) => {
                self.backend_error = None;
                true
            }
            Err(error) => {
                self.backend_error = Some(error.to_string());
                false
            }
        }
    }

    pub fn set_growth_expression(&mut self, source: &str) -> bool {
        let mut candidate = self.spec.clone();
        if let Err(error) = candidate.set_growth_expression(source) {
            self.kernel_error = None;
            self.backend_error = Some(error.to_string());
            return false;
        }

        let result = SimulationBackend::strict_for_kind(
            self.backend.kind(),
            candidate.clone(),
            self.world.width(),
            self.world.height(),
        );
        match result {
            Ok(backend) => {
                self.spec = candidate;
                self.backend = backend;
                self.backend_error = None;
                self.kernel_error = None;
                true
            }
            Err(error) => {
                self.kernel_error = None;
                self.backend_error = Some(error.to_string());
                false
            }
        }
    }

    pub fn handle_command(&mut self, command: Command) {
        match command {
            Command::TogglePause => self.paused = !self.paused,
            Command::Step => {
                self.step();
            }
            Command::Reset => self.reset(),
            Command::Randomize => {
                self.seed = self.seed.wrapping_add(1);
                self.reset();
            }
            Command::Clear => {
                self.world.clear();
                if let Some(service) = &mut self.experiment_service {
                    let zeros = vec![0.0; service.world().cells().len()];
                    let _ = service.world_mut().replace_all(&zeros);
                } else {
                    self.backend = self.recreate_backend();
                }
            }
            Command::Conway => {
                self.spec = SimulationSpec::conway();
                self.reset();
            }
            Command::Lenia => {
                self.spec = SimulationSpec::lenia_orbium();
                self.selected_kernel = self
                    .kernel_definitions
                    .iter()
                    .position(|definition| definition.name == "ring")
                    .unwrap_or(self.selected_kernel);
                self.selected_parameter = None;
                self.reset();
            }
            Command::NextKernel => {
                self.selected_kernel = (self.selected_kernel + 1) % self.kernel_definitions.len();
                self.selected_parameter = None;
            }
            Command::NextKernelParameter => self.cycle_kernel_parameter(),
            Command::IncreaseKernelParameter => self.adjust_kernel_parameter(0.01),
            Command::DecreaseKernelParameter => self.adjust_kernel_parameter(-0.01),
            Command::RegenerateKernel => self.regenerate_kernel(),
            Command::ToggleKernelPreview => {
                self.kernel_preview_enabled = !self.kernel_preview_enabled;
            }
            Command::NextPanel => {
                if self.mode == AppMode::Workbench {
                    let previous = self.workbench.section();
                    self.workbench.section_next();
                    if self.workbench.section() != previous {
                        self.request_workbench_display_clear();
                    }
                    self.workbench_notice =
                        Some(format!("selected {}", self.workbench.section().label()));
                    if std::env::var_os("CELLARIUM_E2E_TRACE").is_some() {
                        eprintln!("E2E_WORKBENCH_SECTION={}", self.workbench.section().label());
                    }
                } else {
                    self.active_panel = self.active_panel.next();
                }
            }
            Command::ToggleExpressionEditor => {
                if self.mode == AppMode::Workbench
                    && self.workbench.section() == crate::workbench::WorkbenchSection::Growth
                {
                    self.workbench.toggle_growth_editing();
                } else {
                    self.toggle_expression_editor();
                }
            }
            Command::ToggleHelp => self.help_visible = !self.help_visible,
            Command::ToggleWorkbench => {
                if self.mode == AppMode::Simulation {
                    self.enter_workbench();
                } else {
                    self.leave_workbench();
                }
            }
            Command::Quit => {}
        }
    }

    pub fn handle_remote_command_optimistically(&mut self, command: Command) {
        match command {
            Command::TogglePause => self.paused = !self.paused,
            Command::Clear => self.world.clear(),
            Command::NextPanel => {
                if self.mode == AppMode::Workbench {
                    let previous = self.workbench.section();
                    self.workbench.section_next();
                    if self.workbench.section() != previous {
                        self.request_workbench_display_clear();
                    }
                    self.workbench_notice =
                        Some(format!("selected {}", self.workbench.section().label()));
                    if std::env::var_os("CELLARIUM_E2E_TRACE").is_some() {
                        eprintln!("E2E_WORKBENCH_SECTION={}", self.workbench.section().label());
                    }
                } else {
                    self.active_panel = self.active_panel.next();
                }
            }
            Command::ToggleKernelPreview => {
                self.kernel_preview_enabled = !self.kernel_preview_enabled;
            }
            Command::ToggleExpressionEditor => {
                if self.mode == AppMode::Workbench
                    && self.workbench.section() == crate::workbench::WorkbenchSection::Growth
                {
                    self.workbench.toggle_growth_editing();
                } else {
                    self.toggle_expression_editor();
                }
            }
            Command::ToggleHelp => self.help_visible = !self.help_visible,
            Command::ToggleWorkbench => {
                if self.mode == AppMode::Simulation {
                    self.enter_workbench();
                } else {
                    self.leave_workbench();
                }
            }
            Command::Quit
            | Command::Step
            | Command::Reset
            | Command::Randomize
            | Command::Conway
            | Command::Lenia
            | Command::NextKernel
            | Command::NextKernelParameter
            | Command::IncreaseKernelParameter
            | Command::DecreaseKernelParameter
            | Command::RegenerateKernel => {}
        }
    }

    fn toggle_expression_editor(&mut self) {
        if self.expression_editing {
            let expression = self.expression_buffer.clone();
            if self.set_growth_expression(&expression) {
                self.expression_editing = false;
            }
            return;
        }
        self.expression_buffer = self
            .spec
            .growth_expression()
            .map(crate::sim::parser::format_expression)
            .unwrap_or_default();
        self.expression_editing = true;
    }

    fn cycle_kernel_parameter(&mut self) {
        let Some(definition) = self.kernel_definitions.get(self.selected_kernel) else {
            self.selected_parameter = None;
            return;
        };
        let names: Vec<_> = definition.parameters.keys().cloned().collect();
        self.selected_parameter = if names.is_empty() {
            None
        } else {
            let current = self
                .selected_parameter
                .as_deref()
                .and_then(|name| names.iter().position(|candidate| candidate == name));
            let next = current.map_or(0, |index| (index + 1) % names.len());
            Some(names[next].clone())
        };
    }

    fn adjust_kernel_parameter(&mut self, amount: f32) {
        let Some(name) = self.selected_parameter.clone() else {
            return;
        };
        let Some(definition) = self.kernel_definitions.get_mut(self.selected_kernel) else {
            return;
        };
        let Some(value) = definition.parameters.get_mut(&name) else {
            return;
        };
        let adjusted = *value + amount;
        if adjusted.is_finite() {
            *value = adjusted;
            self.kernel_error = None;
        } else {
            self.kernel_error = Some("Kernel parameter must remain finite".to_string());
        }
    }

    fn regenerate_kernel(&mut self) {
        let Some(definition) = self.kernel_definitions.get(self.selected_kernel) else {
            self.kernel_error = Some("No kernel definition is selected".to_string());
            return;
        };
        let kernel = match definition.build() {
            Ok(kernel) => kernel,
            Err(error) => {
                self.kernel_error = Some(format!("Kernel regeneration failed: {error}"));
                return;
            }
        };
        let mut next_spec = self.spec.clone();
        next_spec.kernel = kernel;
        let next_backend = SimulationBackend::strict_for_kind(
            self.backend.kind(),
            next_spec.clone(),
            self.world.width(),
            self.world.height(),
        )
        .map_err(|error| error.to_string());
        self.commit_regenerated_kernel(next_spec, next_backend);
    }

    fn commit_regenerated_kernel(
        &mut self,
        next_spec: SimulationSpec,
        next_backend: Result<SimulationBackend, String>,
    ) -> bool {
        let next_backend = match next_backend {
            Ok(backend) => backend,
            Err(error) => {
                self.kernel_error = Some(format!("Kernel backend regeneration failed: {error}"));
                return false;
            }
        };
        self.spec = next_spec;
        self.backend = next_backend;
        self.kernel_error = None;
        true
    }

    fn reset(&mut self) {
        self.experiment_service = None;
        self.world.randomize(self.seed, initial_density(&self.spec));
        self.backend = self.recreate_backend();
        self.inspected = None;
    }

    fn recreate_backend(&self) -> SimulationBackend {
        self.backend_for_spec(&self.spec)
    }

    fn backend_for_spec(&self, spec: &SimulationSpec) -> SimulationBackend {
        match self.backend.kind() {
            BackendKind::Cpu => SimulationBackend::cpu(spec.clone()),
            BackendKind::Cuda => SimulationBackend::cuda_or_cpu(
                spec.clone(),
                self.world.width(),
                self.world.height(),
            ),
        }
    }

    pub fn inspect_world(&mut self, world: [f32; 2]) {
        let x = world[0].floor() as isize;
        let y = world[1].floor() as isize;
        self.inspected = Some(self.experiment_service.as_ref().map_or_else(
            || self.world.get(x, y),
            |service| service.world().get(0, x, y),
        ));
    }

    pub fn paint_world(&mut self, world: [f32; 2], value: f32) {
        let x = world[0].floor() as isize;
        let y = world[1].floor() as isize;
        self.world.set(x, y, value);
        if let Some(service) = &mut self.experiment_service {
            service.world_mut().set(0, x, y, value);
        }
    }

    fn paint_world_segment(&mut self, from: [f32; 2], to: [f32; 2], value: f32) {
        let dx = to[0] - from[0];
        let dy = to[1] - from[1];
        let steps = dx.abs().max(dy.abs()).ceil() as usize;
        if steps == 0 {
            self.paint_world(to, value);
            return;
        }
        for step in 0..=steps {
            let amount = step as f32 / steps as f32;
            self.paint_world([from[0] + dx * amount, from[1] + dy * amount], value);
        }
    }

    pub fn handle_mouse(
        &mut self,
        event: MouseEvent,
        tracker: &mut crate::input::MouseTracker,
    ) -> bool {
        let Some(viewport) = self.viewport else {
            return false;
        };
        if event.column < viewport.x || event.row < viewport.y {
            return false;
        }
        let mut local = event;
        local.column = event.column.saturating_sub(viewport.x);
        local.row = event.row.saturating_sub(viewport.y);
        let tracked = tracker.update(&local, viewport.width, viewport.height);
        if self.mode == AppMode::Workbench
            && self.workbench.section() == crate::workbench::WorkbenchSection::Tiling
            && self.workbench.tiling_tool()
                == crate::workbench::tiling_editor::TilingTool::DrawPolygon
            && matches!(local.kind, crossterm::event::MouseEventKind::Moved)
        {
            let frame_size = self
                .frame_size
                .unwrap_or([viewport.width as usize, viewport.height as usize * 2]);
            let scene = self
                .workbench
                .draft()
                .tiling
                .clone()
                .map(crate::workbench::tiling_editor::TilingScene::new)
                .unwrap_or_else(|| {
                    crate::workbench::tiling_editor::TilingScene::empty(
                        self.workbench.tiling_camera(),
                    )
                })
                .with_camera(self.workbench.tiling_camera());
            let pointer = graphics_pointer_cell(frame_size, viewport, local.column, local.row);
            let [px, py] = pointer.center;
            self.workbench.set_tiling_pointer(Some(scene.pixel_to_world(
                px,
                py,
                frame_size[0] as u32,
                frame_size[1] as u32,
            )));
            self.workbench_draft_scene_generation =
                self.workbench_draft_scene_generation.wrapping_add(1);
            return true;
        }
        if self.mode == AppMode::Workbench
            && self.workbench.section() == crate::workbench::WorkbenchSection::Tiling
            && matches!(
                local.kind,
                crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left)
            )
        {
            self.workbench.finish_tiling_drag();
            return true;
        }
        let Some(mut action) = tracked else {
            return false;
        };
        if self.mode == AppMode::Simulation
            && matches!(
                local.kind,
                crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left)
            )
        {
            action = crate::input::MouseAction::Paint;
        }
        if self.mode == AppMode::Workbench
            && self.workbench.section() == crate::workbench::WorkbenchSection::World
        {
            let crate::sim::experiment_model::GeometrySpec::RasterGrid(grid) =
                &self.workbench.draft().geometry;
            let frame_size = self
                .frame_size
                .unwrap_or([viewport.width as usize, viewport.height as usize * 2]);
            let screen = [
                (local.column as f32 + 0.5) * frame_size[0] as f32 / viewport.width.max(1) as f32,
                (local.row as f32 + 0.5) * frame_size[1] as f32 / viewport.height.max(1) as f32,
            ];
            let world = self
                .camera
                .screen_to_world(screen, frame_size[0], frame_size[1]);
            let value = match action {
                crate::input::MouseAction::Erase => Some(0.0),
                crate::input::MouseAction::Inspect | crate::input::MouseAction::Paint => Some(1.0),
                crate::input::MouseAction::Pan { .. } | crate::input::MouseAction::Zoom { .. } => {
                    None
                }
            };
            if let Some(value) = value {
                let from = tracker
                    .stroke_segment()
                    .map(|(from, _)| {
                        let from_screen = [
                            (from.0 + 0.5) * frame_size[0] as f32 / viewport.width.max(1) as f32,
                            (from.1 + 0.5) * frame_size[1] as f32 / viewport.height.max(1) as f32,
                        ];
                        self.camera
                            .screen_to_world(from_screen, frame_size[0], frame_size[1])
                    })
                    .unwrap_or(world);
                let dx = world[0] - from[0];
                let dy = world[1] - from[1];
                let steps = dx.abs().max(dy.abs()).ceil() as usize;
                let mut values = Vec::with_capacity(steps.saturating_add(1));
                for step in 0..=steps {
                    let amount = if steps == 0 {
                        1.0
                    } else {
                        step as f32 / steps as f32
                    };
                    let point = [from[0] + dx * amount, from[1] + dy * amount];
                    let x = (point[0].floor() as isize).rem_euclid(grid.width as isize) as usize;
                    let y = (point[1].floor() as isize).rem_euclid(grid.height as isize) as usize;
                    let tile = y.saturating_mul(grid.width as usize).saturating_add(x);
                    if !values.iter().any(|(existing, _)| *existing == tile) {
                        values.push((tile, value));
                    }
                }
                let applied = self
                    .workbench
                    .execute(crate::workbench::DraftCommand::SetChannelValues {
                        channel: self.workbench.selected_channel(),
                        values,
                    })
                    .is_ok();
                if applied {
                    self.workbench_draft_scene_generation =
                        self.workbench_draft_scene_generation.wrapping_add(1);
                }
                if applied && std::env::var_os("CELLARIUM_E2E_TRACE").is_some() {
                    eprintln!("E2E_WORKBENCH_DRAFT=Dirty");
                }
                return applied;
            }
        }
        if self.mode == AppMode::Workbench {
            let frame_size = self
                .frame_size
                .unwrap_or([viewport.width as usize, viewport.height as usize * 2]);
            let pointer = graphics_pointer_cell(frame_size, viewport, local.column, local.row);
            let [px, py] = pointer.center;
            match self.workbench.section() {
                crate::workbench::WorkbenchSection::Tiling => {
                    let hit_radius = graphics_pointer_hit_radius(frame_size, viewport);
                    let mut scene = self
                        .workbench
                        .draft()
                        .tiling
                        .clone()
                        .map(crate::workbench::tiling_editor::TilingScene::new)
                        .unwrap_or_else(|| {
                            crate::workbench::tiling_editor::TilingScene::empty(
                                self.workbench.tiling_camera(),
                            )
                        })
                        .with_selected_basis(self.workbench.selected_basis())
                        .with_camera(self.workbench.tiling_camera())
                        .with_selected_vertex(
                            self.workbench
                                .tiling_selected_vertex()
                                .map(|(_, vertex)| vertex),
                        )
                        .with_construction(self.workbench.tiling_construction().to_vec());
                    if self.workbench.tiling_tool()
                        == crate::workbench::tiling_editor::TilingTool::DrawPolygon
                        && matches!(action, crate::input::MouseAction::Inspect)
                    {
                        if let Some(first) = self.workbench.tiling_construction().first().copied()
                            && self.workbench.tiling_construction().len() >= 3
                        {
                            let (first_x, first_y) = scene.world_to_pixel(
                                first,
                                frame_size[0] as u32,
                                frame_size[1] as u32,
                            );
                            let dx = first_x - px as i32;
                            let dy = first_y - py as i32;
                            if dx * dx + dy * dy <= hit_radius * hit_radius {
                                match self.workbench.finish_tiling_construction() {
                                    Ok(()) => {
                                        self.workbench_notice = Some(
                                            if self
                                                .workbench
                                                .draft()
                                                .tiling
                                                .as_ref()
                                                .is_some_and(|tiling| {
                                                    crate::sim::tiling::validate_coverage(tiling)
                                                        .is_ok()
                                                })
                                            {
                                                "polygon closed · unit cell tiles exactly"
                                            } else {
                                                "polygon closed · unit cell incomplete; add polygons or edit lattice"
                                            }
                                            .into(),
                                        )
                                    }
                                    Err(error) => self.workbench_notice = Some(error),
                                }
                                self.workbench_draft_scene_generation =
                                    self.workbench_draft_scene_generation.wrapping_add(1);
                                return true;
                            }
                        }
                        let point = scene.pixel_to_world(
                            px,
                            py,
                            frame_size[0] as u32,
                            frame_size[1] as u32,
                        );
                        self.workbench_notice = Some(
                            match self.workbench.push_tiling_vertex(point) {
                                Ok(()) => format!(
                                    "vertex {} placed · click the highlighted first point or press Enter to close",
                                    self.workbench.tiling_construction().len()
                                ),
                                Err(error) => format!("vertex rejected: {error}"),
                            },
                        );
                        self.workbench_draft_scene_generation =
                            self.workbench_draft_scene_generation.wrapping_add(1);
                        return true;
                    }
                    if matches!(action, crate::input::MouseAction::Inspect) {
                        if let Some((prototype, vertex)) = scene.hit_test_vertex(
                            px,
                            py,
                            frame_size[0] as u32,
                            frame_size[1] as u32,
                            hit_radius,
                        ) {
                            if let Some(basis) = scene
                                .draft
                                .instances
                                .iter()
                                .find(|instance| instance.prototype == prototype)
                                .map(|instance| instance.id)
                            {
                                let _ = self.workbench.set_selected_basis(basis);
                            }
                            let _ = scene.apply_gesture(
                                crate::workbench::tiling_editor::TilingGesture::SelectVertex {
                                    prototype,
                                    vertex,
                                },
                            );
                            self.workbench.select_tiling_vertex(prototype, vertex);
                            self.workbench_notice = Some(format!(
                                "selected basis {} · vertex {} · drag to move",
                                self.workbench.selected_basis().0,
                                vertex,
                            ));
                            self.workbench_draft_scene_generation =
                                self.workbench_draft_scene_generation.wrapping_add(1);
                            return true;
                        }
                        if let Some(basis) = scene.hit_test_polygon(
                            px,
                            py,
                            frame_size[0] as u32,
                            frame_size[1] as u32,
                        ) && self.workbench.set_selected_basis(basis).is_ok()
                        {
                            self.workbench.clear_tiling_vertex();
                            self.workbench_notice = Some(format!(
                                "selected basis {} · Kernels/Growth now target it",
                                basis.0,
                            ));
                            self.workbench_draft_scene_generation =
                                self.workbench_draft_scene_generation.wrapping_add(1);
                            return true;
                        }
                    }
                    match action {
                        crate::input::MouseAction::Pan { dx, dy } => {
                            scene.pan_pixels(
                                f64::from(dx) * frame_size[0] as f64
                                    / f64::from(viewport.width.max(1)),
                                f64::from(dy) * frame_size[1] as f64
                                    / f64::from(viewport.height.max(1)),
                            );
                            self.workbench.set_tiling_camera(scene.camera);
                            self.workbench_draft_scene_generation =
                                self.workbench_draft_scene_generation.wrapping_add(1);
                            return true;
                        }
                        crate::input::MouseAction::Zoom { direction } => {
                            let factor = match direction {
                                crate::input::ZoomDirection::In => 1.4,
                                crate::input::ZoomDirection::Out => 1.0 / 1.4,
                            };
                            scene.zoom_at(
                                px,
                                py,
                                frame_size[0] as u32,
                                frame_size[1] as u32,
                                factor,
                            );
                            self.workbench.set_tiling_camera(scene.camera);
                            self.workbench_draft_scene_generation =
                                self.workbench_draft_scene_generation.wrapping_add(1);
                            return true;
                        }
                        _ => {}
                    }
                    let applied = match action {
                        crate::input::MouseAction::Inspect => false,
                        crate::input::MouseAction::Paint => self
                            .workbench
                            .tiling_selected_vertex()
                            .or_else(|| {
                                scene.hit_test_vertex(
                                    px,
                                    py,
                                    frame_size[0] as u32,
                                    frame_size[1] as u32,
                                    hit_radius,
                                )
                            })
                            .map(|(prototype, vertex)| {
                                let to = scene.pixel_to_world(px, py, frame_size[0] as u32, frame_size[1] as u32);
                                scene.apply_gesture(crate::workbench::tiling_editor::TilingGesture::MoveVertex { prototype, vertex, to }).is_ok()
                            })
                            .unwrap_or(false),
                        crate::input::MouseAction::Erase => scene
                            .hit_test_vertex(
                                px,
                                py,
                                frame_size[0] as u32,
                                frame_size[1] as u32,
                                hit_radius,
                            )
                            .map(|(prototype, vertex)| scene.apply_gesture(crate::workbench::tiling_editor::TilingGesture::RemoveVertex { prototype, vertex }).is_ok())
                            .unwrap_or(false),
                        crate::input::MouseAction::Pan { .. }
                        | crate::input::MouseAction::Zoom { .. } => unreachable!(),
                    };
                    if applied && scene.draft != *self.workbench.draft().tiling.as_ref().unwrap() {
                        let mut draft = self.workbench.draft().clone();
                        draft.tiling = Some(scene.draft);
                        let _ = if matches!(action, crate::input::MouseAction::Paint) {
                            self.workbench.import_tiling_drag_draft(draft)
                        } else {
                            self.workbench.import_draft(draft)
                        };
                        self.workbench_draft_scene_generation =
                            self.workbench_draft_scene_generation.wrapping_add(1);
                    }
                    return applied;
                }
                crate::workbench::WorkbenchSection::Kernels => {
                    if let (Some(tiling), Some(rule_kernel)) = (
                        self.workbench.draft().tiling.clone(),
                        self.workbench.selected_rule_kernel().cloned(),
                    ) && let crate::sim::ruleset::KernelSpatialDefinition::Periodic(definition) =
                        rule_kernel.spatial
                    {
                        let mut scene = crate::workbench::kernel_editor::PeriodicKernelScene::new(
                            tiling,
                            definition,
                            self.workbench.selected_basis(),
                        )
                        .with_view(self.workbench.kernel_view())
                        .with_selected(self.workbench.periodic_kernel_selection());
                        match action {
                            crate::input::MouseAction::Zoom { direction } => {
                                if self.workbench.kernel_tool()
                                    == crate::workbench::kernel_editor::KernelTool::Weights
                                    && let Some(selection) = scene.selection_in_pixel_rect_for_tool(
                                        pointer.bounds[0],
                                        pointer.bounds[1],
                                        pointer.bounds[2],
                                        pointer.bounds[3],
                                        frame_size[0] as u32,
                                        frame_size[1] as u32,
                                        crate::workbench::kernel_editor::KernelTool::Weights,
                                    )
                                {
                                    let step = if event
                                        .modifiers
                                        .contains(crossterm::event::KeyModifiers::CONTROL)
                                    {
                                        0.5
                                    } else if event
                                        .modifiers
                                        .contains(crossterm::event::KeyModifiers::SHIFT)
                                    {
                                        0.005
                                    } else {
                                        0.05
                                    };
                                    let direction = match direction {
                                        crate::input::ZoomDirection::In => 1.0,
                                        crate::input::ZoomDirection::Out => -1.0,
                                    };
                                    let current =
                                        self.periodic_kernel_value(selection).unwrap_or(0.0);
                                    let next = (current + step * direction).clamp(-1.0, 1.0);
                                    if self.set_periodic_kernel_value(selection, next).is_ok() {
                                        let _ = self.workbench.set_kernel_paint_value(next);
                                        self.workbench_notice = Some(format!(
                                            "offset [{},{}] · basis {} = {:.4} · E exact",
                                            selection.offset[0],
                                            selection.offset[1],
                                            selection.source_basis.0,
                                            next,
                                        ));
                                        return true;
                                    }
                                }
                                let factor = match direction {
                                    crate::input::ZoomDirection::In => 1.4,
                                    crate::input::ZoomDirection::Out => 1.0 / 1.4,
                                };
                                scene.zoom_at(
                                    px,
                                    py,
                                    frame_size[0] as u32,
                                    frame_size[1] as u32,
                                    factor,
                                );
                                self.workbench.set_kernel_view(scene.view);
                                self.workbench_draft_scene_generation =
                                    self.workbench_draft_scene_generation.wrapping_add(1);
                                return true;
                            }
                            crate::input::MouseAction::Pan { dx, dy } => {
                                scene.pan_pixels(
                                    f64::from(dx) * frame_size[0] as f64
                                        / f64::from(viewport.width.max(1)),
                                    f64::from(dy) * frame_size[1] as f64
                                        / f64::from(viewport.height.max(1)),
                                    frame_size[0] as u32,
                                    frame_size[1] as u32,
                                );
                                self.workbench.set_kernel_view(scene.view);
                                self.workbench_draft_scene_generation =
                                    self.workbench_draft_scene_generation.wrapping_add(1);
                                return true;
                            }
                            _ => {}
                        }
                        let Some(selection) = scene.selection_in_pixel_rect_for_tool(
                            pointer.bounds[0],
                            pointer.bounds[1],
                            pointer.bounds[2],
                            pointer.bounds[3],
                            frame_size[0] as u32,
                            frame_size[1] as u32,
                            self.workbench.kernel_tool(),
                        ) else {
                            return false;
                        };
                        match action {
                            crate::input::MouseAction::Inspect => {
                                self.workbench.select_periodic_kernel(selection);
                                self.workbench_notice =
                                    self.periodic_kernel_value(selection).map(|value| {
                                        format!(
                                            "selected offset [{},{}] · source basis {} = {:.6}",
                                            selection.offset[0],
                                            selection.offset[1],
                                            selection.source_basis.0,
                                            value,
                                        )
                                    });
                                return true;
                            }
                            crate::input::MouseAction::Paint => {
                                return match self.workbench.kernel_tool() {
                                    crate::workbench::kernel_editor::KernelTool::Weights => self
                                        .set_periodic_kernel_value(
                                            selection,
                                            self.workbench.kernel_paint_value(),
                                        )
                                        .is_ok(),
                                    crate::workbench::kernel_editor::KernelTool::Support => {
                                        self.set_periodic_kernel_active(selection, true).is_ok()
                                    }
                                };
                            }
                            crate::input::MouseAction::Erase => {
                                return match self.workbench.kernel_tool() {
                                    crate::workbench::kernel_editor::KernelTool::Weights => {
                                        self.set_periodic_kernel_value(selection, 0.0).is_ok()
                                    }
                                    crate::workbench::kernel_editor::KernelTool::Support => {
                                        self.set_periodic_kernel_active(selection, false).is_ok()
                                    }
                                };
                            }
                            crate::input::MouseAction::Pan { .. }
                            | crate::input::MouseAction::Zoom { .. } => unreachable!(),
                        }
                    }
                    let Some(definition) =
                        self.workbench.selected_raster_kernel_definition().cloned()
                    else {
                        return false;
                    };
                    let mut scene = crate::workbench::kernel_editor::KernelScene::new(definition)
                        .with_view(self.workbench.kernel_view())
                        .with_selected(self.workbench.kernel_selection());
                    match action {
                        crate::input::MouseAction::Zoom { direction } => {
                            if let Some(point) = scene.cell_at_pixel_in(
                                px,
                                py,
                                frame_size[0] as u32,
                                frame_size[1] as u32,
                            ) {
                                let step = if event
                                    .modifiers
                                    .contains(crossterm::event::KeyModifiers::CONTROL)
                                {
                                    0.5
                                } else if event
                                    .modifiers
                                    .contains(crossterm::event::KeyModifiers::SHIFT)
                                {
                                    0.005
                                } else {
                                    0.05
                                };
                                let direction = match direction {
                                    crate::input::ZoomDirection::In => 1.0,
                                    crate::input::ZoomDirection::Out => -1.0,
                                };
                                let current = self.kernel_cell_value(point).unwrap_or(0.0);
                                let next = (current + step * direction).clamp(-1.0, 1.0);
                                if self.set_kernel_cell_value(point, next).is_ok() {
                                    let _ = self.workbench.set_kernel_paint_value(next);
                                    self.workbench_notice = Some(format!(
                                        "weight[{},{}] = {:.4} · E exact",
                                        point.x, point.y, next
                                    ));
                                    return true;
                                }
                            }
                            let factor = match direction {
                                crate::input::ZoomDirection::In => 1.4,
                                crate::input::ZoomDirection::Out => 1.0 / 1.4,
                            };
                            scene.zoom_at(
                                px,
                                py,
                                frame_size[0] as u32,
                                frame_size[1] as u32,
                                factor,
                            );
                            self.workbench.set_kernel_view(scene.view);
                            self.workbench_draft_scene_generation =
                                self.workbench_draft_scene_generation.wrapping_add(1);
                            return true;
                        }
                        crate::input::MouseAction::Pan { dx, dy } => {
                            scene.pan_pixels(
                                f64::from(dx) * frame_size[0] as f64
                                    / f64::from(viewport.width.max(1)),
                                f64::from(dy) * frame_size[1] as f64
                                    / f64::from(viewport.height.max(1)),
                                frame_size[0] as u32,
                                frame_size[1] as u32,
                            );
                            self.workbench.set_kernel_view(scene.view);
                            self.workbench_draft_scene_generation =
                                self.workbench_draft_scene_generation.wrapping_add(1);
                            return true;
                        }
                        _ => {}
                    }
                    let Some(point) =
                        scene.cell_at_pixel_in(px, py, frame_size[0] as u32, frame_size[1] as u32)
                    else {
                        return false;
                    };
                    let result = match action {
                        crate::input::MouseAction::Inspect => {
                            self.workbench.select_kernel_point(point);
                            self.workbench_notice = self.kernel_cell_value(point).map(|value| {
                                format!(
                                    "selected weight[{},{}] = {:.6} · wheel adjust · E exact",
                                    point.x, point.y, value
                                )
                            });
                            return true;
                        }
                        crate::input::MouseAction::Paint => scene.apply_gesture(
                            crate::workbench::kernel_editor::KernelGesture::SetValue {
                                x: point.x,
                                y: point.y,
                                value: self.workbench.kernel_paint_value(),
                            },
                        ),
                        crate::input::MouseAction::Erase => scene.apply_gesture(
                            crate::workbench::kernel_editor::KernelGesture::ToggleMask {
                                x: point.x,
                                y: point.y,
                            },
                        ),
                        crate::input::MouseAction::Pan { .. }
                        | crate::input::MouseAction::Zoom { .. } => unreachable!(),
                    };
                    if result.is_ok() {
                        let _ = self
                            .workbench
                            .replace_selected_raster_kernel_definition(scene.definition);
                        self.workbench.select_kernel_point(point);
                        self.workbench_draft_scene_generation =
                            self.workbench_draft_scene_generation.wrapping_add(1);
                    }
                    return result.is_ok();
                }
                crate::workbench::WorkbenchSection::Growth
                | crate::workbench::WorkbenchSection::Channels
                | crate::workbench::WorkbenchSection::Experiment => return true,
                crate::workbench::WorkbenchSection::World => {}
            }
        }
        let frame_size = self
            .frame_size
            .unwrap_or([viewport.width as usize, viewport.height as usize * 2]);
        let scale = [
            frame_size[0] as f32 / viewport.width as f32,
            frame_size[1] as f32 / viewport.height as f32,
        ];
        let screen = [
            (local.column as f32 + 0.5) * scale[0],
            (local.row as f32 + 0.5) * scale[1],
        ];
        let world = self
            .camera
            .screen_to_world(screen, frame_size[0], frame_size[1]);

        match action {
            crate::input::MouseAction::Zoom { direction } => {
                let factor = match direction {
                    crate::input::ZoomDirection::In => 1.2,
                    crate::input::ZoomDirection::Out => 1.0 / 1.2,
                };
                self.camera
                    .zoom_at(screen, frame_size[0], frame_size[1], factor);
            }
            crate::input::MouseAction::Pan { dx, dy } => {
                self.camera.pan_screen([dx * scale[0], dy * scale[1]]);
            }
            crate::input::MouseAction::Inspect => self.inspect_world(world),
            crate::input::MouseAction::Paint | crate::input::MouseAction::Erase => {
                let value = if action == crate::input::MouseAction::Paint {
                    1.0
                } else {
                    0.0
                };
                let from = tracker
                    .stroke_segment()
                    .map(|(from, _)| {
                        let from_screen = [(from.0 + 0.5) * scale[0], (from.1 + 0.5) * scale[1]];
                        self.camera
                            .screen_to_world(from_screen, frame_size[0], frame_size[1])
                    })
                    .unwrap_or(world);
                self.paint_world_segment(from, world, value);
                self.inspect_world(world);
            }
        }
        true
    }
}

#[cfg(test)]
mod remote_snapshot_tests {
    use super::*;
    use crate::sim::rule::SimulationSpec;

    #[test]
    fn remote_snapshot_round_trips_world_cells_and_runtime_state() {
        let mut app = App::new(SimulationSpec::conway(), 2, 2);
        app.world_mut().replace_cells(&[0.1, 0.2, 0.3, 0.4]);
        app.handle_command(Command::TogglePause);
        app.set_rates(12.0, 30.0);
        app.record_step_duration(Duration::from_millis(2));
        let mut snapshot = app.remote_snapshot();
        snapshot.tick = 42;
        snapshot.backend = "NVIDIA test GPU".into();
        snapshot.applied_input_sequence = 19;

        let mut mirror = App::new(SimulationSpec::conway(), 2, 2);
        mirror.set_rates(0.0, 7.0);
        assert!(mirror.apply_remote_snapshot(&snapshot));
        assert_eq!(mirror.world().cells(), &[0.1, 0.2, 0.3, 0.4]);
        assert!(mirror.paused());
        assert_eq!(mirror.tick(), 42);
        assert_eq!(mirror.backend_name(), "NVIDIA test GPU");
        assert!(mirror.is_remote_mirror());
        assert_eq!(mirror.applied_input_sequence(), 19);
        assert_eq!(mirror.performance().last_step_ms, 2.0);
        assert_eq!(mirror.rates(), (12.0, 7.0));
    }

    #[test]
    fn late_remote_experiment_state_does_not_erase_a_dirty_workbench_draft() {
        let mut app = App::new(SimulationSpec::conway(), 8, 8);
        app.enter_workbench();
        app.workbench_mut().add_channel().unwrap();
        assert_eq!(app.workbench().draft().channels.len(), 2);
        let mut remote = crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(8, 8);
        remote.name = "late authoritative state".into();

        app.apply_remote_experiment_state(7, remote.clone());

        assert_eq!(app.active_revision(), 7);
        assert_eq!(app.active_experiment().name, remote.name);
        assert_eq!(app.workbench().draft().channels.len(), 2);
        assert_eq!(
            app.workbench().status(),
            crate::workbench::DraftStatus::Dirty
        );
        let stale = app.workbench_apply_request(99);
        assert_eq!(stale.base_revision, 0);
        let rejected = app
            .submit_draft(stale)
            .expect_err("a dirty draft must retain its original base revision");
        assert_eq!(rejected.diagnostics[0].code, "revision_conflict");
    }

    #[test]
    fn remote_snapshot_applies_authoritative_builtin_rule_metadata() {
        let mut mirror = App::new(SimulationSpec::lenia_orbium(), 2, 2);
        let mut snapshot = mirror.remote_snapshot();
        snapshot.rule = "Conway".into();
        *snapshot.spec = SimulationSpec::conway();
        *snapshot.selected_kernel = definition_from_kernel(&snapshot.spec.kernel);

        assert!(mirror.apply_remote_snapshot(&snapshot));

        assert_eq!(mirror.display_rule_name(), "Conway");
        assert!(matches!(mirror.spec().rule, crate::sim::rule::Rule::Conway));
    }

    #[test]
    fn remote_snapshot_applies_authoritative_kernel_parameter_and_growth_metadata() {
        let mut server = App::new(SimulationSpec::lenia_orbium(), 2, 2);
        server.handle_command(Command::NextKernelParameter);
        server.handle_command(Command::IncreaseKernelParameter);
        assert!(server.set_growth_expression("potential - mu"));
        let snapshot = server.remote_snapshot();

        let mut mirror = App::new(SimulationSpec::lenia_orbium(), 2, 2);
        assert!(mirror.apply_remote_snapshot(&snapshot));

        assert_eq!(mirror.spec(), server.spec());
        assert_eq!(mirror.selected_kernel_name(), server.selected_kernel_name());
        assert_eq!(
            mirror.selected_kernel_parameter(),
            server.selected_kernel_parameter()
        );
        assert_eq!(mirror.expression_buffer(), server.expression_buffer());
    }

    #[test]
    fn remote_snapshot_rejects_dimension_mismatch() {
        let app = App::new(SimulationSpec::conway(), 2, 2);
        let mut snapshot = app.remote_snapshot();
        snapshot.width = 3;
        let mut mirror = App::new(SimulationSpec::conway(), 2, 2);
        assert!(!mirror.apply_remote_snapshot(&snapshot));
    }

    #[test]
    fn remote_optimistic_step_never_executes_the_local_backend() {
        let mut mirror = App::new(SimulationSpec::lenia_orbium(), 8, 8);
        let mut snapshot = mirror.remote_snapshot();
        snapshot.tick = 41;
        snapshot.paused = true;
        assert!(mirror.apply_remote_snapshot(&snapshot));

        mirror.handle_remote_command_optimistically(Command::Step);

        assert_eq!(mirror.tick(), 41);
        assert_eq!(mirror.performance().step_samples, 0);
    }

    #[test]
    fn latest_remote_update_slot_overwrites_stale_snapshots() {
        let slot = LatestRemoteUpdate::default();
        let mut first = App::new(SimulationSpec::conway(), 1, 1).remote_snapshot();
        first.tick = 1;
        let mut latest = first.clone();
        latest.tick = 3;

        slot.store(RemoteUpdate::Snapshot {
            snapshot: first,
            receive_rate: 1.0,
        });
        slot.store(RemoteUpdate::Snapshot {
            snapshot: latest,
            receive_rate: 3.0,
        });

        let RemoteUpdate::Snapshot {
            snapshot,
            receive_rate,
        } = slot.take().expect("latest update")
        else {
            panic!("expected snapshot");
        };
        assert_eq!(snapshot.tick, 3);
        assert_eq!(receive_rate, 3.0);
        assert!(slot.take().is_none());
    }

    #[test]
    fn latest_remote_update_does_not_drop_apply_ack_behind_snapshots() {
        let slot = LatestRemoteUpdate::default();
        let app = App::new(SimulationSpec::conway(), 1, 1);
        let accepted = crate::sim::service::ApplyAccepted {
            request_id: 7,
            revision: 2,
            normalized_experiment: app.active_experiment(),
        };
        let mut snapshot = app.remote_snapshot();
        snapshot.tick = 9;

        slot.store(RemoteUpdate::ApplyAccepted(accepted));
        slot.store(RemoteUpdate::Snapshot {
            snapshot,
            receive_rate: 30.0,
        });

        assert!(matches!(
            slot.take(),
            Some(RemoteUpdate::ApplyAccepted(accepted)) if accepted.request_id == 7
        ));
        assert!(matches!(slot.take(), Some(RemoteUpdate::Snapshot { .. })));
    }

    #[test]
    fn remote_server_processes_at_most_one_simulation_step_between_input_checks() {
        assert_eq!(SERVER_MAX_STEPS_PER_ITERATION, 1);
    }
}

pub struct RateMeter {
    window: Duration,
    events: Vec<Instant>,
    rate: f64,
}

const SERVER_MAX_STEPS_PER_ITERATION: usize = 1;

struct LatestServerSnapshotState {
    snapshot: Option<crate::remote::Snapshot>,
    controls: VecDeque<crate::remote::RemoteMessage>,
    closed: bool,
}

#[derive(Clone)]
struct LatestServerSnapshots {
    state: Arc<(Mutex<LatestServerSnapshotState>, Condvar)>,
}

impl LatestServerSnapshots {
    fn new() -> Self {
        Self {
            state: Arc::new((
                Mutex::new(LatestServerSnapshotState {
                    snapshot: None,
                    controls: VecDeque::new(),
                    closed: false,
                }),
                Condvar::new(),
            )),
        }
    }

    fn store(&self, snapshot: crate::remote::Snapshot) {
        let (lock, wake) = &*self.state;
        if let Ok(mut state) = lock.lock()
            && !state.closed
        {
            state.snapshot = Some(snapshot);
            wake.notify_one();
        }
    }

    fn send(&self, message: crate::remote::RemoteMessage) {
        let (lock, wake) = &*self.state;
        if let Ok(mut state) = lock.lock()
            && !state.closed
        {
            state.controls.push_back(message);
            wake.notify_one();
        }
    }

    fn recv(&self) -> Option<crate::remote::RemoteMessage> {
        let (lock, wake) = &*self.state;
        let mut state = lock.lock().ok()?;
        while state.snapshot.is_none() && state.controls.is_empty() && !state.closed {
            state = wake.wait(state).ok()?;
        }
        state.controls.pop_front().or_else(|| {
            state
                .snapshot
                .take()
                .map(crate::remote::RemoteMessage::Snapshot)
        })
    }

    fn close(&self) {
        let (lock, wake) = &*self.state;
        if let Ok(mut state) = lock.lock() {
            state.closed = true;
            state.snapshot = None;
            wake.notify_all();
        }
    }
}

impl RateMeter {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            events: Vec::new(),
            rate: 0.0,
        }
    }

    pub fn record(&mut self, timestamp: Instant) {
        self.events.push(timestamp);
    }

    pub fn refresh(&mut self, now: Instant) {
        self.events
            .retain(|timestamp| now.duration_since(*timestamp) <= self.window);
        self.rate = self.events.len() as f64 / self.window.as_secs_f64();
    }

    pub fn rate(&self) -> f64 {
        self.rate
    }
}

pub fn run() -> std::io::Result<()> {
    let spec = SimulationSpec::lenia_orbium();
    let backend = SimulationBackend::cuda_or_cpu(spec.clone(), 256, 256);
    let mut app = App::with_backend(spec, 256, 256, backend);
    if let Err(error) = app.enable_default_workspace() {
        app.workbench_notice = Some(format!("workspace restore unavailable: {error}"));
    }
    run_app(app)
}

pub fn run_with_save(path: impl AsRef<Path>) -> std::io::Result<()> {
    let spec = SimulationSpec::lenia_orbium();
    let backend = SimulationBackend::cuda_or_cpu(spec.clone(), 256, 256);
    run_app_with_save(
        App::with_backend(spec, 256, 256, backend),
        Some(path.as_ref()),
    )
}

pub fn run_with_kernel(kernel: KernelDefinition) -> std::io::Result<()> {
    run_app(app_for_kernel(kernel)?)
}

pub fn run_with_kernel_and_save(
    kernel: KernelDefinition,
    path: impl AsRef<Path>,
) -> std::io::Result<()> {
    run_app_with_save(app_for_kernel(kernel)?, Some(path.as_ref()))
}

fn app_for_kernel(kernel: KernelDefinition) -> std::io::Result<App> {
    let mut spec = SimulationSpec::lenia_orbium();
    spec.kernel = Kernel::try_from(kernel.clone())
        .map_err(|error| std::io::Error::new(ErrorKind::InvalidInput, error))?;
    let backend = SimulationBackend::cuda_or_cpu(spec.clone(), 256, 256);
    let mut app = App::with_backend(spec, 256, 256, backend);
    if app.selected_kernel == 2 {
        app.kernel_definitions[2] = kernel;
    } else {
        app.kernel_definitions.push(kernel);
        app.selected_kernel = app.kernel_definitions.len() - 1;
    }
    Ok(app)
}

fn app_for_experiment(file: ExperimentFile) -> Result<App, ExperimentError> {
    let built = file.build()?;
    let mut world = World::new(built.world_size[0], built.world_size[1]);
    world.replace_cells(&built.cells);
    let backend = SimulationBackend::cuda_or_cpu(
        built.spec.clone(),
        built.world_size[0],
        built.world_size[1],
    );
    let mut app = App::with_backend(
        built.spec,
        built.world_size[0],
        built.world_size[1],
        backend,
    );
    app.world = world;
    app.seed = built.seed;
    Ok(app)
}

pub fn run_with_experiment(file: ExperimentFile) -> std::io::Result<()> {
    run_app(
        app_for_experiment(file)
            .map_err(|error| std::io::Error::new(ErrorKind::InvalidInput, error))?,
    )
}

pub fn run_with_experiment_and_save(
    file: ExperimentFile,
    path: impl AsRef<Path>,
) -> std::io::Result<()> {
    run_app_with_save(
        app_for_experiment(file)
            .map_err(|error| std::io::Error::new(ErrorKind::InvalidInput, error))?,
        Some(path.as_ref()),
    )
}

fn run_app(app: App) -> std::io::Result<()> {
    run_app_with_save(app, None)
}

pub fn run_server() -> std::io::Result<()> {
    let spec = SimulationSpec::lenia_orbium();
    let backend = SimulationBackend::cuda_or_cpu(spec.clone(), 256, 256);
    let app = App::with_backend(spec, 256, 256, backend);
    run_server_with_streams(app, std::io::stdin(), std::io::stdout())
}

pub fn run_server_with_streams<R, W>(mut app: App, reader: R, writer: W) -> std::io::Result<()>
where
    R: std::io::Read + Send + 'static,
    W: std::io::Write + Send + 'static,
{
    use crate::remote::{ExpressionKey, InputMessage, RemoteMessage, read_message, write_message};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::sync::mpsc::{self, TryRecvError};

    let (input_tx, input_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = reader;
        loop {
            match read_message(&mut reader) {
                Ok(Some(message)) => {
                    if input_tx.send(message).is_err() {
                        break;
                    }
                }
                Ok(None) => {
                    let _ = input_tx.send(RemoteMessage::Quit);
                    break;
                }
                Err(_) => {
                    let _ = input_tx.send(RemoteMessage::Quit);
                    break;
                }
            }
        }
    });

    let snapshot_tx = LatestServerSnapshots::new();
    let snapshot_rx = snapshot_tx.clone();
    std::thread::spawn(move || {
        let mut writer = writer;
        while let Some(message) = snapshot_rx.recv() {
            if write_message(&mut writer, &message).is_err() {
                break;
            }
        }
    });

    let mut tracker = crate::input::MouseTracker::new();
    let mut simulation_meter = RateMeter::new(Duration::from_secs(1));
    let simulation_interval = Duration::from_secs_f64(1.0 / 30.0);
    let snapshot_interval = Duration::from_secs_f64(1.0 / 30.0);
    let mut simulation_backlog = Duration::ZERO;
    let mut last_iteration = Instant::now();
    let mut last_snapshot = last_iteration - snapshot_interval;
    let mut connected = false;

    loop {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(last_iteration);
        last_iteration = now;

        let mut force_snapshot = false;
        loop {
            match input_rx.try_recv() {
                Ok(RemoteMessage::Hello) => {
                    connected = true;
                    snapshot_tx.send(RemoteMessage::ExperimentState {
                        revision: app.active_revision(),
                        normalized_experiment: app.active_experiment(),
                    });
                    snapshot_tx.store(app.remote_snapshot());
                }
                Ok(RemoteMessage::Viewport {
                    width,
                    height,
                    frame_width,
                    frame_height,
                }) => {
                    let width = width.max(1);
                    let height = height.max(1);
                    let frame_width = usize::try_from(frame_width)
                        .ok()
                        .filter(|value| *value > 0)
                        .unwrap_or(width as usize);
                    let frame_height = usize::try_from(frame_height)
                        .ok()
                        .filter(|value| *value > 0)
                        .unwrap_or(height as usize * 2);
                    app.set_viewport(Rect::new(0, 0, width, height), [frame_width, frame_height]);
                }
                Ok(RemoteMessage::Input { sequence, input }) => {
                    match input {
                        InputMessage::Command(Command::Quit) => {
                            snapshot_tx.close();
                            return Ok(());
                        }
                        InputMessage::Command(command) => app.handle_command(command),
                        InputMessage::ExpressionKey(key) => {
                            if app.expression_editing() {
                                let code = match key {
                                    ExpressionKey::Char(c) => KeyCode::Char(c),
                                    ExpressionKey::Backspace => KeyCode::Backspace,
                                    ExpressionKey::Enter => KeyCode::Enter,
                                    ExpressionKey::Escape => KeyCode::Esc,
                                };
                                app.handle_expression_key(KeyEvent::new(code, KeyModifiers::NONE));
                            }
                        }
                        InputMessage::Mouse(mouse) => {
                            app.handle_mouse(mouse, &mut tracker);
                        }
                    }
                    app.applied_input_sequence = app.applied_input_sequence.max(sequence);
                    force_snapshot = true;
                }
                Ok(RemoteMessage::ApplyDraft(request)) => {
                    match app.submit_draft(request) {
                        Ok(accepted) => snapshot_tx.send(RemoteMessage::ApplyAccepted(accepted)),
                        Err(rejected) => snapshot_tx.send(RemoteMessage::ApplyRejected(rejected)),
                    }
                    force_snapshot = true;
                }
                Ok(RemoteMessage::Quit) => {
                    snapshot_tx.close();
                    return Ok(());
                }
                Ok(RemoteMessage::Snapshot(_)) => {}
                Ok(RemoteMessage::ExperimentState { .. })
                | Ok(RemoteMessage::ApplyAccepted(_))
                | Ok(RemoteMessage::ApplyRejected(_)) => {}
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    snapshot_tx.close();
                    return Ok(());
                }
            }
        }

        if !app.paused() {
            simulation_backlog += elapsed;
            let mut steps = 0;
            while simulation_backlog >= simulation_interval
                && steps < SERVER_MAX_STEPS_PER_ITERATION
            {
                if app.step() {
                    simulation_meter.record(Instant::now());
                    simulation_backlog -= simulation_interval;
                    steps += 1;
                } else {
                    simulation_backlog = Duration::ZERO;
                    break;
                }
            }
            if simulation_backlog > simulation_interval * 8 {
                simulation_backlog = simulation_interval * 8;
            }
        } else {
            simulation_backlog = Duration::ZERO;
        }

        let snapshot_now = Instant::now();
        if connected
            && (force_snapshot || snapshot_now.duration_since(last_snapshot) >= snapshot_interval)
        {
            simulation_meter.refresh(snapshot_now);
            app.set_rates(simulation_meter.rate(), 30.0);
            snapshot_tx.store(app.remote_snapshot());
            last_snapshot = snapshot_now;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

pub fn run_connect(host: &str) -> std::io::Result<()> {
    run_connect_with_command(host, None)
}

pub fn run_connect_with_command(host: &str, ssh_command: Option<&str>) -> std::io::Result<()> {
    let command = ssh_command
        .map(str::to_string)
        .or_else(|| std::env::var("CELLARIUM_SSH_COMMAND").ok())
        .unwrap_or_else(|| "ssh".to_string());
    let mut parts = split_command_line(&command).map_err(std::io::Error::other)?;
    if parts.is_empty() {
        return Err(std::io::Error::new(
            ErrorKind::InvalidInput,
            "CELLARIUM_SSH_COMMAND is empty",
        ));
    }
    let executable = parts.remove(0);
    parts.push(host.to_string());
    parts.push("$HOME/.local/bin/cellarium".to_string());
    parts.push("server".to_string());
    let mut child = std::process::Command::new(executable)
        .args(parts)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!("failed to start SSH connector: {error}"),
            )
        })?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("SSH stdin was not piped"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("SSH stdout was not piped"))?;
    let mut app = App::new(SimulationSpec::lenia_orbium(), 256, 256);
    if let Err(error) = app.enable_default_workspace() {
        app.workbench_notice = Some(format!("workspace restore unavailable: {error}"));
    }
    let result = run_local_remote_viewer(app, stdin, stdout);
    if result.is_ok() {
        let _ = child.kill();
        let _ = child.wait();
        return result;
    }

    match wait_for_child_exit(&mut child, Duration::from_millis(500))? {
        Some(status) if !status.success() => {
            let detail = status
                .code()
                .map(|code| format!("status {code}"))
                .unwrap_or_else(|| status.to_string());
            return Err(std::io::Error::other(format!(
                "SSH connector exited with {detail}"
            )));
        }
        Some(_) => {}
        None => {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    result
}

fn wait_for_child_exit(
    child: &mut std::process::Child,
    timeout: Duration,
) -> std::io::Result<Option<std::process::ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn split_command_line(command: &str) -> Result<Vec<String>, &'static str> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    for character in command.chars() {
        match quote {
            None if character == '\'' || character == '"' => quote = Some(character),
            Some(active) if active == character => quote = None,
            None if character.is_whitespace() => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(character),
        }
    }
    if quote.is_some() {
        return Err("CELLARIUM_SSH_COMMAND has an unterminated quote");
    }
    if !current.is_empty() {
        words.push(current);
    }
    Ok(words)
}

enum RemoteUpdate {
    Snapshot {
        snapshot: crate::remote::Snapshot,
        receive_rate: f64,
    },
    ExperimentState {
        revision: u64,
        experiment: crate::sim::experiment_model::ExperimentSpec,
    },
    ApplyAccepted(crate::sim::service::ApplyAccepted),
    ApplyRejected(crate::sim::service::ApplyRejected),
    Closed(Result<(), crate::remote::ProtocolError>),
}

#[derive(Default)]
struct LatestRemoteUpdateState {
    snapshot: Option<RemoteUpdate>,
    controls: VecDeque<RemoteUpdate>,
}

#[derive(Clone, Default)]
struct LatestRemoteUpdate {
    update: Arc<Mutex<LatestRemoteUpdateState>>,
}

impl LatestRemoteUpdate {
    fn store(&self, update: RemoteUpdate) {
        if let Ok(mut slot) = self.update.lock() {
            match update {
                RemoteUpdate::Snapshot { .. } => slot.snapshot = Some(update),
                update => slot.controls.push_back(update),
            }
        }
    }

    fn take(&self) -> Option<RemoteUpdate> {
        let mut slot = self.update.lock().ok()?;
        slot.controls.pop_front().or_else(|| slot.snapshot.take())
    }
}

fn run_local_remote_viewer<R, W>(mut app: App, stdin: R, stdout: W) -> std::io::Result<()>
where
    R: std::io::Write + Send + 'static,
    W: std::io::Read + Send + 'static,
{
    use crate::remote::{RemoteMessage, read_message, write_message};
    use std::sync::{Arc, Mutex};
    let writer = Arc::new(Mutex::new(stdin));
    {
        let mut guard = writer
            .lock()
            .map_err(|_| std::io::Error::other("SSH writer mutex poisoned"))?;
        write_message(&mut *guard, &RemoteMessage::Hello).map_err(std::io::Error::other)?;
    }
    let snapshot_rx = LatestRemoteUpdate::default();
    let snapshot_tx = snapshot_rx.clone();
    std::thread::spawn(move || {
        let mut stdout = stdout;
        let mut receive_meter = RateMeter::new(Duration::from_secs(1));
        loop {
            match read_message(&mut stdout) {
                Ok(Some(crate::remote::RemoteMessage::Snapshot(snapshot))) => {
                    let received = Instant::now();
                    receive_meter.record(received);
                    receive_meter.refresh(received);
                    snapshot_tx.store(RemoteUpdate::Snapshot {
                        snapshot,
                        receive_rate: receive_meter.rate(),
                    });
                }
                Ok(Some(crate::remote::RemoteMessage::ExperimentState {
                    revision,
                    normalized_experiment,
                })) => snapshot_tx.store(RemoteUpdate::ExperimentState {
                    revision,
                    experiment: normalized_experiment,
                }),
                Ok(Some(crate::remote::RemoteMessage::ApplyAccepted(accepted))) => {
                    snapshot_tx.store(RemoteUpdate::ApplyAccepted(accepted));
                }
                Ok(Some(crate::remote::RemoteMessage::ApplyRejected(rejected))) => {
                    snapshot_tx.store(RemoteUpdate::ApplyRejected(rejected));
                }
                Ok(Some(_)) => {}
                Ok(None) => {
                    snapshot_tx.store(RemoteUpdate::Closed(Ok(())));
                    break;
                }
                Err(error) => {
                    snapshot_tx.store(RemoteUpdate::Closed(Err(error)));
                    break;
                }
            }
        }
    });
    crossterm::terminal::enable_raw_mode()?;
    let _terminal_guard = TerminalGuard;
    let mut terminal_stdout = std::io::stdout();
    crossterm::execute!(
        terminal_stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture,
        crossterm::event::PushKeyboardEnhancementFlags(
            crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
        ),
        crossterm::cursor::Hide
    )?;
    let display = crate::render::display::ViewportDisplay::detect();
    let result = if display.uses_async_output() {
        let redraw_required = Arc::new(AtomicBool::new(false));
        let backend = AsyncTerminalBackend {
            inner: ratatui::backend::CrosstermBackend::new(AsyncTerminalWriter::new()),
            shadow: BTreeMap::new(),
        };
        let mut terminal = ratatui::Terminal::new(backend)?;
        run_remote_loop(
            &mut app,
            &mut terminal,
            display,
            writer,
            snapshot_rx,
            Some(redraw_required),
        )
    } else {
        let backend = ratatui::backend::CrosstermBackend::new(std::io::stdout());
        let mut terminal = ratatui::Terminal::new(backend)?;
        run_remote_loop(&mut app, &mut terminal, display, writer, snapshot_rx, None)
    };
    let _ = app.save_default_workspace_now(false);
    result
}

fn run_remote_loop<B, W>(
    app: &mut App,
    terminal: &mut ratatui::Terminal<B>,
    display: crate::render::display::ViewportDisplay,
    writer: std::sync::Arc<std::sync::Mutex<W>>,
    snapshot_rx: LatestRemoteUpdate,
    redraw_required: Option<Arc<AtomicBool>>,
) -> std::io::Result<()>
where
    B: ratatui::backend::Backend<Error = std::io::Error>,
    W: std::io::Write,
{
    use crate::remote::{RemoteMessage, write_message};
    let mut tracker = crate::input::MouseTracker::new();
    let mut render_meter = RateMeter::new(Duration::from_secs(1));
    let mut graphics_meter = RateMeter::new(Duration::from_secs(1));
    let rasterizer = crate::render::display::AsyncRasterizer::new();
    let mut next_input_sequence = 1_u64;
    // Poll the presentation pipeline at 60 Hz. Raster requests are
    // generation-deduplicated, so this does not rerasterize unchanged
    // snapshots; it only reduces the chance of missing a freshly completed
    // frame between two 30 Hz polls.
    let render_interval = Duration::from_secs_f64(1.0 / 60.0);
    let mut last_render = Instant::now() - render_interval;
    let mut last_viewport = None;
    loop {
        app.autosave_workspace_if_due(Instant::now());
        if let Some(update) = snapshot_rx.take() {
            match update {
                RemoteUpdate::Snapshot {
                    snapshot,
                    receive_rate,
                } => {
                    let previous_ack = app.applied_input_sequence();
                    let _ = app.apply_remote_snapshot(&snapshot);
                    app.snapshot_rate = receive_rate;
                    if app.applied_input_sequence() > previous_ack {
                        last_render = Instant::now() - render_interval;
                    }
                }
                RemoteUpdate::ExperimentState {
                    revision,
                    experiment,
                } => {
                    app.apply_remote_experiment_state(revision, experiment);
                }
                RemoteUpdate::ApplyAccepted(accepted) => {
                    app.accept_remote_apply(accepted);
                    if std::env::var_os("CELLARIUM_E2E_TRACE").is_some() {
                        eprintln!("E2E_APPLY_ACCEPTED");
                    }
                }
                RemoteUpdate::ApplyRejected(rejected) => {
                    let message = rejected
                        .diagnostics
                        .first()
                        .map(|diagnostic| diagnostic.message.clone());
                    app.backend_error = message.clone();
                    app.workbench_notice = message;
                    if std::env::var_os("CELLARIUM_E2E_TRACE").is_some() {
                        eprintln!(
                            "E2E_APPLY_REJECTED {}",
                            app.workbench_notice().unwrap_or("")
                        );
                    }
                }
                RemoteUpdate::Closed(Ok(())) => {
                    return Err(std::io::Error::new(
                        ErrorKind::UnexpectedEof,
                        "SSH connector closed the Cellarium protocol stream",
                    ));
                }
                RemoteUpdate::Closed(Err(error)) => {
                    return Err(std::io::Error::other(format!(
                        "SSH connector protocol failed: {error}"
                    )));
                }
            }
        }
        let now = Instant::now();
        let wait = render_interval
            .saturating_sub(now.duration_since(last_render))
            .min(Duration::from_millis(5));
        if crossterm::event::poll(wait)? {
            // A drag can enqueue dozens of mouse reports before the next
            // render. Read a bounded batch so control keys (especially q/Esc)
            // are not stranded behind one-event-per-frame input handling.
            // The cap preserves fairness with snapshots and rendering under
            // an intentionally noisy terminal.
            for _ in 0..256 {
                if handle_remote_terminal_event(
                    app,
                    &mut tracker,
                    &writer,
                    &mut next_input_sequence,
                    crossterm::event::read()?,
                )? {
                    return Ok(());
                }
                if !crossterm::event::poll(Duration::from_millis(10))? {
                    break;
                }
            }
            last_render = Instant::now() - render_interval;
        }

        let now = Instant::now();
        if now.duration_since(last_render) >= render_interval {
            render_meter.refresh(now);
            graphics_meter.refresh(now);
            app.render_rate = render_meter.rate();
            app.graphics_rate = graphics_meter.rate();
            let viewport = app.viewport_geometry().map(|(_, frame)| frame);
            if viewport != last_viewport {
                if let Some((area, frame_size)) = app.viewport_geometry() {
                    let mut guard = writer
                        .lock()
                        .map_err(|_| std::io::Error::other("SSH writer mutex poisoned"))?;
                    write_message(
                        &mut *guard,
                        &RemoteMessage::Viewport {
                            width: area.width,
                            height: area.height,
                            frame_width: frame_size[0].try_into().unwrap_or(u32::MAX),
                            frame_height: frame_size[1].try_into().unwrap_or(u32::MAX),
                        },
                    )
                    .map_err(std::io::Error::other)?;
                }
                last_viewport = viewport;
            }
            let render_started = Instant::now();
            let mut fresh_graphics = false;
            let render_generation = app.render_generation();
            if redraw_required
                .as_ref()
                .is_some_and(|required| required.swap(false, Ordering::AcqRel))
            {
                terminal.clear()?;
            }
            terminal.draw(|frame| {
                fresh_graphics =
                    crate::tui::draw_remote(frame, app, &display, &rasterizer, render_generation);
            })?;
            let completed = Instant::now();
            app.record_render_duration(completed.duration_since(render_started));
            render_meter.record(completed);
            render_meter.refresh(completed);
            if fresh_graphics {
                graphics_meter.record(completed);
            }
            graphics_meter.refresh(completed);
            app.render_rate = render_meter.rate();
            app.graphics_rate = graphics_meter.rate();
            last_render = now;
        }
    }
}

fn handle_remote_terminal_event<W: std::io::Write>(
    app: &mut App,
    tracker: &mut crate::input::MouseTracker,
    writer: &std::sync::Arc<std::sync::Mutex<W>>,
    next_input_sequence: &mut u64,
    event: Event,
) -> std::io::Result<bool> {
    use crate::remote::{ExpressionKey, InputMessage};
    match event {
        Event::Key(key) => {
            if !is_actionable_key_event(&key) {
                return Ok(false);
            }
            if app.handle_workbench_editor_key(key) {
                return Ok(false);
            }
            if app.expression_editing() {
                let expression_key = match key.code {
                    KeyCode::Char(character) => Some(ExpressionKey::Char(character)),
                    KeyCode::Backspace => Some(ExpressionKey::Backspace),
                    KeyCode::Enter => Some(ExpressionKey::Enter),
                    KeyCode::Esc => Some(ExpressionKey::Escape),
                    _ => None,
                };
                if let Some(expression_key) = expression_key {
                    app.handle_expression_key(key);
                    send_remote_input(
                        writer,
                        next_input_sequence,
                        InputMessage::ExpressionKey(expression_key),
                    )?;
                }
                return Ok(false);
            }
            if app.mode() == AppMode::Workbench
                && app.workbench().section() == crate::workbench::WorkbenchSection::Experiment
                && matches!(
                    key.code,
                    KeyCode::Enter | KeyCode::Char('A') | KeyCode::Char('a')
                )
            {
                let ui_command = UiCommand::ApplyDraft;
                app.workbench_notice = Some("apply sent".into());
                let request = app.workbench_apply_request(*next_input_sequence);
                let mut guard = writer
                    .lock()
                    .map_err(|_| std::io::Error::other("SSH writer mutex poisoned"))?;
                crate::remote::write_message(
                    &mut *guard,
                    &crate::remote::RemoteMessage::ApplyDraft(request),
                )
                .map_err(std::io::Error::other)?;
                if std::env::var_os("CELLARIUM_E2E_TRACE").is_some() {
                    eprintln!("E2E_APPLY_SENT");
                }
                *next_input_sequence = (*next_input_sequence).wrapping_add(1).max(1);
                let _ = ui_command;
                return Ok(false);
            }
            if app.mode() == AppMode::Workbench
                && let Some(ui_command) = crate::input::translate_ui_key(&key)
            {
                if ui_command == UiCommand::ApplyDraft {
                    app.workbench_notice = Some("apply sent".into());
                    let request = app.workbench_apply_request(*next_input_sequence);
                    let mut guard = writer
                        .lock()
                        .map_err(|_| std::io::Error::other("SSH writer mutex poisoned"))?;
                    crate::remote::write_message(
                        &mut *guard,
                        &crate::remote::RemoteMessage::ApplyDraft(request),
                    )
                    .map_err(std::io::Error::other)?;
                    if std::env::var_os("CELLARIUM_E2E_TRACE").is_some() {
                        eprintln!("E2E_APPLY_SENT");
                    }
                    *next_input_sequence = (*next_input_sequence).wrapping_add(1).max(1);
                } else {
                    let _ = app.handle_workbench_ui(ui_command);
                }
                return Ok(false);
            }
            if let Some(command) = crate::input::translate_key(&key) {
                if command == Command::ToggleWorkbench
                    && std::env::var_os("CELLARIUM_E2E_TRACE").is_some()
                {
                    eprintln!("E2E_WORKBENCH_TOGGLE");
                }
                send_remote_input(writer, next_input_sequence, InputMessage::Command(command))?;
                if command == Command::Quit {
                    return Ok(true);
                }
                app.handle_remote_command_optimistically(command);
            }
        }
        Event::Mouse(mouse) => {
            if app.mode() == AppMode::Workbench
                && app.workbench().section() == crate::workbench::WorkbenchSection::Experiment
                && matches!(
                    mouse.kind,
                    crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left)
                )
            {
                let layout = crate::tui::workbench::workbench_layout(app.workbench_area);
                let point = ratatui::layout::Position::new(mouse.column, mouse.row);
                let canvas_content = Rect::new(
                    layout.canvas.x.saturating_add(1),
                    layout.canvas.y.saturating_add(1),
                    layout.canvas.width.saturating_sub(2),
                    layout.canvas.height.saturating_sub(2),
                );
                let canvas_header = Rect::new(
                    canvas_content.x,
                    canvas_content.y,
                    canvas_content.width,
                    canvas_content.height.min(2),
                );
                if canvas_header.contains(point) {
                    let column = point.x.saturating_sub(canvas_header.x);
                    if crate::tui::workbench::toolbar_action_at(app.workbench(), column)
                        == Some(crate::tui::workbench::ToolbarAction::Ui(
                            UiCommand::ApplyDraft,
                        ))
                    {
                        app.workbench.set_focus(WorkbenchFocus::Canvas);
                        app.workbench_notice = Some("apply sent".into());
                        let request = app.workbench_apply_request(*next_input_sequence);
                        let mut guard = writer
                            .lock()
                            .map_err(|_| std::io::Error::other("SSH writer mutex poisoned"))?;
                        crate::remote::write_message(
                            &mut *guard,
                            &crate::remote::RemoteMessage::ApplyDraft(request),
                        )
                        .map_err(std::io::Error::other)?;
                        *next_input_sequence = (*next_input_sequence).wrapping_add(1).max(1);
                        return Ok(false);
                    }
                }
            }
            if app.handle_workbench_panel_mouse(mouse) {
                return Ok(false);
            }
            let in_workbench = app.mode() == AppMode::Workbench;
            let applied = app.handle_mouse(mouse, tracker);
            if in_workbench {
                if std::env::var_os("CELLARIUM_E2E_TRACE").is_some() {
                    eprintln!("E2E_WORKBENCH_MOUSE applied={applied}");
                }
                return Ok(false);
            }
            if crate::input::should_forward_mouse_event(&mouse, applied) {
                if std::env::var_os("CELLARIUM_E2E_TRACE").is_some() {
                    eprintln!("E2E_MOUSE_FORWARDED applied={applied}");
                }
                if let Some((area, _)) = app.viewport_geometry() {
                    let mut local = mouse;
                    local.column = local.column.saturating_sub(area.x);
                    local.row = local.row.saturating_sub(area.y);
                    send_remote_input(writer, next_input_sequence, InputMessage::Mouse(local))?;
                }
            }
        }
        Event::Resize(_, _) | Event::FocusGained | Event::FocusLost | Event::Paste(_) => {}
    }
    Ok(false)
}

fn send_remote_input<W: std::io::Write>(
    writer: &std::sync::Arc<std::sync::Mutex<W>>,
    next_sequence: &mut u64,
    input: crate::remote::InputMessage,
) -> std::io::Result<u64> {
    let sequence = *next_sequence;
    let mut guard = writer
        .lock()
        .map_err(|_| std::io::Error::other("SSH writer mutex poisoned"))?;
    crate::remote::write_message(
        &mut *guard,
        &crate::remote::RemoteMessage::Input { sequence, input },
    )
    .map_err(std::io::Error::other)?;
    *next_sequence = sequence.wrapping_add(1).max(1);
    Ok(sequence)
}

fn is_actionable_key_event(key: &KeyEvent) -> bool {
    key.kind != crossterm::event::KeyEventKind::Release
}

fn run_app_with_save(app: App, save_path: Option<&Path>) -> std::io::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    let _terminal_guard = TerminalGuard;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture,
        crossterm::event::PushKeyboardEnhancementFlags(
            crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
        )
    )?;
    crossterm::execute!(stdout, crossterm::cursor::Hide)?;

    let display = crate::render::display::ViewportDisplay::detect();
    if display.uses_async_output() {
        let redraw_required = Arc::new(AtomicBool::new(false));
        let backend = AsyncTerminalBackend {
            inner: ratatui::backend::CrosstermBackend::new(AsyncTerminalWriter::new()),
            shadow: BTreeMap::new(),
        };
        let mut terminal = ratatui::Terminal::new(backend)?;
        run_loop(
            app,
            &mut terminal,
            display,
            save_path,
            Some(redraw_required),
        )
    } else {
        let backend = ratatui::backend::CrosstermBackend::new(std::io::stdout());
        let mut terminal = ratatui::Terminal::new(backend)?;
        run_loop(app, &mut terminal, display, save_path, None)
    }
}

struct TerminalGuard;

struct LatestFrameState {
    clear_prefix: Vec<u8>,
    frame: Option<Vec<u8>>,
    closed: bool,
}

impl LatestFrameState {
    fn take_output(&mut self) -> Option<Vec<u8>> {
        let frame = self.frame.take();
        if self.clear_prefix.is_empty() {
            return frame;
        }
        let mut output = std::mem::take(&mut self.clear_prefix);
        if let Some(mut frame) = frame {
            output.append(&mut frame);
        }
        Some(output)
    }
}

struct AsyncTerminalWriter {
    state: Arc<(Mutex<LatestFrameState>, Condvar)>,
    pending: Vec<u8>,
    worker: Option<JoinHandle<()>>,
}

struct AsyncTerminalBackend {
    inner: ratatui::backend::CrosstermBackend<AsyncTerminalWriter>,
    shadow: BTreeMap<(u16, u16), ratatui::buffer::Cell>,
}

impl ratatui::backend::Backend for AsyncTerminalBackend {
    type Error = std::io::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a ratatui::buffer::Cell)>,
    {
        for (x, y, cell) in content {
            self.shadow.insert((y, x), cell.clone());
        }
        self.inner
            .draw(self.shadow.iter().map(|(&(y, x), cell)| (x, y, cell)))
    }

    fn append_lines(&mut self, n: u16) -> Result<(), Self::Error> {
        self.inner.append_lines(n)
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> Result<ratatui::layout::Position, Self::Error> {
        Ok(ratatui::layout::Position::ORIGIN)
    }

    fn set_cursor_position<P: Into<ratatui::layout::Position>>(
        &mut self,
        position: P,
    ) -> Result<(), Self::Error> {
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.shadow.clear();
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ratatui::backend::ClearType) -> Result<(), Self::Error> {
        self.shadow.clear();
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> Result<ratatui::layout::Size, Self::Error> {
        self.inner.size()
    }

    fn window_size(&mut self) -> Result<ratatui::backend::WindowSize, Self::Error> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        ratatui::backend::Backend::flush(&mut self.inner)
    }
}

impl AsyncTerminalWriter {
    fn new() -> Self {
        let state = Arc::new((
            Mutex::new(LatestFrameState {
                clear_prefix: Vec::new(),
                frame: None,
                closed: false,
            }),
            Condvar::new(),
        ));
        let worker_state = Arc::clone(&state);
        let worker = std::thread::spawn(move || {
            let mut stdout = std::io::stdout();
            loop {
                let frame = {
                    let (lock, wake) = &*worker_state;
                    let mut state = lock.lock().expect("async terminal writer mutex poisoned");
                    while state.frame.is_none() && state.clear_prefix.is_empty() && !state.closed {
                        state = wake
                            .wait(state)
                            .expect("async terminal writer mutex poisoned");
                    }
                    match state.take_output() {
                        Some(frame) => Some(frame),
                        None if state.closed => None,
                        None => continue,
                    }
                };
                let Some(frame) = frame else {
                    break;
                };
                if stdout
                    .write_all(&frame)
                    .and_then(|_| stdout.flush())
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            state,
            pending: Vec::new(),
            worker: Some(worker),
        }
    }
}

impl Write for AsyncTerminalWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.pending.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let (lock, wake) = &*self.state;
        let mut state = lock
            .lock()
            .map_err(|_| std::io::Error::other("async terminal writer mutex poisoned"))?;
        let next = std::mem::take(&mut self.pending);
        if contains_terminal_clear(&next) {
            state.clear_prefix = next;
            if !state.frame.as_deref().is_some_and(contains_kitty_transmit) {
                state.frame = None;
            }
        } else {
            let previous_is_kitty = state.frame.as_deref().is_some_and(contains_kitty_transmit);
            if contains_kitty_transmit(&next) || !previous_is_kitty {
                state.frame = Some(next);
                wake.notify_one();
            }
        }
        Ok(())
    }
}

fn contains_terminal_clear(bytes: &[u8]) -> bool {
    bytes.windows(4).any(|window| window == b"\x1b[2J")
}

fn contains_kitty_transmit(bytes: &[u8]) -> bool {
    bytes.windows(8).any(|window| window == b"_Gq=2,i=")
}

impl Drop for AsyncTerminalWriter {
    fn drop(&mut self) {
        let _ = self.flush();
        let (lock, wake) = &*self.state;
        if let Ok(mut state) = lock.lock() {
            state.closed = true;
            wake.notify_one();
        }
        // A graphics frame can be much larger than the PTY drain rate. Do not
        // join the writer here: joining would wait forever while the terminal
        // is still flushing an obsolete frame and would make `q` appear dead.
        // Dropping the handle detaches the worker; process teardown closes its
        // stdout and wakes the blocked write naturally.
        let _ = self.worker.take();
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut stdout = std::io::stdout();
        let _ = crossterm::execute!(stdout, crossterm::cursor::Show);
        let _ = crossterm::execute!(
            stdout,
            crossterm::event::PopKeyboardEnhancementFlags,
            crossterm::event::DisableMouseCapture,
            crossterm::terminal::LeaveAlternateScreen
        );
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

fn run_loop<B: ratatui::backend::Backend<Error = std::io::Error>>(
    mut app: App,
    terminal: &mut ratatui::Terminal<B>,
    display: crate::render::display::ViewportDisplay,
    save_path: Option<&Path>,
    redraw_required: Option<Arc<AtomicBool>>,
) -> std::io::Result<()> {
    let mut tracker = crate::input::MouseTracker::new();
    let mut simulation_meter = RateMeter::new(Duration::from_secs(1));
    let mut render_meter = RateMeter::new(Duration::from_secs(1));
    let simulation_interval = Duration::from_secs_f64(1.0 / 30.0);
    let render_interval = Duration::from_secs_f64(1.0 / 30.0);
    let mut simulation_backlog = Duration::ZERO;
    let mut last_iteration = Instant::now();
    let mut last_render = last_iteration;
    let (event_tx, event_rx) = std::sync::mpsc::sync_channel::<Event>(4096);
    std::thread::spawn(move || {
        while let Ok(event) = crossterm::event::read() {
            if event_tx.send(event).is_err() {
                break;
            }
        }
    });

    loop {
        let now = Instant::now();
        app.autosave_workspace_if_due(now);
        let elapsed = now - last_iteration;
        last_iteration = now;

        let wait = render_interval
            .saturating_sub(now.duration_since(last_render))
            .min(Duration::from_millis(5));
        let mut quit_requested = false;
        let mut input_seen = false;
        let initially_paused = app.paused();
        let mut deferred_step = false;
        let mut pause_command_seen = false;
        let mut events = Vec::with_capacity(256);
        if let Ok(event) = event_rx.recv_timeout(wait) {
            events.push(event);
            events.extend(event_rx.try_iter().take(255));
        }
        for event in events {
            match event {
                Event::Key(key) => {
                    if !is_actionable_key_event(&key) {
                        continue;
                    }
                    input_seen = true;
                    if app.handle_workbench_editor_key(key) {
                        continue;
                    }
                    if app.expression_editing() {
                        app.handle_expression_key(key);
                        continue;
                    }
                    if app.mode() == AppMode::Workbench
                        && app.workbench().section()
                            == crate::workbench::WorkbenchSection::Experiment
                        && matches!(
                            key.code,
                            KeyCode::Enter | KeyCode::Char('A') | KeyCode::Char('a')
                        )
                    {
                        let _ = app.handle_workbench_ui(UiCommand::ApplyDraft);
                        continue;
                    }
                    if app.mode() == AppMode::Workbench
                        && let Some(ui_command) = crate::input::translate_ui_key(&key)
                    {
                        let _ = app.handle_workbench_ui(ui_command);
                        continue;
                    }
                    if let Some(command) = crate::input::translate_key(&key) {
                        if command == Command::Quit {
                            quit_requested = true;
                            break;
                        }
                        if command == Command::Step {
                            deferred_step = true;
                            continue;
                        }
                        if command == Command::TogglePause {
                            pause_command_seen = true;
                        }
                        app.handle_command(command);
                    }
                }
                Event::Mouse(mouse) => {
                    input_seen = true;
                    if app.handle_workbench_panel_mouse(mouse) {
                        continue;
                    }
                    app.handle_mouse(mouse, &mut tracker);
                }
                Event::Resize(_, _) | Event::FocusGained | Event::FocusLost => {}
                Event::Paste(_) => {}
            }
            if quit_requested {
                break;
            }
        }
        if deferred_step && (!app.paused() || (initially_paused && !pause_command_seen)) {
            app.handle_command(Command::Step);
        }
        if quit_requested {
            let _ = app.save_default_workspace_now(false);
            if let Some(path) = save_path {
                app.save_experiment(path).map_err(std::io::Error::other)?;
            }
            break;
        }

        if input_seen {
            app.autosave_workspace_if_due(Instant::now());
            // Avoid blocking on a large synchronous terminal diff while a
            // drag/key burst is still arriving. The next iteration drains
            // more input before the next presentation.
            continue;
        }

        if app.paused() {
            simulation_backlog = Duration::ZERO;
        } else {
            simulation_backlog += elapsed;
            let mut steps = 0;
            while simulation_backlog >= simulation_interval && steps < 8 {
                if app.step() {
                    simulation_meter.record(now);
                    simulation_backlog -= simulation_interval;
                    steps += 1;
                } else {
                    simulation_backlog = Duration::ZERO;
                    break;
                }
            }
            if simulation_backlog > simulation_interval * 8 {
                simulation_backlog = simulation_interval * 8;
            }
        }

        if now.duration_since(last_render) >= render_interval {
            simulation_meter.refresh(now);
            render_meter.record(now);
            render_meter.refresh(now);
            let rates = (simulation_meter.rate(), render_meter.rate());
            let render_started = Instant::now();
            if redraw_required
                .as_ref()
                .is_some_and(|required| required.swap(false, Ordering::AcqRel))
            {
                terminal.clear()?;
            }
            terminal.draw(|frame| {
                app.set_rates(rates.0, rates.1);
                let _ = crate::tui::draw(frame, &mut app, &display);
            })?;
            app.record_render_duration(render_started.elapsed());
            last_render = now;
        }
    }
    Ok(())
}

fn record_duration(duration: Duration, last: &mut f64, average: &mut f64, samples: &mut u64) {
    let millis = duration.as_secs_f64() * 1_000.0;
    *last = millis;
    *average = (*average * *samples as f64 + millis) / (*samples as f64 + 1.0);
    *samples = samples.saturating_add(1);
}

fn initial_density(spec: &SimulationSpec) -> f64 {
    match spec.rule {
        crate::sim::rule::Rule::Conway => 0.35,
        crate::sim::rule::Rule::Lenia { .. } => 0.25,
        crate::sim::rule::Rule::Program(_) => 0.25,
    }
}

fn kernel_catalog(spec: &SimulationSpec) -> (Vec<KernelDefinition>, usize) {
    let ring = ring_definition(13, 0.5, 0.5);
    let render = render_definition(ring.width, ring.height);
    let mut definitions = vec![ring, render];
    let selected = if spec.kernel.name == "render" {
        1
    } else if spec.kernel.name == "ring" || spec.kernel.name == "none" {
        0
    } else {
        definitions.push(definition_from_kernel(&spec.kernel));
        2
    };
    (definitions, selected)
}

fn definition_from_kernel(kernel: &Kernel) -> KernelDefinition {
    KernelDefinition {
        name: kernel.name.clone(),
        width: kernel.width,
        height: kernel.height,
        anchor_x: kernel.anchor_x,
        anchor_y: kernel.anchor_y,
        mask: kernel.mask.clone(),
        normalization: kernel.normalization,
        parameters: kernel.parameters.clone(),
        values: KernelValues::Explicit(kernel.values.clone()),
    }
}

fn definition_radius(definition: &KernelDefinition) -> usize {
    if definition.width == 0
        || definition.height == 0
        || definition.anchor_x >= definition.width
        || definition.anchor_y >= definition.height
    {
        return 0;
    }
    let Some(mask) = definition.mask.as_deref() else {
        return (definition.width - 1 - definition.anchor_x)
            .max(definition.anchor_x)
            .max(definition.height - 1 - definition.anchor_y)
            .max(definition.anchor_y);
    };
    if mask.len() != definition.width * definition.height {
        return 0;
    }
    mask.iter()
        .enumerate()
        .filter(|(_, active)| **active)
        .map(|(index, _)| {
            let x = index % definition.width;
            let y = index / definition.width;
            (x as isize - definition.anchor_x as isize)
                .abs()
                .max((y as isize - definition.anchor_y as isize).abs())
        })
        .max()
        .unwrap_or(0) as usize
}

pub fn rule_name(spec: &SimulationSpec) -> &'static str {
    match spec.rule {
        crate::sim::rule::Rule::Conway => "Conway",
        crate::sim::rule::Rule::Lenia { .. } => "Lenia/Orbium",
        crate::sim::rule::Rule::Program(_) => "Custom program",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::kernel::{KernelValues, Normalization};

    #[test]
    fn key_release_is_not_actionable_terminal_input() {
        let release = KeyEvent::new_with_kind(
            KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
            crossterm::event::KeyEventKind::Release,
        );
        let press = KeyEvent::new_with_kind(
            KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
            crossterm::event::KeyEventKind::Press,
        );

        assert!(!is_actionable_key_event(&release));
        assert!(is_actionable_key_event(&press));
    }

    #[test]
    fn async_terminal_backend_forwards_clear_to_the_terminal_writer() {
        let state = Arc::new((
            Mutex::new(LatestFrameState {
                clear_prefix: Vec::new(),
                frame: None,
                closed: true,
            }),
            Condvar::new(),
        ));
        let writer = AsyncTerminalWriter {
            state: Arc::clone(&state),
            pending: Vec::new(),
            worker: None,
        };
        let mut backend = AsyncTerminalBackend {
            inner: ratatui::backend::CrosstermBackend::new(writer),
            shadow: BTreeMap::new(),
        };

        ratatui::backend::Backend::clear(&mut backend).unwrap();
        ratatui::backend::Backend::set_cursor_position(
            &mut backend,
            ratatui::layout::Position::new(3, 4),
        )
        .unwrap();

        let emitted = state.0.lock().unwrap().take_output().unwrap_or_default();
        assert!(emitted.windows(4).any(|bytes| bytes == b"\x1b[2J"));
        assert!(emitted.windows(6).any(|bytes| bytes == b"\x1b[5;4H"));
    }

    #[test]
    fn async_terminal_backend_replays_static_cells_in_later_frames() {
        let state = Arc::new((
            Mutex::new(LatestFrameState {
                clear_prefix: Vec::new(),
                frame: None,
                closed: true,
            }),
            Condvar::new(),
        ));
        let writer = AsyncTerminalWriter {
            state: Arc::clone(&state),
            pending: Vec::new(),
            worker: None,
        };
        let mut backend = AsyncTerminalBackend {
            inner: ratatui::backend::CrosstermBackend::new(writer),
            shadow: BTreeMap::new(),
        };
        let static_cell = ratatui::buffer::Cell::new("A");
        ratatui::backend::Backend::draw(&mut backend, [(0, 0, &static_cell)].into_iter()).unwrap();
        ratatui::backend::Backend::flush(&mut backend).unwrap();
        let _ = state.0.lock().unwrap().take_output();

        let changed_cell = ratatui::buffer::Cell::new("B");
        ratatui::backend::Backend::draw(&mut backend, [(1, 0, &changed_cell)].into_iter()).unwrap();
        ratatui::backend::Backend::flush(&mut backend).unwrap();
        let emitted = state.0.lock().unwrap().take_output().unwrap_or_default();

        assert!(emitted.contains(&b'A'));
        assert!(emitted.contains(&b'B'));
    }

    #[test]
    fn async_terminal_backend_forgets_static_cells_after_clear() {
        let state = Arc::new((
            Mutex::new(LatestFrameState {
                clear_prefix: Vec::new(),
                frame: None,
                closed: true,
            }),
            Condvar::new(),
        ));
        let writer = AsyncTerminalWriter {
            state: Arc::clone(&state),
            pending: Vec::new(),
            worker: None,
        };
        let mut backend = AsyncTerminalBackend {
            inner: ratatui::backend::CrosstermBackend::new(writer),
            shadow: BTreeMap::new(),
        };
        let stale_cell = ratatui::buffer::Cell::new("A");
        ratatui::backend::Backend::draw(&mut backend, [(0, 0, &stale_cell)].into_iter()).unwrap();
        ratatui::backend::Backend::flush(&mut backend).unwrap();
        let _ = state.0.lock().unwrap().take_output();

        ratatui::backend::Backend::clear(&mut backend).unwrap();
        let new_cell = ratatui::buffer::Cell::new("B");
        ratatui::backend::Backend::draw(&mut backend, [(1, 0, &new_cell)].into_iter()).unwrap();
        ratatui::backend::Backend::flush(&mut backend).unwrap();
        let emitted = state.0.lock().unwrap().take_output().unwrap_or_default();

        assert!(!emitted.contains(&b'A'));
        assert!(emitted.contains(&b'B'));
    }

    #[test]
    fn async_terminal_writer_does_not_drop_a_pending_kitty_transmission() {
        let state = Arc::new((
            Mutex::new(LatestFrameState {
                clear_prefix: Vec::new(),
                frame: None,
                closed: true,
            }),
            Condvar::new(),
        ));
        let mut writer = AsyncTerminalWriter {
            state: Arc::clone(&state),
            pending: Vec::new(),
            worker: None,
        };

        writer
            .write_all(b"\x1b_Gq=2,i=41,a=T;pixels\x1b\\")
            .unwrap();
        writer.flush().unwrap();
        writer.write_all(b"\x1b[2J").unwrap();
        writer.flush().unwrap();
        writer.write_all(b"ordinary-later-frame").unwrap();
        writer.flush().unwrap();

        let emitted = state.0.lock().unwrap().take_output().unwrap_or_default();
        assert!(contains_terminal_clear(&emitted));
        assert!(contains_kitty_transmit(&emitted));
        assert!(
            !emitted
                .windows(b"ordinary-later-frame".len())
                .any(|bytes| bytes == b"ordinary-later-frame")
        );
    }

    #[cfg(feature = "cuda")]
    fn cuda_available() -> bool {
        crate::sim::cuda::CudaBackend::new(SimulationSpec::conway(), 1, 1).is_ok()
    }

    #[cfg(not(feature = "cuda"))]
    fn cuda_available() -> bool {
        false
    }

    fn custom_definition() -> KernelDefinition {
        KernelDefinition {
            name: "custom".to_string(),
            width: 2,
            height: 1,
            anchor_x: 1,
            anchor_y: 0,
            mask: None,
            normalization: Normalization::None,
            parameters: std::collections::BTreeMap::from([
                ("zeta".to_string(), 4.0),
                ("alpha".to_string(), 2.0),
            ]),
            values: KernelValues::Explicit(vec![2.0, 4.0]),
        }
    }

    #[test]
    fn classic_lenia_still_starts_as_one_channel() {
        let app = App::new(SimulationSpec::lenia_orbium(), 32, 32);
        assert_eq!(app.channel_count(), 1);
        assert_eq!(app.active_revision(), 0);
    }

    #[test]
    fn app_apply_rejection_does_not_advance_revision() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 2, 2);
        let mut invalid = app.active_experiment();
        invalid.channels[0].initial[0] = f32::NAN;
        let result = app.submit_draft(crate::sim::service::ApplyRequest {
            request_id: 1,
            base_revision: 0,
            draft: invalid,
        });
        assert!(result.is_err());
        assert_eq!(app.active_revision(), 0);
    }

    #[test]
    fn app_accepts_a_valid_draft_and_rejects_stale_revision() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 2, 2);
        let draft = app.active_experiment();
        let accepted = app
            .submit_draft(crate::sim::service::ApplyRequest {
                request_id: 2,
                base_revision: 0,
                draft,
            })
            .unwrap();
        assert_eq!(accepted.revision, 1);
        let stale_draft = app.active_experiment();
        let stale = app.submit_draft(crate::sim::service::ApplyRequest {
            request_id: 3,
            base_revision: 0,
            draft: stale_draft,
        });
        assert!(
            stale
                .unwrap_err()
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "revision_conflict")
        );
    }

    #[test]
    fn successful_apply_is_apply_and_run_not_apply_and_stay_paused() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 2, 2);
        app.paused = true;
        app.enter_workbench();

        app.handle_workbench_ui(UiCommand::ApplyDraft).unwrap();

        assert_eq!(app.mode(), AppMode::Simulation);
        assert!(!app.paused());
        assert_eq!(app.active_revision(), 1);
        assert_eq!(app.workbench_notice(), Some("running revision 1"));
    }

    #[test]
    fn remote_apply_acceptance_switches_to_simulation_only_after_ack() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 2, 2);
        app.enter_workbench();
        let normalized = app.workbench().draft().clone();

        app.accept_remote_apply(crate::sim::service::ApplyAccepted {
            request_id: 7,
            revision: 3,
            normalized_experiment: normalized,
        });

        assert_eq!(app.mode(), AppMode::Simulation);
        assert_eq!(app.active_revision(), 3);
        assert_eq!(app.workbench_notice(), Some("running revision 3"));
    }

    #[test]
    fn default_workspace_autosaves_restores_and_writes_a_runnable_experiment() {
        let directory = std::env::temp_dir().join(format!(
            "cellarium-app-workspace-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let paths = crate::workbench::WorkspacePaths::in_directory(&directory);
        let mut app = App::new(SimulationSpec::lenia_orbium(), 4, 4);
        app.enable_workspace(paths.clone()).unwrap();
        app.enter_workbench();
        app.workbench_mut().add_channel().unwrap();

        app.autosave_workspace_if_due(Instant::now() + Duration::from_secs(1));

        let saved = crate::workbench::load_workspace(&paths.workbench).unwrap();
        assert_eq!(saved.draft.channels.len(), 2);
        let mut restored = App::new(SimulationSpec::lenia_orbium(), 4, 4);
        restored.enable_workspace(paths.clone()).unwrap();
        assert_eq!(restored.workbench().draft().channels.len(), 2);

        let restored_dt = restored.workbench().draft().simulation_dt;
        let remote_authoritative = ExperimentSpec::single_channel_lenia(4, 4);
        restored.apply_remote_experiment_state(9, remote_authoritative.clone());
        assert_eq!(restored.workbench().draft().simulation_dt, restored_dt);
        assert_eq!(
            restored.workbench().draft().channels.len(),
            2,
            "the first remote ExperimentState must not erase a restored local draft"
        );
        assert_eq!(restored.workbench().authoritative(), &remote_authoritative);
        assert_eq!(restored.workbench_base_revision, 9);

        restored.enter_workbench();
        restored.handle_workbench_ui(UiCommand::ApplyDraft).unwrap();
        let runnable = crate::sim::experiment::load_experiment_model(&paths.experiment).unwrap();
        assert_eq!(runnable.channels.len(), 2);
        assert_eq!(restored.mode(), AppMode::Simulation);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn experiment_load_restores_seed_dimensions_and_cells() {
        let mut world = World::new(3, 2);
        world.replace_cells(&[0.0, 0.25, 0.5, 0.75, 1.0, 0.125]);
        let mut file = crate::sim::experiment::ExperimentFile::from_parts(
            crate::sim::experiment::ExperimentMetadata {
                name: "fixture".to_string(),
                ..Default::default()
            },
            SimulationSpec::conway(),
            &world,
            77,
        )
        .unwrap();
        file.cells[0] = 0.9;

        let app = app_for_experiment(file).unwrap();

        assert_eq!((app.world().width(), app.world().height()), (3, 2));
        assert_eq!(app.world().cells()[0], 0.9);
        assert_eq!(app.seed(), 77);
    }

    #[test]
    fn panel_navigation_cycles_editor_contexts() {
        let mut app = App::new(SimulationSpec::conway(), 4, 4);
        assert_eq!(app.active_panel(), Panel::Overview);

        app.handle_command(Command::NextPanel);
        assert_eq!(app.active_panel(), Panel::Rule);
        app.handle_command(Command::NextPanel);
        assert_eq!(app.active_panel(), Panel::Kernel);
        app.handle_command(Command::NextPanel);
        assert_eq!(app.active_panel(), Panel::Topology);
        app.handle_command(Command::NextPanel);
        assert_eq!(app.active_panel(), Panel::Errors);
        app.handle_command(Command::NextPanel);
        assert_eq!(app.active_panel(), Panel::Overview);
    }

    #[test]
    fn expression_editor_commits_valid_input_and_keeps_invalid_input_editable() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 4, 3);
        app.handle_command(Command::ToggleExpressionEditor);
        assert!(app.expression_editing());
        app.replace_expression_buffer("0.5");
        app.handle_expression_key(KeyEvent::new(
            KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert!(!app.expression_editing());

        app.handle_command(Command::ToggleExpressionEditor);
        app.replace_expression_buffer("unknown");
        app.handle_expression_key(KeyEvent::new(
            KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert!(app.expression_editing());
        assert!(
            app.backend_error()
                .is_some_and(|error| error.contains("unknown"))
        );
        app.handle_expression_key(KeyEvent::new(
            KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert!(!app.expression_editing());
    }

    #[test]
    fn performance_stats_track_step_and_render_averages() {
        let mut app = App::new(SimulationSpec::conway(), 2, 2);
        app.record_step_duration(Duration::from_millis(2));
        app.record_step_duration(Duration::from_millis(4));
        app.record_render_duration(Duration::from_millis(1));

        let stats = app.performance();
        assert_eq!(stats.step_samples, 2);
        assert_eq!(stats.last_step_ms, 4.0);
        assert_eq!(stats.average_step_ms, 3.0);
        assert_eq!(stats.render_samples, 1);
        assert_eq!(stats.last_render_ms, 1.0);
    }

    #[test]
    fn app_saves_a_reproducible_experiment_file() {
        let mut app = App::new(SimulationSpec::conway(), 2, 2);
        app.world_mut().replace_cells(&[0.1, 0.2, 0.3, 0.4]);
        let path = std::env::temp_dir().join(format!(
            "cellarium-app-experiment-{}.ron",
            std::process::id()
        ));

        app.save_experiment(&path).unwrap();
        let loaded = crate::sim::experiment::load_experiment(&path).unwrap();
        let _ = std::fs::remove_file(path);

        assert_eq!(loaded.world_size, [2, 2]);
        assert_eq!(loaded.cells, vec![0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn kernel_file_custom_definition_becomes_the_active_selected_kernel() {
        let app = app_for_kernel(KernelDefinition {
            name: "custom".to_string(),
            width: 2,
            height: 1,
            anchor_x: 1,
            anchor_y: 0,
            mask: None,
            normalization: Normalization::None,
            parameters: Default::default(),
            values: KernelValues::Explicit(vec![2.0, 4.0]),
        })
        .unwrap();

        assert_eq!(app.spec().kernel.name, "custom");
        assert_eq!(app.spec().kernel.width, 2);
        assert_eq!(app.spec().kernel.height, 1);
        assert_eq!(app.spec().kernel.values, vec![2.0, 4.0]);
    }

    #[test]
    fn kernel_catalog_cycles_presets_and_preserves_custom() {
        let mut app = app_for_kernel(custom_definition()).unwrap();

        assert_eq!(app.kernel_definitions.len(), 3);
        assert_eq!(app.selected_kernel_name(), "custom");
        assert_eq!(app.selected_kernel_dimensions(), (2, 1));
        assert_eq!(app.selected_kernel_anchor(), (1, 0));
        assert_eq!(app.selected_kernel_radius(), 1);
        assert_eq!(app.selected_kernel_normalization(), Normalization::None);
        assert_eq!(app.selected_kernel_parameter(), None);

        app.handle_command(Command::NextKernel);
        assert_eq!(app.selected_kernel_name(), "ring");
        app.handle_command(Command::NextKernel);
        assert_eq!(app.selected_kernel_name(), "render");
        app.handle_command(Command::NextKernel);
        assert_eq!(app.selected_kernel_name(), "custom");
    }

    #[test]
    fn kernel_parameters_cycle_in_sorted_name_order() {
        let mut app = app_for_kernel(custom_definition()).unwrap();

        app.handle_command(Command::NextKernelParameter);
        assert_eq!(
            app.selected_kernel_parameter(),
            Some(("alpha".to_string(), 2.0))
        );
        app.handle_command(Command::NextKernelParameter);
        assert_eq!(
            app.selected_kernel_parameter(),
            Some(("zeta".to_string(), 4.0))
        );
        app.handle_command(Command::NextKernelParameter);
        assert_eq!(
            app.selected_kernel_parameter(),
            Some(("alpha".to_string(), 2.0))
        );
    }

    #[test]
    fn parameter_edits_change_the_selected_definition_only() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 8, 8);
        app.handle_command(Command::NextKernelParameter);

        app.handle_command(Command::IncreaseKernelParameter);
        app.handle_command(Command::IncreaseKernelParameter);
        app.handle_command(Command::DecreaseKernelParameter);

        assert_eq!(
            app.selected_kernel_parameter(),
            Some(("center".to_string(), 0.51))
        );
        assert_eq!(app.spec().kernel.parameters["center"], 0.5);
    }

    #[test]
    fn non_finite_parameter_results_are_rejected() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 8, 8);
        app.selected_parameter = Some("width".to_string());
        app.kernel_definitions[0]
            .parameters
            .insert("width".to_string(), f32::NAN);

        app.handle_command(Command::DecreaseKernelParameter);

        assert!(
            app.selected_kernel_parameter()
                .is_some_and(|(_, value)| value.is_nan())
        );
        assert!(
            app.backend_error()
                .is_some_and(|error| error.contains("finite"))
        );
    }

    #[test]
    fn keyboard_commands_control_pause_step_and_world() {
        let mut app = App::new(SimulationSpec::conway(), 8, 8);
        app.world_mut().set(3, 3, 1.0);
        app.world_mut().set(3, 4, 1.0);
        app.world_mut().set(3, 5, 1.0);

        app.handle_command(Command::TogglePause);
        assert!(app.paused());
        app.handle_command(Command::Step);
        assert_eq!(app.tick(), 1);
        assert!(app.paused());

        app.handle_command(Command::Clear);
        assert!(app.world().cells().iter().all(|value| *value == 0.0));
        app.handle_command(Command::Randomize);
        assert!(app.world().cells().iter().any(|value| *value > 0.0));

        let before = app.world().cells().to_vec();
        app.handle_command(Command::Reset);
        assert_eq!(app.world().cells(), before);
        assert_eq!(app.tick(), 0);
    }

    #[test]
    fn randomize_is_reproducible_and_reset_restores_seed() {
        let mut first = App::new(SimulationSpec::conway(), 8, 8);
        let mut second = App::new(SimulationSpec::conway(), 8, 8);
        first.handle_command(Command::Randomize);
        second.handle_command(Command::Randomize);
        assert_eq!(first.world().cells(), second.world().cells());

        first.handle_command(Command::Reset);
        assert_eq!(first.world().cells(), second.world().cells());
    }

    #[test]
    fn successful_regeneration_rebuilds_the_active_kernel_and_backend() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 8, 8);
        app.handle_command(Command::Step);
        app.handle_command(Command::NextKernelParameter);
        app.handle_command(Command::IncreaseKernelParameter);

        app.handle_command(Command::RegenerateKernel);

        assert_eq!(app.spec().kernel.name, "ring");
        assert_eq!(app.spec().kernel.parameters["center"], 0.51);
        assert_eq!(app.tick(), 0);
        assert_eq!(app.backend_error(), None);
    }

    #[test]
    fn successful_cuda_regeneration_preserves_the_cuda_backend_kind() {
        if !cuda_available() {
            return;
        }

        let spec = SimulationSpec::lenia_orbium();
        let backend = SimulationBackend::cuda_or_cpu(spec.clone(), 8, 8);
        let mut app = App::with_backend(spec, 8, 8, backend);
        app.handle_command(Command::Step);
        app.handle_command(Command::NextKernelParameter);
        app.handle_command(Command::IncreaseKernelParameter);

        app.handle_command(Command::RegenerateKernel);

        assert_eq!(app.backend_kind(), BackendKind::Cuda);
        assert_eq!(app.spec().kernel.parameters["center"], 0.51);
        assert_eq!(app.tick(), 0);
        assert_eq!(app.backend_error(), None);
    }

    #[test]
    fn failed_backend_regeneration_preserves_the_previous_state() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 8, 8);
        app.handle_command(Command::Step);
        let previous_kernel = app.spec().kernel.clone();
        let previous_world = app.world().cells().to_vec();
        let mut next_spec = app.spec().clone();
        next_spec
            .kernel
            .parameters
            .insert("center".to_string(), 0.51);

        let committed = app.commit_regenerated_kernel(
            next_spec,
            Err("forced CUDA construction failure".to_string()),
        );

        assert!(!committed);
        assert_eq!(app.spec().kernel, previous_kernel);
        assert_eq!(app.world().cells(), previous_world);
        assert_eq!(app.backend_kind(), BackendKind::Cpu);
        assert_eq!(app.tick(), 1);
        assert!(
            app.backend_error()
                .is_some_and(|error| error.contains("forced CUDA construction failure"))
        );
    }

    #[test]
    fn catalog_selection_regenerates_the_requested_definition() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 8, 8);
        app.handle_command(Command::NextKernel);

        app.handle_command(Command::RegenerateKernel);

        assert_eq!(app.spec().kernel.name, "render");
        assert_eq!(app.selected_kernel_name(), "render");
        assert_eq!(app.selected_kernel_parameter(), None);
        assert_eq!(app.tick(), 0);
    }

    #[test]
    fn invalid_regeneration_preserves_the_previous_kernel_and_backend() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 8, 8);
        app.handle_command(Command::Step);
        let previous_kernel = app.spec().kernel.clone();
        let previous_world = app.world().cells().to_vec();
        app.kernel_definitions[0]
            .parameters
            .insert("width".to_string(), 0.0);

        app.handle_command(Command::RegenerateKernel);

        assert_eq!(app.spec().kernel, previous_kernel);
        assert_eq!(app.world().cells(), previous_world);
        assert_eq!(app.tick(), 1);
        assert!(
            app.backend_error()
                .is_some_and(|error| error.contains("kernel"))
        );
    }

    #[test]
    fn kernel_preview_state_is_independent_from_simulation_pause_state() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 8, 8);

        assert!(!app.kernel_preview_enabled());
        app.handle_command(Command::ToggleKernelPreview);
        assert!(app.kernel_preview_enabled());
        assert!(!app.paused());

        app.handle_command(Command::TogglePause);
        app.handle_command(Command::ToggleKernelPreview);
        assert!(!app.kernel_preview_enabled());
        assert!(app.paused());
    }

    #[test]
    fn simulation_rate_and_render_rate_are_counted_independently() {
        let mut simulation_rate = RateMeter::new(Duration::from_secs(1));
        let mut render_rate = RateMeter::new(Duration::from_secs(1));
        let start = Instant::now();

        simulation_rate.record(start);
        simulation_rate.record(start + Duration::from_millis(500));
        render_rate.record(start);
        render_rate.record(start + Duration::from_millis(250));
        render_rate.record(start + Duration::from_millis(500));

        simulation_rate.refresh(start + Duration::from_secs(1));
        render_rate.refresh(start + Duration::from_secs(1));
        assert_eq!(simulation_rate.rate(), 2.0);
        assert_eq!(render_rate.rate(), 3.0);
    }

    #[test]
    fn app_uses_the_selected_backend_without_exposing_cuda_handles() {
        if !cuda_available() {
            return;
        }

        let spec = SimulationSpec::conway();
        let backend = crate::sim::backend::SimulationBackend::cuda_or_cpu(spec.clone(), 8, 8);
        let mut app = App::with_backend(spec, 8, 8, backend);

        assert_eq!(app.backend_kind(), crate::sim::backend::BackendKind::Cuda);
        assert!(!app.backend_name().is_empty());
        app.handle_command(Command::Step);
        assert_eq!(app.tick(), 1);
    }

    #[test]
    fn growth_expression_edits_rebuild_the_backend_transactionally() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 4, 3);
        app.world_mut().replace_cells(&[0.2; 12]);

        assert!(app.set_growth_expression("0.5"));
        assert!(app.step());
        assert!(
            app.world()
                .cells()
                .iter()
                .all(|value| (*value - 0.25).abs() < 1e-6)
        );
        let before = app.world().cells().to_vec();
        let tick = app.tick();

        assert!(!app.set_growth_expression("unknown + potential"));

        assert_eq!(app.world().cells(), before);
        assert_eq!(app.tick(), tick);
        assert!(app.backend_error().is_some());
    }

    #[test]
    fn rule_switches_reinitialize_the_world_and_backend() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 8, 8);
        app.handle_command(Command::Step);
        assert_eq!(
            app.spec().rule,
            crate::sim::rule::Rule::Lenia {
                mu: 0.135,
                sigma: 0.015
            }
        );

        app.handle_command(Command::Conway);
        assert_eq!(app.spec().rule, crate::sim::rule::Rule::Conway);
        assert_eq!(app.tick(), 0);

        app.handle_command(Command::Lenia);
        assert!(matches!(
            app.spec().rule,
            crate::sim::rule::Rule::Lenia { .. }
        ));
        assert_eq!(app.tick(), 0);
    }

    #[test]
    fn mouse_coordinates_use_the_active_display_framebuffer_scale() {
        let mut app = App::new(SimulationSpec::conway(), 32, 16);
        app.world_mut().clear();
        app.world_mut().set(18, 9, 0.75);
        app.set_viewport(ratatui::layout::Rect::new(0, 0, 10, 10), [40, 20]);
        let mut tracker = crate::input::MouseTracker::new();
        let event = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 5,
            row: 5,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };

        assert!(app.handle_mouse(event, &mut tracker));
        assert_eq!(app.world().get(18, 9), 1.0);
    }

    #[test]
    fn sparse_left_drag_paints_a_continuous_world_stroke() {
        let mut app = App::new(SimulationSpec::conway(), 32, 16);
        app.world_mut().clear();
        app.set_viewport(ratatui::layout::Rect::new(0, 0, 32, 16), [32, 16]);
        let mut tracker = crate::input::MouseTracker::new();
        let down = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 5,
            row: 8,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        let drag = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
            column: 13,
            row: 8,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };

        assert!(app.handle_mouse(down, &mut tracker));
        assert!(app.handle_mouse(drag, &mut tracker));
        let painted = app
            .world()
            .cells()
            .iter()
            .filter(|value| **value > 0.5)
            .count();
        assert!(
            painted >= 8,
            "two sparse pointer samples must be interpolated, got {painted} painted cells"
        );
    }

    #[test]
    fn graphics_vertex_hit_radius_covers_terminal_cell_quantization() {
        let viewport = ratatui::layout::Rect::new(24, 2, 104, 38);

        assert!(
            graphics_pointer_hit_radius([1248, 912], viewport) >= 14,
            "a 12×24 pixel terminal cell can report a pointer over 13 pixels from the visible handle"
        );
    }

    #[test]
    fn workbench_world_paint_invalidates_the_graphics_scene() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 16, 16);
        app.enter_workbench();
        app.set_viewport(ratatui::layout::Rect::new(2, 2, 20, 10), [160, 160]);
        let before = app.workbench_draft_scene_generation;
        let mut tracker = crate::input::MouseTracker::new();
        let event = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 8,
            row: 5,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };

        assert!(app.handle_mouse(event, &mut tracker));
        assert_ne!(app.workbench_draft_scene_generation, before);
    }

    #[test]
    fn both_entering_and_leaving_workbench_request_a_graphics_pipeline_clear() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 16, 16);

        app.enter_workbench();
        assert!(app.take_workbench_display_clear());
        assert!(!app.take_workbench_display_clear());

        app.leave_workbench();
        assert!(app.take_workbench_display_clear());
    }

    #[test]
    fn moving_pointer_while_drawing_updates_tiling_preview_without_adding_a_vertex() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 16, 16);
        app.enter_workbench();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Tiling);
        app.workbench_mut().begin_new_basis_polygon();
        app.workbench_mut()
            .push_tiling_vertex(crate::sim::tiling::Vec2::new(0.0, 0.0))
            .unwrap();
        app.set_viewport(ratatui::layout::Rect::new(2, 2, 20, 10), [200, 100]);
        let mut tracker = crate::input::MouseTracker::new();
        let before = app.workbench_draft_scene_generation;
        let moved = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Moved,
            column: 17,
            row: 7,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };

        assert!(app.handle_mouse(moved, &mut tracker));
        assert_eq!(app.workbench().tiling_construction().len(), 1);
        assert!(app.workbench().tiling_pointer().is_some());
        assert_ne!(app.workbench_draft_scene_generation, before);
    }

    #[test]
    fn ctrl_z_while_drawing_removes_the_last_uncommitted_polygon_vertex() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 16, 16);
        app.enter_workbench();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Tiling);
        app.workbench_mut().begin_new_basis_polygon();
        app.workbench_mut()
            .push_tiling_vertex(crate::sim::tiling::Vec2::new(0.0, 0.0))
            .unwrap();
        app.workbench_mut()
            .push_tiling_vertex(crate::sim::tiling::Vec2::new(1.0, 0.0))
            .unwrap();

        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Char('z'),
            crossterm::event::KeyModifiers::CONTROL,
        )));
        assert_eq!(app.workbench().tiling_construction().len(), 1);
        assert_eq!(
            app.workbench_notice(),
            Some("removed vertex 2 · 1 vertex remains")
        );
    }

    #[test]
    fn enter_closes_a_valid_partial_unit_cell_and_explains_its_status() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 16, 16);
        app.enter_workbench();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Tiling);
        app.workbench_mut().begin_new_basis_polygon();
        for point in [
            crate::sim::tiling::Vec2::new(0.0, 0.0),
            crate::sim::tiling::Vec2::new(2.0, 0.0),
            crate::sim::tiling::Vec2::new(0.0, 1.0),
        ] {
            app.workbench_mut().push_tiling_vertex(point).unwrap();
        }

        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        )));

        assert!(app.workbench().tiling_construction().is_empty());
        assert_eq!(
            app.workbench_notice(),
            Some("polygon closed · unit cell incomplete; add polygons or edit lattice")
        );
    }

    #[test]
    fn sparse_workbench_world_drag_paints_a_continuous_initial_field_stroke() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 32, 16);
        app.enter_workbench();
        let mut draft = app.workbench().draft().clone();
        draft.channels[0].initial.fill(0.0);
        app.workbench_mut().import_draft(draft).unwrap();
        app.set_viewport(ratatui::layout::Rect::new(0, 0, 32, 16), [32, 16]);
        let mut tracker = crate::input::MouseTracker::new();
        let down = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 5,
            row: 8,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        let drag = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
            column: 13,
            row: 8,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };

        assert!(app.handle_mouse(down, &mut tracker));
        assert!(app.handle_mouse(drag, &mut tracker));
        let painted = app.workbench().draft().channels[0]
            .initial
            .iter()
            .filter(|value| **value > 0.5)
            .count();
        assert!(
            painted >= 8,
            "two sparse pointer samples must be interpolated in the initial field, got {painted}"
        );
    }

    #[test]
    fn workbench_toolbar_click_executes_the_visible_channel_add_action() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 16, 16);
        app.enter_workbench();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Channels);
        app.set_workbench_area(ratatui::layout::Rect::new(0, 0, 160, 40));
        let layout =
            crate::tui::workbench::workbench_layout(ratatui::layout::Rect::new(0, 0, 160, 40));
        let before = app.workbench().draft().channels.len();
        let click = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: layout.canvas.x + 3,
            row: layout.canvas.y + 1,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };

        assert!(app.handle_workbench_panel_mouse(click));
        assert_eq!(app.workbench().draft().channels.len(), before + 1);
    }

    #[test]
    fn remote_experiment_apply_toolbar_click_sends_an_apply_request() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 16, 16);
        app.enter_workbench();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Experiment);
        app.set_workbench_area(ratatui::layout::Rect::new(0, 0, 160, 40));
        let layout =
            crate::tui::workbench::workbench_layout(ratatui::layout::Rect::new(0, 0, 160, 40));
        let click = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: layout.canvas.x + 3,
            row: layout.canvas.y + 1,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        let writer = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
        let mut tracker = crate::input::MouseTracker::new();
        let mut next_sequence = 7;

        assert!(
            !handle_remote_terminal_event(
                &mut app,
                &mut tracker,
                &writer,
                &mut next_sequence,
                Event::Mouse(click),
            )
            .unwrap()
        );
        let bytes = writer.lock().unwrap().clone();
        let message = crate::remote::read_message(&mut std::io::Cursor::new(bytes))
            .unwrap()
            .expect("clicking the visible Apply action must write a protocol request");
        assert!(matches!(
            message,
            crate::remote::RemoteMessage::ApplyDraft(_)
        ));
        assert_eq!(app.workbench_notice(), Some("apply sent"));
    }

    #[test]
    fn kernel_arrows_select_cells_and_exact_typing_replaces_the_old_value() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 16, 16);
        app.enter_workbench();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Kernels);
        let raster_anchor = app
            .workbench()
            .draft()
            .kernels
            .first()
            .map(|kernel| (kernel.definition.anchor_x, kernel.definition.anchor_y));

        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Right,
            crossterm::event::KeyModifiers::NONE,
        )));
        if let Some((anchor_x, anchor_y)) = raster_anchor {
            let point = app.workbench().kernel_selection().unwrap();
            assert_eq!((point.x, point.y), (anchor_x + 1, anchor_y));
        } else {
            let selection = app.workbench().periodic_kernel_selection().unwrap();
            assert_eq!(selection.offset, [1, 0]);
        }
        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Char('e'),
            crossterm::event::KeyModifiers::NONE,
        )));
        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Char('-'),
            crossterm::event::KeyModifiers::NONE,
        )));
        assert_eq!(app.workbench().numeric_editor().unwrap().buffer(), "-");
        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Char('0'),
            crossterm::event::KeyModifiers::NONE,
        )));
        assert_eq!(
            app.workbench().numeric_editor().unwrap().buffer(),
            "-0",
            "numeric input must receive zero instead of triggering the kernel fit shortcut",
        );
    }

    #[test]
    fn kernel_m_key_switches_between_weights_and_support_tools() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 16, 16);
        app.enter_workbench();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Kernels);

        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Char('m'),
            crossterm::event::KeyModifiers::NONE,
        )));
        assert_eq!(
            app.workbench().kernel_tool(),
            crate::workbench::kernel_editor::KernelTool::Support,
        );
        assert_eq!(
            app.workbench_notice(),
            Some("Support tool · left activate · right deactivate"),
        );
    }

    #[test]
    fn kernel_resize_editor_changes_periodic_dimensions_and_anchor_exactly() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 16, 16);
        app.enter_workbench();
        app.workbench_mut().cycle_tiling_preset().unwrap();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Kernels);
        assert!(matches!(
            app.workbench()
                .selected_rule_kernel()
                .map(|kernel| &kernel.spatial),
            Some(crate::sim::ruleset::KernelSpatialDefinition::Periodic(_)),
        ));

        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Char('r'),
            crossterm::event::KeyModifiers::NONE,
        )));
        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Char('a'),
            crossterm::event::KeyModifiers::CONTROL,
        )));
        for character in "29,29,14,14".chars() {
            assert!(app.handle_workbench_editor_key(KeyEvent::new(
                KeyCode::Char(character),
                crossterm::event::KeyModifiers::NONE,
            )));
        }
        assert_eq!(app.workbench().kernel_resize_editor(), Some("29,29,14,14"),);
        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        )));

        let Some(kernel) = app.workbench().selected_rule_kernel() else {
            panic!("selected kernel disappeared");
        };
        let crate::sim::ruleset::KernelSpatialDefinition::Periodic(definition) = &kernel.spatial
        else {
            panic!("kernel stopped being periodic");
        };
        assert_eq!(
            (
                definition.width,
                definition.height,
                definition.anchor_x,
                definition.anchor_y,
            ),
            (29, 29, 14, 14),
        );
    }

    #[test]
    fn kernel_metadata_keys_cycle_source_and_output_channels() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 16, 16);
        app.enter_workbench();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Channels);
        app.workbench_mut().add_channel().unwrap();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Kernels);
        assert_eq!(
            app.workbench().selected_channel(),
            crate::sim::experiment_model::ChannelId(1)
        );
        assert_eq!(
            app.workbench().selected_legacy_kernel().unwrap().source,
            crate::sim::experiment_model::ChannelId(1)
        );

        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Char('s'),
            crossterm::event::KeyModifiers::NONE,
        )));
        assert_eq!(
            app.workbench().selected_legacy_kernel().unwrap().source,
            crate::sim::experiment_model::ChannelId(0)
        );
        assert_eq!(
            app.workbench_notice(),
            Some("kernel source = channel 0 (state)"),
        );

        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Char('u'),
            crossterm::event::KeyModifiers::NONE,
        )));
        assert_eq!(
            app.workbench().selected_channel(),
            crate::sim::experiment_model::ChannelId(0)
        );
        assert!(app.workbench().selected_legacy_kernel().is_some());
        assert_eq!(
            app.workbench_notice(),
            Some("kernel output = channel 0 (state)"),
        );
    }

    #[test]
    fn editing_the_selected_second_kernel_does_not_mutate_the_first_kernel() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 16, 16);
        app.enter_workbench();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Kernels);
        app.workbench_mut().add_kernel_for_selected().unwrap();
        let selected = app.workbench().selected_kernel().unwrap();
        let first = app.workbench().draft().kernels[0].clone();
        let second = app
            .workbench()
            .draft()
            .kernels
            .iter()
            .find(|kernel| kernel.id == selected)
            .unwrap()
            .clone();
        let point = crate::workbench::kernel_editor::KernelPoint {
            x: second.definition.anchor_x,
            y: second.definition.anchor_y,
        };

        app.set_kernel_cell_value(point, 0.1234).unwrap();

        assert_eq!(app.workbench().draft().kernels[0], first);
        assert_eq!(app.workbench().selected_kernel(), Some(selected));
        assert!((app.kernel_cell_value(point).unwrap() - 0.1234).abs() < 1.0e-6);
    }

    #[test]
    fn growth_ctrl_a_then_typing_replaces_the_complete_source() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 16, 16);
        app.enter_workbench();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Growth);
        app.workbench_mut().toggle_growth_editing();
        let original = app
            .workbench()
            .growth_editor()
            .buffer()
            .as_str()
            .to_string();
        assert!(!original.is_empty());

        assert!(app.handle_workbench_growth_key(KeyEvent::new(
            KeyCode::Char('a'),
            crossterm::event::KeyModifiers::CONTROL,
        )));
        assert!(app.handle_workbench_growth_key(KeyEvent::new(
            KeyCode::Char('0'),
            crossterm::event::KeyModifiers::NONE,
        )));

        assert_eq!(
            app.workbench().growth_editor().buffer().as_str(),
            "0",
            "Ctrl+A must select the source so normal typing replaces it"
        );
    }

    #[test]
    fn workbench_key_routing_does_not_steal_zero_from_growth_source() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 16, 16);
        app.enter_workbench();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Growth);
        app.workbench_mut().toggle_growth_editing();

        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Char('a'),
            crossterm::event::KeyModifiers::CONTROL,
        )));
        for character in "0.5 + 1.0".chars() {
            assert!(
                app.handle_workbench_editor_key(KeyEvent::new(
                    KeyCode::Char(character),
                    crossterm::event::KeyModifiers::NONE,
                )),
                "growth editor must receive {character:?}"
            );
        }

        assert_eq!(
            app.workbench().growth_editor().buffer().as_str(),
            "0.5 + 1.0"
        );
        assert!(app.workbench().growth_editor().diagnostics().is_empty());
    }

    #[test]
    fn invalid_growth_edit_keeps_the_last_valid_curve_through_app_routing() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 16, 16);
        app.enter_workbench();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Growth);
        app.workbench_mut().toggle_growth_editing();

        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Char('a'),
            crossterm::event::KeyModifiers::CONTROL,
        )));
        for character in "if potential > 0.5 { 1.0 - potential } else { potential }".chars() {
            assert!(app.handle_workbench_editor_key(KeyEvent::new(
                KeyCode::Char(character),
                crossterm::event::KeyModifiers::NONE,
            )));
        }
        assert!(app.workbench().growth_editor().diagnostics().is_empty());
        let valid = app.workbench().growth_editor().plot().data.clone();
        assert!(valid.iter().flatten().any(|value| *value > 0.4));

        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Backspace,
            crossterm::event::KeyModifiers::NONE,
        )));

        assert!(!app.workbench().growth_editor().diagnostics().is_empty());
        assert_eq!(app.workbench().growth_editor().plot().data, valid);
        assert!(app.workbench().growth_editor().plot().stale);
    }

    #[test]
    fn growth_mode_key_switches_rate_to_value_and_updates_the_signature() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 16, 16);
        app.enter_workbench();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Growth);
        assert!(
            app.workbench()
                .growth_editor()
                .signature()
                .ends_with("-> Rate")
        );

        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Char('m'),
            crossterm::event::KeyModifiers::NONE,
        )));

        assert_eq!(
            app.workbench().selected_growth_mode(),
            Some(crate::sim::experiment_model::UpdateMode::DirectUpdate),
        );
        assert!(
            app.workbench()
                .growth_editor()
                .signature()
                .ends_with("-> Value")
        );
        assert!(
            app.workbench_notice()
                .is_some_and(|notice| notice.contains("next = clamp(result, 0, 1)"))
        );
    }

    #[test]
    fn growth_help_scroll_wheel_is_consumed_only_inside_the_inspector() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 16, 16);
        app.enter_workbench();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Growth);
        app.set_workbench_area(Rect::new(0, 0, 160, 40));
        let before = app.workbench().growth_help_scroll();
        let event = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: 150,
            row: 10,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };

        assert!(app.handle_workbench_panel_mouse(event));
        assert_eq!(app.workbench().growth_help_scroll(), before + 3);
        assert_eq!(
            app.workbench().status(),
            crate::workbench::DraftStatus::Clean
        );
    }

    #[test]
    fn experiment_dt_editor_accepts_an_exact_positive_value() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 16, 16);
        app.enter_workbench();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Experiment);

        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Char('d'),
            crossterm::event::KeyModifiers::NONE,
        )));
        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Char('a'),
            crossterm::event::KeyModifiers::CONTROL,
        )));
        for character in "0.025".chars() {
            assert!(app.handle_workbench_editor_key(KeyEvent::new(
                KeyCode::Char(character),
                crossterm::event::KeyModifiers::NONE,
            )));
        }
        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        )));

        assert_eq!(app.workbench().draft().simulation_dt, 0.025);
    }

    #[test]
    fn channels_exact_color_editor_supports_ctrl_a_and_hash_prefixed_rgb() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 16, 16);
        app.enter_workbench();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Channels);

        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Char('e'),
            crossterm::event::KeyModifiers::NONE,
        )));
        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Char('a'),
            crossterm::event::KeyModifiers::CONTROL,
        )));
        for character in "#3366CC".chars() {
            assert!(app.handle_workbench_editor_key(KeyEvent::new(
                KeyCode::Char(character),
                crossterm::event::KeyModifiers::NONE,
            )));
        }
        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        )));

        assert_eq!(
            app.workbench().draft().channels[0].display.color,
            crate::sim::experiment_model::DisplayColor::Custom(
                crate::sim::experiment_model::RgbColor {
                    red: 0x33,
                    green: 0x66,
                    blue: 0xCC,
                }
            )
        );
    }

    #[test]
    fn mouse_events_outside_the_viewport_are_rejected_before_clamping() {
        let mut app = App::new(SimulationSpec::conway(), 32, 16);
        app.set_viewport(ratatui::layout::Rect::new(2, 1, 10, 10), [20, 20]);
        let mut tracker = crate::input::MouseTracker::new();
        let event = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 1,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };

        assert!(!app.handle_mouse(event, &mut tracker));
        assert_eq!(app.inspected(), None);
    }

    #[test]
    fn vertical_pan_uses_the_active_display_framebuffer_scale() {
        let mut app = App::new(SimulationSpec::conway(), 32, 16);
        app.set_viewport(ratatui::layout::Rect::new(0, 0, 10, 10), [60, 30]);
        let mut tracker = crate::input::MouseTracker::new();
        let down = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Middle),
            column: 4,
            row: 4,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        let drag = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Middle),
            column: 5,
            row: 5,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };

        app.handle_mouse(down, &mut tracker);
        assert!(app.handle_mouse(drag, &mut tracker));
        assert_eq!(app.camera().center(), [10.0, 5.0]);
    }

    #[test]
    fn workbench_world_canvas_supports_middle_button_pan() {
        let mut app = App::new(SimulationSpec::conway(), 32, 16);
        app.enter_workbench();
        app.set_viewport(ratatui::layout::Rect::new(24, 1, 60, 32), [480, 512]);
        let before = app.camera().center();
        let mut tracker = crate::input::MouseTracker::new();
        let down = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Middle),
            column: 50,
            row: 12,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        let drag = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Middle),
            column: 54,
            row: 14,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };

        app.handle_mouse(down, &mut tracker);
        assert!(app.handle_mouse(drag, &mut tracker));
        assert_ne!(app.camera().center(), before);
    }

    #[test]
    fn tiling_pan_changes_its_camera_without_dirtying_the_draft() {
        let section = crate::workbench::WorkbenchSection::Tiling;
        let mut app = App::new(SimulationSpec::conway(), 32, 16);
        app.enter_workbench();
        let mut draft = app.workbench().draft().clone();
        draft.tiling = Some(crate::sim::tiling::build_preset(
            crate::sim::tiling::TilingPreset::Square,
            1.0,
        ));
        app.workbench_mut().import_draft(draft).unwrap();
        let status_before = app.workbench().status();
        app.workbench_mut().select_section(section);
        app.set_viewport(ratatui::layout::Rect::new(24, 1, 60, 32), [480, 512]);
        let mut tracker = crate::input::MouseTracker::new();
        let down = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Middle),
            column: 50,
            row: 12,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        let drag = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Middle),
            column: 54,
            row: 14,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };

        let before = app.workbench().tiling_camera();
        app.handle_mouse(down, &mut tracker);
        assert!(app.handle_mouse(drag, &mut tracker));
        assert_ne!(app.workbench().tiling_camera(), before);
        assert_eq!(
            app.workbench().status(),
            status_before,
            "{section:?} pan must not create a fake edit"
        );
    }

    #[test]
    fn transformed_tiling_vertex_follows_a_real_down_drag_up_sequence() {
        let mut app = App::new(SimulationSpec::conway(), 32, 16);
        app.enter_workbench();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Tiling);
        let tiling =
            crate::sim::tiling::build_preset(crate::sim::tiling::TilingPreset::OctagonSquare, 1.0);
        let mut draft = app.workbench().draft().clone();
        draft.tiling = Some(tiling.clone());
        app.workbench_mut().import_draft(draft).unwrap();
        app.workbench_mut()
            .set_selected_basis(tiling.instances[1].id)
            .unwrap();
        app.set_viewport(ratatui::layout::Rect::new(0, 0, 400, 400), [400, 400]);

        let instance = tiling.instances[1].clone();
        let prototype = tiling
            .prototypes
            .iter()
            .find(|prototype| prototype.id == instance.prototype)
            .unwrap();
        let vertices = crate::sim::tiling::polygon::prototype_vertices(&prototype.shape).unwrap();
        let world =
            crate::sim::tiling::polygon::transform_vertices(&vertices, instance.transform)[0];
        let scene = crate::workbench::tiling_editor::TilingScene::new(tiling.clone())
            .with_selected_basis(instance.id);
        let (x, y) = scene.world_to_pixel(world, 400, 400);
        let event = |kind, column: i32, row: i32| crossterm::event::MouseEvent {
            kind,
            column: column as u16,
            row: row as u16,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        let mut tracker = crate::input::MouseTracker::new();

        assert!(app.handle_mouse(
            event(
                crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left,),
                x,
                y,
            ),
            &mut tracker,
        ));
        assert!(app.handle_mouse(
            event(
                crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left,),
                x + 8,
                y + 4,
            ),
            &mut tracker,
        ));
        assert!(app.handle_mouse(
            event(
                crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left,),
                x + 8,
                y + 4,
            ),
            &mut tracker,
        ));

        let moved_draft = app.workbench().draft().tiling.as_ref().unwrap();
        let moved_prototype = moved_draft
            .prototypes
            .iter()
            .find(|prototype| prototype.id == instance.prototype)
            .unwrap();
        let moved_local =
            crate::sim::tiling::polygon::prototype_vertices(&moved_prototype.shape).unwrap();
        let moved_world =
            crate::sim::tiling::polygon::transform_vertices(&moved_local, instance.transform)[0];
        let expected = scene.pixel_to_world((x + 8) as u32, (y + 4) as u32, 400, 400);
        assert!(
            (moved_world - expected).length() < 0.03,
            "transformed vertex must land under the pointer: actual={moved_world:?}, expected={expected:?}"
        );
    }

    #[test]
    fn tiling_fit_restores_the_default_camera_without_dirtying_the_draft() {
        let mut app = App::new(SimulationSpec::conway(), 32, 16);
        app.enter_workbench();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Tiling);
        app.workbench_mut()
            .set_tiling_camera(crate::workbench::tiling_editor::TilingCamera {
                center: crate::sim::tiling::Vec2::new(9.0, -7.0),
                scale: 0.05,
            });
        let status_before = app.workbench().status();

        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Char('0'),
            crossterm::event::KeyModifiers::NONE,
        )));
        assert_eq!(
            app.workbench().tiling_camera(),
            crate::workbench::tiling_editor::TilingCamera::default()
        );
        assert_eq!(app.workbench().status(), status_before);
    }

    #[test]
    fn loading_a_tiling_preset_replaces_stale_tool_feedback() {
        let mut app = App::new(SimulationSpec::conway(), 32, 16);
        app.enter_workbench();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Tiling);
        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Char('d'),
            crossterm::event::KeyModifiers::NONE,
        )));
        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        )));
        assert_eq!(app.workbench_notice(), Some("polygon drawing cancelled"));

        app.handle_workbench_ui(UiCommand::CyclePreset).unwrap();

        assert!(
            app.workbench_notice().is_some_and(|notice| {
                notice.contains("preset") && !notice.contains("cancelled")
            })
        );
    }

    #[test]
    fn kernel_pan_changes_the_view_without_dirtying_the_draft() {
        let mut app = App::new(SimulationSpec::conway(), 32, 16);
        app.enter_workbench();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Kernels);
        app.workbench_mut()
            .set_kernel_view(crate::workbench::kernel_editor::KernelView {
                center: [0.4, 0.4],
                zoom: 4.0,
            });
        app.set_viewport(ratatui::layout::Rect::new(24, 1, 60, 32), [480, 512]);
        let before = app.workbench().kernel_view();
        let generation_before = app.workbench_draft_scene_generation;
        let mut tracker = crate::input::MouseTracker::new();
        let down = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Middle),
            column: 50,
            row: 12,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        let drag = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Middle),
            column: 54,
            row: 14,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        app.handle_mouse(down, &mut tracker);
        assert!(app.handle_mouse(drag, &mut tracker));
        assert_ne!(app.workbench().kernel_view(), before);
        assert_ne!(app.workbench_draft_scene_generation, generation_before);
        assert_eq!(
            app.workbench().status(),
            crate::workbench::DraftStatus::Clean
        );
    }

    #[test]
    fn kernel_fit_restores_the_default_view_without_dirtying_the_draft() {
        let mut app = App::new(SimulationSpec::conway(), 32, 16);
        app.enter_workbench();
        app.workbench_mut()
            .select_section(crate::workbench::WorkbenchSection::Kernels);
        app.workbench_mut()
            .set_kernel_view(crate::workbench::kernel_editor::KernelView {
                center: [0.4, 0.4],
                zoom: 4.0,
            });
        let status_before = app.workbench().status();

        assert!(app.handle_workbench_editor_key(KeyEvent::new(
            KeyCode::Char('0'),
            crossterm::event::KeyModifiers::NONE,
        )));
        assert_eq!(
            app.workbench().kernel_view(),
            crate::workbench::kernel_editor::KernelView::default()
        );
        assert_eq!(app.workbench().status(), status_before);
    }

    #[test]
    fn topology_draft_is_validated_before_becoming_active_metadata() {
        let mut app = App::new(SimulationSpec::conway(), 8, 8);
        let square =
            crate::sim::tiling::build_preset(crate::sim::tiling::TilingPreset::Square, 1.0);
        app.set_tiling_draft(Some(square)).unwrap();
        assert!(app.tiling_draft().is_some());
        let mut invalid = app.tiling_draft().unwrap().clone();
        if let crate::sim::tiling::PrototypeShape::SimplePolygon { vertices } =
            &mut invalid.prototypes[0].shape
        {
            vertices.swap(1, 2);
        }
        let errors = app.set_tiling_draft(Some(invalid)).unwrap_err();
        assert!(!errors.is_empty());
        assert!(app.tiling_draft().is_some());
    }
}
