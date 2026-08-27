//! Which compute backend runs, and what to try when one fails.
//!
//! Auto prefers CUDA, then a discrete portable GPU, then an integrated one, then
//! the CPU. Every rejection carries a reason so Settings can explain the choice
//! instead of silently falling back.

use crate::sim::compute_plan::ComputePlan;
use crate::sim::local_backend::{
    BackendKind, BackendProbe, CpuBackend, GpuDeviceType, LocalBackend,
};

/// What the user asked for. An explicit requirement is never silently changed
/// for another kind.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum BackendPolicy {
    #[default]
    Auto,
    RequireCuda,
    RequireWgpu {
        adapter: Option<String>,
    },
    RequireCpu,
}

/// One backend the selector is willing to build, in preference order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Candidate {
    Cuda,
    /// The adapter name, so a machine with several GPUs stays unambiguous.
    Wgpu(String),
    Cpu,
}

impl Candidate {
    pub fn kind(&self) -> BackendKind {
        match self {
            Candidate::Cuda => BackendKind::Cuda,
            Candidate::Wgpu(_) => BackendKind::Wgpu,
            Candidate::Cpu => BackendKind::Cpu,
        }
    }
}

/// A built backend plus the reasons the preferred candidates were rejected.
pub type BuiltBackend = (Box<dyn LocalBackend>, Vec<String>);

pub struct BackendSelector;

impl BackendSelector {
    /// Order the candidates a policy allows, best first.
    ///
    /// CPU is always last and always present: it needs no device, so there is
    /// always something to fall back to.
    pub fn candidates(policy: BackendPolicy, probes: Vec<BackendProbe>) -> Vec<Candidate> {
        let available = |kind: BackendKind| {
            probes
                .iter()
                .filter(move |probe| probe.kind == kind && probe.available)
        };

        match policy {
            BackendPolicy::RequireCpu => vec![Candidate::Cpu],
            BackendPolicy::RequireCuda => available(BackendKind::Cuda)
                .map(|_| Candidate::Cuda)
                .collect(),
            BackendPolicy::RequireWgpu { adapter } => available(BackendKind::Wgpu)
                .filter(|probe| match (&adapter, &probe.device_name) {
                    (Some(wanted), Some(name)) => wanted == name,
                    (Some(_), None) => false,
                    (None, _) => true,
                })
                .map(|probe| Candidate::Wgpu(probe.device_name.clone().unwrap_or_default()))
                .collect(),
            BackendPolicy::Auto => {
                let mut candidates = Vec::new();
                if available(BackendKind::Cuda).next().is_some() {
                    candidates.push(Candidate::Cuda);
                }
                let mut gpus = available(BackendKind::Wgpu)
                    .filter(|probe| probe.device_type != Some(GpuDeviceType::Cpu))
                    .collect::<Vec<_>>();
                gpus.sort_by_key(|probe| match probe.device_type {
                    Some(GpuDeviceType::Discrete) => 0,
                    Some(GpuDeviceType::Integrated) => 1,
                    Some(GpuDeviceType::Virtual) => 2,
                    _ => 3,
                });
                candidates.extend(
                    gpus.into_iter().map(|probe| {
                        Candidate::Wgpu(probe.device_name.clone().unwrap_or_default())
                    }),
                );
                candidates.push(Candidate::Cpu);
                candidates
            }
        }
    }

    /// Probe every backend for this plan on this machine.
    pub fn probe_all(plan: &ComputePlan) -> Vec<BackendProbe> {
        let mut probes = Vec::new();
        probes.push(cuda_probe(plan));

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapters = crate::sim::wgpu_backend::compatible_adapters(&instance, plan);
        if adapters.is_empty() {
            probes.push(crate::sim::wgpu_backend::probe(&instance, plan));
        } else {
            for candidate in adapters {
                probes.push(BackendProbe::available(
                    BackendKind::Wgpu,
                    candidate.info.name.clone(),
                    crate::sim::wgpu_backend::device_type_of(&candidate.info),
                ));
            }
        }

        probes.push(BackendProbe::available(
            BackendKind::Cpu,
            "CPU reference",
            GpuDeviceType::Cpu,
        ));
        probes
    }

    /// Build the first candidate that constructs successfully, reporting each
    /// rejection. A caller that gets `Err` has no way to run the plan at all.
    pub fn build(
        candidates: &[Candidate],
        plan: &ComputePlan,
        initial: &[f32],
    ) -> Result<BuiltBackend, Vec<String>> {
        let mut rejected = Vec::new();
        for candidate in candidates {
            match build_one(candidate, plan, initial) {
                Ok(backend) => return Ok((backend, rejected)),
                Err(reason) => rejected.push(reason),
            }
        }
        Err(rejected)
    }
}

#[cfg(feature = "cuda")]
fn cuda_probe(plan: &ComputePlan) -> BackendProbe {
    crate::sim::cuda::CudaLocalBackend::probe(plan)
}

#[cfg(not(feature = "cuda"))]
fn cuda_probe(_plan: &ComputePlan) -> BackendProbe {
    BackendProbe::unavailable(BackendKind::Cuda, "built without the cuda feature")
}

fn build_one(
    candidate: &Candidate,
    plan: &ComputePlan,
    initial: &[f32],
) -> Result<Box<dyn LocalBackend>, String> {
    match candidate {
        Candidate::Cuda => build_cuda(plan, initial),
        Candidate::Wgpu(_) => crate::sim::wgpu_backend::WgpuExperimentBackend::new(plan, initial)
            .map(|backend| Box::new(backend) as Box<dyn LocalBackend>)
            .map_err(|error| format!("wgpu: {error}")),
        Candidate::Cpu => CpuBackend::new(plan, initial)
            .map(|backend| Box::new(backend) as Box<dyn LocalBackend>)
            .map_err(|error| format!("cpu: {error}")),
    }
}

#[cfg(feature = "cuda")]
fn build_cuda(plan: &ComputePlan, initial: &[f32]) -> Result<Box<dyn LocalBackend>, String> {
    crate::sim::cuda::CudaLocalBackend::new(plan, initial)
        .map(|backend| Box::new(backend) as Box<dyn LocalBackend>)
        .map_err(|error| format!("cuda: {error}"))
}

#[cfg(not(feature = "cuda"))]
fn build_cuda(_plan: &ComputePlan, _initial: &[f32]) -> Result<Box<dyn LocalBackend>, String> {
    Err("cuda: built without the cuda feature".to_string())
}

/// One line explaining an Auto fallback, for the persistent notice.
pub fn fallback_notice(
    rejected: &[String],
    chosen: &crate::sim::local_backend::BackendDescriptor,
) -> Option<String> {
    if rejected.is_empty() {
        return None;
    }
    Some(format!(
        "{}; using {}",
        rejected.join("; "),
        chosen.summary()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const AMD: &str = "AMD Radeon";
    const INTEL: &str = "Intel Iris Xe";

    fn cuda_ok() -> BackendProbe {
        BackendProbe::available(BackendKind::Cuda, "NVIDIA", GpuDeviceType::Discrete)
    }

    fn amd_discrete() -> BackendProbe {
        BackendProbe::available(BackendKind::Wgpu, AMD, GpuDeviceType::Discrete)
    }

    fn intel_integrated() -> BackendProbe {
        BackendProbe::available(BackendKind::Wgpu, INTEL, GpuDeviceType::Integrated)
    }

    fn cpu_ok() -> BackendProbe {
        BackendProbe::available(BackendKind::Cpu, "CPU reference", GpuDeviceType::Cpu)
    }

    fn fake_probes(probes: impl IntoIterator<Item = BackendProbe>) -> Vec<BackendProbe> {
        let mut all = probes.into_iter().collect::<Vec<_>>();
        all.push(cpu_ok());
        all
    }

    #[test]
    fn auto_orders_cuda_then_discrete_wgpu_then_integrated_wgpu_then_cpu() {
        let probes = fake_probes([cuda_ok(), intel_integrated(), amd_discrete()]);
        assert_eq!(
            BackendSelector::candidates(BackendPolicy::Auto, probes),
            vec![
                Candidate::Cuda,
                Candidate::Wgpu(AMD.into()),
                Candidate::Wgpu(INTEL.into()),
                Candidate::Cpu
            ]
        );
    }

    #[test]
    fn auto_falls_through_to_cpu_when_no_gpu_is_available() {
        let probes = fake_probes([
            BackendProbe::unavailable(BackendKind::Cuda, "NVRTC missing"),
            BackendProbe::unavailable(BackendKind::Wgpu, "no adapter"),
        ]);
        assert_eq!(
            BackendSelector::candidates(BackendPolicy::Auto, probes),
            vec![Candidate::Cpu]
        );
    }

    #[test]
    fn a_cpu_wgpu_adapter_is_not_counted_as_the_gpu_fallback() {
        let probes = fake_probes([BackendProbe::available(
            BackendKind::Wgpu,
            "llvmpipe",
            GpuDeviceType::Cpu,
        )]);
        assert_eq!(
            BackendSelector::candidates(BackendPolicy::Auto, probes),
            vec![Candidate::Cpu]
        );
    }

    #[test]
    fn requiring_a_backend_never_offers_another_kind() {
        let probes = fake_probes([cuda_ok(), amd_discrete()]);
        assert_eq!(
            BackendSelector::candidates(BackendPolicy::RequireCpu, probes.clone()),
            vec![Candidate::Cpu]
        );
        assert_eq!(
            BackendSelector::candidates(BackendPolicy::RequireCuda, probes.clone()),
            vec![Candidate::Cuda]
        );
        assert_eq!(
            BackendSelector::candidates(BackendPolicy::RequireWgpu { adapter: None }, probes),
            vec![Candidate::Wgpu(AMD.into())]
        );
    }

    #[test]
    fn an_unavailable_required_backend_offers_nothing_rather_than_a_substitute() {
        let probes = fake_probes([BackendProbe::unavailable(
            BackendKind::Cuda,
            "no NVIDIA driver",
        )]);
        assert!(BackendSelector::candidates(BackendPolicy::RequireCuda, probes).is_empty());
    }

    #[test]
    fn requiring_a_named_adapter_ignores_the_others() {
        let probes = fake_probes([amd_discrete(), intel_integrated()]);
        assert_eq!(
            BackendSelector::candidates(
                BackendPolicy::RequireWgpu {
                    adapter: Some(INTEL.into())
                },
                probes
            ),
            vec![Candidate::Wgpu(INTEL.into())]
        );
    }

    #[test]
    fn a_fallback_notice_names_what_was_rejected_and_what_runs() {
        let chosen = crate::sim::local_backend::BackendDescriptor::cpu();
        assert_eq!(fallback_notice(&[], &chosen), None);
        let notice = fallback_notice(&["cuda: NVRTC missing".into()], &chosen).unwrap();
        assert!(notice.contains("NVRTC missing"));
        assert!(notice.contains("CPU"));
    }

    #[test]
    fn the_cpu_candidate_always_builds_for_a_valid_plan() {
        let spec = crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(4, 4)
            .normalize_rules()
            .unwrap();
        let plan = crate::sim::compute_plan::compile_compute_plan(&spec).unwrap();
        let cells = crate::sim::local_backend::initial_cells(&plan, &spec);
        let (backend, rejected) = BackendSelector::build(&[Candidate::Cpu], &plan, &cells).unwrap();
        assert_eq!(backend.descriptor().kind, BackendKind::Cpu);
        assert!(rejected.is_empty());
    }

    #[test]
    fn probing_this_machine_always_reports_a_usable_cpu_backend() {
        let spec = crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(4, 4)
            .normalize_rules()
            .unwrap();
        let plan = crate::sim::compute_plan::compile_compute_plan(&spec).unwrap();
        let probes = BackendSelector::probe_all(&plan);
        assert!(
            probes
                .iter()
                .any(|probe| probe.kind == BackendKind::Cpu && probe.available)
        );
        // Whatever is missing must say why, so Settings can explain it.
        for probe in probes.iter().filter(|probe| !probe.available) {
            assert!(probe.reason.is_some(), "{probe:?} has no reason");
        }
    }
}
