use std::sync::Arc;

use cudarc::driver::{
    CudaContext, CudaFunction, CudaSlice, CudaStream, DriverError, LaunchConfig, PushKernelArg,
};
use cudarc::nvrtc::{CompileError, compile_ptx};

use crate::sim::rule::{Rule, SimulationSpec};
use crate::sim::world::World;

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("CUDA driver error: {0}")]
    Driver(#[from] DriverError),
    #[error("CUDA compilation error: {0}")]
    Compile(#[from] CompileError),
    #[error("world dimensions must fit a CUDA launch")]
    InvalidWorld,
}

pub struct CudaBackend {
    spec: SimulationSpec,
    stream: Arc<CudaStream>,
    function: CudaFunction,
    kernel: CudaSlice<f32>,
    current: CudaSlice<f32>,
    next: CudaSlice<f32>,
    width: usize,
    height: usize,
    tick: u64,
    device_name: String,
}

const CUDA_SOURCE: &str = r#"
extern "C" __device__ int cellarium_wrap(int value, int size) {
    int wrapped = value % size;
    return wrapped < 0 ? wrapped + size : wrapped;
}

extern "C" __global__ void cellarium_step(
    float* next,
    const float* current,
    const float* kernel,
    int width,
    int height,
    int radius,
    int mode,
    float dt,
    float mu,
    float sigma
) {
    int linear = blockIdx.x * blockDim.x + threadIdx.x;
    int cell_count = width * height;
    if (linear >= cell_count) return;

    int x = linear % width;
    int y = linear / width;
    if (mode == 0) {
        int neighbors = 0;
        for (int dy = -1; dy <= 1; ++dy) {
            for (int dx = -1; dx <= 1; ++dx) {
                if (dx == 0 && dy == 0) continue;
                int nx = cellarium_wrap(x + dx, width);
                int ny = cellarium_wrap(y + dy, height);
                neighbors += current[ny * width + nx] > 0.5f ? 1 : 0;
            }
        }
        float self_state = current[linear];
        bool alive = self_state > 0.5f;
        bool survives = alive && (neighbors == 2 || neighbors == 3);
        bool born = !alive && neighbors == 3;
        next[linear] = (survives || born) ? 1.0f : 0.0f;
    } else {
        float potential = 0.0f;
        int kernel_size = radius * 2 + 1;
        for (int ky = -radius; ky <= radius; ++ky) {
            for (int kx = -radius; kx <= radius; ++kx) {
                int nx = cellarium_wrap(x + kx, width);
                int ny = cellarium_wrap(y + ky, height);
                int kernel_index = (ky + radius) * kernel_size + kx + radius;
                potential += kernel[kernel_index] * current[ny * width + nx];
            }
        }
        float ratio = (potential - mu) / sigma;
        float growth = 2.0f * expf(-(ratio * ratio)) - 1.0f;
        float updated = current[linear] + dt * growth;
        next[linear] = fminf(1.0f, fmaxf(0.0f, updated));
    }
}
"#;

impl CudaBackend {
    pub fn new(spec: SimulationSpec, width: usize, height: usize) -> Result<Self, BackendError> {
        if width == 0
            || height == 0
            || width > i32::MAX as usize
            || height > i32::MAX as usize
            || width
                .checked_mul(height)
                .is_none_or(|cells| cells > u32::MAX as usize)
        {
            return Err(BackendError::InvalidWorld);
        }

        let context = CudaContext::new(0)?;
        let stream = context.default_stream();
        let device_name = context.name()?;
        let ptx = compile_ptx(CUDA_SOURCE)?;
        let module = context.load_module(ptx)?;
        let function = module.load_function("cellarium_step")?;
        let kernel_values = if spec.kernel.values.is_empty() {
            vec![0.0]
        } else {
            spec.kernel.values.clone()
        };
        let kernel = stream.clone_htod(&kernel_values)?;
        let cells = width * height;
        let current = stream.alloc_zeros::<f32>(cells)?;
        let next = stream.alloc_zeros::<f32>(cells)?;

        Ok(Self {
            spec,
            stream,
            function,
            kernel,
            current,
            next,
            width,
            height,
            tick: 0,
            device_name,
        })
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn step(&mut self, world: &mut World) -> Result<(), BackendError> {
        assert_eq!(
            (world.width(), world.height()),
            (self.width, self.height),
            "CUDA buffers and world must have the same shape"
        );
        self.stream.memcpy_htod(world.cells(), &mut self.current)?;
        let (mode, mu, sigma) = match self.spec.rule {
            Rule::Conway => (0_i32, 0.0_f32, 1.0_f32),
            Rule::Lenia { mu, sigma } => (1_i32, mu, sigma),
        };
        let radius = self.spec.kernel.radius as i32;
        let width = self.width as i32;
        let height = self.height as i32;
        let cell_count = self.width * self.height;

        unsafe {
            self.stream
                .launch_builder(&self.function)
                .arg(&mut self.next)
                .arg(&self.current)
                .arg(&self.kernel)
                .arg(&width)
                .arg(&height)
                .arg(&radius)
                .arg(&mode)
                .arg(&self.spec.dt)
                .arg(&mu)
                .arg(&sigma)
                .launch(LaunchConfig::for_num_elems(cell_count as u32))
        }?;
        let updated = self.stream.clone_dtoh(&self.next)?;
        world.replace_cells(&updated);
        std::mem::swap(&mut self.current, &mut self.next);
        self.tick += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::cpu::CpuBackend;
    use crate::sim::rule::SimulationSpec;
    use crate::sim::world::World;

    fn identical_step_matches_cpu(spec: SimulationSpec, width: usize, height: usize) {
        let mut cpu_world = World::new(width, height);
        let mut gpu_world = World::new(width, height);
        cpu_world.randomize(1234, 0.31);
        gpu_world.randomize(1234, 0.31);

        let mut cpu = CpuBackend::new(spec.clone());
        let mut gpu = CudaBackend::new(spec, width, height).expect("CUDA device is available");
        cpu.step(&mut cpu_world);
        gpu.step(&mut gpu_world).unwrap();

        for (cpu_value, gpu_value) in cpu_world.cells().iter().zip(gpu_world.cells()) {
            assert!(
                (cpu_value - gpu_value).abs() <= 1e-5,
                "CPU {cpu_value} did not match CUDA {gpu_value}"
            );
        }
        assert_eq!(gpu.tick(), 1);
    }

    #[test]
    fn conway_step_matches_cpu_backend() {
        identical_step_matches_cpu(SimulationSpec::conway(), 32, 24);
    }

    #[test]
    fn lenia_step_matches_cpu_backend() {
        identical_step_matches_cpu(SimulationSpec::lenia_orbium(), 32, 24);
    }

    #[test]
    fn device_name_is_reported_without_exposing_cuda_handles() {
        let backend = CudaBackend::new(SimulationSpec::conway(), 8, 8).unwrap();
        assert!(!backend.device_name().is_empty());
    }
}
