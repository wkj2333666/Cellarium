use crate::sim::topology::{BoardSpec, BoundarySpec, LatticeSpec, TopologyError, compile_topology};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const LATTICE_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LatticeFile {
    pub format_version: u32,
    pub name: String,
    pub description: String,
    pub author: String,
    pub tags: Vec<String>,
    pub lattice: LatticeSpec,
    pub board: BoardSpec,
    pub boundary: BoundarySpec,
}

#[derive(Debug, thiserror::Error)]
pub enum LatticeFileError {
    #[error("unsupported lattice format version {0}")]
    UnsupportedVersion(u32),
    #[error("failed to read lattice `{path}`: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to parse lattice `{path}`: {source}")]
    Parse {
        path: String,
        source: ron::error::SpannedError,
    },
    #[error("invalid lattice `{path}`: {source}")]
    Validation { path: String, source: TopologyError },
}

impl LatticeFile {
    fn validate(&self) -> Result<(), TopologyError> {
        compile_topology(&self.lattice, &self.board, &self.boundary).map(|_| ())
    }
}

pub fn load_lattice(path: impl AsRef<Path>) -> Result<LatticeFile, LatticeFileError> {
    let path = path.as_ref();
    let display = path.display().to_string();
    let source = std::fs::read_to_string(path).map_err(|source| LatticeFileError::Io {
        path: display.clone(),
        source,
    })?;
    let file: LatticeFile = ron::from_str(&source).map_err(|source| LatticeFileError::Parse {
        path: display.clone(),
        source,
    })?;
    if file.format_version != LATTICE_FORMAT_VERSION {
        return Err(LatticeFileError::UnsupportedVersion(file.format_version));
    }
    file.validate()
        .map_err(|source| LatticeFileError::Validation {
            path: display,
            source,
        })?;
    Ok(file)
}

pub fn save_lattice(path: impl AsRef<Path>, file: &LatticeFile) -> Result<(), LatticeFileError> {
    let path = path.as_ref();
    if file.format_version != LATTICE_FORMAT_VERSION {
        return Err(LatticeFileError::UnsupportedVersion(file.format_version));
    }
    file.validate()
        .map_err(|source| LatticeFileError::Validation {
            path: path.display().to_string(),
            source,
        })?;
    let source =
        ron::ser::to_string_pretty(file, ron::ser::PrettyConfig::default()).map_err(|source| {
            LatticeFileError::Io {
                path: path.display().to_string(),
                source: std::io::Error::other(source.to_string()),
            }
        })?;
    std::fs::write(path, source).map_err(|source| LatticeFileError::Io {
        path: path.display().to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::topology::{Basis2, DomainSpec, NeighborTemplate, SiteSpec};

    fn lattice_file() -> LatticeFile {
        LatticeFile {
            format_version: LATTICE_FORMAT_VERSION,
            name: "square".to_string(),
            description: "unit square lattice".to_string(),
            author: "cellarium".to_string(),
            tags: vec!["square".to_string()],
            lattice: LatticeSpec {
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
                    weight: 1.0,
                }],
            },
            board: BoardSpec {
                domain: DomainSpec::Rect { size: [4, 4] },
            },
            boundary: BoundarySpec::Periodic,
        }
    }

    #[test]
    fn lattice_asset_roundtrips_and_validates_topology() {
        let path =
            std::env::temp_dir().join(format!("cellarium-lattice-{}.ron", std::process::id()));
        save_lattice(&path, &lattice_file()).unwrap();
        let loaded = load_lattice(&path).unwrap();
        let _ = std::fs::remove_file(path);

        assert_eq!(loaded, lattice_file());
    }
}
