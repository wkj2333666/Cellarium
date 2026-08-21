use super::cpu::CpuBackend;
#[cfg(feature = "cuda")]
use super::cuda::CudaBackend;
use super::rule::SimulationSpec;
use super::world::World;

pub use super::backend_error::BackendError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    Cpu,
    Cuda,
}

pub enum SimulationBackend {
    Cpu(Box<CpuBackend>),
    #[cfg(feature = "cuda")]
    Cuda(Box<CudaBackend>),
}

impl SimulationBackend {
    pub fn cpu(spec: SimulationSpec) -> Self {
        Self::Cpu(Box::new(CpuBackend::new(spec)))
    }

    pub fn cuda_or_cpu(spec: SimulationSpec, width: usize, height: usize) -> Self {
        #[cfg(feature = "cuda")]
        match CudaBackend::new(spec.clone(), width, height) {
            Ok(backend) => Self::Cuda(Box::new(backend)),
            Err(_) => Self::cpu(spec),
        }
        #[cfg(not(feature = "cuda"))]
        {
            let _ = (width, height);
            Self::cpu(spec)
        }
    }

    pub fn strict_for_kind(
        kind: BackendKind,
        spec: SimulationSpec,
        width: usize,
        height: usize,
    ) -> Result<Self, BackendError> {
        match kind {
            BackendKind::Cpu => Ok(Self::cpu(spec)),
            #[cfg(feature = "cuda")]
            BackendKind::Cuda => {
                CudaBackend::new(spec, width, height).map(|backend| Self::Cuda(Box::new(backend)))
            }
            #[cfg(not(feature = "cuda"))]
            BackendKind::Cuda => {
                let _ = (spec, width, height);
                Err(BackendError::CudaNotCompiled)
            }
        }
    }

    pub fn kind(&self) -> BackendKind {
        match self {
            Self::Cpu(_) => BackendKind::Cpu,
            #[cfg(feature = "cuda")]
            Self::Cuda(_) => BackendKind::Cuda,
        }
    }

    pub fn device_name(&self) -> &str {
        match self {
            Self::Cpu(_) => "CPU",
            #[cfg(feature = "cuda")]
            Self::Cuda(backend) => backend.device_name(),
        }
    }

    pub fn tick(&self) -> u64 {
        match self {
            Self::Cpu(backend) => backend.tick(),
            #[cfg(feature = "cuda")]
            Self::Cuda(backend) => backend.tick(),
        }
    }

    pub fn step(&mut self, world: &mut World) -> Result<(), BackendError> {
        match self {
            Self::Cpu(backend) => {
                backend.step(world)?;
                Ok(())
            }
            #[cfg(feature = "cuda")]
            Self::Cuda(backend) => backend.step(world),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "cuda")]
    use crate::sim::cpu::CpuBackend;
    use crate::sim::rule::SimulationSpec;
    #[cfg(feature = "cuda")]
    use crate::sim::world::World;

    #[cfg(feature = "cuda")]
    fn cuda_available() -> bool {
        CudaBackend::new(SimulationSpec::conway(), 1, 1).is_ok()
    }

    #[cfg(feature = "cuda")]
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
    #[cfg(feature = "cuda")]
    fn strict_backend_construction_does_not_fall_back_from_cuda() {
        let result =
            SimulationBackend::strict_for_kind(BackendKind::Cuda, SimulationSpec::conway(), 0, 0);

        assert!(matches!(result, Err(BackendError::InvalidWorld)));
    }

    #[cfg(not(feature = "cuda"))]
    #[test]
    fn cpu_only_build_falls_back_and_rejects_explicit_cuda() {
        let selected = SimulationBackend::cuda_or_cpu(SimulationSpec::conway(), 8, 8);
        assert_eq!(selected.kind(), BackendKind::Cpu);

        let strict =
            SimulationBackend::strict_for_kind(BackendKind::Cuda, SimulationSpec::conway(), 8, 8);
        assert!(matches!(strict, Err(BackendError::CudaNotCompiled)));
    }

    #[test]
    #[cfg(feature = "cuda")]
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

        let mut cpu = SimulationBackend::Cpu(Box::new(CpuBackend::new(SimulationSpec::conway())));
        let mut selected = SimulationBackend::cuda_or_cpu(SimulationSpec::conway(), 5, 5);
        cpu.step(&mut cpu_world).unwrap();
        selected.step(&mut selected_world).unwrap();

        assert_eq!(cpu_world.cells(), selected_world.cells());
        assert_eq!(selected.tick(), 1);
    }
}
