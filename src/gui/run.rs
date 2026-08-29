use std::path::PathBuf;

use crate::gui::app::CellariumGui;
use crate::sim::experiment::load_experiment_model;
use crate::sim::experiment_model::ExperimentSpec;

use crate::gui::app::DEFAULT_WORLD;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuiLaunchOptions {
    pub experiment: Option<PathBuf>,
    pub backend: crate::sim::backend_selector::BackendPolicy,
    /// Start without probing a GPU, for a machine where the probe is what hangs.
    pub safe_mode: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum GuiStartupError {
    #[error("failed to load experiment: {0}")]
    Experiment(String),
    #[error("failed to start the window: {0}")]
    Window(String),
}

/// Resolve the experiment the GUI opens with. Kept separate from the event loop
/// so it can be tested without a display server.
pub fn initial_spec(options: &GuiLaunchOptions) -> Result<ExperimentSpec, GuiStartupError> {
    match &options.experiment {
        Some(path) => load_experiment_model(path)
            .map_err(|error| GuiStartupError::Experiment(error.to_string())),
        None => Ok(ExperimentSpec::single_channel_lenia(
            DEFAULT_WORLD,
            DEFAULT_WORLD,
        )),
    }
}

/// Where settings and the autosave live, following the platform's convention.
///
/// A session without one still runs; it simply has nothing to remember with.
fn data_root() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".local/share"))
        })
        .map(|root| root.join("cellarium"))
}

pub fn run(options: GuiLaunchOptions) -> Result<(), GuiStartupError> {
    let spec = initial_spec(&options)?;
    let opened_from = options.experiment.clone();
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Cellarium")
            .with_inner_size([1280.0, 720.0])
            .with_min_inner_size([960.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Cellarium",
        native_options,
        Box::new(move |creation| {
            // The workbench's own visuals, before the first frame is laid out.
            crate::gui::style::install(&creation.egui_ctx);
            let mut app = CellariumGui::new(spec);
            // Safe mode never asks the driver anything, so a machine whose GPU
            // probe hangs can still reach the window and change the setting.
            app.select_backend(if options.safe_mode {
                crate::sim::backend_selector::BackendPolicy::RequireCpu
            } else {
                options.backend.clone()
            });
            if let Some(root) = data_root() {
                app.use_data_root(root);
            }
            // Launching with a path is opening that file: Save has to know
            // where to write without asking again.
            if let Some(path) = opened_from {
                app.set_experiment_path(path);
            }
            // And a session that ended without saving gets its work offered
            // back rather than left on disk unread.
            app.offer_recovery();
            Ok(Box::new(app))
        }),
    )
    .map_err(|error| GuiStartupError::Window(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::experiment_model::GeometrySpec;

    #[test]
    fn the_default_launch_opens_a_single_channel_world() {
        let spec = initial_spec(&GuiLaunchOptions::default()).unwrap();
        let GeometrySpec::RasterGrid(grid) = &spec.geometry;
        assert_eq!((grid.width, grid.height), (DEFAULT_WORLD, DEFAULT_WORLD));
        assert_eq!(spec.channels.len(), 1);
    }

    #[test]
    fn a_missing_experiment_path_reports_the_load_failure() {
        let options = GuiLaunchOptions {
            backend: Default::default(),
            safe_mode: false,
            experiment: Some(PathBuf::from("/nonexistent/cellarium-missing.ron")),
        };
        let error = initial_spec(&options).unwrap_err();
        assert!(matches!(error, GuiStartupError::Experiment(_)));
    }
}
