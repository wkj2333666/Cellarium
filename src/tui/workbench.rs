use crate::app::App;
use crate::workbench::{WorkbenchFocus, WorkbenchSection};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorkbenchLayout {
    pub outline: Rect,
    pub canvas: Rect,
    pub inspector: Option<Rect>,
}
pub fn workbench_layout(area: Rect) -> WorkbenchLayout {
    if area.width >= 120 {
        let regions = Layout::new(
            Direction::Horizontal,
            [
                Constraint::Length(24),
                Constraint::Min(60),
                Constraint::Length(36),
            ],
        )
        .split(area);
        WorkbenchLayout {
            outline: regions[0],
            canvas: regions[1],
            inspector: Some(regions[2]),
        }
    } else {
        let regions = Layout::new(
            Direction::Horizontal,
            [
                Constraint::Length(22.min(area.width / 3)),
                Constraint::Min(20),
            ],
        )
        .split(area);
        WorkbenchLayout {
            outline: regions[0],
            canvas: regions[1],
            inspector: None,
        }
    }
}

pub fn draw_workbench(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let layout = workbench_layout(area);
    let state = app.workbench();
    let outline_lines = WorkbenchSection::ALL
        .into_iter()
        .map(|section| {
            Line::from(format!(
                "{} {}",
                if section == state.section() {
                    "▸"
                } else {
                    " "
                },
                section.label()
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(outline_lines).block(panel(
            " Experiment ",
            state.focus() == WorkbenchFocus::Outline,
        )),
        layout.outline,
    );
    let canvas_lines = match state.section() {
        WorkbenchSection::World => vec![
            "Initial field editor",
            "Paint selected channel on the canvas",
            "Mouse: left paint · right erase",
        ],
        WorkbenchSection::Tiling => vec![
            "Periodic tiling editor",
            "Place polygons · snap edges · validate seams",
            "Square and octagon-square presets",
        ],
        WorkbenchSection::Channels => vec![
            "Channel compositor",
            "Composite / Solo / Grid",
            "Add, rename, freeze, color, visibility",
        ],
        WorkbenchSection::Kernels => vec![
            "Kernel routing editor",
            "One or more kernels per target channel",
            "Edit weights, mask, anchor and normalization",
        ],
        WorkbenchSection::Growth => vec![
            "Growth source editor",
            "let bindings · if/else · live curve",
            "Ctrl+Enter applies complete draft",
        ],
        WorkbenchSection::Experiment => vec![
            "Experiment review",
            "Validate · Apply · Revert · Save · Load",
            "Runtime changes only after Apply",
        ],
    };
    frame.render_widget(
        Paragraph::new(canvas_lines.into_iter().map(Line::from).collect::<Vec<_>>())
            .block(panel(" Canvas ", state.focus() == WorkbenchFocus::Canvas)),
        layout.canvas,
    );
    if let Some(inspector) = layout.inspector {
        let selected = state
            .draft()
            .channels
            .iter()
            .find(|channel| channel.id == state.selected_channel());
        let lines = vec![
            Line::from(format!("section: {}", state.section().label())),
            Line::from(format!("draft: {:?}", state.status())),
            Line::from(format!("channels: {}", state.draft().channels.len())),
            Line::from(format!("kernels: {}", state.draft().kernels.len())),
            Line::from(format!(
                "selected: {}",
                selected.map_or("—", |c| c.name.as_str())
            )),
            Line::from(format!("view: {:?}", state.channel_view())),
            Line::from(""),
            Line::from("Tab focus · T section"),
            Line::from("Ctrl+Z/Y undo/redo"),
            Line::from("Ctrl+Enter Apply"),
            Line::from("W leave Workbench · ? help"),
        ];
        frame.render_widget(
            Paragraph::new(lines).block(panel(
                " Inspector ",
                state.focus() == WorkbenchFocus::Inspector,
            )),
            inspector,
        );
    }
}

fn panel(title: &'static str, focused: bool) -> Block<'static> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(
            Style::default()
                .fg(if focused {
                    Color::Rgb(245, 190, 90)
                } else {
                    Color::Rgb(96, 140, 220)
                })
                .add_modifier(if focused {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wide_has_three_regions_and_narrow_two() {
        assert!(
            workbench_layout(Rect::new(0, 0, 180, 50))
                .inspector
                .is_some()
        );
        assert!(
            workbench_layout(Rect::new(0, 0, 80, 30))
                .inspector
                .is_none()
        );
    }
    #[test]
    fn draft_status_debug_is_stable() {
        assert_eq!(
            format!("{:?}", crate::workbench::DraftStatus::Dirty),
            "Dirty"
        );
    }
}
