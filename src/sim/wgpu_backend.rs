//! Portable GPU compute backend.
//!
//! State stays in device storage buffers between steps. The host only reads back
//! for snapshots, edits and recovery, which is what keeps a GPU backend from
//! degenerating into a per-step round trip.

use std::fmt::Write as _;
use std::sync::Arc;

use wgpu::util::DeviceExt as _;

use crate::sim::basis_runtime::StateLayout;
use crate::sim::compute_plan::ComputePlan;
use crate::sim::experiment_model::UpdateMode;
use crate::sim::local_backend::{
    BackendDescriptor, BackendFailure, BackendKind, BackendProbe, GpuDeviceType, LocalBackend,
    StepStats, WorldEdit, WorldSnapshot,
};
use crate::sim::runtime::CompiledBoundary;
use crate::sim::wgsl_codegen::generate_wgsl;

/// A GPU that never completes must surface as an error, not as a worker thread
/// that blocks forever.
const GPU_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

const WORKGROUP_X: u32 = 8;
const WORKGROUP_Y: u32 = 8;

/// Buffer sizes and dispatch shape for a plan. Computed without a device so a
/// plan can be rejected, or a test run, with no adapter present.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WgpuBufferLayout {
    pub state_scalars: usize,
    pub state_bytes: u64,
    pub binding_count: usize,
    pub kernel_count: usize,
    pub weight_count: usize,
    pub max_kernels: usize,
    /// Dispatch is one work item per cell per binding.
    pub workgroups: [u32; 3],
    pub required_storage_bytes: u64,
}

impl WgpuBufferLayout {
    pub fn for_plan(plan: &ComputePlan) -> Result<Self, BackendFailure> {
        let state_scalars = plan.state_scalars();
        if state_scalars == 0 {
            return Err(BackendFailure::Unsupported("plan has no state".into()));
        }
        let state_bytes = (state_scalars * size_of::<f32>()) as u64;
        let kernel_count = plan.kernel_count();
        let weight_count = plan
            .bindings
            .iter()
            .flat_map(|binding| binding.kernels.iter())
            .map(|kernel| kernel.weights.len())
            .sum();
        let max_kernels = plan
            .bindings
            .iter()
            .map(|binding| binding.kernels.len())
            .max()
            .unwrap_or(0);
        let workgroups = [
            plan.width.div_ceil(WORKGROUP_X),
            plan.height.div_ceil(WORKGROUP_Y),
            plan.bindings.len() as u32,
        ];
        Ok(Self {
            state_scalars,
            state_bytes,
            binding_count: plan.bindings.len(),
            kernel_count,
            weight_count,
            max_kernels,
            workgroups,
            // Two state buffers plus the read-only topology tables.
            required_storage_bytes: state_bytes * 2
                + (weight_count * size_of::<GpuWeight>()) as u64
                + (kernel_count * size_of::<GpuKernel>()) as u64
                + (plan.bindings.len() * size_of::<GpuBinding>()) as u64,
        })
    }

    /// Device limits this plan needs. Compared against an adapter before a
    /// device is requested, so a plan that is too large reports the limit.
    pub fn required_limits(&self) -> wgpu::Limits {
        let mut limits = wgpu::Limits::downlevel_defaults();
        limits.max_storage_buffer_binding_size = self.state_bytes.max(self.required_storage_bytes);
        limits.max_buffer_size = self.required_storage_bytes.max(self.state_bytes);
        limits.max_storage_buffers_per_shader_stage =
            limits.max_storage_buffers_per_shader_stage.max(5);
        limits.max_compute_workgroup_size_x = limits.max_compute_workgroup_size_x.max(WORKGROUP_X);
        limits.max_compute_workgroup_size_y = limits.max_compute_workgroup_size_y.max(WORKGROUP_Y);
        limits.max_compute_invocations_per_workgroup = limits
            .max_compute_invocations_per_workgroup
            .max(WORKGROUP_X * WORKGROUP_Y);
        limits.max_compute_workgroups_per_dimension = limits
            .max_compute_workgroups_per_dimension
            .max(self.workgroups.into_iter().max().unwrap_or(1));
        limits
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuMeta {
    width: u32,
    height: u32,
    bases: u32,
    channels: u32,
    binding_count: u32,
    boundary: u32,
    dt: f32,
    _padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuBinding {
    basis_index: u32,
    output_index: u32,
    frozen: u32,
    mode: u32,
    kernel_offset: u32,
    kernel_count: u32,
    _padding: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuKernel {
    source_channel: u32,
    weight_offset: u32,
    weight_count: u32,
    boundary_constant: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuWeight {
    dx: i32,
    dy: i32,
    source_basis: u32,
    weight: f32,
}

fn boundary_code(boundary: CompiledBoundary) -> u32 {
    match boundary {
        CompiledBoundary::Open => 0,
        CompiledBoundary::Constant => 1,
        CompiledBoundary::Periodic => 2,
        CompiledBoundary::Clamp => 3,
        CompiledBoundary::Reflect => 4,
    }
}

/// Flatten the plan's tables into the layout the shader indexes.
fn gpu_tables(plan: &ComputePlan) -> (Vec<GpuBinding>, Vec<GpuKernel>, Vec<GpuWeight>) {
    let mut bindings = Vec::with_capacity(plan.bindings.len());
    let mut kernels = Vec::new();
    let mut weights = Vec::new();
    for binding in &plan.bindings {
        let kernel_offset = kernels.len() as u32;
        for kernel in &binding.kernels {
            let weight_offset = weights.len() as u32;
            for weight in &kernel.weights {
                weights.push(GpuWeight {
                    dx: weight.dx as i32,
                    dy: weight.dy as i32,
                    source_basis: weight.source_basis as u32,
                    weight: weight.weight,
                });
            }
            kernels.push(GpuKernel {
                source_channel: kernel.source_channel_index as u32,
                weight_offset,
                weight_count: kernel.weights.len() as u32,
                boundary_constant: kernel.boundary_constant,
            });
        }
        bindings.push(GpuBinding {
            basis_index: binding.basis_index as u32,
            output_index: binding.output_index as u32,
            frozen: u32::from(binding.frozen),
            mode: match binding.mode {
                UpdateMode::GrowthRate => 0,
                UpdateMode::DirectUpdate => 1,
            },
            kernel_offset,
            kernel_count: binding.kernels.len() as u32,
            _padding: [0; 2],
        });
    }
    (bindings, kernels, weights)
}

/// Build the complete compute shader: the generated growth functions plus the
/// sampling and update kernel that calls them.
pub fn compute_shader(plan: &ComputePlan) -> Result<String, BackendFailure> {
    let generated = generate_wgsl(plan)?;
    let layout = WgpuBufferLayout::for_plan(plan)?;
    let mut source = generated.source;

    let max_kernels = layout.max_kernels.max(1);
    let _ = write!(
        source,
        r#"
struct Meta {{
    width: u32,
    height: u32,
    bases: u32,
    channels: u32,
    binding_count: u32,
    boundary: u32,
    dt: f32,
    padding: u32,
}};

struct BindingMeta {{
    basis_index: u32,
    output_index: u32,
    frozen: u32,
    mode: u32,
    kernel_offset: u32,
    kernel_count: u32,
    padding0: u32,
    padding1: u32,
}};

struct KernelMeta {{
    source_channel: u32,
    weight_offset: u32,
    weight_count: u32,
    boundary_constant: f32,
}};

struct Weight {{
    dx: i32,
    dy: i32,
    source_basis: u32,
    weight: f32,
}};

@group(0) @binding(0) var<uniform> sim_meta: Meta;
@group(0) @binding(1) var<storage, read> bindings_meta: array<BindingMeta>;
@group(0) @binding(2) var<storage, read> kernels_meta: array<KernelMeta>;
@group(0) @binding(3) var<storage, read> weights: array<Weight>;
@group(0) @binding(4) var<storage, read> state_in: array<f32>;
@group(0) @binding(5) var<storage, read_write> state_out: array<f32>;

fn state_index(channel: u32, x: u32, y: u32, basis: u32) -> u32 {{
    let channel_len = sim_meta.width * sim_meta.height * sim_meta.bases;
    return channel * channel_len + (y * sim_meta.width + x) * sim_meta.bases + basis;
}}

fn fold(value: i32, size: i32) -> i32 {{
    let span = size - 1;
    if (span <= 0) {{ return 0; }}
    let period = span * 2;
    var folded = value % period;
    if (folded < 0) {{ folded = folded + period; }}
    if (folded <= span) {{ return folded; }}
    return period - folded;
}}

/// Sample one source cell using the plan's boundary rule. Mirrors the CPU
/// reference exactly; a mismatch here is a correctness bug, not a nuance.
fn sample_cell(channel: u32, x: i32, y: i32, basis: u32, boundary_constant: f32) -> f32 {{
    let w = i32(sim_meta.width);
    let h = i32(sim_meta.height);
    var sx = x;
    var sy = y;
    switch (sim_meta.boundary) {{
        case 0u: {{
            if (x < 0 || y < 0 || x >= w || y >= h) {{ return 0.0; }}
        }}
        case 1u: {{
            if (x < 0 || y < 0 || x >= w || y >= h) {{ return boundary_constant; }}
        }}
        case 2u: {{
            sx = x % w; if (sx < 0) {{ sx = sx + w; }}
            sy = y % h; if (sy < 0) {{ sy = sy + h; }}
        }}
        case 3u: {{
            sx = clamp(x, 0, w - 1);
            sy = clamp(y, 0, h - 1);
        }}
        default: {{
            sx = fold(x, w);
            sy = fold(y, h);
        }}
    }}
    return state_in[state_index(channel, u32(sx), u32(sy), basis)];
}}

@compute @workgroup_size({WORKGROUP_X}, {WORKGROUP_Y}, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    if (id.x >= sim_meta.width || id.y >= sim_meta.height || id.z >= sim_meta.binding_count) {{
        return;
    }}
    let active_binding = bindings_meta[id.z];
    let out_index = state_index(active_binding.output_index, id.x, id.y, active_binding.basis_index);
    let self_value = state_in[out_index];
    if (active_binding.frozen != 0u) {{
        state_out[out_index] = self_value;
        return;
    }}

    var potentials: array<f32, {max_kernels}>;
    for (var i = 0u; i < active_binding.kernel_count; i = i + 1u) {{
        let active_kernel = kernels_meta[active_binding.kernel_offset + i];
        var total = 0.0;
        for (var w = 0u; w < active_kernel.weight_count; w = w + 1u) {{
            let entry = weights[active_kernel.weight_offset + w];
            total = total + entry.weight * sample_cell(
                active_kernel.source_channel,
                i32(id.x) + entry.dx,
                i32(id.y) + entry.dy,
                entry.source_basis,
                active_kernel.boundary_constant,
            );
        }}
        potentials[i] = total;
    }}

    var result = self_value;
    switch (id.z) {{
"#
    );

    for (index, binding) in plan.bindings.iter().enumerate() {
        // The generated signature lists kernel potentials first and `self`
        // last, matching ExternalSymbols::ordered. The call has to agree.
        let mut arguments = Vec::new();
        if !binding.frozen {
            for kernel in 0..binding.kernels.len() {
                arguments.push(format!("potentials[{kernel}u]"));
            }
        }
        arguments.push("self_value".to_string());
        let _ = writeln!(
            source,
            "        case {index}u: {{ result = growth_{index}({}); }}",
            arguments.join(", ")
        );
    }

    let _ = write!(
        source,
        r#"        default: {{ result = self_value; }}
    }}

    if (active_binding.mode == 0u) {{
        state_out[out_index] = clamp(self_value + sim_meta.dt * result, 0.0, 1.0);
    }} else {{
        state_out[out_index] = clamp(result, 0.0, 1.0);
    }}
}}
"#
    );
    Ok(source)
}

/// A wgpu adapter that satisfies a plan, ready to build a device.
pub struct WgpuAdapter {
    pub adapter: wgpu::Adapter,
    pub info: wgpu::AdapterInfo,
}

pub fn device_type_of(info: &wgpu::AdapterInfo) -> GpuDeviceType {
    match info.device_type {
        wgpu::DeviceType::DiscreteGpu => GpuDeviceType::Discrete,
        wgpu::DeviceType::IntegratedGpu => GpuDeviceType::Integrated,
        wgpu::DeviceType::VirtualGpu => GpuDeviceType::Virtual,
        wgpu::DeviceType::Cpu => GpuDeviceType::Cpu,
        wgpu::DeviceType::Other => GpuDeviceType::Other,
    }
}

/// Adapters that can run `plan`, best first: discrete, then integrated, then
/// virtual. CPU adapters are never counted as the GPU fallback.
pub fn compatible_adapters(instance: &wgpu::Instance, plan: &ComputePlan) -> Vec<WgpuAdapter> {
    let Ok(layout) = WgpuBufferLayout::for_plan(plan) else {
        return Vec::new();
    };
    let required = layout.required_limits();
    let mut adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()))
        .into_iter()
        .map(|adapter| {
            let info = adapter.get_info();
            WgpuAdapter { adapter, info }
        })
        .filter(|candidate| candidate.info.device_type != wgpu::DeviceType::Cpu)
        .filter(|candidate| satisfies(&candidate.adapter.limits(), &required))
        .collect::<Vec<_>>();
    adapters.sort_by_key(|candidate| match candidate.info.device_type {
        wgpu::DeviceType::DiscreteGpu => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        wgpu::DeviceType::VirtualGpu => 2,
        _ => 3,
    });
    adapters
}

fn satisfies(available: &wgpu::Limits, required: &wgpu::Limits) -> bool {
    available.max_storage_buffer_binding_size >= required.max_storage_buffer_binding_size
        && available.max_buffer_size >= required.max_buffer_size
        && available.max_storage_buffers_per_shader_stage
            >= required.max_storage_buffers_per_shader_stage
        && available.max_compute_workgroups_per_dimension
            >= required.max_compute_workgroups_per_dimension
}

/// Report whether a portable GPU backend can run this plan here.
pub fn probe(instance: &wgpu::Instance, plan: &ComputePlan) -> BackendProbe {
    match WgpuBufferLayout::for_plan(plan) {
        Err(error) => BackendProbe::unavailable(BackendKind::Wgpu, error.to_string()),
        Ok(layout) => match compatible_adapters(instance, plan).into_iter().next() {
            Some(candidate) => BackendProbe::available(
                BackendKind::Wgpu,
                candidate.info.name.clone(),
                device_type_of(&candidate.info),
            ),
            None => BackendProbe::unavailable(
                BackendKind::Wgpu,
                format!(
                    "no non-CPU adapter satisfies {} MiB of storage",
                    layout.required_storage_bytes / (1024 * 1024)
                ),
            ),
        },
    }
}

pub struct WgpuExperimentBackend {
    descriptor: BackendDescriptor,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    bind_groups: [wgpu::BindGroup; 2],
    state: [wgpu::Buffer; 2],
    staging: wgpu::Buffer,
    layout: StateLayout,
    buffers: WgpuBufferLayout,
    current: usize,
    tick: u64,
}

impl WgpuExperimentBackend {
    /// Build a backend on the best compatible adapter, or report why none fits.
    pub fn new(plan: &ComputePlan, initial: &[f32]) -> Result<Self, BackendFailure> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let candidate = compatible_adapters(&instance, plan)
            .into_iter()
            .next()
            .ok_or_else(|| {
                BackendFailure::Unsupported("no non-CPU wgpu adapter satisfies this plan".into())
            })?;
        Self::with_adapter(&candidate.adapter, plan, initial)
    }

    pub fn with_adapter(
        adapter: &wgpu::Adapter,
        plan: &ComputePlan,
        initial: &[f32],
    ) -> Result<Self, BackendFailure> {
        let buffers = WgpuBufferLayout::for_plan(plan)?;
        if initial.len() != buffers.state_scalars {
            return Err(BackendFailure::State(format!(
                "initial state has {} scalars but the plan needs {}",
                initial.len(),
                buffers.state_scalars
            )));
        }
        let info = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("cellarium-compute"),
            required_features: wgpu::Features::empty(),
            required_limits: buffers.required_limits(),
            ..Default::default()
        }))
        .map_err(|error| BackendFailure::Unsupported(error.to_string()))?;

        let source = compute_shader(plan)?;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cellarium-growth"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });

        let (binding_table, kernel_table, weight_table) = gpu_tables(plan);
        let meta = GpuMeta {
            width: plan.width,
            height: plan.height,
            bases: plan.bases.len() as u32,
            channels: plan.channels.len() as u32,
            binding_count: plan.bindings.len() as u32,
            boundary: boundary_code(plan.boundary),
            dt: plan.dt,
            _padding: 0,
        };

        let meta_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cellarium-meta"),
            contents: bytemuck::bytes_of(&meta),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bindings_buffer = storage_table(&device, "cellarium-bindings", &binding_table);
        let kernels_buffer = storage_table(&device, "cellarium-kernels", &kernel_table);
        let weights_buffer = storage_table(&device, "cellarium-weights", &weight_table);

        let state = [
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cellarium-state-a"),
                contents: bytemuck::cast_slice(initial),
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
            }),
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cellarium-state-b"),
                contents: bytemuck::cast_slice(initial),
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
            }),
        ];
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cellarium-readback"),
            size: buffers.state_bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("cellarium-step"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let group_layout = pipeline.get_bind_group_layout(0);
        let bind_groups = [
            bind_group(
                &device,
                &group_layout,
                &meta_buffer,
                &bindings_buffer,
                &kernels_buffer,
                &weights_buffer,
                &state[0],
                &state[1],
            ),
            bind_group(
                &device,
                &group_layout,
                &meta_buffer,
                &bindings_buffer,
                &kernels_buffer,
                &weights_buffer,
                &state[1],
                &state[0],
            ),
        ];

        Ok(Self {
            descriptor: BackendDescriptor {
                kind: BackendKind::Wgpu,
                device_name: info.name,
            },
            device,
            queue,
            pipeline,
            bind_groups,
            state,
            staging,
            layout: plan.layout.clone(),
            buffers,
            current: 0,
            tick: 0,
        })
    }

    fn read_current(&mut self) -> Result<Vec<f32>, BackendFailure> {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_buffer_to_buffer(
            &self.state[self.current],
            0,
            &self.staging,
            0,
            self.buffers.state_bytes,
        );
        self.queue.submit(Some(encoder.finish()));

        let (sender, receiver) = std::sync::mpsc::channel();
        self.staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(GPU_WAIT_TIMEOUT),
            })
            .map_err(|error| BackendFailure::Step(error.to_string()))?;
        receiver
            .recv()
            .map_err(|_| BackendFailure::Step("readback was dropped".into()))?
            .map_err(|error| BackendFailure::Step(error.to_string()))?;
        let cells = {
            let view = self
                .staging
                .slice(..)
                .get_mapped_range()
                .map_err(|error| BackendFailure::Step(error.to_string()))?;
            bytemuck::cast_slice::<u8, f32>(&view).to_vec()
        };
        self.staging.unmap();
        Ok(cells)
    }
}

fn storage_table<T: bytemuck::Pod>(
    device: &wgpu::Device,
    label: &str,
    values: &[T],
) -> wgpu::Buffer {
    // An empty storage buffer is invalid, so a plan with no weights still gets
    // one zeroed element the shader never reads.
    let fallback = [T::zeroed()];
    let contents = if values.is_empty() {
        &fallback[..]
    } else {
        values
    };
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(contents),
        usage: wgpu::BufferUsages::STORAGE,
    })
}

#[allow(clippy::too_many_arguments)]
fn bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    meta: &wgpu::Buffer,
    bindings: &wgpu::Buffer,
    kernels: &wgpu::Buffer,
    weights: &wgpu::Buffer,
    state_in: &wgpu::Buffer,
    state_out: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("cellarium-state"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: meta.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: bindings.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: kernels.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: weights.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: state_in.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: state_out.as_entire_binding(),
            },
        ],
    })
}

impl LocalBackend for WgpuExperimentBackend {
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
        self.queue.write_buffer(
            &self.state[self.current],
            0,
            bytemuck::cast_slice(&state.cells),
        );
        self.queue.submit(std::iter::empty());
        Ok(())
    }

    fn apply_edits(&mut self, edits: &[WorldEdit]) -> Result<(), BackendFailure> {
        for edit in edits {
            let index = self
                .layout
                .index_by_position(edit.channel, edit.x, edit.y, edit.basis)
                .ok_or_else(|| {
                    BackendFailure::State(format!(
                        "edit at channel {} basis {} ({}, {}) is outside the world",
                        edit.channel, edit.basis, edit.x, edit.y
                    ))
                })?;
            self.queue.write_buffer(
                &self.state[self.current],
                (index * size_of::<f32>()) as u64,
                bytemuck::bytes_of(&edit.value),
            );
        }
        self.queue.submit(std::iter::empty());
        Ok(())
    }

    fn step(&mut self, steps: u32) -> Result<StepStats, BackendFailure> {
        let started = std::time::Instant::now();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        for _ in 0..steps {
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("cellarium-step"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind_groups[self.current], &[]);
                pass.dispatch_workgroups(
                    self.buffers.workgroups[0],
                    self.buffers.workgroups[1],
                    self.buffers.workgroups[2],
                );
            }
            // State stays on the device: the next step reads what this one wrote.
            self.current ^= 1;
            self.tick += 1;
        }
        self.queue.submit(Some(encoder.finish()));
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(GPU_WAIT_TIMEOUT),
            })
            .map_err(|error| BackendFailure::Step(error.to_string()))?;
        Ok(StepStats {
            steps,
            elapsed_micros: started.elapsed().as_micros() as u64,
        })
    }

    fn readback(&mut self) -> Result<WorldSnapshot, BackendFailure> {
        let cells = self.read_current()?;
        Ok(WorldSnapshot {
            layout: self.layout.clone(),
            cells: Arc::from(cells),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::compute_plan::compile_compute_plan;
    use crate::sim::experiment_model::{ChannelId, ExperimentSpec};
    use crate::sim::ruleset::BindingKey;
    use crate::sim::tiling::{BasisId, TilingPreset, build_preset};

    fn two_basis_three_channel_plan() -> ComputePlan {
        let mut spec = ExperimentSpec::single_channel_lenia(4, 4);
        spec.tiling = Some(build_preset(TilingPreset::EquilateralTriangles, 1.0));
        for _ in 0..2 {
            spec = crate::document::channels::add_channel(&spec).unwrap().spec;
        }
        let spec = spec.normalize_rules().unwrap();
        compile_compute_plan(&spec).unwrap()
    }

    #[test]
    fn buffer_layout_matches_cpu_state_layout() {
        let plan = two_basis_three_channel_plan();
        let layout = WgpuBufferLayout::for_plan(&plan).unwrap();
        assert_eq!(
            layout.state_scalars,
            plan.width as usize * plan.height as usize * 2 * 3
        );
        assert_eq!(layout.workgroups[2], plan.bindings.len() as u32);
        assert_eq!(layout.state_bytes, layout.state_scalars as u64 * 4);
    }

    #[test]
    fn the_dispatch_covers_every_cell_of_every_binding() {
        let plan = two_basis_three_channel_plan();
        let layout = WgpuBufferLayout::for_plan(&plan).unwrap();
        assert!(layout.workgroups[0] * WORKGROUP_X >= plan.width);
        assert!(layout.workgroups[1] * WORKGROUP_Y >= plan.height);
        assert_eq!(layout.binding_count, plan.bindings.len());
        assert_eq!(layout.max_kernels, 1);
    }

    #[test]
    fn the_generated_shader_is_valid_and_dispatches_every_binding() {
        let plan = two_basis_three_channel_plan();
        let source = compute_shader(&plan).unwrap();
        for index in 0..plan.bindings.len() {
            assert!(source.contains(&format!("case {index}u: {{ result = growth_{index}(")));
        }
        assert!(source.contains("@compute @workgroup_size(8, 8, 1)"));

        let module = wgpu::naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("shader did not parse: {error:?}\n{source}"));
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|error| panic!("shader did not validate: {error:?}\n{source}"));
    }

    #[test]
    fn a_boundary_mode_reaches_the_shader_as_its_own_code() {
        assert_eq!(boundary_code(CompiledBoundary::Open), 0);
        assert_eq!(boundary_code(CompiledBoundary::Periodic), 2);
        assert_eq!(boundary_code(CompiledBoundary::Reflect), 4);
    }

    #[test]
    fn gpu_tables_flatten_bindings_kernels_and_weights_in_plan_order() {
        let plan = two_basis_three_channel_plan();
        let (bindings, kernels, weights) = gpu_tables(&plan);
        assert_eq!(bindings.len(), plan.bindings.len());
        assert_eq!(kernels.len(), plan.kernel_count());
        for (index, binding) in bindings.iter().enumerate() {
            let source = &plan.bindings[index];
            assert_eq!(binding.basis_index, source.basis_index as u32);
            assert_eq!(binding.kernel_count, source.kernels.len() as u32);
            let first = binding.kernel_offset as usize;
            for (offset, kernel) in source.kernels.iter().enumerate() {
                let flat = &kernels[first + offset];
                assert_eq!(flat.weight_count, kernel.weights.len() as u32);
                assert_eq!(
                    weights[flat.weight_offset as usize].weight,
                    kernel.weights[0].weight
                );
            }
        }
    }

    #[test]
    fn a_plan_with_more_kernels_widens_the_potential_array() {
        let mut spec = ExperimentSpec::single_channel_lenia(4, 4)
            .normalize_rules()
            .unwrap();
        let binding = BindingKey {
            basis: spec.basis_ids()[0],
            output: ChannelId(0),
        };
        let rule_set = spec.rules.detach(binding).unwrap();
        let rule = spec.rules.get_mut(rule_set).unwrap();
        let template = rule.kernels[0].clone();
        rule.kernels.push(crate::sim::ruleset::RuleKernel {
            id: crate::sim::experiment_model::KernelId(template.id.0 + 1),
            symbol: format!("k{}", template.id.0 + 1),
            ..template
        });
        rule.growth.kernel_inputs = rule.kernels.iter().map(|kernel| kernel.id).collect();
        rule.growth.source = "self".into();
        let plan = compile_compute_plan(&spec).unwrap();
        let layout = WgpuBufferLayout::for_plan(&plan).unwrap();
        assert_eq!(layout.max_kernels, 2);
        assert!(compute_shader(&plan).unwrap().contains("array<f32, 2>"));
    }

    #[test]
    fn a_basis_id_keeps_its_dense_index_in_the_binding_table() {
        let plan = two_basis_three_channel_plan();
        let (bindings, _, _) = gpu_tables(&plan);
        assert!(plan.bases.contains(&BasisId(0)));
        assert!(bindings.iter().any(|binding| binding.basis_index == 1));
    }
}
