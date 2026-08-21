use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

use cudarc::driver::{
    CudaContext, CudaFunction, CudaModule, CudaSlice, CudaStream, LaunchConfig, PushKernelArg,
};
#[cfg(test)]
use cudarc::nvrtc::CompileError;
use cudarc::nvrtc::{Ptx, compile_ptx};

pub use crate::sim::backend_error::BackendError;
use crate::sim::cuda_codegen::{
    generate_cuda_expression, generate_cuda_source, generate_program_cuda_source,
    generate_topology_cuda_source, program_kernel_data,
};
use crate::sim::experiment_model::UpdateMode;
use crate::sim::expression::KernelExpression;
use crate::sim::program::InputSource;
use crate::sim::rule::{Rule, SimulationSpec};
use crate::sim::runtime::{CompiledExperiment, RuntimeError};
use crate::sim::topology::CompiledTopology;
use crate::sim::world::{ChannelWorld, World};

struct ProgramCudaBuffers {
    masks: CudaSlice<i32>,
    offsets: CudaSlice<i32>,
    widths: CudaSlice<i32>,
    heights: CudaSlice<i32>,
    anchor_x: CudaSlice<i32>,
    anchor_y: CudaSlice<i32>,
    channels: CudaSlice<i32>,
    parameters: Vec<f32>,
}

pub struct CudaTopologyBackend {
    stream: Arc<CudaStream>,
    function: CudaFunction,
    offsets: CudaSlice<u32>,
    neighbors: CudaSlice<u32>,
    weights: CudaSlice<f32>,
    current: CudaSlice<f32>,
    next: CudaSlice<f32>,
    count: usize,
}

pub struct CudaBackend {
    spec: SimulationSpec,
    stream: Arc<CudaStream>,
    function: CudaFunction,
    kernel: CudaSlice<f32>,
    kernel_mask: CudaSlice<i32>,
    program: Option<ProgramCudaBuffers>,
    current: CudaSlice<f32>,
    next: CudaSlice<f32>,
    width: usize,
    height: usize,
    channel_count: usize,
    tick: u64,
    device_name: String,
}

pub struct CudaExperimentBackend {
    stream: Arc<CudaStream>,
    functions: Vec<CudaFunction>,
    kernel_values: CudaSlice<f32>,
    kernel_masks: CudaSlice<i32>,
    kernel_offsets: CudaSlice<i32>,
    kernel_widths: CudaSlice<i32>,
    kernel_heights: CudaSlice<i32>,
    kernel_anchor_x: CudaSlice<i32>,
    kernel_anchor_y: CudaSlice<i32>,
    kernel_sources: CudaSlice<i32>,
    channel_constants: CudaSlice<f32>,
    current: CudaSlice<f32>,
    next: CudaSlice<f32>,
    width: usize,
    height: usize,
    channels: usize,
    boundary_mode: i32,
    dt: f32,
    rules: Vec<CudaTargetRule>,
    tick: u64,
    device_name: String,
}

struct CudaTargetRule {
    target: usize,
    mode: UpdateMode,
    parameters: Vec<f32>,
}

static PTX_CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
static MODULE_CACHE: OnceLock<Mutex<HashMap<String, Arc<CudaModule>>>> = OnceLock::new();
static CUDA_CONTEXT: OnceLock<Arc<CudaContext>> = OnceLock::new();
const MAX_RUNTIME_CACHE_ENTRIES: usize = 32;

fn require_dynamic_library(present: bool) -> Result<(), BackendError> {
    if present {
        Ok(())
    } else {
        Err(BackendError::CudaUnavailable)
    }
}

fn nvrtc_available() -> bool {
    // SAFETY: cudarc's presence probe only attempts to load candidate shared
    // libraries and does not dereference caller-provided pointers.
    unsafe { cudarc::nvrtc::sys::is_culib_present() }
}

fn shared_context() -> Result<Arc<CudaContext>, BackendError> {
    if let Some(context) = CUDA_CONTEXT.get() {
        return Ok(context.clone());
    }
    // cudarc's dynamic loader panics when no candidate library exists. Its
    // presence probe uses the same candidate list without entering that path.
    require_dynamic_library(unsafe { cudarc::driver::sys::is_culib_present() })?;
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

    require_dynamic_library(nvrtc_available())?;
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

impl CudaTopologyBackend {
    pub fn new(topology: &CompiledTopology) -> Result<Self, BackendError> {
        let count = topology.site_count();
        if count == 0
            || topology.offsets.len() != count + 1
            || topology.offsets.first().copied() != Some(0)
            || topology.offsets.last().copied().map(|value| value as usize)
                != Some(topology.neighbors.len())
            || topology.weights.len() != topology.neighbors.len()
            || topology
                .neighbors
                .iter()
                .any(|neighbor| *neighbor as usize >= count)
            || topology.weights.iter().any(|weight| !weight.is_finite())
        {
            return Err(BackendError::InvalidTopology);
        }
        let context = shared_context()?;
        let stream = context.default_stream();
        let generated = generate_topology_cuda_source();
        let module = load_cached_module(&context, &generated.source)?;
        let function = module.load_function(generated.entry_point)?;
        Ok(Self {
            offsets: stream.clone_htod(&topology.offsets)?,
            neighbors: stream.clone_htod(&topology.neighbors)?,
            weights: stream.clone_htod(&topology.weights)?,
            current: stream.alloc_zeros(count)?,
            next: stream.alloc_zeros(count)?,
            stream,
            function,
            count,
        })
    }

    pub fn step(&mut self, state: &mut [f32], dt: f32) -> Result<(), BackendError> {
        if state.len() != self.count || !dt.is_finite() {
            return Err(BackendError::InvalidTopology);
        }
        self.stream.memcpy_htod(state, &mut self.current)?;
        let count = self.count as u32;
        unsafe {
            self.stream
                .launch_builder(&self.function)
                .arg(&mut self.next)
                .arg(&self.current)
                .arg(&self.offsets)
                .arg(&self.neighbors)
                .arg(&self.weights)
                .arg(&dt)
                .arg(&count)
                .launch(LaunchConfig::for_num_elems(count))
        }?;
        let updated = self.stream.clone_dtoh(&self.next)?;
        self.stream.synchronize()?;
        state.copy_from_slice(&updated);
        std::mem::swap(&mut self.current, &mut self.next);
        Ok(())
    }
}

impl CudaBackend {
    pub fn new(spec: SimulationSpec, width: usize, height: usize) -> Result<Self, BackendError> {
        Self::new_with_channels(spec, width, height, 1)
    }

    pub fn new_with_channels(
        spec: SimulationSpec,
        width: usize,
        height: usize,
        channel_count: usize,
    ) -> Result<Self, BackendError> {
        if width == 0
            || height == 0
            || channel_count == 0
            || width > i32::MAX as usize
            || height > i32::MAX as usize
            || width
                .checked_mul(height)
                .and_then(|cells| cells.checked_mul(channel_count))
                .is_none_or(|cells| cells > u32::MAX as usize)
        {
            return Err(BackendError::InvalidWorld);
        }
        if channel_count != 1 && !matches!(spec.rule, Rule::Program(_)) {
            return Err(BackendError::InvalidWorld);
        }
        if let Rule::Program(program) = &spec.rule
            && program.inputs.iter().any(|input| match input.source {
                InputSource::ChannelState { channel }
                | InputSource::ChannelConvolution { channel, .. } => channel >= channel_count,
                InputSource::State | InputSource::Convolution { .. } => false,
            })
        {
            return Err(BackendError::InvalidWorld);
        }

        let context = shared_context()?;
        let stream = context.default_stream();
        let device_name = context.name()?;
        let program_data = match &spec.rule {
            Rule::Program(program) => Some(program_kernel_data(program)?),
            Rule::Conway | Rule::Lenia { .. } => None,
        };
        let generated = match &spec.rule {
            Rule::Program(program) => generate_program_cuda_source(program)?,
            Rule::Conway | Rule::Lenia { .. } => {
                let fallback_growth = KernelExpression::Constant(0.0);
                let growth = spec.growth_expression().unwrap_or(&fallback_growth);
                generate_cuda_source(growth)?
            }
        };
        let module = load_cached_module(&context, &generated.source)?;
        let function = module.load_function(generated.entry_point)?;
        let kernel_values = if let Some(data) = &program_data {
            data.values.clone()
        } else if spec.kernel.values.is_empty() {
            vec![0.0]
        } else {
            spec.kernel.values.clone()
        };
        let mask_values = if let Some(data) = &program_data {
            data.masks.clone()
        } else if spec
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
        let program = if let Some(data) = program_data {
            Some(ProgramCudaBuffers {
                masks: stream.clone_htod(&data.masks)?,
                offsets: stream.clone_htod(&data.offsets)?,
                widths: stream.clone_htod(&data.widths)?,
                heights: stream.clone_htod(&data.heights)?,
                anchor_x: stream.clone_htod(&data.anchor_x)?,
                anchor_y: stream.clone_htod(&data.anchor_y)?,
                channels: stream.clone_htod(&data.channels)?,
                parameters: match &spec.rule {
                    Rule::Program(program) => program.parameters.values().copied().collect(),
                    Rule::Conway | Rule::Lenia { .. } => Vec::new(),
                },
            })
        } else {
            None
        };
        let cells = width * height * channel_count;
        let current = stream.alloc_zeros::<f32>(cells)?;
        let next = stream.alloc_zeros::<f32>(cells)?;

        Ok(Self {
            spec,
            stream,
            function,
            kernel,
            kernel_mask,
            program,
            current,
            next,
            width,
            height,
            channel_count,
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
            self.channel_count, 1,
            "scalar worlds require one CUDA channel"
        );
        assert_eq!(
            (world.width(), world.height()),
            (self.width, self.height),
            "CUDA buffers and world must have the same shape"
        );
        self.stream.memcpy_htod(world.cells(), &mut self.current)?;
        let width = self.width as i32;
        let height = self.height as i32;
        let cell_count = self.width * self.height;

        match &self.spec.rule {
            Rule::Program(_) => {
                let program = self
                    .program
                    .as_ref()
                    .expect("program buffers are initialized");
                let mut launch = self.stream.launch_builder(&self.function);
                launch
                    .arg(&mut self.next)
                    .arg(&self.current)
                    .arg(&self.kernel)
                    .arg(&program.masks)
                    .arg(&program.offsets)
                    .arg(&program.widths)
                    .arg(&program.heights)
                    .arg(&program.anchor_x)
                    .arg(&program.anchor_y)
                    .arg(&program.channels)
                    .arg(&width)
                    .arg(&height)
                    .arg(&self.spec.dt);
                for parameter in &program.parameters {
                    launch.arg(parameter);
                }
                unsafe { launch.launch(LaunchConfig::for_num_elems(cell_count as u32)) }?;
            }
            Rule::Conway | Rule::Lenia { .. } => {
                let (mode, mu, sigma) = match &self.spec.rule {
                    Rule::Conway => (0_i32, 0.0_f32, 1.0_f32),
                    Rule::Lenia { mu, sigma } => (1_i32, *mu, *sigma),
                    Rule::Program(_) => unreachable!(),
                };
                let kernel_width = self.spec.kernel.width as i32;
                let kernel_height = self.spec.kernel.height as i32;
                let kernel_anchor_x = self.spec.kernel.anchor_x as i32;
                let kernel_anchor_y = self.spec.kernel.anchor_y as i32;
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
            }
        }
        let updated = self.stream.clone_dtoh(&self.next)?;
        self.stream.synchronize()?;
        world.replace_cells(&updated);
        std::mem::swap(&mut self.current, &mut self.next);
        self.tick += 1;
        Ok(())
    }
    pub fn step_channels(&mut self, world: &mut ChannelWorld) -> Result<(), BackendError> {
        assert_eq!(self.channel_count, world.channels());
        assert_eq!((world.width(), world.height()), (self.width, self.height));
        if !matches!(self.spec.rule, Rule::Program(_)) {
            return Err(BackendError::InvalidWorld);
        }
        let program = self
            .program
            .as_ref()
            .expect("program buffers are initialized");
        self.stream.memcpy_htod(world.cells(), &mut self.current)?;
        let width = self.width as i32;
        let height = self.height as i32;
        let cell_count = self.width * self.height;
        let mut launch = self.stream.launch_builder(&self.function);
        launch
            .arg(&mut self.next)
            .arg(&self.current)
            .arg(&self.kernel)
            .arg(&program.masks)
            .arg(&program.offsets)
            .arg(&program.widths)
            .arg(&program.heights)
            .arg(&program.anchor_x)
            .arg(&program.anchor_y)
            .arg(&program.channels)
            .arg(&width)
            .arg(&height)
            .arg(&self.spec.dt);
        for parameter in &program.parameters {
            launch.arg(parameter);
        }
        unsafe { launch.launch(LaunchConfig::for_num_elems(cell_count as u32)) }?;
        let updated = self.stream.clone_dtoh(&self.next)?;
        self.stream.synchronize()?;
        world.replace_channel(0, &updated[..cell_count]);
        std::mem::swap(&mut self.current, &mut self.next);
        self.tick += 1;
        Ok(())
    }
}

impl CudaExperimentBackend {
    pub fn new(compiled: CompiledExperiment) -> Result<Self, BackendError> {
        if compiled.width == 0
            || compiled.height == 0
            || compiled.rules.is_empty()
            || compiled
                .rules
                .iter()
                .any(|rule| rule.target >= compiled.rules.len())
        {
            return Err(BackendError::InvalidWorld);
        }
        let context = shared_context()?;
        let stream = context.default_stream();
        let device_name = context.name()?;
        let mut functions = Vec::new();
        let mut rules = Vec::new();
        if compiled.rules.iter().any(|rule| !rule.frozen) {
            let source =
                generate_experiment_cuda_source(&compiled).map_err(BackendError::Runtime)?;
            let module = load_cached_module(&context, &source.source)?;
            for rule in &compiled.rules {
                if rule.frozen {
                    continue;
                }
                let entry_point = format!("cellarium_target_{}", rule.target);
                functions.push(module.load_function(&entry_point)?);
                rules.push(CudaTargetRule {
                    target: rule.target,
                    mode: rule.mode,
                    parameters: rule.parameters.values().copied().collect(),
                });
            }
        }

        let mut values = Vec::new();
        let mut masks = Vec::new();
        let mut offsets = Vec::new();
        let mut widths = Vec::new();
        let mut heights = Vec::new();
        let mut anchor_x = Vec::new();
        let mut anchor_y = Vec::new();
        let mut sources = Vec::new();
        let mut constants = vec![0.0; compiled.rules.len()];
        for rule in &compiled.rules {
            for input in &rule.inputs {
                offsets.push(i32::try_from(values.len()).map_err(|_| BackendError::InvalidWorld)?);
                widths.push(
                    i32::try_from(input.kernel.width).map_err(|_| BackendError::InvalidWorld)?,
                );
                heights.push(
                    i32::try_from(input.kernel.height).map_err(|_| BackendError::InvalidWorld)?,
                );
                anchor_x.push(
                    i32::try_from(input.kernel.anchor_x).map_err(|_| BackendError::InvalidWorld)?,
                );
                anchor_y.push(
                    i32::try_from(input.kernel.anchor_y).map_err(|_| BackendError::InvalidWorld)?,
                );
                sources.push(i32::try_from(input.source).map_err(|_| BackendError::InvalidWorld)?);
                constants[input.source] = input.boundary_constant;
                values.extend_from_slice(&input.kernel.values);
                masks.extend(input.kernel.mask.as_ref().map_or_else(
                    || vec![1; input.kernel.values.len()],
                    |mask| mask.iter().map(|active| i32::from(*active)).collect(),
                ));
            }
        }
        if values.is_empty() {
            values.push(0.0);
            masks.push(0);
        }
        let cell_count = compiled.width * compiled.height;
        let total = cell_count * compiled.rules.len();
        Ok(Self {
            kernel_values: stream.clone_htod(&values)?,
            kernel_masks: stream.clone_htod(&masks)?,
            kernel_offsets: stream.clone_htod(&offsets)?,
            kernel_widths: stream.clone_htod(&widths)?,
            kernel_heights: stream.clone_htod(&heights)?,
            kernel_anchor_x: stream.clone_htod(&anchor_x)?,
            kernel_anchor_y: stream.clone_htod(&anchor_y)?,
            kernel_sources: stream.clone_htod(&sources)?,
            channel_constants: stream.clone_htod(&constants)?,
            current: stream.alloc_zeros(total)?,
            next: stream.alloc_zeros(total)?,
            stream,
            functions,
            width: compiled.width,
            height: compiled.height,
            channels: compiled.rules.len(),
            boundary_mode: boundary_mode(compiled.boundary),
            dt: compiled.simulation_dt,
            rules,
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

    pub fn step(&mut self, world: &mut ChannelWorld) -> Result<(), BackendError> {
        if world.width() != self.width
            || world.height() != self.height
            || world.channels() != self.channels
        {
            return Err(BackendError::InvalidWorld);
        }
        self.stream.memcpy_htod(world.cells(), &mut self.current)?;
        self.stream.memcpy_htod(world.cells(), &mut self.next)?;
        let width = self.width as i32;
        let height = self.height as i32;
        let boundary = self.boundary_mode;
        let cell_count = (self.width * self.height) as u32;
        for (function, rule) in self.functions.iter().zip(&self.rules) {
            let target = rule.target as i32;
            let mode = match rule.mode {
                UpdateMode::GrowthRate => 0_i32,
                UpdateMode::DirectUpdate => 1_i32,
            };
            let mut launch = self.stream.launch_builder(function);
            launch
                .arg(&mut self.next)
                .arg(&self.current)
                .arg(&self.kernel_values)
                .arg(&self.kernel_masks)
                .arg(&self.kernel_offsets)
                .arg(&self.kernel_widths)
                .arg(&self.kernel_heights)
                .arg(&self.kernel_anchor_x)
                .arg(&self.kernel_anchor_y)
                .arg(&self.kernel_sources)
                .arg(&self.channel_constants)
                .arg(&width)
                .arg(&height)
                .arg(&boundary)
                .arg(&target)
                .arg(&mode)
                .arg(&self.dt);
            for parameter in &rule.parameters {
                launch.arg(parameter);
            }
            unsafe { launch.launch(LaunchConfig::for_num_elems(cell_count)) }?;
        }
        let updated = self.stream.clone_dtoh(&self.next)?;
        self.stream.synchronize()?;
        world
            .replace_all(&updated)
            .map_err(|error| BackendError::Runtime(RuntimeError::World(error)))?;
        std::mem::swap(&mut self.current, &mut self.next);
        self.tick += 1;
        Ok(())
    }
}

fn boundary_mode(boundary: crate::sim::runtime::CompiledBoundary) -> i32 {
    match boundary {
        crate::sim::runtime::CompiledBoundary::Open => 0,
        crate::sim::runtime::CompiledBoundary::Constant => 1,
        crate::sim::runtime::CompiledBoundary::Periodic => 2,
        crate::sim::runtime::CompiledBoundary::Clamp => 3,
        crate::sim::runtime::CompiledBoundary::Reflect => 4,
    }
}

fn generate_experiment_cuda_source(
    compiled: &CompiledExperiment,
) -> Result<crate::sim::cuda_codegen::GeneratedCudaSource, RuntimeError> {
    let mut source = String::from(EXPERIMENT_CUDA_HEADER);
    let mut input_index = 0;
    for rule in &compiled.rules {
        if rule.frozen {
            continue;
        }
        let mut symbols = BTreeSet::from(["self".to_string()]);
        symbols.extend(rule.parameters.keys().cloned());
        symbols.extend(rule.inputs.iter().map(|input| input.symbol.clone()));
        let expression = generate_cuda_expression(&rule.update, &symbols)
            .map_err(|error| RuntimeError::Model(error.to_string()))?;
        let locals = rule
            .inputs
            .iter()
            .map(|input| {
                let line = format!(
                    "float {symbol} = cellarium_convolve(current, kernel_values, kernel_masks, kernel_offsets, kernel_widths, kernel_heights, kernel_anchor_x, kernel_anchor_y, kernel_sources, channel_constants, width, height, boundary_mode, linear, {index});",
                    symbol = input.symbol,
                    index = input_index
                );
                input_index += 1;
                line
            })
            .collect::<Vec<_>>()
            .join("\n    ");
        let params = rule
            .parameters
            .keys()
            .map(|name| format!(", float {name}"))
            .collect::<String>();
        let mode = match rule.mode {
            UpdateMode::GrowthRate => "fminf(1.0f, fmaxf(0.0f, self + dt * (EXPR)))",
            UpdateMode::DirectUpdate => "fminf(1.0f, fmaxf(0.0f, EXPR))",
        }
        .replace("EXPR", &expression);
        source.push_str(&format!(
            "extern \"C\" __global__ void cellarium_target_{target}(float* next, const float* current, const float* kernel_values, const int* kernel_masks, const int* kernel_offsets, const int* kernel_widths, const int* kernel_heights, const int* kernel_anchor_x, const int* kernel_anchor_y, const int* kernel_sources, const float* channel_constants, int width, int height, int boundary_mode, int target, int mode, float dt{params}) {{ int linear = (int)(blockIdx.x * blockDim.x + threadIdx.x); int cell_count = width * height; if (linear >= cell_count) return; int x = linear % width; int y = linear / width; float self = current[target * cell_count + linear]; {locals} next[target * cell_count + linear] = {mode}; }}\n",
            target = rule.target,
            params = params,
            locals = locals,
            mode = mode,
        ));
    }
    Ok(crate::sim::cuda_codegen::GeneratedCudaSource {
        source,
        entry_point: "cellarium_target_0",
    })
}

const EXPERIMENT_CUDA_HEADER: &str = r#"
extern "C" __device__ int cellarium_index(int value, int size, int mode, int* valid) {
    if (value >= 0 && value < size) { *valid = 1; return value; }
    if (mode == 0) { *valid = 0; return 0; }
    if (mode == 1) { *valid = 0; return 0; }
    if (mode == 2) { int wrapped = value % size; return wrapped < 0 ? wrapped + size : wrapped; }
    if (mode == 3) { return value < 0 ? 0 : (value >= size ? size - 1 : value); }
    int span = size - 1; if (span == 0) return 0; int period = span * 2; int folded = value % period; if (folded < 0) folded += period; return folded <= span ? folded : period - folded;
}
extern "C" __device__ float cellarium_convolve(const float* current, const float* values, const int* masks, const int* offsets, const int* widths, const int* heights, const int* anchors_x, const int* anchors_y, const int* sources, const float* constants, int width, int height, int boundary_mode, int linear, int input) {
    int x = linear % width; int y = linear / width; float result = 0.0f;
    for (int ky = 0; ky < heights[input]; ++ky) for (int kx = 0; kx < widths[input]; ++kx) { int ki = offsets[input] + ky * widths[input] + kx; if (!masks[ki]) continue; int valid_x = 1; int valid_y = 1; int nx = cellarium_index(x + kx - anchors_x[input], width, boundary_mode, &valid_x); int ny = cellarium_index(y + ky - anchors_y[input], height, boundary_mode, &valid_y); float sample = (valid_x && valid_y) ? current[sources[input] * width * height + ny * width + nx] : (boundary_mode == 1 ? constants[sources[input]] : 0.0f); result += values[ki] * sample; } return result;
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::cpu::CpuBackend;
    #[cfg(feature = "cuda")]
    use crate::sim::cpu::CpuExperimentBackend;
    #[cfg(feature = "cuda")]
    use crate::sim::experiment_model::{
        ExperimentSpec, GrowthSource, KernelId, KernelSlot, UpdateMode,
    };
    use crate::sim::kernel::{KernelDefinition, KernelValues, Normalization};
    use crate::sim::parser::parse_expression;
    use crate::sim::program::{RuleInput, RuleProgram};
    use crate::sim::rule::SimulationSpec;
    use crate::sim::topology::{
        Basis2, BoardSpec, BoundarySpec, DomainSpec, LatticeSpec, NeighborTemplate, SiteSpec,
        compile_topology,
    };
    #[cfg(feature = "cuda")]
    use crate::sim::world::ChannelWorld;
    use crate::sim::world::World;
    use std::collections::BTreeMap;

    #[cfg(feature = "cuda")]
    fn routed_two_channel_fixture() -> ExperimentSpec {
        let mut spec = ExperimentSpec::single_channel_lenia(2, 1);
        spec.kernels.clear();
        spec.growth.clear();
        let first = spec.channels[0].id;
        let second = spec.add_channel("second", false);
        spec.kernels = vec![
            KernelSlot::identity(KernelId(10), "from_second", second, first),
            KernelSlot::identity(KernelId(20), "from_first", first, second),
        ];
        spec.growth = vec![
            GrowthSource {
                target: first,
                kernel_inputs: vec![KernelId(10)],
                parameters: BTreeMap::new(),
                source: "from_second".to_string(),
                mode: UpdateMode::DirectUpdate,
            },
            GrowthSource {
                target: second,
                kernel_inputs: vec![KernelId(20)],
                parameters: BTreeMap::new(),
                source: "from_first".to_string(),
                mode: UpdateMode::DirectUpdate,
            },
        ];
        spec
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn multi_target_cuda_matches_cpu_or_skips_without_driver() {
        let spec = routed_two_channel_fixture();
        let compiled = crate::sim::runtime::compile_experiment(&spec).unwrap();
        let Ok(mut gpu) = CudaExperimentBackend::new(compiled.clone()) else {
            return;
        };
        let mut cpu_world =
            ChannelWorld::from_channels(2, 1, &[vec![0.25, 0.5], vec![0.75, 0.25]]).unwrap();
        let mut gpu_world =
            ChannelWorld::from_channels(2, 1, &[vec![0.25, 0.5], vec![0.75, 0.25]]).unwrap();
        CpuExperimentBackend::new(compiled)
            .step(&mut cpu_world)
            .unwrap();
        gpu.step(&mut gpu_world).unwrap();
        for (lhs, rhs) in cpu_world.cells().iter().zip(gpu_world.cells()) {
            assert!((lhs - rhs).abs() < 1e-5, "{lhs} != {rhs}");
        }
    }

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

    fn program_spec() -> SimulationSpec {
        let identity = KernelDefinition {
            name: "identity".to_string(),
            width: 1,
            height: 1,
            anchor_x: 0,
            anchor_y: 0,
            mask: None,
            normalization: Normalization::None,
            parameters: BTreeMap::new(),
            values: KernelValues::Explicit(vec![1.0]),
        }
        .build()
        .unwrap();
        let program = RuleProgram::new(
            vec![
                RuleInput::state("self"),
                RuleInput::convolution("food", identity),
            ],
            BTreeMap::from([("gain".to_string(), 0.5)]),
            parse_expression("(self + food) * gain").unwrap(),
        )
        .unwrap();
        SimulationSpec::custom_program(program, 0.1)
    }

    fn channel_program_spec() -> SimulationSpec {
        let identity = KernelDefinition {
            name: "identity".to_string(),
            width: 1,
            height: 1,
            anchor_x: 0,
            anchor_y: 0,
            mask: None,
            normalization: Normalization::None,
            parameters: BTreeMap::new(),
            values: KernelValues::Explicit(vec![1.0]),
        }
        .build()
        .unwrap();
        let program = RuleProgram::new(
            vec![
                RuleInput::state("self"),
                RuleInput::channel_state("signal", 1),
                RuleInput::channel_convolution("neighbor", 1, identity),
            ],
            BTreeMap::from([("gain".to_string(), 0.25)]),
            parse_expression("(self + signal + neighbor) * gain").unwrap(),
        )
        .unwrap();
        SimulationSpec::custom_program(program, 0.1)
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

    #[test]
    fn missing_dynamic_libraries_become_unavailable_errors() {
        let result = require_dynamic_library(false);
        assert!(matches!(result, Err(BackendError::CudaUnavailable)));
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
    fn multi_input_program_step_matches_cpu_backend() {
        if !cuda_available() {
            return;
        }
        identical_step_matches_cpu(program_spec(), 19, 13);
    }

    #[test]
    fn cross_channel_program_step_matches_cpu_backend() {
        if !cuda_available() {
            return;
        }
        let spec = channel_program_spec();
        let mut cpu_world = ChannelWorld::new(19, 13, 2);
        let mut gpu_world = ChannelWorld::new(19, 13, 2);
        for y in 0..13 {
            for x in 0..19 {
                let value = ((x + 3 * y) % 11) as f32 / 10.0;
                cpu_world.set(0, x, y, value);
                cpu_world.set(1, x, y, 1.0 - value);
                gpu_world.set(0, x, y, value);
                gpu_world.set(1, x, y, 1.0 - value);
            }
        }
        let mut cpu = CpuBackend::new(spec.clone());
        let mut gpu = CudaBackend::new_with_channels(spec, 19, 13, 2).unwrap();
        cpu.step_channels(&mut cpu_world).unwrap();
        gpu.step_channels(&mut gpu_world).unwrap();
        for (cpu_value, gpu_value) in cpu_world
            .channel_cells(0)
            .iter()
            .zip(gpu_world.channel_cells(0))
        {
            assert!((cpu_value - gpu_value).abs() <= 1e-5);
        }
    }

    #[test]
    fn generic_csr_topology_step_matches_cpu_reference() {
        if !cuda_available() {
            return;
        }
        let topology = compile_topology(
            &LatticeSpec {
                basis: Basis2 {
                    first: [1.0, 0.0],
                    second: [0.0, 1.0],
                },
                sites: vec![SiteSpec {
                    name: "cell".to_string(),
                }],
                neighborhoods: vec![NeighborTemplate {
                    source_site: 0,
                    target_site: 0,
                    cell_offset: [1, 0],
                    weight: 0.75,
                }],
            },
            &BoardSpec {
                domain: DomainSpec::Rect { size: [4, 1] },
            },
            &BoundarySpec::Periodic,
        )
        .unwrap();
        let mut actual = vec![0.1, 0.2, 0.3, 0.4];
        let original = actual.clone();
        let mut expected = original.clone();
        for site in 0..topology.site_count() {
            let start = topology.offsets[site] as usize;
            let end = topology.offsets[site + 1] as usize;
            let total = (start..end)
                .map(|edge| topology.weights[edge] * original[topology.neighbors[edge] as usize])
                .sum::<f32>();
            expected[site] = (original[site] + 0.5 * total).clamp(0.0, 1.0);
        }
        let mut gpu = CudaTopologyBackend::new(&topology).unwrap();
        gpu.step(&mut actual, 0.5).unwrap();
        for (actual, expected) in actual.iter().zip(expected) {
            assert!((actual - expected).abs() <= 1e-6);
        }
    }

    #[test]
    fn runtime_compilation_cache_reuses_the_same_generated_source() {
        if !nvrtc_available() {
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
        if !nvrtc_available() {
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
