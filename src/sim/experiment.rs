use crate::sim::expression::KernelExpression;
use crate::sim::kernel::{Kernel, KernelDefinition, KernelError, KernelValues};
use crate::sim::program::RuleProgram;
use crate::sim::rule::{Rule, RuleConfigError, SimulationSpec};
use crate::sim::topology::{BoardSpec, BoundarySpec, LatticeSpec, TopologyError, compile_topology};
use crate::sim::world::World;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const EXPERIMENT_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExperimentMetadata {
    pub name: String,
    pub description: String,
    pub author: String,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ExperimentRule {
    Conway,
    Lenia {
        kernel: KernelDefinition,
        mu: f32,
        sigma: f32,
        dt: f32,
        growth: Option<KernelExpression>,
    },
    Program {
        program: RuleProgram,
        dt: f32,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExperimentTopology {
    pub lattice: LatticeSpec,
    pub board: BoardSpec,
    pub boundary: BoundarySpec,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExperimentFile {
    pub format_version: u32,
    pub metadata: ExperimentMetadata,
    pub world_size: [usize; 2],
    pub seed: u64,
    pub cells: Vec<f32>,
    pub rule: ExperimentRule,
    pub topology: Option<ExperimentTopology>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
struct ExperimentWire {
    format_version: u32,
    #[serde(default)]
    metadata: ExperimentMetadata,
    world_size: [usize; 2],
    seed: u64,
    cells: Vec<f32>,
    rule: ExperimentRule,
    #[serde(default)]
    topology: Option<ExperimentTopology>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BuiltExperiment {
    pub metadata: ExperimentMetadata,
    pub spec: SimulationSpec,
    pub world_size: [usize; 2],
    pub seed: u64,
    pub cells: Vec<f32>,
    pub topology: Option<ExperimentTopology>,
}

#[derive(Debug, thiserror::Error)]
pub enum ExperimentError {
    #[error("unsupported experiment format version {0}")]
    UnsupportedVersion(u32),
    #[error("world dimensions must be positive")]
    InvalidWorldSize,
    #[error("world cell count is {actual}, expected {expected}")]
    CellCount { expected: usize, actual: usize },
    #[error("world cell {index} is not finite")]
    NonFiniteCell { index: usize },
    #[error("invalid kernel: {0}")]
    Kernel(#[from] KernelError),
    #[error("invalid rule program: {0}")]
    Program(#[from] crate::sim::program::RuleProgramError),
    #[error("invalid topology: {0}")]
    Topology(#[from] TopologyError),
    #[error("invalid rule configuration: {0}")]
    Rule(#[from] RuleConfigError),
    #[error("failed to read experiment `{path}`: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to parse experiment `{path}`: {source}")]
    Parse {
        path: String,
        source: ron::error::SpannedError,
    },
    #[error("failed to encode experiment: {0}")]
    Encode(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompatibilityReport {
    pub supported: bool,
    pub issues: Vec<String>,
}

impl ExperimentFile {
    pub fn compatibility(&self) -> CompatibilityReport {
        match self.validate() {
            Ok(()) => CompatibilityReport {
                supported: true,
                issues: Vec::new(),
            },
            Err(error) => CompatibilityReport {
                supported: false,
                issues: vec![error.to_string()],
            },
        }
    }

    pub fn from_parts(
        metadata: ExperimentMetadata,
        spec: SimulationSpec,
        world: &World,
        seed: u64,
    ) -> Result<Self, ExperimentError> {
        let rule = match spec.rule.clone() {
            Rule::Conway => ExperimentRule::Conway,
            Rule::Lenia { mu, sigma } => ExperimentRule::Lenia {
                kernel: definition_from_kernel(&spec.kernel),
                mu,
                sigma,
                dt: spec.dt,
                growth: spec.growth_expression().cloned(),
            },
            Rule::Program(program) => ExperimentRule::Program {
                program,
                dt: spec.dt,
            },
        };
        let file = Self {
            format_version: EXPERIMENT_FORMAT_VERSION,
            metadata,
            world_size: [world.width(), world.height()],
            seed,
            cells: world.cells().to_vec(),
            rule,
            topology: None,
        };
        file.validate()?;
        Ok(file)
    }

    pub fn build(&self) -> Result<BuiltExperiment, ExperimentError> {
        self.validate()?;
        let spec = match &self.rule {
            ExperimentRule::Conway => SimulationSpec::conway(),
            ExperimentRule::Lenia {
                kernel,
                mu,
                sigma,
                dt,
                growth,
            } => {
                let mut spec = SimulationSpec {
                    rule: Rule::Lenia {
                        mu: *mu,
                        sigma: *sigma,
                    },
                    kernel: kernel.build()?,
                    dt: *dt,
                    growth: growth.clone(),
                };
                if let Some(expression) = growth {
                    spec.growth = Some(expression.clone());
                }
                spec
            }
            ExperimentRule::Program { program, dt } => SimulationSpec::custom_program(
                RuleProgram::new(
                    program.inputs.clone(),
                    program.parameters.clone(),
                    program.update.clone(),
                )?,
                *dt,
            ),
        };
        Ok(BuiltExperiment {
            metadata: self.metadata.clone(),
            spec,
            world_size: self.world_size,
            seed: self.seed,
            cells: self.cells.clone(),
            topology: self.topology.clone(),
        })
    }

    fn validate(&self) -> Result<(), ExperimentError> {
        if self.format_version != EXPERIMENT_FORMAT_VERSION {
            return Err(ExperimentError::UnsupportedVersion(self.format_version));
        }
        if self.world_size.contains(&0) {
            return Err(ExperimentError::InvalidWorldSize);
        }
        let expected = self.world_size[0] * self.world_size[1];
        if self.cells.len() != expected {
            return Err(ExperimentError::CellCount {
                expected,
                actual: self.cells.len(),
            });
        }
        if let Some((index, _)) = self
            .cells
            .iter()
            .enumerate()
            .find(|(_, value)| !value.is_finite())
        {
            return Err(ExperimentError::NonFiniteCell { index });
        }
        if let ExperimentRule::Lenia { kernel, .. } = &self.rule {
            kernel.build()?;
        }
        if let ExperimentRule::Program { program, .. } = &self.rule {
            RuleProgram::new(
                program.inputs.clone(),
                program.parameters.clone(),
                program.update.clone(),
            )?;
        }
        if let Some(topology) = &self.topology {
            compile_topology(&topology.lattice, &topology.board, &topology.boundary)?;
        }
        Ok(())
    }
}

pub fn load_experiment(path: impl AsRef<Path>) -> Result<ExperimentFile, ExperimentError> {
    let path = path.as_ref();
    let display = path.display().to_string();
    let source = std::fs::read_to_string(path).map_err(|source| ExperimentError::Io {
        path: display.clone(),
        source,
    })?;
    let wire: ExperimentWire = ron::from_str(&source).map_err(|source| ExperimentError::Parse {
        path: display,
        source,
    })?;
    let file = ExperimentFile {
        format_version: if wire.format_version == 0 {
            EXPERIMENT_FORMAT_VERSION
        } else {
            wire.format_version
        },
        metadata: wire.metadata,
        world_size: wire.world_size,
        seed: wire.seed,
        cells: wire.cells,
        rule: wire.rule,
        topology: wire.topology,
    };
    file.validate()?;
    Ok(file)
}

pub fn save_experiment(
    path: impl AsRef<Path>,
    file: &ExperimentFile,
) -> Result<(), ExperimentError> {
    file.validate()?;
    let path = path.as_ref();
    let source = ron::ser::to_string_pretty(file, ron::ser::PrettyConfig::default())
        .map_err(|error| ExperimentError::Encode(error.to_string()))?;
    let temporary = path.with_extension(format!(
        "{}tmp-{}",
        path.extension()
            .and_then(|extension| extension.to_str())
            .map_or(String::new(), |extension| format!("{extension}.")),
        std::process::id()
    ));
    std::fs::write(&temporary, source).map_err(|source| ExperimentError::Io {
        path: temporary.display().to_string(),
        source,
    })?;
    std::fs::rename(&temporary, path).map_err(|source| ExperimentError::Io {
        path: path.display().to_string(),
        source,
    })
}

fn definition_from_kernel(kernel: &Kernel) -> KernelDefinition {
    KernelDefinition {
        name: kernel.name.clone(),
        width: kernel.width,
        height: kernel.height,
        anchor_x: kernel.anchor_x,
        anchor_y: kernel.anchor_y,
        mask: kernel.mask.clone(),
        normalization: kernel.normalization,
        parameters: kernel.parameters.clone(),
        values: KernelValues::Explicit(kernel.values.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::expression::KernelExpression;
    use crate::sim::rule::SimulationSpec;
    use crate::sim::world::World;
    use std::collections::BTreeMap;

    #[test]
    fn experiment_roundtrip_preserves_metadata_seed_rule_and_world() {
        let mut world = World::new(3, 2);
        world.replace_cells(&[0.0, 0.25, 0.5, 0.75, 1.0, 0.125]);
        let metadata = ExperimentMetadata {
            name: "Orbium test".to_string(),
            description: "deterministic fixture".to_string(),
            author: "cellarium".to_string(),
            tags: vec!["lenia".to_string(), "test".to_string()],
        };
        let file = ExperimentFile::from_parts(
            metadata.clone(),
            SimulationSpec::lenia_orbium(),
            &world,
            42,
        )
        .unwrap();

        let path = std::env::temp_dir().join(format!(
            "cellarium-experiment-{}-roundtrip.ron",
            std::process::id()
        ));
        save_experiment(&path, &file).unwrap();
        let loaded = load_experiment(&path).unwrap();
        let _ = std::fs::remove_file(path);

        assert_eq!(loaded, file);
        assert_eq!(loaded.metadata, metadata);
        assert_eq!(loaded.seed, 42);
        assert_eq!(loaded.cells, world.cells());
        assert!(matches!(loaded.rule, ExperimentRule::Lenia { .. }));
    }

    #[test]
    fn experiment_build_rejects_wrong_version_and_cell_count() {
        let mut file = ExperimentFile::from_parts(
            ExperimentMetadata::default(),
            SimulationSpec::conway(),
            &World::new(2, 2),
            1,
        )
        .unwrap();
        file.format_version += 1;
        assert!(matches!(
            file.build(),
            Err(ExperimentError::UnsupportedVersion(2))
        ));

        file.format_version = EXPERIMENT_FORMAT_VERSION;
        file.cells.pop();
        assert!(matches!(
            file.build(),
            Err(ExperimentError::CellCount { .. })
        ));
    }

    #[test]
    fn experiment_program_rule_revalidates_on_build() {
        let program = crate::sim::program::RuleProgram::new(
            vec![crate::sim::program::RuleInput::state("self")],
            BTreeMap::new(),
            KernelExpression::Parameter("self".to_string()),
        )
        .unwrap();
        let file = ExperimentFile {
            format_version: EXPERIMENT_FORMAT_VERSION,
            metadata: ExperimentMetadata::default(),
            world_size: [1, 1],
            seed: 9,
            cells: vec![1.0],
            rule: ExperimentRule::Program { program, dt: 1.0 },
            topology: None,
        };

        let built = file.build().unwrap();
        assert_eq!(built.spec.dt, 1.0);
        assert_eq!(built.cells, vec![1.0]);
    }

    #[test]
    fn legacy_version_zero_experiment_migrates_to_current_schema() {
        let path = std::env::temp_dir().join(format!(
            "cellarium-experiment-{}-legacy.ron",
            std::process::id()
        ));
        std::fs::write(
            &path,
            r#"(
                format_version: 0,
                world_size: (1, 1),
                seed: 3,
                cells: [1.0],
                rule: Conway,
            )"#,
        )
        .unwrap();

        let loaded = load_experiment(&path).unwrap();
        let _ = std::fs::remove_file(path);

        assert_eq!(loaded.format_version, EXPERIMENT_FORMAT_VERSION);
        assert_eq!(loaded.metadata, ExperimentMetadata::default());
        assert!(matches!(loaded.rule, ExperimentRule::Conway));
    }

    #[test]
    fn compatibility_report_explains_unsupported_experiment_versions() {
        let mut file = ExperimentFile::from_parts(
            ExperimentMetadata::default(),
            SimulationSpec::conway(),
            &World::new(1, 1),
            1,
        )
        .unwrap();
        assert!(file.compatibility().supported);

        file.format_version = 9;
        let report = file.compatibility();
        assert!(!report.supported);
        assert!(report.issues.iter().any(|issue| issue.contains("version")));
    }
}
