use crate::sim::kernel::{
    Kernel, KernelDefinition, KernelValues, Normalization, render_definition, ring_definition,
};
use std::io::{ErrorKind, Write};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::input::Command;
use crate::render::camera::Camera;
use crate::render::raster::{Framebuffer, rasterize_world_into};
use crate::sim::backend::{BackendKind, SimulationBackend};
use crate::sim::experiment::{ExperimentError, ExperimentFile, ExperimentMetadata};
use crate::sim::rule::SimulationSpec;
use crate::sim::world::World;
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
    framebuffer: Option<Framebuffer>,
    performance: PerformanceStats,
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
            expression_buffer: String::new(),
            framebuffer: None,
            performance: PerformanceStats::default(),
        }
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn tick(&self) -> u64 {
        self.backend.tick()
    }

    pub fn backend_error(&self) -> Option<&str> {
        self.kernel_error
            .as_deref()
            .or(self.backend_error.as_deref())
    }

    pub fn backend_kind(&self) -> BackendKind {
        self.backend.kind()
    }

    pub fn backend_name(&self) -> &str {
        self.backend.device_name()
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
        crate::remote::Snapshot {
            width: self.world.width() as u32,
            height: self.world.height() as u32,
            tick: self.tick(),
            paused: self.paused,
            simulation_rate,
            render_rate,
            backend: self.backend_name().to_string(),
            rule: rule_name(&self.spec).to_string(),
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
        self.world.replace_cells(&snapshot.cells);
        self.paused = snapshot.paused;
        self.simulation_rate = snapshot.simulation_rate;
        self.backend_error = snapshot.error.clone();
        true
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
                self.backend = self.recreate_backend();
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
            Command::NextPanel => self.active_panel = self.active_panel.next(),
            Command::ToggleExpressionEditor => self.toggle_expression_editor(),
            Command::Quit => {}
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
        self.inspected = Some(self.world.get(x, y));
    }

    pub fn paint_world(&mut self, world: [f32; 2], value: f32) {
        let x = world[0].floor() as isize;
        let y = world[1].floor() as isize;
        self.world.set(x, y, value);
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
        let Some(action) = tracker.update(&local, viewport.width, viewport.height) else {
            return false;
        };
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
            crate::input::MouseAction::Paint => self.paint_world(world, 1.0),
            crate::input::MouseAction::Erase => self.paint_world(world, 0.0),
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
        let snapshot = app.remote_snapshot();

        let mut mirror = App::new(SimulationSpec::conway(), 2, 2);
        mirror.set_rates(0.0, 7.0);
        assert!(mirror.apply_remote_snapshot(&snapshot));
        assert_eq!(mirror.world().cells(), &[0.1, 0.2, 0.3, 0.4]);
        assert!(mirror.paused());
        assert_eq!(mirror.rates(), (12.0, 7.0));
    }

    #[test]
    fn remote_snapshot_rejects_dimension_mismatch() {
        let app = App::new(SimulationSpec::conway(), 2, 2);
        let mut snapshot = app.remote_snapshot();
        snapshot.width = 3;
        let mut mirror = App::new(SimulationSpec::conway(), 2, 2);
        assert!(!mirror.apply_remote_snapshot(&snapshot));
    }
}

pub struct RateMeter {
    window: Duration,
    events: Vec<Instant>,
    rate: f64,
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
    run_app(App::with_backend(spec, 256, 256, backend))
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

    let (snapshot_tx, snapshot_rx) = mpsc::sync_channel::<Option<crate::remote::Snapshot>>(1);
    std::thread::spawn(move || {
        let mut writer = writer;
        while let Ok(snapshot) = snapshot_rx.recv() {
            let Some(snapshot) = snapshot else { break };
            if write_message(&mut writer, &RemoteMessage::Snapshot(snapshot)).is_err() {
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

        loop {
            match input_rx.try_recv() {
                Ok(RemoteMessage::Hello) => {
                    connected = true;
                    let _ = snapshot_tx.try_send(Some(app.remote_snapshot()));
                }
                Ok(RemoteMessage::Viewport { width, height }) => {
                    let width = width.max(1);
                    let height = height.max(1);
                    app.set_viewport(
                        Rect::new(0, 0, width, height),
                        [width as usize, height as usize * 2],
                    );
                }
                Ok(RemoteMessage::Input(input)) => match input {
                    InputMessage::Command(Command::Quit) => {
                        let _ = snapshot_tx.send(None);
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
                },
                Ok(RemoteMessage::Quit) => {
                    let _ = snapshot_tx.send(None);
                    return Ok(());
                }
                Ok(RemoteMessage::Snapshot(_)) => {}
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let _ = snapshot_tx.send(None);
                    return Ok(());
                }
            }
        }

        if !app.paused() {
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
        } else {
            simulation_backlog = Duration::ZERO;
        }

        if connected && now.duration_since(last_snapshot) >= snapshot_interval {
            simulation_meter.refresh(now);
            app.set_rates(simulation_meter.rate(), 30.0);
            let _ = snapshot_tx.try_send(Some(app.remote_snapshot()));
            last_snapshot = now;
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
    let app = App::new(SimulationSpec::lenia_orbium(), 256, 256);
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
    Snapshot(crate::remote::Snapshot),
    Closed(Result<(), crate::remote::ProtocolError>),
}

fn run_local_remote_viewer<R, W>(mut app: App, stdin: R, stdout: W) -> std::io::Result<()>
where
    R: std::io::Write + Send + 'static,
    W: std::io::Read + Send + 'static,
{
    use crate::remote::{RemoteMessage, read_message, write_message};
    use std::sync::{Arc, Mutex, mpsc};
    let writer = Arc::new(Mutex::new(stdin));
    {
        let mut guard = writer
            .lock()
            .map_err(|_| std::io::Error::other("SSH writer mutex poisoned"))?;
        write_message(&mut *guard, &RemoteMessage::Hello).map_err(std::io::Error::other)?;
    }
    let (snapshot_tx, snapshot_rx) = mpsc::sync_channel(2);
    std::thread::spawn(move || {
        let mut stdout = stdout;
        loop {
            match read_message(&mut stdout) {
                Ok(Some(crate::remote::RemoteMessage::Snapshot(snapshot))) => {
                    let _ = snapshot_tx.try_send(RemoteUpdate::Snapshot(snapshot));
                }
                Ok(Some(_)) => {}
                Ok(None) => {
                    let _ = snapshot_tx.send(RemoteUpdate::Closed(Ok(())));
                    break;
                }
                Err(error) => {
                    let _ = snapshot_tx.send(RemoteUpdate::Closed(Err(error)));
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
        crossterm::cursor::Hide
    )?;
    let display = crate::render::display::ViewportDisplay::detect();
    if display.uses_async_output() {
        let backend = AsyncTerminalBackend {
            inner: ratatui::backend::CrosstermBackend::new(AsyncTerminalWriter::new()),
        };
        let mut terminal = ratatui::Terminal::new(backend)?;
        run_remote_loop(&mut app, &mut terminal, display, writer, snapshot_rx)
    } else {
        let backend = ratatui::backend::CrosstermBackend::new(std::io::stdout());
        let mut terminal = ratatui::Terminal::new(backend)?;
        run_remote_loop(&mut app, &mut terminal, display, writer, snapshot_rx)
    }
}

fn run_remote_loop<B, W>(
    app: &mut App,
    terminal: &mut ratatui::Terminal<B>,
    display: crate::render::display::ViewportDisplay,
    writer: std::sync::Arc<std::sync::Mutex<W>>,
    snapshot_rx: std::sync::mpsc::Receiver<RemoteUpdate>,
) -> std::io::Result<()>
where
    B: ratatui::backend::Backend<Error = std::io::Error>,
    W: std::io::Write,
{
    use crate::remote::{ExpressionKey, InputMessage, RemoteMessage, write_message};
    let mut tracker = crate::input::MouseTracker::new();
    let mut render_meter = RateMeter::new(Duration::from_secs(1));
    let render_interval = Duration::from_secs_f64(1.0 / 30.0);
    let mut last_render = Instant::now() - render_interval;
    let mut last_viewport = None;
    loop {
        loop {
            match snapshot_rx.try_recv() {
                Ok(RemoteUpdate::Snapshot(snapshot)) => {
                    let _ = app.apply_remote_snapshot(&snapshot);
                }
                Ok(RemoteUpdate::Closed(Ok(()))) => {
                    return Err(std::io::Error::new(
                        ErrorKind::UnexpectedEof,
                        "SSH connector closed the Cellarium protocol stream",
                    ));
                }
                Ok(RemoteUpdate::Closed(Err(error))) => {
                    return Err(std::io::Error::other(format!(
                        "SSH connector protocol failed: {error}"
                    )));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err(std::io::Error::new(
                        ErrorKind::UnexpectedEof,
                        "SSH connector protocol reader stopped",
                    ));
                }
            }
        }
        let now = Instant::now();
        if now.duration_since(last_render) >= render_interval {
            render_meter.record(now);
            render_meter.refresh(now);
            app.render_rate = render_meter.rate();
            let viewport = app.viewport_geometry().map(|(_, frame)| frame);
            if viewport != last_viewport {
                if let Some((area, _frame_size)) = app.viewport_geometry() {
                    let mut guard = writer
                        .lock()
                        .map_err(|_| std::io::Error::other("SSH writer mutex poisoned"))?;
                    write_message(
                        &mut *guard,
                        &RemoteMessage::Viewport {
                            width: area.width,
                            height: area.height,
                        },
                    )
                    .map_err(std::io::Error::other)?;
                }
                last_viewport = viewport;
            }
            terminal.draw(|frame| crate::tui::draw(frame, app, &display))?;
            last_render = now;
        }

        let wait = render_interval
            .saturating_sub(now.duration_since(last_render))
            .min(Duration::from_millis(5));
        if crossterm::event::poll(wait)? {
            match crossterm::event::read()? {
                Event::Key(key) => {
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
                                &writer,
                                InputMessage::ExpressionKey(expression_key),
                            )?;
                        }
                        continue;
                    }
                    if let Some(command) = crate::input::translate_key(&key) {
                        send_remote_input(&writer, InputMessage::Command(command))?;
                        if command == Command::Quit {
                            return Ok(());
                        }
                        app.handle_command(command);
                    }
                }
                Event::Mouse(mouse) => {
                    if app.handle_mouse(mouse, &mut tracker)
                        && let Some((area, _)) = app.viewport_geometry()
                    {
                        let mut local = mouse;
                        local.column = local.column.saturating_sub(area.x);
                        local.row = local.row.saturating_sub(area.y);
                        send_remote_input(&writer, InputMessage::Mouse(local))?;
                    }
                }
                Event::Resize(_, _) | Event::FocusGained | Event::FocusLost => {}
                Event::Paste(_) => {}
            }
        }
    }
}

fn send_remote_input<W: std::io::Write>(
    writer: &std::sync::Arc<std::sync::Mutex<W>>,
    input: crate::remote::InputMessage,
) -> std::io::Result<()> {
    let mut guard = writer
        .lock()
        .map_err(|_| std::io::Error::other("SSH writer mutex poisoned"))?;
    crate::remote::write_message(&mut *guard, &crate::remote::RemoteMessage::Input(input))
        .map_err(std::io::Error::other)
}

fn run_app_with_save(app: App, save_path: Option<&Path>) -> std::io::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    let _terminal_guard = TerminalGuard;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    crossterm::execute!(stdout, crossterm::cursor::Hide)?;

    let display = crate::render::display::ViewportDisplay::detect();
    if display.uses_async_output() {
        let backend = AsyncTerminalBackend {
            inner: ratatui::backend::CrosstermBackend::new(AsyncTerminalWriter::new()),
        };
        let mut terminal = ratatui::Terminal::new(backend)?;
        run_loop(app, &mut terminal, display, save_path)
    } else {
        let backend = ratatui::backend::CrosstermBackend::new(std::io::stdout());
        let mut terminal = ratatui::Terminal::new(backend)?;
        run_loop(app, &mut terminal, display, save_path)
    }
}

struct TerminalGuard;

struct LatestFrameState {
    frame: Option<Vec<u8>>,
    closed: bool,
}

struct AsyncTerminalWriter {
    state: Arc<(Mutex<LatestFrameState>, Condvar)>,
    pending: Vec<u8>,
    worker: Option<JoinHandle<()>>,
}

struct AsyncTerminalBackend {
    inner: ratatui::backend::CrosstermBackend<AsyncTerminalWriter>,
}

impl ratatui::backend::Backend for AsyncTerminalBackend {
    type Error = std::io::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a ratatui::buffer::Cell)>,
    {
        self.inner.draw(content)
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
        Ok(())
    }

    fn clear_region(
        &mut self,
        _clear_type: ratatui::backend::ClearType,
    ) -> Result<(), Self::Error> {
        Ok(())
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
                    while state.frame.is_none() && !state.closed {
                        state = wake
                            .wait(state)
                            .expect("async terminal writer mutex poisoned");
                    }
                    match state.frame.take() {
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
        state.frame = Some(std::mem::take(&mut self.pending));
        wake.notify_one();
        Ok(())
    }
}

impl Drop for AsyncTerminalWriter {
    fn drop(&mut self) {
        let (lock, wake) = &*self.state;
        if let Ok(mut state) = lock.lock() {
            if !self.pending.is_empty() {
                state.frame = Some(std::mem::take(&mut self.pending));
            }
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
) -> std::io::Result<()> {
    let mut tracker = crate::input::MouseTracker::new();
    let mut simulation_meter = RateMeter::new(Duration::from_secs(1));
    let mut render_meter = RateMeter::new(Duration::from_secs(1));
    let simulation_interval = Duration::from_secs_f64(1.0 / 30.0);
    let render_interval = Duration::from_secs_f64(1.0 / 30.0);
    let mut simulation_backlog = Duration::ZERO;
    let mut last_iteration = Instant::now();
    let mut last_render = last_iteration;

    loop {
        let now = Instant::now();
        let elapsed = now - last_iteration;
        last_iteration = now;

        let wait = render_interval
            .saturating_sub(now.duration_since(last_render))
            .min(Duration::from_millis(5));
        if crossterm::event::poll(wait)? {
            match crossterm::event::read()? {
                Event::Key(key) => {
                    if app.expression_editing() {
                        app.handle_expression_key(key);
                        continue;
                    }
                    if let Some(command) = crate::input::translate_key(&key) {
                        if command == Command::Quit {
                            if let Some(path) = save_path {
                                app.save_experiment(path).map_err(std::io::Error::other)?;
                            }
                            break;
                        }
                        app.handle_command(command);
                    }
                }
                Event::Mouse(mouse) => {
                    app.handle_mouse(mouse, &mut tracker);
                }
                Event::Resize(_, _) | Event::FocusGained | Event::FocusLost => {}
                Event::Paste(_) => {}
            }
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
            terminal.draw(|frame| {
                app.set_rates(rates.0, rates.1);
                crate::tui::draw(frame, &mut app, &display);
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
        assert_eq!(app.inspected(), Some(0.75));
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
}
