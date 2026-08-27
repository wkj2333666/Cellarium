use cellarium::cli::{CliMode, parse_cli};
use cellarium::gui::{CellariumGui, Section};
use cellarium::sim::experiment_model::ExperimentSpec;
use std::ffi::OsString;

#[test]
fn running_with_no_arguments_opens_the_window() {
    // The window is the product, so it needs no flag to select it.
    let options = parse_cli(Vec::<OsString>::new()).unwrap();
    assert_eq!(options.mode, CliMode::Gui);
}

#[test]
fn gui_model_constructs_without_opening_a_window() {
    let model = CellariumGui::for_test(ExperimentSpec::single_channel_lenia(8, 8));
    assert_eq!(model.navigation().selected(), Section::Simulation);
}
