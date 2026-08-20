use crate::sim::kernel::{
    Kernel, KernelDefinition, KernelValues, Normalization, render_definition, ring_definition,
};
use std::io::ErrorKind;
use std::time::{Duration, Instant};

use crate::input::Command;
use crate::render::camera::Camera;
use crate::sim::backend::{BackendKind, SimulationBackend};
use crate::sim::rule::SimulationSpec;
use crate::sim::world::World;
use crossterm::event::{Event, MouseEvent};
use ratatui::layout::Rect;

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

    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    pub fn spec(&self) -> &SimulationSpec {
        &self.spec
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

    pub fn set_viewport(&mut self, viewport: Rect, frame_size: [usize; 2]) {
        self.viewport = Some(viewport);
        self.frame_size = Some(frame_size);
    }

    pub fn step(&mut self) -> bool {
        match self.backend.step(&mut self.world) {
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
            Command::Quit => {}
        }
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

pub fn run_with_kernel(kernel: KernelDefinition) -> std::io::Result<()> {
    run_app(app_for_kernel(kernel)?)
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

fn run_app(app: App) -> std::io::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    let _terminal_guard = TerminalGuard;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    crossterm::execute!(stdout, crossterm::cursor::Hide)?;

    (|| {
        let backend = ratatui::backend::CrosstermBackend::new(std::io::stdout());
        let mut terminal = ratatui::Terminal::new(backend)?;
        run_loop(app, &mut terminal)
    })()
}

struct TerminalGuard;

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

fn run_loop(mut app: App, terminal: &mut ratatui::DefaultTerminal) -> std::io::Result<()> {
    let display = crate::render::display::ViewportDisplay::detect();
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
            terminal.draw(|frame| {
                app.set_rates(rates.0, rates.1);
                crate::tui::draw(frame, &mut app, &display);
            })?;
            last_render = now;
        }

        let wait = render_interval
            .saturating_sub(now.duration_since(last_render))
            .min(Duration::from_millis(5));
        if crossterm::event::poll(wait)? {
            match crossterm::event::read()? {
                Event::Key(key) => {
                    if let Some(command) = crate::input::translate_key(&key) {
                        if command == Command::Quit {
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
    }
    Ok(())
}

fn initial_density(spec: &SimulationSpec) -> f64 {
    match spec.rule {
        crate::sim::rule::Rule::Conway => 0.35,
        crate::sim::rule::Rule::Lenia { .. } => 0.25,
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::kernel::{KernelValues, Normalization};

    fn cuda_available() -> bool {
        crate::sim::cuda::CudaBackend::new(SimulationSpec::conway(), 1, 1).is_ok()
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
