//! Saving, opening and recovering local experiments through the GUI.

use cellarium::gui::CellariumGui;
use cellarium::sim::experiment_model::ExperimentSpec;
use std::path::PathBuf;

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("cellarium-gui-persistence-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("the test directory can be created");
    root
}

fn app(root: &PathBuf) -> CellariumGui {
    let spec = ExperimentSpec::single_channel_lenia(16, 16)
        .normalize_rules()
        .expect("the fixture normalizes");
    let mut app = CellariumGui::for_test(spec);
    app.use_data_root(root);
    app
}

#[test]
fn an_experiment_saves_reopens_and_keeps_what_was_edited() {
    let root = temp_root("reopen");
    let path = root.join("experiment.ron");

    let mut first = app(&root);
    first.set_growth_source("self * 0.25");
    first.save_experiment_as(&path);
    // A successful save confirms itself. Silence is what made saving feel like
    // nothing had happened.
    let notice = first.notice().expect("a save reports where it went");
    assert!(notice.contains("experiment.ron"), "{notice}");
    assert_eq!(first.experiment_path(), Some(path.as_path()));

    // A second session opens the file and finds the same experiment.
    let mut second = app(&root);
    second.open_experiment(&path);
    let notice = second.notice().expect("opening reports what was opened");
    assert!(notice.contains("experiment.ron"), "{notice}");
    assert_eq!(second.growth_source(), "self * 0.25");
    assert_eq!(second.experiment_path(), Some(path.as_path()));
}

#[test]
fn saving_without_a_path_asks_where_rather_than_writing_somewhere_arbitrary() {
    let root = temp_root("no-path");
    let mut app = app(&root);
    app.save_experiment();
    // Save on an unnamed experiment asks where to put it. Reporting "this
    // experiment has no path yet" and stopping told the user to use a control
    // that did not exist, which left no way at all to save from the window.
    assert!(
        app.file_dialog_open(),
        "an unnamed experiment asks for a name instead of refusing"
    );
    assert_eq!(
        app.experiment_path(),
        None,
        "and nothing is written until the user answers"
    );
}

#[test]
fn a_saved_path_is_remembered_for_next_time() {
    let root = temp_root("recent");
    let path = root.join("experiment.ron");
    let mut app = app(&root);
    app.save_experiment_as(&path);
    assert_eq!(app.settings().recent.first(), Some(&path));

    // The setting outlives the session.
    let next = self::app(&root);
    assert_eq!(next.settings().recent.first(), Some(&path));
}

#[test]
fn opening_a_missing_file_reports_it_and_leaves_the_experiment_alone() {
    let root = temp_root("missing");
    let mut app = app(&root);
    let before = app.spec().clone();
    app.open_experiment(root.join("absent.ron"));
    assert!(app.notice().is_some(), "the failure must be reported");
    assert_eq!(app.spec(), &before, "a failed open must change nothing");
    assert_eq!(app.experiment_path(), None);
}

#[test]
fn opening_replaces_the_view_state_that_described_the_old_experiment() {
    let root = temp_root("view-state");
    let path = root.join("experiment.ron");
    let mut app = app(&root);
    app.save_experiment_as(&path);

    // Leave view state behind that belongs to the experiment being replaced.
    app.world_canvas_mut().brush.radius = 9;
    app.kernel_canvas_mut().selected_cell = Some((3, 4));
    app.open_experiment(&path);

    assert_ne!(
        app.world_canvas().brush.radius,
        9,
        "a zoom or selection from the previous experiment must not carry over"
    );
    assert_eq!(app.kernel_canvas().selected_cell, None);
}

#[test]
fn an_autosave_is_offered_for_recovery_and_can_be_discarded() {
    let root = temp_root("recovery");
    let mut app = app(&root);
    assert_eq!(
        app.recoverable(),
        None,
        "a clean session has nothing to recover"
    );

    app.set_growth_source("self * 0.75");
    app.autosave();
    let recovered = app.recoverable().expect("the autosave is recoverable");
    assert_eq!(recovered, *app.spec());

    app.discard_recovery();
    assert_eq!(app.recoverable(), None);
}

#[test]
fn an_old_format_file_opens_without_being_rewritten_on_the_way_in() {
    let root = temp_root("legacy");
    let path = root.join("experiment.ron");
    let spec = ExperimentSpec::single_channel_lenia(8, 8);
    cellarium::document::persistence::save_experiment(&path, &spec).unwrap();

    let mut app = app(&root);
    app.open_experiment(&path);
    assert!(app.notice().is_some_and(|notice| notice.contains("opened")));
    // The experiment that comes back is the experiment that went in: opening
    // does not normalize on the user's behalf.
    assert_eq!(app.spec(), &spec);
}
