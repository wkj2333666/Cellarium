use super::cpu::CpuBackend;
use super::cuda::{BackendError, CudaBackend};
use super::rule::SimulationSpec;
use super::world::World;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    Cpu,
    Cuda,
}

pub enum SimulationBackend {
    Cpu(CpuBackend),
    Cuda(Box<CudaBackend>),
}

impl SimulationBackend {
    pub fn cpu(spec: SimulationSpec) -> Self {
        Self::Cpu(CpuBackend::new(spec))
    }

    pub fn cuda_or_cpu(spec: SimulationSpec, width: usize, height: usize) -> Self {
        match CudaBackend::new(spec.clone(), width, height) {
            Ok(backend) => Self::Cuda(Box::new(backend)),
            Err(_) => Self::cpu(spec),
        }
    }

    pub fn kind(&self) -> BackendKind {
        match self {
            Self::Cpu(_) => BackendKind::Cpu,
            Self::Cuda(_) => BackendKind::Cuda,
        }
    }

    pub fn device_name(&self) -> &str {
        match self {
            Self::Cpu(_) => "CPU",
            Self::Cuda(backend) => backend.device_name(),
        }
    }

    pub fn tick(&self) -> u64 {
        match self {
            Self::Cpu(backend) => backend.tick(),
            Self::Cuda(backend) => backend.tick(),
        }
    }

    pub fn step(&mut self, world: &mut World) -> Result<(), BackendError> {
        match self {
            Self::Cpu(backend) => {
                backend.step(world);
                Ok(())
            }
            Self::Cuda(backend) => backend.step(world),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::cpu::CpuBackend;
    use crate::sim::rule::SimulationSpec;
    use crate::sim::world::World;

    fn cuda_available() -> bool {
        CudaBackend::new(SimulationSpec::conway(), 1, 1).is_ok()
    }

    #[test]
    fn selects_cuda_when_available_and_reports_the_actual_backend() {
        if !cuda_available() {
            let backend = SimulationBackend::cuda_or_cpu(SimulationSpec::conway(), 8, 8);
            assert!(matches!(backend, SimulationBackend::Cpu(_)));
            return;
        }

        let backend = SimulationBackend::cuda_or_cpu(SimulationSpec::conway(), 8, 8);
        assert!(matches!(backend, SimulationBackend::Cuda(_)));
        assert_eq!(backend.kind(), BackendKind::Cuda);
        assert!(!backend.device_name().is_empty());
    }

    #[test]
    fn delegates_conway_step_to_the_selected_backend() {
        if !cuda_available() {
            return;
        }

        let mut cpu_world = World::new(5, 5);
        let mut selected_world = World::new(5, 5);
        cpu_world.set(2, 1, 1.0);
        cpu_world.set(2, 2, 1.0);
        cpu_world.set(2, 3, 1.0);
        selected_world.set(2, 1, 1.0);
        selected_world.set(2, 2, 1.0);
        selected_world.set(2, 3, 1.0);

        let mut cpu = SimulationBackend::Cpu(CpuBackend::new(SimulationSpec::conway()));
        let mut selected = SimulationBackend::cuda_or_cpu(SimulationSpec::conway(), 5, 5);
        cpu.step(&mut cpu_world).unwrap();
        selected.step(&mut selected_world).unwrap();

        assert_eq!(cpu_world.cells(), selected_world.cells());
        assert_eq!(selected.tick(), 1);
    }
}
