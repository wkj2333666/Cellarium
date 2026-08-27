use std::path::PathBuf;

use crate::gui::app::CellariumGui;
use crate::sim::experiment::load_experiment_model;
use crate::sim::experiment_model::ExperimentSpec;

const DEFAULT_WORLD: u32 = 256;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GuiLaunchOptions {
    pub experiment: Option<PathBuf>,
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

pub fn run(options: GuiLaunchOptions) -> Result<(), GuiStartupError> {
    let spec = initial_spec(&options)?;
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
        Box::new(move |_creation| Ok(Box::new(CellariumGui::new(spec)))),
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
            experiment: Some(PathBuf::from("/nonexistent/cellarium-missing.ron")),
        };
        let error = initial_spec(&options).unwrap_err();
        assert!(matches!(error, GuiStartupError::Experiment(_)));
    }
}
