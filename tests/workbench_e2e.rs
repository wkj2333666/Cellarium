//! Deterministic client-side E2E contract.  It drives the same command and
//! mouse paths used by the terminal loop, then verifies the draft reaches the
//! atomic Apply boundary and that invalid geometry is rejected without
//! replacing the previous draft.

use cellarium::app::App;
use cellarium::input::{Command, MouseTracker, translate_key};
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
    if let Some(tiling) = &mut invalid.tiling {
        if let cellarium::sim::tiling::PrototypeShape::SimplePolygon { vertices } =
            &mut tiling.prototypes[0].shape
        {
            vertices.swap(1, 2);
        }
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
