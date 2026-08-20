use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use cudarc::driver::{
    CudaContext, CudaFunction, CudaModule, CudaSlice, CudaStream, DriverError, LaunchConfig,
    PushKernelArg,
};
use cudarc::nvrtc::{CompileError, Ptx, compile_ptx};

use crate::sim::cuda_codegen::{CodegenError, generate_cuda_source};
use crate::sim::expression::{KernelExpression, KernelExpressionError};
use crate::sim::rule::{Rule, SimulationSpec};
use crate::sim::world::World;

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("CUDA driver error: {0}")]
    Driver(#[from] DriverError),
    #[error("CUDA compilation error: {0}")]
    Compile(#[from] CompileError),
    #[error("rule evaluation error: {0}")]
    RuleEvaluation(#[from] KernelExpressionError),
    #[error("CUDA code generation error: {0}")]
    Codegen(#[from] CodegenError),
    #[error("the runtime compilation cache is unavailable")]
    CompilationCachePoisoned,
    #[error("world dimensions must fit a CUDA launch")]
    InvalidWorld,
}

pub struct CudaBackend {
    spec: SimulationSpec,
    stream: Arc<CudaStream>,
    function: CudaFunction,
    kernel: CudaSlice<f32>,
    kernel_mask: CudaSlice<i32>,
    current: CudaSlice<f32>,
    next: CudaSlice<f32>,
    width: usize,
    height: usize,
    tick: u64,
    device_name: String,
}

static PTX_CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
static MODULE_CACHE: OnceLock<Mutex<HashMap<String, Arc<CudaModule>>>> = OnceLock::new();
static CUDA_CONTEXT: OnceLock<Arc<CudaContext>> = OnceLock::new();
const MAX_RUNTIME_CACHE_ENTRIES: usize = 32;

fn shared_context() -> Result<Arc<CudaContext>, BackendError> {
    if let Some(context) = CUDA_CONTEXT.get() {
        return Ok(context.clone());
    }
    let context = CudaContext::new(0)?;
    let _ = CUDA_CONTEXT.set(context.clone());
    Ok(CUDA_CONTEXT.get().cloned().unwrap_or(context))
}

fn evict_if_full<T>(cache: &mut HashMap<String, T>) {
    if cache.len() >= MAX_RUNTIME_CACHE_ENTRIES
        && let Some(key) = cache.keys().next().cloned()
    {
        cache.remove(&key);
    }
}

fn compile_cached(source: &str) -> Result<Ptx, BackendError> {
    let cache = PTX_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache
        .lock()
        .map_err(|_| BackendError::CompilationCachePoisoned)?;
    if let Some(ptx) = cache.get(source) {
        return Ok(Ptx::from_src(ptx.clone()));
    }

    let ptx = compile_ptx(source)?;
    evict_if_full(&mut cache);
    cache.insert(source.to_string(), ptx.to_src());
    Ok(ptx)
}

fn load_cached_module(
    context: &Arc<CudaContext>,
    source: &str,
) -> Result<Arc<CudaModule>, BackendError> {
    let cache = MODULE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache
        .lock()
        .map_err(|_| BackendError::CompilationCachePoisoned)?;
    if let Some(module) = cache.get(source) {
        return Ok(module.clone());
    }

    let module = context.load_module(compile_cached(source)?)?;
    evict_if_full(&mut cache);
    cache.insert(source.to_string(), module.clone());
    Ok(module)
}

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

        let context = shared_context()?;
        let stream = context.default_stream();
        let device_name = context.name()?;
        let fallback_growth = KernelExpression::Constant(0.0);
        let growth = spec.growth_expression().unwrap_or(&fallback_growth);
        let generated = generate_cuda_source(growth)?;
        let module = load_cached_module(&context, &generated.source)?;
        let function = module.load_function(generated.entry_point)?;
        let kernel_values = if spec.kernel.values.is_empty() {
            vec![0.0]
        } else {
            spec.kernel.values.clone()
        };
        let mask_values = if spec
            .kernel
            .mask
            .as_ref()
            .is_some_and(|mask| !mask.is_empty())
        {
            spec.kernel
                .mask
                .iter()
                .flatten()
                .map(|active| i32::from(*active))
                .collect()
        } else {
            vec![1; spec.kernel.values.len().max(1)]
        };
        let kernel = stream.clone_htod(&kernel_values)?;
        let kernel_mask = stream.clone_htod(&mask_values)?;
        let cells = width * height;
        let current = stream.alloc_zeros::<f32>(cells)?;
        let next = stream.alloc_zeros::<f32>(cells)?;

        Ok(Self {
            spec,
            stream,
            function,
            kernel,
            kernel_mask,
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
        let kernel_width = self.spec.kernel.width as i32;
        let kernel_height = self.spec.kernel.height as i32;
        let kernel_anchor_x = self.spec.kernel.anchor_x as i32;
        let kernel_anchor_y = self.spec.kernel.anchor_y as i32;
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
                .arg(&kernel_width)
                .arg(&kernel_height)
                .arg(&kernel_anchor_x)
                .arg(&kernel_anchor_y)
                .arg(&self.kernel_mask)
                .arg(&mode)
                .arg(&self.spec.dt)
                .arg(&mu)
                .arg(&sigma)
                .launch(LaunchConfig::for_num_elems(cell_count as u32))
        }?;
        let updated = self.stream.clone_dtoh(&self.next)?;
        self.stream.synchronize()?;
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
    use crate::sim::kernel::{KernelDefinition, KernelValues, Normalization};
    use crate::sim::rule::SimulationSpec;
    use crate::sim::world::World;
    use std::collections::BTreeMap;

    fn kernel_spec(definition: KernelDefinition) -> SimulationSpec {
        SimulationSpec {
            rule: Rule::Lenia {
                mu: 0.5,
                sigma: 1.0,
            },
            kernel: definition.build().expect("test kernel is valid"),
            dt: 0.1,
            growth: SimulationSpec::lenia_orbium().growth,
        }
    }

    fn centered_square_kernel() -> KernelDefinition {
        KernelDefinition {
            name: "centered-square".to_string(),
            width: 3,
            height: 3,
            anchor_x: 1,
            anchor_y: 1,
            mask: None,
            normalization: Normalization::Sum,
            parameters: BTreeMap::new(),
            values: KernelValues::Explicit(vec![1.0; 9]),
        }
    }

    fn non_square_kernel() -> KernelDefinition {
        KernelDefinition {
            name: "non-square".to_string(),
            width: 5,
            height: 3,
            anchor_x: 2,
            anchor_y: 1,
            mask: None,
            normalization: Normalization::Sum,
            parameters: BTreeMap::new(),
            values: KernelValues::Explicit(vec![1.0; 15]),
        }
    }

    fn asymmetric_masked_kernel() -> KernelDefinition {
        KernelDefinition {
            name: "asymmetric-masked".to_string(),
            width: 3,
            height: 2,
            anchor_x: 2,
            anchor_y: 0,
            mask: Some(vec![false, true, true, false, true, false]),
            normalization: Normalization::Sum,
            parameters: BTreeMap::new(),
            values: KernelValues::Explicit(vec![0.0, 0.5, 1.0, 0.0, 2.0, 0.0]),
        }
    }

    fn unnormalized_kernel() -> KernelDefinition {
        KernelDefinition {
            name: "unnormalized".to_string(),
            width: 3,
            height: 1,
            anchor_x: 1,
            anchor_y: 0,
            mask: None,
            normalization: Normalization::None,
            parameters: BTreeMap::new(),
            values: KernelValues::Explicit(vec![1.0, 2.0, 3.0]),
        }
    }

    fn cuda_available() -> bool {
        CudaBackend::new(SimulationSpec::conway(), 1, 1).is_ok()
    }

    fn identical_step_matches_cpu(spec: SimulationSpec, width: usize, height: usize) {
        let mut cpu_world = World::new(width, height);
        let mut gpu_world = World::new(width, height);
        cpu_world.randomize(1234, 0.31);
        gpu_world.randomize(1234, 0.31);

        let mut cpu = CpuBackend::new(spec.clone());
        let mut gpu = CudaBackend::new(spec, width, height).expect("CUDA device is available");
        cpu.step(&mut cpu_world).unwrap();
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
        if !cuda_available() {
            return;
        }
        identical_step_matches_cpu(SimulationSpec::conway(), 32, 24);
    }

    #[test]
    fn lenia_step_matches_cpu_backend() {
        if !cuda_available() {
            return;
        }
        identical_step_matches_cpu(SimulationSpec::lenia_orbium(), 32, 24);
    }

    #[test]
    fn edited_growth_expression_is_jit_compiled_and_matches_cpu_backend() {
        if !cuda_available() {
            return;
        }
        let spec = SimulationSpec::lenia_orbium()
            .with_growth_expression("clamp((potential - mu) / sigma, -0.25, 0.25)")
            .unwrap();

        identical_step_matches_cpu(spec, 19, 13);
    }

    #[test]
    fn runtime_compilation_cache_reuses_the_same_generated_source() {
        if compile_ptx("extern \"C\" __global__ void probe() {}").is_err() {
            return;
        }
        let generated = generate_cuda_source(&KernelExpression::Constant(0.123_456_7)).unwrap();
        PTX_CACHE
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .remove(&generated.source);

        let first = compile_cached(&generated.source).unwrap().to_src();
        assert!(
            PTX_CACHE
                .get()
                .unwrap()
                .lock()
                .unwrap()
                .contains_key(&generated.source)
        );
        let second = compile_cached(&generated.source).unwrap().to_src();

        assert_eq!(first, second);
    }

    #[test]
    fn runtime_module_cache_reuses_the_loaded_driver_module() {
        let Ok(context) = shared_context() else {
            return;
        };
        let generated = generate_cuda_source(&KernelExpression::Constant(0.234_567_8)).unwrap();
        MODULE_CACHE
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .remove(&generated.source);

        let first = load_cached_module(&context, &generated.source).unwrap();
        let second = load_cached_module(&context, &generated.source).unwrap();

        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn nvrtc_errors_retain_the_user_expression_source_mapping() {
        if compile_ptx("extern \"C\" __global__ void probe() {}").is_err() {
            return;
        }
        let generated = generate_cuda_source(&KernelExpression::Constant(0.0)).unwrap();
        let invalid = generated
            .source
            .replace("return 0.0f;", "return invalid @ token;");

        let error = compile_cached(&invalid).unwrap_err();
        let BackendError::Compile(CompileError::CompileError { log, .. }) = error else {
            panic!("expected an NVRTC compiler diagnostic");
        };

        assert!(
            log.to_string_lossy()
                .contains("cellarium-growth-expression")
        );
    }

    #[test]
    fn centered_square_step_matches_cpu_backend() {
        if !cuda_available() {
            return;
        }
        identical_step_matches_cpu(kernel_spec(centered_square_kernel()), 17, 11);
    }

    #[test]
    fn non_square_step_matches_cpu_backend() {
        if !cuda_available() {
            return;
        }
        identical_step_matches_cpu(kernel_spec(non_square_kernel()), 17, 11);
    }

    #[test]
    fn asymmetric_masked_step_matches_cpu_backend() {
        if !cuda_available() {
            return;
        }
        identical_step_matches_cpu(kernel_spec(asymmetric_masked_kernel()), 17, 11);
    }

    #[test]
    fn unnormalized_step_matches_cpu_backend() {
        if !cuda_available() {
            return;
        }
        identical_step_matches_cpu(kernel_spec(unnormalized_kernel()), 17, 11);
    }

    #[test]
    fn device_name_is_reported_without_exposing_cuda_handles() {
        let Ok(backend) = CudaBackend::new(SimulationSpec::conway(), 8, 8) else {
            return;
        };
        assert!(!backend.device_name().is_empty());
    }
}
