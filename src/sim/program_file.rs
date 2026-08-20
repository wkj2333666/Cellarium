use std::path::Path;

use crate::sim::program::{RuleProgram, RuleProgramError};

#[derive(Debug, thiserror::Error)]
pub enum ProgramFileError {
    #[error("failed to read rule program `{path}`: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse rule program `{path}`: {source}")]
    Parse {
        path: String,
        #[source]
        source: ron::error::SpannedError,
    },
    #[error("rule program `{path}` failed validation: {source}")]
    Validation {
        path: String,
        #[source]
        source: RuleProgramError,
    },
}

pub fn load_rule_program(path: impl AsRef<Path>) -> Result<RuleProgram, ProgramFileError> {
    let path = path.as_ref();
    let display = path.display().to_string();
    let source = std::fs::read_to_string(path).map_err(|source| ProgramFileError::Io {
        path: display.clone(),
        source,
    })?;
    let decoded: RuleProgram =
        ron::from_str(&source).map_err(|source| ProgramFileError::Parse {
            path: display.clone(),
            source,
        })?;
    RuleProgram::new(decoded.inputs, decoded.parameters, decoded.update).map_err(|source| {
        ProgramFileError::Validation {
            path: display,
            source,
        }
    })
}

pub fn save_rule_program(
    path: impl AsRef<Path>,
    program: &RuleProgram,
) -> Result<(), ProgramFileError> {
    let path = path.as_ref();
    let display = path.display().to_string();
    let source = ron::ser::to_string_pretty(program, ron::ser::PrettyConfig::default()).map_err(
        |source| ProgramFileError::Io {
            path: display.clone(),
            source: std::io::Error::other(source.to_string()),
        },
    )?;
    std::fs::write(path, source).map_err(|source| ProgramFileError::Io {
        path: display,
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::expression::KernelExpression;
    use crate::sim::program::RuleInput;
    use std::collections::BTreeMap;

    #[test]
    fn rule_program_roundtrips_through_ron_and_revalidates() {
        let program = RuleProgram::new(
            vec![RuleInput::state("self")],
            BTreeMap::from([("gain".to_string(), 0.5)]),
            KernelExpression::Parameter("gain".to_string()),
        )
        .unwrap();
        let path = std::env::temp_dir().join(format!(
            "cellarium-program-{}-{}.ron",
            std::process::id(),
            "roundtrip"
        ));
        save_rule_program(&path, &program).unwrap();
        let loaded = load_rule_program(&path).unwrap();
        let _ = std::fs::remove_file(path);

        assert_eq!(loaded, program);
    }
}
