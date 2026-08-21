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
}
use crate::sim::experiment_model::{ExperimentSpec, validate_structure};
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
        let compiled = compile_or_reject(self.request.request_id, &self.request.draft)?;
        let world = world_from_spec(&self.request.draft)
            .map_err(|error| reject(self.request.request_id, error.to_string()))?;
        Ok(PreparedExperiment {
            request_id: self.request.request_id,
            base_revision: self.request.base_revision,
            spec: self.request.draft,
            world,
            compiled,
        })
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
    compile_experiment(spec).map_err(|error| reject(request_id, error.to_string()))
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
