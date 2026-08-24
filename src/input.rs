use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Quit,
    TogglePause,
    Step,
    Reset,
    Randomize,
    Clear,
    Conway,
    Lenia,
    NextKernel,
    NextKernelParameter,
    IncreaseKernelParameter,
    DecreaseKernelParameter,
    RegenerateKernel,
    ToggleKernelPreview,
    NextPanel,
    ToggleExpressionEditor,
    ToggleHelp,
    ToggleWorkbench,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiCommand {
    ApplyDraft,
    RevertDraft,
    Undo,
    Redo,
    FocusNext,
    FocusPrevious,
    ContextAdd,
    ContextDelete,
    SelectNext,
    CyclePresentation,
    CycleColor,
    ToggleVisibility,
    ToggleFrozen,
    CyclePreset,
    SaveActive,
    ExportDraft,
    LoadDraft,
    ShapeNext,
    ShapeIncrease,
    ShapeDecrease,
}

pub fn translate_ui_key(event: &KeyEvent) -> Option<UiCommand> {
    if event.kind == crossterm::event::KeyEventKind::Release {
        return None;
    }
    match (event.code, event.modifiers) {
        (KeyCode::Enter, modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(UiCommand::ApplyDraft)
        }
        (KeyCode::Char('z'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(UiCommand::Undo)
        }
        (KeyCode::Char('y'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(UiCommand::Redo)
        }
        (KeyCode::Char('r'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(UiCommand::RevertDraft)
        }
        (KeyCode::Char('s'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(UiCommand::SaveActive)
        }
        (KeyCode::Char('e'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(UiCommand::ExportDraft)
        }
        (KeyCode::Char('l'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(UiCommand::LoadDraft)
        }
        (KeyCode::BackTab, _) => Some(UiCommand::FocusPrevious),
        (KeyCode::Tab, _) => Some(UiCommand::FocusNext),
        (KeyCode::Char('a'), KeyModifiers::NONE) => Some(UiCommand::ContextAdd),
        (KeyCode::Delete, KeyModifiers::NONE) => Some(UiCommand::ContextDelete),
        (KeyCode::Char(']'), KeyModifiers::NONE) => Some(UiCommand::SelectNext),
        (KeyCode::Char('v'), KeyModifiers::NONE) => Some(UiCommand::CyclePresentation),
        (KeyCode::Char('c'), KeyModifiers::NONE) => Some(UiCommand::CycleColor),
        (KeyCode::Char('x'), KeyModifiers::NONE) => Some(UiCommand::ToggleVisibility),
        (KeyCode::Char('f'), KeyModifiers::NONE) => Some(UiCommand::ToggleFrozen),
        (KeyCode::Char('p'), KeyModifiers::NONE) => Some(UiCommand::CyclePreset),
        (KeyCode::Char('n'), KeyModifiers::NONE) => Some(UiCommand::ShapeNext),
        (KeyCode::Char('+'), KeyModifiers::NONE) => Some(UiCommand::ShapeIncrease),
        (KeyCode::Char('-'), KeyModifiers::NONE) => Some(UiCommand::ShapeDecrease),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomDirection {
    In,
    Out,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MouseAction {
    Zoom { direction: ZoomDirection },
    Pan { dx: f32, dy: f32 },
    Inspect,
    Paint,
    Erase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogicalPoint {
    pub x: u32,
    pub y: u32,
}

pub fn map_viewport_point(
    event: &MouseEvent,
    viewport: Rect,
    logical_size: [u32; 2],
) -> Option<LogicalPoint> {
    let x = u32::from(event.column).checked_sub(u32::from(viewport.x))?;
    let y = u32::from(event.row).checked_sub(u32::from(viewport.y))?;
    if x >= u32::from(viewport.width) || y >= u32::from(viewport.height) {
        return None;
    }
    Some(LogicalPoint {
        x: (x * logical_size[0] / u32::from(viewport.width)).min(logical_size[0].saturating_sub(1)),
        y: (y * logical_size[1] / u32::from(viewport.height))
            .min(logical_size[1].saturating_sub(1)),
    })
}

pub fn should_forward_mouse_event(event: &MouseEvent, applied: bool) -> bool {
    applied || matches!(event.kind, MouseEventKind::Down(_) | MouseEventKind::Up(_))
}

pub fn translate_key(event: &KeyEvent) -> Option<Command> {
    if event.kind == crossterm::event::KeyEventKind::Release {
        return None;
    }
    if event.modifiers.contains(KeyModifiers::CONTROL) {
        return (event.code == KeyCode::Char('c')).then_some(Command::Quit);
    }
    match event.code {
        KeyCode::Char(' ') | KeyCode::Char('p') => Some(Command::TogglePause),
        KeyCode::Char('n') | KeyCode::Enter => Some(Command::Step),
        KeyCode::Char('r') => Some(Command::Reset),
        KeyCode::Char('a') => Some(Command::Randomize),
        KeyCode::Char('c') => Some(Command::Clear),
        KeyCode::Char('1') => Some(Command::Conway),
        KeyCode::Char('2') => Some(Command::Lenia),
        KeyCode::Char('k') => Some(Command::NextKernel),
        KeyCode::Tab => Some(Command::NextKernelParameter),
        KeyCode::Char('+') | KeyCode::Char('=') => Some(Command::IncreaseKernelParameter),
        KeyCode::Char('-') | KeyCode::Char('_') => Some(Command::DecreaseKernelParameter),
        KeyCode::Char('g') => Some(Command::RegenerateKernel),
        KeyCode::Char('v') => Some(Command::ToggleKernelPreview),
        KeyCode::Char('t') => Some(Command::NextPanel),
        KeyCode::Char('e') => Some(Command::ToggleExpressionEditor),
        KeyCode::Char('?') => Some(Command::ToggleHelp),
        KeyCode::Char('w') => Some(Command::ToggleWorkbench),
        KeyCode::Char('q') | KeyCode::Esc => Some(Command::Quit),
        _ => None,
    }
}

#[derive(Default)]
pub struct MouseTracker {
    previous: Option<(f32, f32)>,
    stroke_previous: Option<(f32, f32)>,
    stroke_segment: Option<((f32, f32), (f32, f32))>,
}

impl MouseTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stroke_segment(&self) -> Option<((f32, f32), (f32, f32))> {
        self.stroke_segment
    }

    pub fn update(
        &mut self,
        event: &MouseEvent,
        viewport_width: u16,
        viewport_height: u16,
    ) -> Option<MouseAction> {
        if event.column >= viewport_width || event.row >= viewport_height {
            return None;
        }

        let current = (event.column as f32, event.row as f32);
        match event.kind {
            MouseEventKind::ScrollUp => {
                self.stroke_segment = None;
                Some(MouseAction::Zoom {
                    direction: ZoomDirection::In,
                })
            }
            MouseEventKind::ScrollDown => {
                self.stroke_segment = None;
                Some(MouseAction::Zoom {
                    direction: ZoomDirection::Out,
                })
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.stroke_previous = Some(current);
                self.stroke_segment = Some((current, current));
                Some(MouseAction::Inspect)
            }
            MouseEventKind::Down(MouseButton::Right) => {
                self.stroke_previous = Some(current);
                self.stroke_segment = Some((current, current));
                Some(MouseAction::Erase)
            }
            MouseEventKind::Down(MouseButton::Middle) => {
                self.stroke_segment = None;
                self.previous = Some(current);
                None
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let previous = self.stroke_previous.replace(current).unwrap_or(current);
                self.stroke_segment = Some((previous, current));
                Some(MouseAction::Paint)
            }
            MouseEventKind::Drag(MouseButton::Right) => {
                let previous = self.stroke_previous.replace(current).unwrap_or(current);
                self.stroke_segment = Some((previous, current));
                Some(MouseAction::Erase)
            }
            MouseEventKind::Drag(MouseButton::Middle) => {
                self.stroke_segment = None;
                let Some(previous) = self.previous else {
                    self.previous = Some(current);
                    return None;
                };
                self.previous = Some(current);
                Some(MouseAction::Pan {
                    dx: current.0 - previous.0,
                    dy: current.1 - previous.1,
                })
            }
            MouseEventKind::Up(_) => {
                self.previous = None;
                self.stroke_previous = None;
                self.stroke_segment = None;
                None
            }
            _ => {
                self.stroke_segment = None;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn translates_keyboard_commands() {
        assert_eq!(translate_key(&key(KeyCode::Char('q'))), Some(Command::Quit));
        assert_eq!(
            translate_key(&key(KeyCode::Char(' '))),
            Some(Command::TogglePause)
        );
        assert_eq!(translate_key(&key(KeyCode::Char('n'))), Some(Command::Step));
        assert_eq!(
            translate_key(&key(KeyCode::Char('r'))),
            Some(Command::Reset)
        );
        assert_eq!(
            translate_key(&key(KeyCode::Char('a'))),
            Some(Command::Randomize)
        );
        assert_eq!(
            translate_key(&key(KeyCode::Char('c'))),
            Some(Command::Clear)
        );
        assert_eq!(
            translate_key(&key(KeyCode::Char('1'))),
            Some(Command::Conway)
        );
        assert_eq!(
            translate_key(&key(KeyCode::Char('2'))),
            Some(Command::Lenia)
        );
        assert_eq!(
            translate_key(&key(KeyCode::Char('k'))),
            Some(Command::NextKernel)
        );
        assert_eq!(
            translate_key(&key(KeyCode::Tab)),
            Some(Command::NextKernelParameter)
        );
        assert_eq!(
            translate_key(&key(KeyCode::Char('+'))),
            Some(Command::IncreaseKernelParameter)
        );
        assert_eq!(
            translate_key(&key(KeyCode::Char('='))),
            Some(Command::IncreaseKernelParameter)
        );
        assert_eq!(
            translate_key(&key(KeyCode::Char('-'))),
            Some(Command::DecreaseKernelParameter)
        );
        assert_eq!(
            translate_key(&key(KeyCode::Char('_'))),
            Some(Command::DecreaseKernelParameter)
        );
        assert_eq!(
            translate_key(&key(KeyCode::Char('g'))),
            Some(Command::RegenerateKernel)
        );
        assert_eq!(
            translate_key(&key(KeyCode::Char('v'))),
            Some(Command::ToggleKernelPreview)
        );
        assert_eq!(
            translate_key(&key(KeyCode::Char('t'))),
            Some(Command::NextPanel)
        );
        assert_eq!(
            translate_key(&key(KeyCode::Char('e'))),
            Some(Command::ToggleExpressionEditor)
        );
        assert_eq!(
            translate_key(&key(KeyCode::Char('?'))),
            Some(Command::ToggleHelp)
        );
        assert_eq!(
            translate_key(&key(KeyCode::Char('w'))),
            Some(Command::ToggleWorkbench)
        );
        assert_eq!(
            translate_key(&key(KeyCode::Char('e'))),
            Some(Command::ToggleExpressionEditor)
        );
        assert_eq!(
            translate_key(&key(KeyCode::Char('t'))),
            Some(Command::NextPanel)
        );
    }

    #[test]
    fn ctrl_c_also_quits() {
        let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(translate_key(&event), Some(Command::Quit));
    }

    #[test]
    fn workbench_shortcuts_require_expected_modifiers() {
        assert_eq!(
            translate_ui_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
            Some(UiCommand::ApplyDraft)
        );
        assert_ne!(
            translate_ui_key(&key(KeyCode::Enter)),
            Some(UiCommand::ApplyDraft)
        );
        assert_eq!(
            translate_ui_key(&KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL)),
            Some(UiCommand::Undo)
        );
        assert_eq!(
            translate_ui_key(&KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)),
            Some(UiCommand::FocusPrevious)
        );
        assert_eq!(
            translate_ui_key(&key(KeyCode::Char('a'))),
            Some(UiCommand::ContextAdd)
        );
        assert_eq!(
            translate_ui_key(&key(KeyCode::Delete)),
            Some(UiCommand::ContextDelete)
        );
        assert_eq!(
            translate_ui_key(&key(KeyCode::Char(']'))),
            Some(UiCommand::SelectNext)
        );
        assert_eq!(
            translate_ui_key(&KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL)),
            Some(UiCommand::LoadDraft)
        );
        assert_ne!(
            translate_ui_key(&KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL)),
            Some(UiCommand::LoadDraft)
        );
    }

    #[test]
    fn key_releases_are_ignored() {
        let mut event = key(KeyCode::Char('q'));
        event.kind = crossterm::event::KeyEventKind::Release;

        assert_eq!(translate_key(&event), None);
    }

    #[test]
    fn mouse_wheel_and_drag_have_viewport_actions() {
        let scroll = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 5,
            row: 3,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            MouseTracker::new().update(&scroll, 10, 8),
            Some(MouseAction::Zoom {
                direction: ZoomDirection::In
            })
        );

        let mut tracker = MouseTracker::new();
        tracker.update(
            &MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Middle),
                column: 6,
                row: 2,
                modifiers: KeyModifiers::NONE,
            },
            10,
            8,
        );

        let drag = MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Middle),
            column: 7,
            row: 2,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            tracker.update(&drag, 10, 8),
            Some(MouseAction::Pan { dx: 1.0, dy: 0.0 })
        );
    }

    #[test]
    fn left_drag_retains_the_previous_pointer_sample_for_stroke_interpolation() {
        let mut tracker = MouseTracker::new();
        let down = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 5,
            row: 3,
            modifiers: KeyModifiers::NONE,
        };
        let drag = MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 9,
            row: 6,
            modifiers: KeyModifiers::NONE,
        };

        assert_eq!(tracker.update(&down, 10, 8), Some(MouseAction::Inspect));
        assert_eq!(tracker.update(&drag, 10, 8), Some(MouseAction::Paint));
        assert_eq!(tracker.stroke_segment(), Some(((5.0, 3.0), (9.0, 6.0))));
    }

    #[test]
    fn middle_boundaries_are_forwarded_even_without_an_immediate_action() {
        let down = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Middle),
            column: 2,
            row: 3,
            modifiers: KeyModifiers::NONE,
        };
        let up = MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Middle),
            column: 4,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        assert!(should_forward_mouse_event(&down, false));
        assert!(should_forward_mouse_event(&up, false));
        assert!(!should_forward_mouse_event(
            &MouseEvent {
                kind: MouseEventKind::Moved,
                ..down
            },
            false
        ));
    }

    #[test]
    fn viewport_mapping_matches_canvas_logical_coordinates() {
        let viewport = Rect::new(10, 4, 20, 10);
        let event = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 29,
            row: 13,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            map_viewport_point(&event, viewport, [256, 128]),
            Some(LogicalPoint { x: 243, y: 115 })
        );
        let outside = MouseEvent { column: 9, ..event };
        assert_eq!(map_viewport_point(&outside, viewport, [256, 128]), None);
    }
}
