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
        }
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn tick(&self) -> u64 {
        self.backend.tick()
    }

    pub fn backend_kind(&self) -> BackendKind {
        self.backend.kind()
    }

    pub fn backend_name(&self) -> &str {
        self.backend.device_name()
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

    pub fn step(&mut self) {
        match self.backend.step(&mut self.world) {
            Ok(()) => self.backend_error = None,
            Err(error) => self.backend_error = Some(error.to_string()),
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
                self.reset();
            }
            Command::Quit => {}
        }
    }

    fn reset(&mut self) {
        self.world.randomize(self.seed, initial_density(&self.spec));
        self.backend = self.recreate_backend();
        self.inspected = None;
    }

    fn recreate_backend(&self) -> SimulationBackend {
        match self.backend.kind() {
            BackendKind::Cpu => SimulationBackend::cpu(self.spec.clone()),
            BackendKind::Cuda => SimulationBackend::cuda_or_cpu(
                self.spec.clone(),
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
                self.camera.pan_screen([dx, dy * 2.0]);
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
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    crossterm::execute!(stdout, crossterm::cursor::Hide)?;

    let terminal_result = (|| {
        let backend = ratatui::backend::CrosstermBackend::new(std::io::stdout());
        let mut terminal = ratatui::Terminal::new(backend)?;
        run_loop(&mut terminal)
    })();

    crossterm::execute!(stdout, crossterm::cursor::Show)?;
    crossterm::execute!(
        stdout,
        crossterm::event::DisableMouseCapture,
        crossterm::terminal::LeaveAlternateScreen
    )?;
    crossterm::terminal::disable_raw_mode()?;
    terminal_result
}

fn run_loop(terminal: &mut ratatui::DefaultTerminal) -> std::io::Result<()> {
    let spec = SimulationSpec::lenia_orbium();
    let backend = SimulationBackend::cuda_or_cpu(spec.clone(), 256, 256);
    let mut app = App::with_backend(spec, 256, 256, backend);
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
                app.step();
                simulation_meter.record(now);
                simulation_backlog -= simulation_interval;
                steps += 1;
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

pub fn rule_name(spec: &SimulationSpec) -> &'static str {
    match spec.rule {
        crate::sim::rule::Rule::Conway => "Conway",
        crate::sim::rule::Rule::Lenia { .. } => "Lenia/Orbium",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let spec = SimulationSpec::conway();
        let backend = crate::sim::backend::SimulationBackend::cuda_or_cpu(spec.clone(), 8, 8);
        let mut app = App::with_backend(spec, 8, 8, backend);

        assert_eq!(app.backend_kind(), crate::sim::backend::BackendKind::Cuda);
        assert!(app.backend_name().contains("2080"));
        app.handle_command(Command::Step);
        assert_eq!(app.tick(), 1);
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
}
