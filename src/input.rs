use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

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
        (KeyCode::BackTab, _) => Some(UiCommand::FocusPrevious),
        (KeyCode::Tab, _) => Some(UiCommand::FocusNext),
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
}

impl MouseTracker {
    pub fn new() -> Self {
        Self::default()
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
            MouseEventKind::ScrollUp => Some(MouseAction::Zoom {
                direction: ZoomDirection::In,
            }),
            MouseEventKind::ScrollDown => Some(MouseAction::Zoom {
                direction: ZoomDirection::Out,
            }),
            MouseEventKind::Down(MouseButton::Left) => Some(MouseAction::Inspect),
            MouseEventKind::Down(MouseButton::Middle) => {
                self.previous = Some(current);
                None
            }
            MouseEventKind::Drag(MouseButton::Left) => Some(MouseAction::Paint),
            MouseEventKind::Drag(MouseButton::Right) => Some(MouseAction::Erase),
            MouseEventKind::Drag(MouseButton::Middle) => {
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
                None
            }
            _ => None,
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
}
