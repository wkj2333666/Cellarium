//! The simulation worker and its controller.
//!
//! Simulation runs on its own thread. The GUI thread only sends commands and
//! reads the newest published snapshot, so compiling, stepping and reading back
//! never block a frame.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::sim::basis_runtime::StateLayout;
use crate::sim::local_backend::{
    BackendDescriptor, BackendFailure, LocalBackend, StepStats, WorldEdit,
};

/// What the worker is asked to do. Commands are ordered and never dropped.
#[derive(Clone, Debug, PartialEq)]
pub enum SimulationCommand {
    SetRunning(bool),
    Step(u32),
    Reset,
    EditWorld(Vec<WorldEdit>),
    Shutdown,
}

/// A runtime problem the user needs to see.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeNotice {
    pub message: String,
}

/// The newest displayable state. Publishing replaces the previous snapshot, so
/// a slow reader never builds a queue of stale frames.
#[derive(Clone, Debug, PartialEq)]
pub struct SimulationSnapshot {
    pub generation: u64,
    pub tick: u64,
    pub running: bool,
    pub backend: BackendDescriptor,
    pub layout: StateLayout,
    pub cells: Arc<[f32]>,
    pub step_stats: StepStats,
    pub error: Option<RuntimeNotice>,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ControllerError {
    #[error("the simulation worker has stopped")]
    WorkerStopped,
}

/// GUI-side handle to the worker.
pub struct SimulationController {
    commands: Sender<SimulationCommand>,
    latest: Arc<RwLock<Arc<SimulationSnapshot>>>,
    published: Arc<AtomicU64>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl SimulationController {
    /// Start a worker owning `backend` and publish its first snapshot before
    /// returning, so the GUI always has something valid to draw.
    pub fn spawn(mut backend: Box<dyn LocalBackend>) -> Result<Self, BackendFailure> {
        let first = snapshot_of(backend.as_mut(), 0, false, StepStats::default(), None)?;
        let latest = Arc::new(RwLock::new(Arc::new(first)));
        let published = Arc::new(AtomicU64::new(1));
        let (commands, inbox) = channel();
        let worker_latest = Arc::clone(&latest);
        let worker_published = Arc::clone(&published);
        let worker = std::thread::Builder::new()
            .name("cellarium-simulation".into())
            .spawn(move || {
                run_worker(backend, inbox, worker_latest, worker_published);
            })
            .map_err(|error| BackendFailure::Unsupported(error.to_string()))?;
        Ok(Self {
            commands,
            latest,
            published,
            worker: Some(worker),
        })
    }

    pub fn send(&self, command: SimulationCommand) -> Result<(), ControllerError> {
        self.commands
            .send(command)
            .map_err(|_| ControllerError::WorkerStopped)
    }

    pub fn step(&self, steps: u32) -> Result<(), ControllerError> {
        self.send(SimulationCommand::Step(steps))
    }

    pub fn set_running(&self, running: bool) -> Result<(), ControllerError> {
        self.send(SimulationCommand::SetRunning(running))
    }

    /// The newest published snapshot. Never blocks on the worker.
    pub fn snapshot(&self) -> Arc<SimulationSnapshot> {
        Arc::clone(&self.latest.read().expect("snapshot lock"))
    }

    /// Unread snapshots are replaced rather than queued, so the slot always
    /// holds exactly one.
    pub fn snapshot_slot_depth(&self) -> usize {
        1
    }

    /// How many snapshots the worker has published so far.
    pub fn published_count(&self) -> u64 {
        self.published.load(Ordering::Acquire)
    }

    /// Wait until a published snapshot satisfies `predicate`.
    pub fn wait_for(
        &self,
        predicate: impl Fn(&SimulationSnapshot) -> bool,
    ) -> Arc<SimulationSnapshot> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let snapshot = self.snapshot();
            if predicate(&snapshot) {
                return snapshot;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for a matching snapshot"
            );
            std::thread::yield_now();
        }
    }

    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        let _ = self.commands.send(SimulationCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for SimulationController {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_worker(
    mut backend: Box<dyn LocalBackend>,
    inbox: Receiver<SimulationCommand>,
    latest: Arc<RwLock<Arc<SimulationSnapshot>>>,
    published: Arc<AtomicU64>,
) {
    let mut running = false;
    let mut generation = 1;
    let mut error = None;
    let initial = backend.readback().ok();

    loop {
        // Drain every pending command before scheduling more work, so a pause
        // that arrives during a step is honoured before the next step starts.
        let mut publish = false;
        let mut stats = StepStats::default();
        // While paused there is no work to do, so wait for the first command.
        // Everything already queued behind it is then drained without blocking.
        let mut blocking = !running;
        loop {
            let command = if blocking {
                match inbox.recv() {
                    Ok(command) => command,
                    Err(_) => return,
                }
            } else {
                match inbox.try_recv() {
                    Ok(command) => command,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            };
            blocking = false;
            match command {
                SimulationCommand::Shutdown => return,
                SimulationCommand::SetRunning(next) => {
                    running = next;
                    publish = true;
                }
                SimulationCommand::Step(steps) => match backend.step(steps) {
                    Ok(step_stats) => {
                        stats = step_stats;
                        publish = true;
                    }
                    Err(failure) => {
                        running = false;
                        error = Some(RuntimeNotice {
                            message: failure.to_string(),
                        });
                        publish = true;
                    }
                },
                SimulationCommand::Reset => {
                    if let Some(state) = &initial
                        && let Err(failure) = backend.set_running_state(state)
                    {
                        error = Some(RuntimeNotice {
                            message: failure.to_string(),
                        });
                    }
                    publish = true;
                }
                SimulationCommand::EditWorld(edits) => {
                    if let Err(failure) = backend.apply_edits(&edits) {
                        error = Some(RuntimeNotice {
                            message: failure.to_string(),
                        });
                    }
                    publish = true;
                }
            }
        }

        if running && error.is_none() {
            match backend.step(1) {
                Ok(step_stats) => {
                    stats = step_stats;
                    publish = true;
                }
                Err(failure) => {
                    running = false;
                    error = Some(RuntimeNotice {
                        message: failure.to_string(),
                    });
                    publish = true;
                }
            }
        }

        if publish
            && let Ok(snapshot) =
                snapshot_of(backend.as_mut(), generation, running, stats, error.clone())
        {
            generation += 1;
            *latest.write().expect("snapshot lock") = Arc::new(snapshot);
            published.fetch_add(1, Ordering::Release);
        }
    }
}

fn snapshot_of(
    backend: &mut dyn LocalBackend,
    generation: u64,
    running: bool,
    step_stats: StepStats,
    error: Option<RuntimeNotice>,
) -> Result<SimulationSnapshot, BackendFailure> {
    let state = backend.readback()?;
    Ok(SimulationSnapshot {
        generation,
        tick: backend.tick(),
        running,
        backend: backend.descriptor().clone(),
        layout: state.layout,
        cells: state.cells,
        step_stats,
        error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::compute_plan::compile_compute_plan;
    use crate::sim::experiment_model::ExperimentSpec;
    use crate::sim::local_backend::{BackendDescriptor, CpuBackend, WorldSnapshot, initial_cells};
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;

    /// A backend whose step blocks until the test releases it, so command
    /// ordering during a long step is observable.
    #[derive(Clone)]
    struct BlockingBackend {
        inner: Arc<BlockingInner>,
    }

    struct BlockingInner {
        descriptor: BackendDescriptor,
        layout: StateLayout,
        steps_started: AtomicUsize,
        release: Mutex<bool>,
        tick: AtomicU64,
    }

    impl BlockingBackend {
        fn new() -> Self {
            Self {
                inner: Arc::new(BlockingInner {
                    descriptor: BackendDescriptor::cpu(),
                    layout: StateLayout::new(1, 1, vec![crate::sim::tiling::BasisId(0)], 1)
                        .unwrap(),
                    steps_started: AtomicUsize::new(0),
                    release: Mutex::new(false),
                    tick: AtomicU64::new(0),
                }),
            }
        }

        fn wait_for_step_start(&self) {
            let deadline = Instant::now() + Duration::from_secs(5);
            while self.inner.steps_started.load(Ordering::Acquire) == 0 {
                assert!(Instant::now() < deadline, "step never started");
                std::thread::yield_now();
            }
        }

        fn finish_step(&self) {
            *self.inner.release.lock().unwrap() = true;
        }

        fn steps_started(&self) -> usize {
            self.inner.steps_started.load(Ordering::Acquire)
        }
    }

    impl LocalBackend for BlockingBackend {
        fn descriptor(&self) -> &BackendDescriptor {
            &self.inner.descriptor
        }

        fn tick(&self) -> u64 {
            self.inner.tick.load(Ordering::Acquire)
        }

        fn set_running_state(&mut self, _state: &WorldSnapshot) -> Result<(), BackendFailure> {
            Ok(())
        }

        fn apply_edits(&mut self, _edits: &[WorldEdit]) -> Result<(), BackendFailure> {
            Ok(())
        }

        fn step(&mut self, steps: u32) -> Result<StepStats, BackendFailure> {
            self.inner.steps_started.fetch_add(1, Ordering::AcqRel);
            let deadline = Instant::now() + Duration::from_secs(5);
            while !*self.inner.release.lock().unwrap() {
                assert!(Instant::now() < deadline, "step was never released");
                std::thread::yield_now();
            }
            self.inner.tick.fetch_add(steps as u64, Ordering::AcqRel);
            Ok(StepStats {
                steps,
                elapsed_micros: 0,
            })
        }

        fn readback(&mut self) -> Result<WorldSnapshot, BackendFailure> {
            Ok(WorldSnapshot {
                layout: self.inner.layout.clone(),
                cells: Arc::from(vec![0.0]),
            })
        }
    }

    fn fast_fixture() -> SimulationController {
        let spec = ExperimentSpec::single_channel_lenia(4, 4)
            .normalize_rules()
            .unwrap();
        let plan = compile_compute_plan(&spec).unwrap();
        let cells = initial_cells(&plan, &spec);
        SimulationController::spawn(Box::new(CpuBackend::new(&plan, &cells).unwrap())).unwrap()
    }

    #[test]
    fn worker_acks_pause_before_scheduling_another_step() {
        let fake = BlockingBackend::new();
        let controller = SimulationController::spawn(Box::new(fake.clone())).unwrap();
        controller
            .send(SimulationCommand::SetRunning(true))
            .unwrap();
        fake.wait_for_step_start();
        controller
            .send(SimulationCommand::SetRunning(false))
            .unwrap();
        fake.finish_step();
        let snapshot = controller.wait_for(|state| !state.running);
        assert_eq!(fake.steps_started(), 1);
        assert!(!snapshot.running);
    }

    #[test]
    fn unread_snapshots_are_replaced_not_queued() {
        let controller = fast_fixture();
        controller.step(100).unwrap();
        controller.wait_for(|state| state.tick >= 100);
        assert_eq!(controller.snapshot_slot_depth(), 1);
    }

    #[test]
    fn the_first_snapshot_is_available_before_any_command() {
        let controller = fast_fixture();
        let snapshot = controller.snapshot();
        assert_eq!(snapshot.tick, 0);
        assert!(!snapshot.running);
        assert_eq!(snapshot.cells.len(), 4 * 4);
    }

    #[test]
    fn an_edit_reaches_the_backend_and_appears_in_the_next_snapshot() {
        let controller = fast_fixture();
        controller
            .send(SimulationCommand::EditWorld(vec![WorldEdit {
                channel: 0,
                basis: 0,
                x: 2,
                y: 1,
                value: 0.5,
            }]))
            .unwrap();
        let snapshot = controller.wait_for(|state| {
            state
                .layout
                .index_by_position(0, 2, 1, 0)
                .is_some_and(|index| state.cells[index] == 0.5)
        });
        assert!(!snapshot.running);
    }

    #[test]
    fn reset_restores_the_state_the_worker_started_with() {
        let controller = fast_fixture();
        let start = controller.snapshot().cells.clone();
        controller
            .send(SimulationCommand::EditWorld(vec![WorldEdit {
                channel: 0,
                basis: 0,
                x: 0,
                y: 0,
                value: 1.0,
            }]))
            .unwrap();
        controller.wait_for(|state| state.cells[0] == 1.0);
        controller.send(SimulationCommand::Reset).unwrap();
        let snapshot = controller.wait_for(|state| state.cells[0] == start[0]);
        assert_eq!(snapshot.cells, start);
    }

    #[test]
    fn a_stopped_worker_reports_that_commands_cannot_be_sent() {
        let controller = fast_fixture();
        controller.send(SimulationCommand::Shutdown).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while controller.send(SimulationCommand::Step(1)).is_ok() {
            assert!(Instant::now() < deadline, "worker never stopped");
            std::thread::yield_now();
        }
    }
}
