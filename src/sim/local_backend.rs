//! The contract every local compute backend implements.
//!
//! A backend owns its working state between steps. Host readback happens only
//! for snapshots, edits, Apply transitions and recovery, never once per step, so
//! a GPU backend can keep everything device-resident.

use std::sync::Arc;

use crate::sim::basis_runtime::{CompiledBasisExperiment, StateLayout};
use crate::sim::compute_plan::ComputePlan;
use crate::sim::world::ChannelWorld;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendKind {
    Cuda,
    Wgpu,
    Cpu,
}

impl BackendKind {
    pub fn label(self) -> &'static str {
        match self {
            BackendKind::Cuda => "CUDA",
            BackendKind::Wgpu => "wgpu",
            BackendKind::Cpu => "CPU",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendDescriptor {
    pub kind: BackendKind,
    pub device_name: String,
}

impl BackendDescriptor {
    pub fn cpu() -> Self {
        Self {
            kind: BackendKind::Cpu,
            device_name: "CPU reference".into(),
        }
    }

    /// Short text for the status bar, e.g. `CPU (CPU reference)`.
    pub fn summary(&self) -> String {
        format!("{} ({})", self.kind.label(), self.device_name)
    }
}

/// How a probed adapter is classified, used to order Auto candidates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GpuDeviceType {
    Discrete,
    Integrated,
    Virtual,
    Cpu,
    Other,
}

/// The result of asking whether a backend can run a plan on this machine.
/// An unavailable probe always carries the reason, so Settings can show why
/// rather than silently falling back.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendProbe {
    pub kind: BackendKind,
    pub available: bool,
    pub device_name: Option<String>,
    pub device_type: Option<GpuDeviceType>,
    pub reason: Option<String>,
}

impl BackendProbe {
    pub fn available(
        kind: BackendKind,
        device_name: impl Into<String>,
        device_type: GpuDeviceType,
    ) -> Self {
        Self {
            kind,
            available: true,
            device_name: Some(device_name.into()),
            device_type: Some(device_type),
            reason: None,
        }
    }

    pub fn unavailable(kind: BackendKind, reason: impl Into<String>) -> Self {
        Self {
            kind,
            available: false,
            device_name: None,
            device_type: None,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum BackendFailure {
    #[error("backend rejected the plan: {0}")]
    Unsupported(String),
    #[error("backend state is invalid: {0}")]
    State(String),
    #[error("backend step failed: {0}")]
    Step(String),
}

/// A full copy of the simulated state plus the layout needed to read it.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldSnapshot {
    pub layout: StateLayout,
    pub cells: Arc<[f32]>,
}

/// One user edit of the world, in dense layout coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldEdit {
    pub channel: usize,
    pub basis: usize,
    pub x: usize,
    pub y: usize,
    pub value: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StepStats {
    pub steps: u32,
    pub elapsed_micros: u64,
}

pub trait LocalBackend: Send {
    fn descriptor(&self) -> &BackendDescriptor;
    fn tick(&self) -> u64;
    fn set_running_state(&mut self, state: &WorldSnapshot) -> Result<(), BackendFailure>;
    fn apply_edits(&mut self, edits: &[WorldEdit]) -> Result<(), BackendFailure>;
    fn step(&mut self, steps: u32) -> Result<StepStats, BackendFailure>;
    fn readback(&mut self) -> Result<WorldSnapshot, BackendFailure>;
}

/// The reference backend. Its results define correct behaviour for every other
/// backend.
pub struct CpuBackend {
    descriptor: BackendDescriptor,
    runtime: CompiledBasisExperiment,
    world: ChannelWorld,
    layout: StateLayout,
    tick: u64,
}

impl CpuBackend {
    pub fn new(plan: &ComputePlan, initial: &[f32]) -> Result<Self, BackendFailure> {
        let runtime = CompiledBasisExperiment::from_plan(plan);
        let layout = plan.layout.clone();
        let channel_len = plan.width as usize * plan.height as usize * plan.bases.len();
        if initial.len() != channel_len * plan.channels.len() {
            return Err(BackendFailure::State(format!(
                "initial state has {} scalars but the plan needs {}",
                initial.len(),
                channel_len * plan.channels.len()
            )));
        }
        let channels = initial
            .chunks(channel_len)
            .map(<[f32]>::to_vec)
            .collect::<Vec<_>>();
        let world = ChannelWorld::from_basis_channels(
            plan.width as usize,
            plan.height as usize,
            plan.bases.len(),
            &channels,
        )
        .map_err(|error| BackendFailure::State(error.to_string()))?;
        Ok(Self {
            descriptor: BackendDescriptor::cpu(),
            runtime,
            world,
            layout,
            tick: 0,
        })
    }

    /// Build from a plan using each channel's declared initial values.
    pub fn from_plan(plan: &ComputePlan, initial: &[f32]) -> Result<Self, BackendFailure> {
        Self::new(plan, initial)
    }

    pub fn layout(&self) -> &StateLayout {
        &self.layout
    }
}

impl LocalBackend for CpuBackend {
    fn descriptor(&self) -> &BackendDescriptor {
        &self.descriptor
    }

    fn tick(&self) -> u64 {
        self.tick
    }

    fn set_running_state(&mut self, state: &WorldSnapshot) -> Result<(), BackendFailure> {
        if state.layout != self.layout {
            return Err(BackendFailure::State(
                "snapshot layout does not match the compiled plan".into(),
            ));
        }
        self.world
            .replace_all(&state.cells)
            .map_err(|error| BackendFailure::State(error.to_string()))
    }

    fn apply_edits(&mut self, edits: &[WorldEdit]) -> Result<(), BackendFailure> {
        for edit in edits {
            if self
                .layout
                .index_by_position(edit.channel, edit.x, edit.y, edit.basis)
                .is_none()
            {
                return Err(BackendFailure::State(format!(
                    "edit at channel {} basis {} ({}, {}) is outside the world",
                    edit.channel, edit.basis, edit.x, edit.y
                )));
            }
            self.world.set_basis(
                edit.channel,
                edit.x as isize,
                edit.y as isize,
                edit.basis,
                edit.value,
            );
        }
        Ok(())
    }

    fn step(&mut self, steps: u32) -> Result<StepStats, BackendFailure> {
        let started = std::time::Instant::now();
        for _ in 0..steps {
            self.runtime
                .step(&mut self.world)
                .map_err(|error| BackendFailure::Step(error.to_string()))?;
            self.tick += 1;
        }
        Ok(StepStats {
            steps,
            elapsed_micros: started.elapsed().as_micros() as u64,
        })
    }

    fn readback(&mut self) -> Result<WorldSnapshot, BackendFailure> {
        Ok(WorldSnapshot {
            layout: self.layout.clone(),
            cells: Arc::from(self.world.cells().to_vec()),
        })
    }
}

/// Flat initial state for a plan, taken from each channel's declared initial
/// values. A channel that declares fewer values than the world holds keeps zeros
/// for the remainder rather than shifting the rest of the layout.
pub fn initial_cells(
    plan: &ComputePlan,
    spec: &crate::sim::experiment_model::ExperimentSpec,
) -> Vec<f32> {
    let channel_len = plan.width as usize * plan.height as usize * plan.bases.len();
    let mut cells = vec![0.0; channel_len * plan.channels.len()];
    for channel in &plan.channels {
        let Some(source) = spec
            .channels
            .iter()
            .find(|entry| entry.id == channel.id)
            .map(|entry| &entry.initial)
        else {
            continue;
        };
        let base = channel.index * channel_len;
        for (offset, value) in source.iter().take(channel_len).enumerate() {
            cells[base + offset] = *value;
        }
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::compute_plan::compile_compute_plan;
    use crate::sim::experiment_model::ExperimentSpec;

    fn cpu_fixture() -> (ComputePlan, CpuBackend) {
        let spec = ExperimentSpec::single_channel_lenia(4, 4)
            .normalize_rules()
            .unwrap();
        let plan = compile_compute_plan(&spec).unwrap();
        let cells = initial_cells(&plan, &spec);
        let backend = CpuBackend::new(&plan, &cells).unwrap();
        (plan, backend)
    }

    #[test]
    fn the_cpu_backend_reports_itself_and_starts_at_tick_zero() {
        let (_, backend) = cpu_fixture();
        assert_eq!(backend.descriptor().kind, BackendKind::Cpu);
        assert_eq!(backend.tick(), 0);
        assert!(backend.descriptor().summary().starts_with("CPU"));
    }

    #[test]
    fn stepping_advances_the_tick_and_reports_stats() {
        let (_, mut backend) = cpu_fixture();
        let stats = backend.step(3).unwrap();
        assert_eq!(stats.steps, 3);
        assert_eq!(backend.tick(), 3);
    }

    #[test]
    fn an_edit_is_visible_in_the_next_readback() {
        let (_, mut backend) = cpu_fixture();
        backend
            .apply_edits(&[WorldEdit {
                channel: 0,
                basis: 0,
                x: 1,
                y: 2,
                value: 0.75,
            }])
            .unwrap();
        let snapshot = backend.readback().unwrap();
        let index = snapshot.layout.index_by_position(0, 1, 2, 0).unwrap();
        assert_eq!(snapshot.cells[index], 0.75);
    }

    #[test]
    fn an_edit_outside_the_world_is_refused_without_touching_state() {
        let (_, mut backend) = cpu_fixture();
        let before = backend.readback().unwrap();
        let error = backend
            .apply_edits(&[WorldEdit {
                channel: 0,
                basis: 0,
                x: 99,
                y: 0,
                value: 1.0,
            }])
            .unwrap_err();
        assert!(matches!(error, BackendFailure::State(_)));
        assert_eq!(backend.readback().unwrap(), before);
    }

    #[test]
    fn a_snapshot_from_another_layout_is_refused() {
        let (_, mut backend) = cpu_fixture();
        let other = ExperimentSpec::single_channel_lenia(8, 8)
            .normalize_rules()
            .unwrap();
        let other_plan = compile_compute_plan(&other).unwrap();
        let snapshot = WorldSnapshot {
            layout: other_plan.layout.clone(),
            cells: Arc::from(vec![0.0; other_plan.state_scalars()]),
        };
        assert!(backend.set_running_state(&snapshot).is_err());
    }

    #[test]
    fn restoring_a_snapshot_reproduces_its_cells() {
        let (_, mut backend) = cpu_fixture();
        backend
            .apply_edits(&[WorldEdit {
                channel: 0,
                basis: 0,
                x: 0,
                y: 0,
                value: 0.5,
            }])
            .unwrap();
        let saved = backend.readback().unwrap();
        backend.step(2).unwrap();
        backend.set_running_state(&saved).unwrap();
        assert_eq!(backend.readback().unwrap(), saved);
    }
}
