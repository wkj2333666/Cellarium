//! Numerical parity between the CPU reference and the portable wgpu backend.
//!
//! The CPU backend defines correct behaviour. A GPU backend that disagrees is
//! wrong, so these run the same fixtures through both and compare scalars.
//!
//! When the machine exposes no non-CPU adapter the tests report that they did
//! not run. A skipped comparison is never reported as a passing comparison.

use cellarium::document::channels;
use cellarium::sim::compute_plan::{ComputePlan, compile_compute_plan};
use cellarium::sim::experiment_model::{ChannelId, ExperimentSpec, KernelId, UpdateMode};
use cellarium::sim::local_backend::{CpuBackend, LocalBackend, initial_cells};
use cellarium::sim::ruleset::{BindingKey, RuleKernel};
use cellarium::sim::tiling::{TilingPreset, build_preset};
use cellarium::sim::wgpu_backend::{WgpuExperimentBackend, compatible_adapters};

/// Creating many GPU devices at once starves a shared adapter, so the GPU tests
/// take turns. Without this the suite can outlive its timeout on a busy card.
static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu_turn() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Tolerances from the design: per scalar after one step, and after a hundred.
const ONE_STEP_TOLERANCE: f32 = 1e-5;
const HUNDRED_STEP_TOLERANCE: f32 = 1e-4;

fn gpu_available(plan: &ComputePlan) -> bool {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    !compatible_adapters(&instance, plan).is_empty()
}

fn seeded(spec: &mut ExperimentSpec, cells: usize) {
    for (index, channel) in spec.channels.iter_mut().enumerate() {
        channel.initial = (0..cells)
            .map(|cell| {
                let value = ((cell * 37 + index * 11) % 100) as f32 / 100.0;
                value.clamp(0.0, 1.0)
            })
            .collect();
    }
}

fn compare(name: &str, spec: ExperimentSpec, steps: u32, tolerance: f32) {
    let plan = compile_compute_plan(&spec).unwrap_or_else(|errors| {
        panic!("{name}: plan failed: {errors:?}");
    });
    if !gpu_available(&plan) {
        eprintln!("{name}: SKIPPED, no non-CPU wgpu adapter on this machine");
        return;
    }
    let cells = initial_cells(&plan, &spec);
    let _turn = gpu_turn();

    let mut cpu = CpuBackend::new(&plan, &cells).expect("cpu backend");
    let mut gpu = WgpuExperimentBackend::new(&plan, &cells).expect("wgpu backend");

    cpu.step(steps).expect("cpu step");
    gpu.step(steps).expect("gpu step");

    let cpu_state = cpu.readback().expect("cpu readback");
    let gpu_state = gpu.readback().expect("gpu readback");
    assert_eq!(
        cpu_state.cells.len(),
        gpu_state.cells.len(),
        "{name}: state length differs"
    );

    let mut worst = 0.0f32;
    let mut worst_index = 0;
    for (index, (left, right)) in cpu_state
        .cells
        .iter()
        .zip(gpu_state.cells.iter())
        .enumerate()
    {
        let difference = (left - right).abs();
        if difference > worst {
            worst = difference;
            worst_index = index;
        }
    }
    assert!(
        worst <= tolerance,
        "{name}: after {steps} steps the backends differ by {worst} at scalar {worst_index} \
         (cpu {}, gpu {})",
        cpu_state.cells[worst_index],
        gpu_state.cells[worst_index]
    );
}

fn lenia(width: u32, height: u32) -> ExperimentSpec {
    let mut spec = ExperimentSpec::single_channel_lenia(width, height)
        .normalize_rules()
        .unwrap();
    seeded(&mut spec, (width * height) as usize);
    spec
}

fn with_growth(mut spec: ExperimentSpec, source: &str, mode: UpdateMode) -> ExperimentSpec {
    let bases = spec.basis_ids();
    for basis in bases {
        for channel in spec
            .channels
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>()
        {
            let binding = BindingKey {
                basis,
                output: channel,
            };
            if spec.rules.binding(basis, channel).is_none() {
                continue;
            }
            let rule_set = spec.rules.detach(binding).unwrap();
            let rule = spec.rules.get_mut(rule_set).unwrap();
            // Name the kernels k1..kN so a fixture can write a readable program
            // without depending on whatever the preset happened to call them.
            for (index, kernel) in rule.kernels.iter_mut().enumerate() {
                kernel.symbol = format!("k{}", index + 1);
            }
            rule.growth.kernel_inputs = rule.kernels.iter().map(|kernel| kernel.id).collect();
            rule.growth.source = source.to_string();
            rule.growth.mode = mode;
        }
    }
    spec
}

#[test]
fn lenia_matches_the_cpu_reference_after_one_step() {
    compare("lenia one step", lenia(16, 16), 1, ONE_STEP_TOLERANCE);
}

#[test]
fn lenia_matches_the_cpu_reference_after_a_hundred_steps() {
    compare(
        "lenia hundred steps",
        lenia(16, 16),
        100,
        HUNDRED_STEP_TOLERANCE,
    );
}

#[test]
fn a_raw_unnormalized_kernel_matches_the_cpu_reference() {
    let spec = with_growth(lenia(16, 16), "k1 * 0.5 - self", UpdateMode::GrowthRate);
    compare("raw kernel", spec, 10, ONE_STEP_TOLERANCE);
}

#[test]
fn value_mode_matches_the_cpu_reference() {
    let spec = with_growth(lenia(16, 16), "clamp(k1, 0, 1)", UpdateMode::DirectUpdate);
    compare("value mode", spec, 10, ONE_STEP_TOLERANCE);
}

#[test]
fn a_branching_program_matches_the_cpu_reference() {
    let spec = with_growth(
        lenia(16, 16),
        "if k1 > 0.2 { 1 - self } else { -self * 0.5 }",
        UpdateMode::GrowthRate,
    );
    compare("branching growth", spec, 10, ONE_STEP_TOLERANCE);
}

#[test]
fn multiple_channels_match_the_cpu_reference() {
    let mut spec = ExperimentSpec::single_channel_lenia(16, 16);
    for _ in 0..2 {
        spec = channels::add_channel(&spec).unwrap().spec;
    }
    let mut spec = spec.normalize_rules().unwrap();
    seeded(&mut spec, 16 * 16);
    compare("three channels", spec, 10, ONE_STEP_TOLERANCE);
}

#[test]
fn a_frozen_channel_matches_the_cpu_reference() {
    let mut spec = ExperimentSpec::single_channel_lenia(16, 16);
    spec = channels::add_channel(&spec).unwrap().spec;
    let mut spec = spec.normalize_rules().unwrap();
    spec = channels::set_channel_frozen(&spec, ChannelId(1), true).unwrap();
    seeded(&mut spec, 16 * 16);
    compare("frozen channel", spec, 10, ONE_STEP_TOLERANCE);
}

#[test]
fn a_two_basis_tiling_matches_the_cpu_reference() {
    let mut spec = ExperimentSpec::single_channel_lenia(16, 16);
    spec.tiling = Some(build_preset(TilingPreset::EquilateralTriangles, 1.0));
    let mut spec = spec.normalize_rules().unwrap();
    seeded(&mut spec, 16 * 16 * 2);
    compare("two basis tiling", spec, 10, ONE_STEP_TOLERANCE);
}

#[test]
fn several_kernels_on_one_binding_match_the_cpu_reference() {
    let mut spec = ExperimentSpec::single_channel_lenia(16, 16)
        .normalize_rules()
        .unwrap();
    let binding = BindingKey {
        basis: spec.basis_ids()[0],
        output: ChannelId(0),
    };
    let rule_set = spec.rules.detach(binding).unwrap();
    let rule = spec.rules.get_mut(rule_set).unwrap();
    let template = rule.kernels[0].clone();
    for extra in 1..3u32 {
        rule.kernels.push(RuleKernel {
            id: KernelId(template.id.0 + extra),
            symbol: format!("k{}", template.id.0 + extra),
            ..template.clone()
        });
    }
    for (index, kernel) in rule.kernels.iter_mut().enumerate() {
        kernel.symbol = format!("k{}", index + 1);
    }
    rule.growth.kernel_inputs = rule.kernels.iter().map(|kernel| kernel.id).collect();
    rule.growth.source = "(k1 + k2 + k3) / 3 - self".into();
    seeded(&mut spec, 16 * 16);
    compare("three kernels", spec, 10, ONE_STEP_TOLERANCE);
}

#[test]
fn the_backend_reports_which_device_it_used() {
    let spec = lenia(8, 8);
    let plan = compile_compute_plan(&spec).unwrap();
    if !gpu_available(&plan) {
        eprintln!("device report: SKIPPED, no non-CPU wgpu adapter on this machine");
        return;
    }
    let cells = initial_cells(&plan, &spec);
    let _turn = gpu_turn();
    let backend = WgpuExperimentBackend::new(&plan, &cells).expect("wgpu backend");
    assert_eq!(
        backend.descriptor().kind,
        cellarium::sim::local_backend::BackendKind::Wgpu
    );
    assert!(!backend.descriptor().device_name.is_empty());
    eprintln!("device report: {}", backend.descriptor().summary());
}
