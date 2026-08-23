#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::experiment_model::ExperimentSpec;

    fn service_fixture() -> ExperimentService {
        ExperimentService::new(ExperimentSpec::single_channel_lenia(2, 2)).unwrap()
    }

    #[test]
    fn rejected_apply_preserves_runtime_tick_state_and_revision() {
        let mut service = service_fixture();
        service.step().unwrap();
        let before = service.audit_snapshot();
        let mut invalid = service.active_spec().clone();
        invalid.channels[0].initial[0] = f32::NAN;
        let rejected = service.apply(ApplyRequest {
            request_id: 9,
            base_revision: service.revision(),
            draft: invalid,
        });
        assert!(rejected.is_err());
        assert_eq!(service.audit_snapshot(), before);
    }

    #[test]
    fn stale_revision_is_rejected_before_build() {
        let mut service = service_fixture();
        let result = service.apply(ApplyRequest {
            request_id: 10,
            base_revision: service.revision() + 1,
            draft: service.active_spec().clone(),
        });
        assert!(
            result
                .unwrap_err()
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "revision_conflict")
        );
    }

    #[test]
    fn apply_jobs_and_candidates_are_send() {
        fn assert_send<T: Send>() {}
        assert_send::<PrepareJob>();
        assert_send::<PreparedExperiment>();
    }

    #[test]
    fn active_snapshot_contains_current_channel_state() {
        let mut service = service_fixture();
        service.step().unwrap();
        let exported = service.snapshot_active_experiment();
        assert_eq!(
            exported.channels[0].initial,
            service.world().channel_cells(0)
        );
    }

    #[test]
    fn apply_normalizes_once_and_returns_the_authoritative_model() {
        let mut service = service_fixture();
        let accepted = service
            .apply(ApplyRequest {
                request_id: 12,
                base_revision: service.revision(),
                draft: service.active_spec().clone(),
            })
            .unwrap();
        assert!(accepted.normalized_experiment.kernels.is_empty());
        assert!(accepted.normalized_experiment.growth.is_empty());
        assert!(!accepted.normalized_experiment.rules.is_empty());
        assert_eq!(service.active_spec(), &accepted.normalized_experiment);
    }

    #[test]
    fn basis_diagnostics_have_stable_paths() {
        use crate::sim::basis_kernel::PeriodicKernelDefinition;
        use crate::sim::ruleset::KernelSpatialDefinition;

        let mut service = service_fixture();
        let mut draft = service.active_spec().clone().normalize_rules().unwrap();
        let rule_set = draft.rules.defaults[&crate::sim::experiment_model::ChannelId(0)];
        let rule = draft.rules.get_mut(rule_set).unwrap();
        rule.kernels[0].spatial = KernelSpatialDefinition::Periodic(PeriodicKernelDefinition {
            width: 0,
            height: 1,
            anchor_x: 0,
            anchor_y: 0,
            planes: Default::default(),
        });
        let rejected = service
            .apply(ApplyRequest {
                request_id: 13,
                base_revision: service.revision(),
                draft,
            })
            .unwrap_err();
        assert!(rejected.diagnostics.iter().any(|diagnostic| {
            diagnostic.path.0 == ["basis", "0", "channel", "0", "ruleset", "0", "kernel", "0"]
        }));
    }
}
use crate::sim::experiment_model::{ExperimentSpec, KernelId, KernelSlot, validate_structure};
use crate::sim::ruleset::{KernelSpatialDefinition, RuleSetError};
use crate::sim::runtime::{
    CompiledExperiment, CpuExperimentBackend, RuntimeError, compile_experiment,
};
use crate::sim::world::{ChannelWorld, ChannelWorldError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticPath(pub Vec<String>);

impl DiagnosticPath {
    pub fn field(name: impl Into<String>) -> Self {
        Self(vec![name.into()])
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    pub path: DiagnosticPath,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplyRejected {
    pub request_id: u64,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ApplyAccepted {
    pub request_id: u64,
    pub revision: u64,
    pub normalized_experiment: ExperimentSpec,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ApplyRequest {
    pub request_id: u64,
    pub base_revision: u64,
    pub draft: ExperimentSpec,
}

pub struct PrepareJob {
    request: ApplyRequest,
}

pub struct PreparedExperiment {
    request_id: u64,
    base_revision: u64,
    spec: ExperimentSpec,
    world: ChannelWorld,
    compiled: CompiledExperiment,
}

pub struct ActiveExperiment {
    spec: ExperimentSpec,
    world: ChannelWorld,
    backend: CpuExperimentBackend,
}

pub struct ExperimentService {
    active: ActiveExperiment,
    revision: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AuditSnapshot {
    pub revision: u64,
    pub tick: u64,
    pub cells: Vec<f32>,
}

impl ExperimentService {
    pub fn new(spec: ExperimentSpec) -> Result<Self, ApplyRejected> {
        let spec = normalize_or_reject(0, spec)?;
        let compiled = compile_or_reject(0, &spec)?;
        let world = world_from_spec(&spec).map_err(|error| reject(0, error.to_string()))?;
        Ok(Self {
            active: ActiveExperiment {
                spec,
                world,
                backend: CpuExperimentBackend::new(compiled),
            },
            revision: 0,
        })
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn active_spec(&self) -> &ExperimentSpec {
        &self.active.spec
    }

    pub fn world(&self) -> &ChannelWorld {
        &self.active.world
    }
    pub fn world_mut(&mut self) -> &mut ChannelWorld {
        &mut self.active.world
    }

    pub fn step(&mut self) -> Result<(), RuntimeError> {
        self.active.backend.step(&mut self.active.world)
    }
    pub fn tick(&self) -> u64 {
        self.active.backend.tick()
    }

    pub fn audit_snapshot(&self) -> AuditSnapshot {
        AuditSnapshot {
            revision: self.revision,
            tick: self.active.backend.tick(),
            cells: self.active.world.cells().to_vec(),
        }
    }

    pub fn snapshot_active_experiment(&self) -> ExperimentSpec {
        let mut snapshot = self.active.spec.clone();
        for (channel, values) in snapshot.channels.iter_mut().zip(
            (0..self.active.world.channels())
                .map(|channel| self.active.world.channel_cells(channel)),
        ) {
            channel.initial = values.to_vec();
        }
        snapshot
    }

    pub fn begin_prepare(&self, request: ApplyRequest) -> Result<PrepareJob, ApplyRejected> {
        if request.base_revision != self.revision {
            return Err(reject_with_code(
                request.request_id,
                "revision_conflict",
                format!(
                    "draft is based on revision {}, active revision is {}",
                    request.base_revision, self.revision
                ),
                DiagnosticPath::field("base_revision"),
            ));
        }
        Ok(PrepareJob { request })
    }

    pub fn commit_prepared(
        &mut self,
        prepared: PreparedExperiment,
    ) -> Result<ApplyAccepted, ApplyRejected> {
        if prepared.base_revision != self.revision {
            return Err(reject_with_code(
                prepared.request_id,
                "revision_conflict",
                "active experiment changed while draft was prepared",
                DiagnosticPath::field("base_revision"),
            ));
        }
        let revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| reject(prepared.request_id, "experiment revision overflow"))?;
        let backend = CpuExperimentBackend::new(prepared.compiled);
        self.active = ActiveExperiment {
            spec: prepared.spec.clone(),
            world: prepared.world,
            backend,
        };
        self.revision = revision;
        Ok(ApplyAccepted {
            request_id: prepared.request_id,
            revision,
            normalized_experiment: prepared.spec,
        })
    }

    pub fn apply(&mut self, request: ApplyRequest) -> Result<ApplyAccepted, ApplyRejected> {
        let job = self.begin_prepare(request)?;
        let prepared = job.build()?;
        self.commit_prepared(prepared)
    }
}

impl PrepareJob {
    pub fn build(self) -> Result<PreparedExperiment, ApplyRejected> {
        let spec = normalize_or_reject(self.request.request_id, self.request.draft)?;
        let compiled = compile_or_reject(self.request.request_id, &spec)?;
        let world = world_from_spec(&spec)
            .map_err(|error| reject(self.request.request_id, error.to_string()))?;
        Ok(PreparedExperiment {
            request_id: self.request.request_id,
            base_revision: self.request.base_revision,
            spec,
            world,
            compiled,
        })
    }
}

fn normalize_or_reject(
    request_id: u64,
    spec: ExperimentSpec,
) -> Result<ExperimentSpec, ApplyRejected> {
    if !spec.rules.is_empty()
        && let Err(errors) = spec.rules.validate(&spec.basis_ids(), &spec.channels)
    {
        return Err(ApplyRejected {
            request_id,
            diagnostics: errors
                .iter()
                .map(|error| Diagnostic {
                    code: "invalid_basis_rule".into(),
                    message: error.to_string(),
                    path: rule_error_path(&spec, error),
                })
                .collect(),
        });
    }
    spec.normalize_rules().map_err(|errors| ApplyRejected {
        request_id,
        diagnostics: errors
            .into_iter()
            .map(|error| Diagnostic {
                code: "invalid_experiment".into(),
                message: error.to_string(),
                path: DiagnosticPath::field("experiment"),
            })
            .collect(),
    })
}

fn rule_error_path(spec: &ExperimentSpec, error: &RuleSetError) -> DiagnosticPath {
    use RuleSetError::*;
    let rule_set = match error {
        DuplicateRuleSetId(id) | MissingRuleSet(id) => Some(*id),
        DuplicateKernelId { rule_set, .. }
        | InvalidKernelSymbol { rule_set, .. }
        | InvalidKernel { rule_set, .. }
        | GrowthKernelMismatch { rule_set, .. }
        | EmptyGrowthSource { rule_set }
        | InvalidSharedName { rule_set }
        | InvalidGrowthParameter { rule_set, .. }
        | MissingOutputChannel { rule_set, .. }
        | MissingSourceChannel { rule_set, .. } => Some(*rule_set),
        DefaultTargetMismatch { rule_set, .. } => Some(*rule_set),
        _ => None,
    };
    let kernel = match error {
        DuplicateKernelId { kernel, .. }
        | InvalidKernel { kernel, .. }
        | MissingSourceChannel { kernel, .. } => Some(*kernel),
        _ => None,
    };
    if let Some(rule_set) = rule_set {
        let binding = spec
            .rules
            .bindings
            .iter()
            .filter(|binding| binding.rule_set == rule_set)
            .min_by_key(|binding| (binding.basis, binding.output));
        let mut path = if let Some(binding) = binding {
            vec![
                "basis".into(),
                binding.basis.0.to_string(),
                "channel".into(),
                binding.output.0.to_string(),
            ]
        } else {
            vec!["rules".into()]
        };
        path.extend(["ruleset".into(), rule_set.0.to_string()]);
        if let Some(kernel) = kernel {
            path.extend(["kernel".into(), kernel.0.to_string()]);
        }
        return DiagnosticPath(path);
    }
    match error {
        DuplicateBinding(key)
        | MissingBinding(key)
        | BindingTargetMismatch { binding: key, .. } => DiagnosticPath(vec![
            "basis".into(),
            key.basis.0.to_string(),
            "channel".into(),
            key.output.0.to_string(),
        ]),
        MissingBasis(basis) => DiagnosticPath(vec!["basis".into(), basis.0.to_string()]),
        InvalidBindingOutput(channel) | MissingDefault(channel) | FrozenChannelDefault(channel) => {
            DiagnosticPath(vec!["channel".into(), channel.0.to_string()])
        }
        RuleSetIdExhausted => DiagnosticPath::field("rules"),
        _ => DiagnosticPath::field("rules"),
    }
}

fn compile_or_reject(
    request_id: u64,
    spec: &ExperimentSpec,
) -> Result<CompiledExperiment, ApplyRejected> {
    validate_structure(spec).map_err(|errors| ApplyRejected {
        request_id,
        diagnostics: errors
            .into_iter()
            .map(|error| Diagnostic {
                code: "invalid_experiment".to_string(),
                message: error.to_string(),
                path: DiagnosticPath::field("experiment"),
            })
            .collect(),
    })?;
    let runtime_spec = legacy_runtime_view(spec).map_err(|message| reject(request_id, message))?;
    compile_experiment(&runtime_spec).map_err(|error| reject(request_id, error.to_string()))
}

fn legacy_runtime_view(spec: &ExperimentSpec) -> Result<ExperimentSpec, String> {
    if spec.rules.is_empty() {
        return Ok(spec.clone());
    }
    let mut legacy = spec.clone();
    legacy.rules = Default::default();
    legacy.kernels.clear();
    legacy.growth.clear();
    let mut next_kernel = 0_u32;
    for channel in spec.channels.iter().filter(|channel| !channel.frozen) {
        let rule_set_id = spec
            .rules
            .defaults
            .get(&channel.id)
            .ok_or_else(|| format!("channel {:?} has no default rule set", channel.id))?;
        let rule = spec
            .rules
            .get(*rule_set_id)
            .ok_or_else(|| format!("missing default rule set {rule_set_id:?}"))?;
        let mut growth = rule.growth.clone();
        growth.kernel_inputs.clear();
        for kernel in &rule.kernels {
            let id = KernelId(next_kernel);
            next_kernel = next_kernel
                .checked_add(1)
                .ok_or_else(|| "too many kernels for legacy raster runtime".to_string())?;
            let definition = match &kernel.spatial {
                KernelSpatialDefinition::Raster(definition) => definition.clone(),
                KernelSpatialDefinition::Periodic(_) => {
                    return Err(format!(
                        "periodic kernel {:?} in rule set {:?} requires the basis runtime",
                        kernel.id, rule.id
                    ));
                }
            };
            legacy.kernels.push(KernelSlot {
                id,
                symbol: kernel.symbol.clone(),
                name: kernel.name.clone(),
                source: kernel.source_channel,
                target: channel.id,
                definition,
            });
            growth.kernel_inputs.push(id);
        }
        legacy.growth.push(growth);
    }
    Ok(legacy)
}

fn world_from_spec(spec: &ExperimentSpec) -> Result<ChannelWorld, ChannelWorldError> {
    let (width, height) = match &spec.geometry {
        crate::sim::experiment_model::GeometrySpec::RasterGrid(grid) => {
            (grid.width as usize, grid.height as usize)
        }
    };
    ChannelWorld::from_channels(
        width,
        height,
        &spec
            .channels
            .iter()
            .map(|channel| channel.initial.clone())
            .collect::<Vec<_>>(),
    )
}

fn reject(request_id: u64, message: impl Into<String>) -> ApplyRejected {
    reject_with_code(
        request_id,
        "invalid_experiment",
        message,
        DiagnosticPath::field("experiment"),
    )
}

fn reject_with_code(
    request_id: u64,
    code: &str,
    message: impl Into<String>,
    path: DiagnosticPath,
) -> ApplyRejected {
    ApplyRejected {
        request_id,
        diagnostics: vec![Diagnostic {
            code: code.to_string(),
            message: message.into(),
            path,
        }],
    }
}
