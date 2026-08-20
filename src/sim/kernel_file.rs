use crate::sim::kernel::{Kernel, KernelDefinition};
use std::convert::TryFrom;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum KernelFileError {
    #[error("{path}: {source}")]
    Io {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Parse {
        path: std::path::PathBuf,
        source: ron::error::SpannedError,
    },
    #[error("{path}: {source}")]
    Validation {
        path: std::path::PathBuf,
        source: crate::sim::kernel::KernelError,
    },
}

pub fn load_kernel(path: &Path) -> Result<KernelDefinition, KernelFileError> {
    let contents = std::fs::read_to_string(path).map_err(|source| KernelFileError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let definition: KernelDefinition =
        ron::from_str(&contents).map_err(|source| KernelFileError::Parse {
            path: path.to_path_buf(),
            source,
        })?;

    Kernel::try_from(definition.clone()).map_err(|source| KernelFileError::Validation {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(definition)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::expression::KernelExpression;
    use crate::sim::kernel::{KernelValues, Normalization};
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEMP_FILE_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    struct TempKernelFile {
        path: PathBuf,
    }

    impl TempKernelFile {
        fn create(contents: &str) -> Self {
            let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let mut path = std::env::temp_dir();
            path.push(format!(
                "cellarium-kernel-file-{}-{sequence}.ron",
                std::process::id()
            ));
            fs::write(&path, contents).expect("temporary kernel file should be writable");
            Self { path }
        }
    }

    impl Drop for TempKernelFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    #[test]
    fn kernel_file_loads_a_valid_expression_definition() {
        let file = TempKernelFile::create(
            r#"(
                name: "expression",
                width: 2,
                height: 1,
                anchor_x: 1,
                anchor_y: 0,
                mask: Some([true, false]),
                normalization: None,
                parameters: {"scale": 2.0},
                values: Expression(Binary(
                    op: Multiply,
                    lhs: Parameter("scale"),
                    rhs: Constant(3.0),
                )),
            )"#,
        );

        let definition = load_kernel(&file.path).unwrap();

        assert_eq!(definition.name, "expression");
        assert_eq!(
            definition.parameters,
            BTreeMap::from([("scale".to_string(), 2.0)])
        );
        assert_eq!(
            definition.values,
            KernelValues::Expression(KernelExpression::Binary {
                op: crate::sim::expression::BinaryOp::Multiply,
                lhs: Box::new(KernelExpression::Parameter("scale".to_string())),
                rhs: Box::new(KernelExpression::Constant(3.0)),
            })
        );
        assert!(
            !Path::new(&file.path)
                .metadata()
                .is_ok_and(|data| data.len() == 0)
        );
    }

    #[test]
    fn kernel_file_loads_a_valid_explicit_value_definition() {
        let file = TempKernelFile::create(
            r#"(
                name: "explicit",
                width: 2,
                height: 2,
                anchor_x: 0,
                anchor_y: 1,
                mask: None,
                normalization: Sum,
                parameters: {},
                values: Explicit([1.0, 2.0, 3.0, 4.0]),
            )"#,
        );

        let definition = load_kernel(&file.path).unwrap();

        assert_eq!(
            definition,
            KernelDefinition {
                name: "explicit".to_string(),
                width: 2,
                height: 2,
                anchor_x: 0,
                anchor_y: 1,
                mask: None,
                normalization: Normalization::Sum,
                parameters: BTreeMap::new(),
                values: KernelValues::Explicit(vec![1.0, 2.0, 3.0, 4.0]),
            }
        );
    }

    #[test]
    fn kernel_file_reports_malformed_ron_with_file_context() {
        let file = TempKernelFile::create("this is not RON");

        let error = load_kernel(&file.path).unwrap_err();

        assert!(matches!(error, KernelFileError::Parse { .. }));
        assert!(
            error.to_string().contains("cellarium-kernel-file"),
            "error should include the source file: {error}"
        );
    }

    #[test]
    fn kernel_file_reports_invalid_model_data_with_validation_context() {
        let file = TempKernelFile::create(
            r#"(
                name: "invalid",
                width: 0,
                height: 1,
                anchor_x: 0,
                anchor_y: 0,
                mask: None,
                normalization: None,
                parameters: {},
                values: Explicit([]),
            )"#,
        );

        let error = load_kernel(&file.path).unwrap_err();

        assert!(matches!(error, KernelFileError::Validation { .. }));
        let message = error.to_string();
        assert!(
            message.contains("cellarium-kernel-file"),
            "error should include the source file: {message}"
        );
        assert!(
            message.contains("dimensions must be between 1 and 129 cells"),
            "error should include validation context: {message}"
        );
    }
}
