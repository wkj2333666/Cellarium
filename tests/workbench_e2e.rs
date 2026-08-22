//! Deterministic client-side E2E contract.  It drives the same command and
//! mouse paths used by the terminal loop, then verifies the draft reaches the
//! atomic Apply boundary and that invalid geometry is rejected without
//! replacing the previous draft.

use cellarium::app::App;
use cellarium::input::{Command, MouseTracker, UiCommand, translate_key};
use cellarium::sim::rule::SimulationSpec;
use cellarium::sim::tiling::{TilingPreset, build_preset};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

#[test]
fn keyboard_mouse_growth_and_tiling_apply_are_end_to_end() {
    let mut app = App::new(SimulationSpec::lenia_orbium(), 16, 16);

    assert_eq!(
        translate_key(&KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE)),
        Some(Command::ToggleHelp)
    );
    app.handle_command(Command::ToggleHelp);
    assert!(app.help_visible());
    app.handle_command(Command::ToggleExpressionEditor);
    app.replace_expression_buffer("potential - mu");
    app.handle_expression_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(!app.expression_editing());

    app.set_viewport(ratatui::layout::Rect::new(0, 0, 16, 16), [16, 16]);
    let mut tracker = MouseTracker::new();
    assert!(app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 4,
            row: 4,
            modifiers: KeyModifiers::NONE
        },
        &mut tracker
    ));
    assert!(app.inspected().is_some());

    let mut draft = app.active_experiment();
    draft.tiling = Some(build_preset(TilingPreset::Square, 1.0));
    let accepted = app
        .submit_draft(cellarium::sim::service::ApplyRequest {
            request_id: 7,
            base_revision: 0,
            draft: draft.clone(),
        })
        .expect("valid draft applies");
    assert_eq!(accepted.revision, 1);
    assert!(app.tiling_draft().is_some());

    let mut invalid = draft;
    if let Some(tiling) = &mut invalid.tiling
        && let cellarium::sim::tiling::PrototypeShape::SimplePolygon { vertices } =
            &mut tiling.prototypes[0].shape
    {
        vertices.swap(1, 2);
    }
    let rejected = app
        .submit_draft(cellarium::sim::service::ApplyRequest {
            request_id: 8,
            base_revision: 1,
            draft: invalid,
        })
        .expect_err("invalid polygon must be rejected");
    assert!(
        rejected
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "invalid_experiment")
    );
    assert_eq!(app.active_revision(), 1);
}

#[test]
fn user_journey_edits_every_workbench_section_before_authoritative_apply() {
    let mut app = App::new(SimulationSpec::lenia_orbium(), 8, 8);
    app.handle_command(Command::ToggleWorkbench);

    // World: a mouse gesture changes the draft only, then undo/redo remains
    // local and observable before the runtime is touched.
    app.set_viewport(ratatui::layout::Rect::new(2, 2, 20, 10), [8, 8]);
    let mut tracker = MouseTracker::new();
    assert!(app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 8,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        &mut tracker,
    ));
    assert_eq!(app.workbench().draft().channels[0].initial[2 * 8 + 2], 1.0);
    app.handle_workbench_ui(UiCommand::Undo).unwrap();
    assert_eq!(app.workbench().draft().channels[0].initial[18], 0.0);
    app.handle_workbench_ui(UiCommand::Redo).unwrap();

    // Channels: add/select/color/view/visibility/freeze are all user-facing
    // semantic operations and remain valid model drafts.
    app.handle_command(Command::NextPanel);
    app.handle_command(Command::NextPanel);
    app.handle_workbench_ui(UiCommand::ContextAdd).unwrap();
    app.handle_workbench_ui(UiCommand::CycleColor).unwrap();
    app.handle_workbench_ui(UiCommand::CyclePresentation)
        .unwrap();
    app.handle_workbench_ui(UiCommand::ToggleVisibility)
        .unwrap();
    app.handle_workbench_ui(UiCommand::SelectNext).unwrap();
    app.handle_workbench_ui(UiCommand::ToggleFrozen).unwrap();
    app.handle_workbench_ui(UiCommand::ToggleFrozen).unwrap();
    assert_eq!(app.workbench().draft().channels.len(), 2);

    // Kernels: adding/removing is atomic with GrowthSource.kernel_inputs.
    app.handle_command(Command::NextPanel);
    app.handle_workbench_ui(UiCommand::ContextAdd).unwrap();
    let draft = app.workbench().draft();
    let selected = app.workbench().selected_channel();
    let growth = draft
        .growth
        .iter()
        .find(|growth| growth.target == selected)
        .unwrap();
    let expected = draft
        .kernels
        .iter()
        .filter(|kernel| kernel.target == selected)
        .map(|kernel| kernel.id)
        .collect::<Vec<_>>();
    assert_eq!(growth.kernel_inputs, expected);

    // Growth: multiline editing and live plot diagnostics are visible before
    // Apply; this source remains valid after inserting whitespace/newline.
    app.handle_command(Command::NextPanel);
    app.handle_command(Command::ToggleExpressionEditor);
    app.handle_workbench_growth_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    app.handle_workbench_growth_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.workbench().growth_editor().diagnostics().is_empty());
    assert!(!app.workbench().growth_editor().plot().data.is_empty());
    app.handle_workbench_growth_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    // Tiling: both presets validate, and a regular polygon can be adjusted
    // through the same key path as a terminal user.
    app.handle_command(Command::NextPanel);
    app.handle_workbench_ui(UiCommand::CyclePreset).unwrap();
    app.handle_workbench_ui(UiCommand::CyclePreset).unwrap();
    app.handle_workbench_ui(UiCommand::ShapeIncrease).unwrap();
    assert!(app.workbench().draft().tiling.is_some());
    cellarium::sim::experiment_model::validate_structure(app.workbench().draft()).unwrap();

    // Experiment: Apply is the only operation that changes the runtime.  The
    // accepted revision is the authoritative proof, not a local optimistic UI.
    let before = app.active_revision();
    let accepted = app
        .submit_draft(app.workbench_apply_request(99))
        .expect("valid Workbench draft applies");
    assert_eq!(accepted.revision, before + 1);
    assert_eq!(app.active_experiment(), accepted.normalized_experiment);
    assert_eq!(
        app.workbench().status(),
        cellarium::workbench::DraftStatus::Clean
    );
}

#[test]
fn mouse_navigation_selects_workbench_sections_and_focuses_panels() {
    let mut app = App::new(SimulationSpec::lenia_orbium(), 8, 8);
    app.handle_command(Command::ToggleWorkbench);
    app.set_workbench_area(ratatui::layout::Rect::new(0, 0, 180, 48));

    let click = |column, row| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    assert!(app.handle_workbench_panel_mouse(click(8, 6)));
    assert_eq!(
        app.workbench().section(),
        cellarium::workbench::WorkbenchSection::Experiment
    );
    assert_eq!(
        app.workbench().focus(),
        cellarium::workbench::WorkbenchFocus::Outline
    );

    assert!(app.handle_workbench_panel_mouse(click(150, 8)));
    assert_eq!(
        app.workbench().focus(),
        cellarium::workbench::WorkbenchFocus::Inspector
    );
    assert!(!app.handle_workbench_panel_mouse(click(80, 8)));
    assert_eq!(
        app.workbench().focus(),
        cellarium::workbench::WorkbenchFocus::Canvas
    );
}
