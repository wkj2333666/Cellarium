//! The Growth workspace: signature, source editing, axes and the plot.

use cellarium::document::selection::{PlotAxes, PlotSymbol};
use cellarium::gui::canvas::growth::PlotScene;
use cellarium::gui::sections::growth::Axis;
use cellarium::gui::{CellariumGui, Section, layout};
use cellarium::sim::experiment_model::{ExperimentSpec, KernelId, UpdateMode};
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

type Gui = Harness<'static, CellariumGui>;

fn growth_gui() -> Gui {
    let spec = ExperimentSpec::single_channel_lenia(16, 16)
        .normalize_rules()
        .expect("the fixture normalizes");
    let mut app = CellariumGui::for_test(spec);
    app.navigation_mut().select(Section::Growth);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 720.0))
        .build_ui_state(|ui, app: &mut CellariumGui| layout::draw(app, ui), app);
    harness.run();
    harness
}

/// A binding with four kernels and a source that reads one of them.
fn growth_gui_with_four_kernels_source(template: &str) -> (Gui, Vec<KernelId>) {
    let mut gui = growth_gui();
    while gui.state().kernel_cards().len() < 4 {
        gui.state_mut().add_kernel();
        gui.run();
    }
    let ids: Vec<KernelId> = gui
        .state()
        .kernel_cards()
        .into_iter()
        .map(|card| card.id)
        .collect();
    let symbols: Vec<String> = gui
        .state()
        .kernel_cards()
        .into_iter()
        .map(|card| card.symbol)
        .collect();
    // `k3` in the template names the fourth kernel of the signature.
    let source = template
        .replace("k3", &symbols[3])
        .replace("k1", &symbols[1]);
    gui.state_mut().set_growth_source(source);
    gui.run();
    (gui, ids)
}

fn click(gui: &mut Gui, label: &str) {
    gui.get_by_label(label).click();
    gui.run();
}

#[test]
fn adding_kernels_updates_signature_but_only_referenced_inputs_choose_axes() {
    let (mut gui, ids) = growth_gui_with_four_kernels_source("gauss(k3, 0.5, 0.1)");
    assert_eq!(gui.state().growth_signature().kernel_inputs.len(), 4);
    assert_eq!(
        gui.state().plot_axes(),
        PlotAxes::Curve(PlotSymbol::Kernel(ids[3])),
        "the one kernel the program reads is the axis"
    );

    gui.state_mut()
        .set_plot_axis(Axis::Y, PlotSymbol::Kernel(ids[1]));
    gui.run();
    assert_eq!(
        gui.state().plot_axes(),
        PlotAxes::Heatmap(PlotSymbol::Kernel(ids[3]), PlotSymbol::Kernel(ids[1])),
        "asking for a second axis promotes the curve to a heatmap"
    );
}

#[test]
fn the_signature_widens_the_moment_a_kernel_is_added() {
    let mut gui = growth_gui();
    let before = gui.state().growth_signature().kernel_inputs.len();
    gui.state_mut().add_kernel();
    gui.run();
    assert_eq!(
        gui.state().growth_signature().kernel_inputs.len(),
        before + 1
    );
    // The rendered signature is what the editor shows above the source.
    let rendered = gui.state().growth_signature().rendered();
    gui.get_by_label(rendered.as_str());
}

#[test]
fn a_program_that_reads_no_kernel_plots_against_its_own_value() {
    let mut gui = growth_gui();
    gui.state_mut().set_growth_source("0.5 - self");
    gui.run();
    assert_eq!(
        gui.state().plot_axes(),
        PlotAxes::Curve(PlotSymbol::SelfValue)
    );
    assert!(matches!(
        gui.state().growth_scene(),
        Some(PlotScene::Curve { .. })
    ));
}

#[test]
fn invalid_source_is_kept_reported_in_place_and_stops_the_plot() {
    let mut gui = growth_gui();
    gui.state_mut().set_growth_source("gauss(");
    gui.run();

    assert_eq!(
        gui.state().growth_source(),
        "gauss(",
        "work in progress must not be discarded for being incomplete"
    );
    let diagnostics = gui.state().growth_diagnostics();
    assert!(!diagnostics.is_empty(), "the error must be reported");
    assert!(
        gui.state().growth_scene().is_none(),
        "a program that does not compile has no plot"
    );
    // The location is stated in editor coordinates, not as a byte offset, and
    // the reason is a sentence rather than the compiler's own error code: a
    // user cannot act on `expected_expression`.
    let described = cellarium::document::growth::describe_diagnostic(&diagnostics[0].code, "");
    assert!(
        !described.contains('_'),
        "the message must read as prose, not as an identifier: {described}"
    );
    gui.get_by_label(format!("line 1, column 7: {described}").as_str());
}

#[test]
fn fixing_the_source_brings_the_plot_back() {
    let mut gui = growth_gui();
    gui.state_mut().set_growth_source("gauss(");
    gui.run();
    assert!(gui.state().growth_scene().is_none());

    gui.state_mut().set_growth_source("self * 0.5");
    gui.run();
    assert!(gui.state().growth_diagnostics().is_empty());
    assert!(gui.state().growth_scene().is_some());
    gui.get_by_label("compiles");
}

#[test]
fn rate_and_value_are_a_visible_choice_that_reaches_the_model() {
    let mut gui = growth_gui();
    let before = gui.state().growth_mode();
    let (other, label) = match before {
        UpdateMode::GrowthRate => (UpdateMode::DirectUpdate, "Value"),
        UpdateMode::DirectUpdate => (UpdateMode::GrowthRate, "Rate"),
    };
    click(&mut gui, label);
    assert_eq!(gui.state().growth_mode(), other);
}

#[test]
fn a_pinned_input_holds_still_while_the_axis_varies() {
    let (mut gui, ids) = growth_gui_with_four_kernels_source("k3 + k1");
    // Both kernels are read, so the default is a heatmap. Choosing a single
    // axis leaves the other one pinned, which is the case under test.
    gui.state_mut()
        .set_plot_axis(Axis::X, PlotSymbol::Kernel(ids[3]));
    gui.state_mut()
        .set_plot_axis(Axis::Y, PlotSymbol::Kernel(ids[3]));
    gui.run();
    assert_eq!(
        gui.state().plot_axes(),
        PlotAxes::Curve(PlotSymbol::Kernel(ids[3]))
    );

    gui.state_mut()
        .growth_plot_mut()
        .pinned
        .set_kernel(ids[1], 0.0);
    gui.run();
    let Some(PlotScene::Curve { samples, .. }) = gui.state().growth_scene() else {
        panic!("one referenced kernel gives a curve");
    };
    let at_zero = samples[0].1;

    gui.state_mut()
        .growth_plot_mut()
        .pinned
        .set_kernel(ids[1], 0.25);
    gui.run();
    let Some(PlotScene::Curve { samples, .. }) = gui.state().growth_scene() else {
        panic!("one referenced kernel gives a curve");
    };
    assert!(
        (samples[0].1 - at_zero - 0.25).abs() < 1e-5,
        "the pinned value must enter the result"
    );
}

#[test]
fn a_kernel_chip_navigates_to_the_kernel_it_names() {
    let (mut gui, ids) = growth_gui_with_four_kernels_source("gauss(k3, 0.5, 0.1)");
    let symbol = gui.state().kernel_symbol(ids[2]);
    click(&mut gui, symbol.as_str());
    assert_eq!(gui.state().navigation().selected(), Section::Kernels);
    assert_eq!(gui.state().selected_kernel(), Some(ids[2]));
}

#[test]
fn choosing_the_same_symbol_for_both_axes_collapses_back_to_a_curve() {
    let (mut gui, _ids) = growth_gui_with_four_kernels_source("k3 + k1");
    // Two referenced kernels give a heatmap without the user asking.
    let PlotAxes::Heatmap(x, y) = gui.state().plot_axes() else {
        panic!("two referenced kernels give a heatmap");
    };
    assert_ne!(x, y);

    // Asking for the x symbol on y would plot a diagonal and say nothing.
    gui.state_mut().set_plot_axis(Axis::Y, x);
    gui.run();
    assert_eq!(gui.state().plot_axes(), PlotAxes::Curve(x));
}

#[test]
fn a_two_kernel_program_defaults_to_a_heatmap() {
    let (gui, ids) = growth_gui_with_four_kernels_source("k3 * k1");
    assert_eq!(
        gui.state().plot_axes(),
        PlotAxes::Heatmap(PlotSymbol::Kernel(ids[1]), PlotSymbol::Kernel(ids[3])),
        "the axes follow signature order, not the order the symbols are written"
    );
    assert!(matches!(
        gui.state().growth_scene(),
        Some(PlotScene::Heatmap { .. })
    ));
}
