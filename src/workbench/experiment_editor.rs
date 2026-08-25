use crate::sim::experiment_model::ExperimentSpec;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const DRAFT_FORMAT_VERSION: u32 = 1;
pub const WORKSPACE_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspacePaths {
    pub workbench: PathBuf,
    pub experiment: PathBuf,
}

impl WorkspacePaths {
    pub fn in_directory(directory: impl AsRef<Path>) -> Self {
        Self {
            workbench: directory.as_ref().join("workbench.ron"),
            experiment: directory.as_ref().join("experiment.ron"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceEnvelope {
    pub format_version: u32,
    pub active_revision: u64,
    pub base_revision: u64,
    pub active: ExperimentSpec,
    pub draft: ExperimentSpec,
}

pub fn default_workspace_paths() -> Result<WorkspacePaths, String> {
    workspace_paths_from(
        std::env::var_os("XDG_DATA_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

fn workspace_paths_from(
    xdg_data_home: Option<&OsStr>,
    home: Option<&OsStr>,
) -> Result<WorkspacePaths, String> {
    let data = xdg_data_home
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && !path.as_os_str().is_empty())
        .or_else(|| {
            home.map(PathBuf::from)
                .map(|path| path.join(".local/share"))
        })
        .ok_or_else(|| "cannot locate user data directory: HOME is not set".to_string())?;
    Ok(WorkspacePaths::in_directory(data.join("cellarium")))
}

pub fn save_workspace(path: impl AsRef<Path>, workspace: &WorkspaceEnvelope) -> Result<(), String> {
    let source = ron::ser::to_string_pretty(workspace, ron::ser::PrettyConfig::default())
        .map_err(|error| error.to_string())?;
    write_atomically(path.as_ref(), source.as_bytes())
}

pub fn load_workspace(path: impl AsRef<Path>) -> Result<WorkspaceEnvelope, String> {
    let source = std::fs::read_to_string(path.as_ref()).map_err(|error| error.to_string())?;
    let workspace: WorkspaceEnvelope = ron::from_str(&source).map_err(|error| error.to_string())?;
    if workspace.format_version != WORKSPACE_FORMAT_VERSION {
        return Err(format!(
            "unsupported workspace format version {}",
            workspace.format_version
        ));
    }
    Ok(workspace)
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| "workspace path has no parent directory".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = parent.join(format!(
        ".{}.tmp-{}-{}",
        path.file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("workspace"),
        std::process::id(),
        NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed),
    ));
    let write_result = (|| -> Result<(), String> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        file.write_all(bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        std::fs::rename(&temporary, path).map_err(|error| error.to_string())?;
        if let Ok(directory) = std::fs::File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    write_result
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DraftEnvelope {
    pub format_version: u32,
    pub base_revision: u64,
    pub draft: ExperimentSpec,
}

pub fn encode_draft(base_revision: u64, draft: &ExperimentSpec) -> Result<String, String> {
    ron::ser::to_string_pretty(
        &DraftEnvelope {
            format_version: DRAFT_FORMAT_VERSION,
            base_revision,
            draft: draft.clone(),
        },
        ron::ser::PrettyConfig::default(),
    )
    .map_err(|error| error.to_string())
}

pub fn decode_draft(source: &str) -> Result<DraftEnvelope, String> {
    let envelope: DraftEnvelope = ron::from_str(source).map_err(|error| error.to_string())?;
    if envelope.format_version != DRAFT_FORMAT_VERSION {
        return Err(format!(
            "unsupported draft format version {}",
            envelope.format_version
        ));
    }
    Ok(envelope)
}

pub fn export_draft(
    path: impl AsRef<Path>,
    base_revision: u64,
    draft: &ExperimentSpec,
) -> Result<(), String> {
    let encoded = encode_draft(base_revision, draft)?;
    std::fs::write(path.as_ref(), encoded).map_err(|error| error.to_string())
}

pub fn load_draft(path: impl AsRef<Path>) -> Result<DraftEnvelope, String> {
    let source = std::fs::read_to_string(path.as_ref()).map_err(|error| error.to_string())?;
    decode_draft(&source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_load_is_rejected_and_invalid_growth_is_recoverable() {
        assert!(decode_draft("not ron").is_err());
        let mut draft = ExperimentSpec::single_channel_lenia(4, 4);
        draft.growth[0].source = "if potential {".into();
        let encoded = encode_draft(7, &draft).unwrap();
        let loaded = decode_draft(&encoded).unwrap();
        assert_eq!(loaded.base_revision, 7);
        assert_eq!(loaded.draft.growth[0].source, "if potential {");
    }

    #[test]
    fn default_workspace_uses_xdg_or_the_standard_home_fallback() {
        assert_eq!(
            workspace_paths_from(
                Some(std::ffi::OsStr::new("/data")),
                Some(std::ffi::OsStr::new("/home/alice")),
            )
            .unwrap(),
            WorkspacePaths {
                workbench: PathBuf::from("/data/cellarium/workbench.ron"),
                experiment: PathBuf::from("/data/cellarium/experiment.ron"),
            },
        );
        assert_eq!(
            workspace_paths_from(None, Some(std::ffi::OsStr::new("/home/alice")))
                .unwrap()
                .workbench,
            PathBuf::from("/home/alice/.local/share/cellarium/workbench.ron"),
        );
    }

    #[test]
    fn workspace_roundtrip_is_atomic_and_keeps_an_invalid_draft_recoverable() {
        let directory = std::env::temp_dir().join(format!(
            "cellarium-workspace-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let paths = WorkspacePaths::in_directory(&directory);
        let active = ExperimentSpec::single_channel_lenia(4, 4);
        let mut draft = active.clone();
        draft.growth[0].source = "if potential {".into();
        let workspace = WorkspaceEnvelope {
            format_version: WORKSPACE_FORMAT_VERSION,
            active_revision: 3,
            base_revision: 3,
            active,
            draft,
        };

        save_workspace(&paths.workbench, &workspace).unwrap();
        let loaded = load_workspace(&paths.workbench).unwrap();

        assert_eq!(loaded, workspace);
        assert!(paths.workbench.exists());
        let temporary_files = std::fs::read_dir(&directory)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains("tmp"))
            .count();
        assert_eq!(
            temporary_files, 0,
            "atomic save left temporary files behind"
        );
        let _ = std::fs::remove_dir_all(directory);
    }
}
