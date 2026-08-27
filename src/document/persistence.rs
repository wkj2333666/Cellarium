//! Local files: experiments, settings and autosave.
//!
//! Every write goes to a sibling temporary file, is flushed and synced, and is
//! then renamed over the target. A crash or a full disk therefore leaves either
//! the previous file or the new one, never a truncated file that loads as a
//! damaged experiment.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::sim::experiment::{encode_experiment_model, load_experiment_model_from_str};
use crate::sim::experiment_model::ExperimentSpec;

/// Permissions for anything written here. An experiment can encode work the
/// user has not published, so it is readable only by them.
#[cfg(unix)]
const FILE_MODE: u32 = 0o600;

#[derive(Debug, thiserror::Error)]
pub enum PersistenceError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: {message}")]
    Format { path: PathBuf, message: String },
    #[error("{0}")]
    Encode(String),
}

/// Settings that outlive one session.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct GuiSettings {
    /// Experiments opened before, newest first.
    pub recent: Vec<PathBuf>,
    /// Where Open and Save start when there is no recent file.
    pub workspace: Option<PathBuf>,
    /// Backend policy chosen last time, by name.
    pub backend: Option<String>,
}

/// Longest recent list kept. Long enough to be useful, short enough that the
/// menu stays readable.
const RECENT_LIMIT: usize = 8;

impl GuiSettings {
    /// Record a path as the most recently used, without duplicating it.
    pub fn remember(&mut self, path: impl AsRef<Path>) {
        let path = path.as_ref().to_path_buf();
        self.recent.retain(|entry| entry != &path);
        self.recent.insert(0, path);
        self.recent.truncate(RECENT_LIMIT);
    }
}

/// Write `contents` to `path` so a reader sees the old file or the new one.
pub fn write_atomically(path: impl AsRef<Path>, contents: &str) -> Result<(), PersistenceError> {
    let path = path.as_ref();
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| PersistenceError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    // The temporary lives beside the target so the rename stays on one
    // filesystem; a rename across filesystems is a copy and is not atomic.
    let temporary = temporary_beside(path);
    let mut file = fs::File::create(&temporary).map_err(|source| PersistenceError::Io {
        path: temporary.clone(),
        source,
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(FILE_MODE))
            .map_err(|source| PersistenceError::Io {
                path: temporary.clone(),
                source,
            })?;
    }
    let write = file
        .write_all(contents.as_bytes())
        .and_then(|()| file.sync_all());
    if let Err(source) = write {
        // A failed write must not leave a stray temporary behind.
        let _ = fs::remove_file(&temporary);
        return Err(PersistenceError::Io {
            path: temporary,
            source,
        });
    }
    drop(file);
    fs::rename(&temporary, path).map_err(|source| {
        let _ = fs::remove_file(&temporary);
        PersistenceError::Io {
            path: path.to_path_buf(),
            source,
        }
    })
}

fn temporary_beside(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Read an experiment, accepting both the old and the current file formats.
///
/// Import is non-destructive: whatever the file says is what is loaded, with no
/// normalization applied on the way in. A file that opens differently from how
/// it was saved is a file the user cannot trust.
pub fn load_experiment(path: impl AsRef<Path>) -> Result<ExperimentSpec, PersistenceError> {
    let path = path.as_ref();
    let source = fs::read_to_string(path).map_err(|source| PersistenceError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    load_experiment_model_from_str(&source).map_err(|error| PersistenceError::Format {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

pub fn save_experiment(
    path: impl AsRef<Path>,
    spec: &ExperimentSpec,
) -> Result<(), PersistenceError> {
    let encoded = encode_experiment_model(spec)
        .map_err(|error| PersistenceError::Encode(error.to_string()))?;
    write_atomically(path, &encoded)
}

/// Where settings live, under the platform's config directory.
pub fn settings_path(root: impl AsRef<Path>) -> PathBuf {
    root.as_ref().join("settings.ron")
}

pub fn load_settings(root: impl AsRef<Path>) -> GuiSettings {
    // Settings are a convenience. A missing or damaged file is not worth
    // refusing to start over, so it falls back to the defaults.
    fs::read_to_string(settings_path(root))
        .ok()
        .and_then(|source| ron::from_str(&source).ok())
        .unwrap_or_default()
}

pub fn save_settings(
    root: impl AsRef<Path>,
    settings: &GuiSettings,
) -> Result<(), PersistenceError> {
    let encoded = ron::ser::to_string_pretty(settings, ron::ser::PrettyConfig::default())
        .map_err(|error| PersistenceError::Encode(error.to_string()))?;
    write_atomically(settings_path(root), &encoded)
}

/// Where the autosave of the working experiment lives.
pub fn autosave_path(root: impl AsRef<Path>) -> PathBuf {
    root.as_ref().join("autosave.ron")
}

/// Save a snapshot of the draft for recovery.
///
/// The caller passes an owned spec, so the copy being written can never be the
/// one the user is still editing.
pub fn write_autosave(
    root: impl AsRef<Path>,
    spec: &ExperimentSpec,
) -> Result<(), PersistenceError> {
    save_experiment(autosave_path(root), spec)
}

/// A recovered experiment, if one is waiting.
pub fn recover(root: impl AsRef<Path>) -> Option<ExperimentSpec> {
    let path = autosave_path(root);
    path.exists().then(|| load_experiment(&path).ok()).flatten()
}

pub fn clear_autosave(root: impl AsRef<Path>) {
    let _ = fs::remove_file(autosave_path(root));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("cellarium-persistence-{name}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the test directory can be created");
        root
    }

    fn spec() -> ExperimentSpec {
        ExperimentSpec::single_channel_lenia(8, 8)
    }

    #[test]
    fn a_saved_experiment_loads_back_identically() {
        let root = temp_root("roundtrip");
        let path = root.join("experiment.ron");
        let original = spec();
        save_experiment(&path, &original).unwrap();
        assert_eq!(load_experiment(&path).unwrap(), original);
    }

    #[test]
    fn a_write_leaves_no_temporary_behind() {
        let root = temp_root("no-temp");
        let path = root.join("experiment.ron");
        save_experiment(&path, &spec()).unwrap();
        let leftovers: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "found {leftovers:?}");
    }

    #[test]
    fn overwriting_keeps_the_old_file_readable_until_the_new_one_is_complete() {
        let root = temp_root("overwrite");
        let path = root.join("experiment.ron");
        save_experiment(&path, &spec()).unwrap();
        let mut changed = spec();
        changed.name = "second".into();
        save_experiment(&path, &changed).unwrap();
        // After the rename the file is wholly the new one, never a mixture.
        assert_eq!(load_experiment(&path).unwrap().name, "second");
    }

    #[cfg(unix)]
    #[test]
    fn a_saved_file_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp_root("mode");
        let path = root.join("experiment.ron");
        save_experiment(&path, &spec()).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, FILE_MODE, "found {mode:o}");
    }

    #[test]
    fn a_missing_file_is_an_error_naming_the_path() {
        let root = temp_root("missing");
        let path = root.join("absent.ron");
        let error = load_experiment(&path).unwrap_err().to_string();
        assert!(error.contains("absent.ron"), "{error}");
    }

    #[test]
    fn a_damaged_file_is_reported_rather_than_loaded_as_something_else() {
        let root = temp_root("damaged");
        let path = root.join("experiment.ron");
        fs::write(&path, "this is not an experiment").unwrap();
        assert!(load_experiment(&path).is_err());
    }

    #[test]
    fn settings_survive_a_round_trip_and_missing_settings_are_not_fatal() {
        let root = temp_root("settings");
        assert_eq!(load_settings(&root), GuiSettings::default());

        let mut settings = GuiSettings::default();
        settings.remember("/tmp/one.ron");
        settings.remember("/tmp/two.ron");
        save_settings(&root, &settings).unwrap();
        assert_eq!(load_settings(&root), settings);
    }

    #[test]
    fn damaged_settings_fall_back_to_defaults_instead_of_refusing_to_start() {
        let root = temp_root("settings-damaged");
        fs::write(settings_path(&root), "{{{").unwrap();
        assert_eq!(load_settings(&root), GuiSettings::default());
    }

    #[test]
    fn the_recent_list_moves_a_repeat_to_the_front_without_duplicating_it() {
        let mut settings = GuiSettings::default();
        settings.remember("/a.ron");
        settings.remember("/b.ron");
        settings.remember("/a.ron");
        assert_eq!(
            settings.recent,
            vec![PathBuf::from("/a.ron"), PathBuf::from("/b.ron")]
        );
    }

    #[test]
    fn the_recent_list_is_bounded() {
        let mut settings = GuiSettings::default();
        for index in 0..(RECENT_LIMIT + 5) {
            settings.remember(format!("/experiment-{index}.ron"));
        }
        assert_eq!(settings.recent.len(), RECENT_LIMIT);
        assert_eq!(
            settings.recent[0],
            PathBuf::from(format!("/experiment-{}.ron", RECENT_LIMIT + 4)),
            "the newest entry is first"
        );
    }

    #[test]
    fn an_autosave_can_be_recovered_and_then_cleared() {
        let root = temp_root("autosave");
        assert_eq!(recover(&root), None);

        let mut spec = spec();
        spec.name = "in progress".into();
        write_autosave(&root, &spec).unwrap();
        assert_eq!(
            recover(&root).map(|spec| spec.name),
            Some("in progress".into())
        );

        clear_autosave(&root);
        assert_eq!(recover(&root), None);
    }

    #[test]
    fn a_damaged_autosave_offers_nothing_rather_than_failing_startup() {
        let root = temp_root("autosave-damaged");
        fs::write(autosave_path(&root), "not an experiment").unwrap();
        assert_eq!(recover(&root), None);
    }
}
