use crate::app::App;
use crate::workbench::{WorkbenchFocus, WorkbenchSection};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};

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

pub fn draw_workbench(frame: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let layout = workbench_layout(area);
    app.set_workbench_area(area);
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
    let canvas_lines: Vec<String> = match state.section() {
        WorkbenchSection::World => vec![
            "Initial field editor".into(),
            "Paint selected channel on the canvas".into(),
            "Mouse: left paint · right erase".into(),
        ],
        WorkbenchSection::Tiling => {
            let mut lines = vec![
                "Periodic tiling editor".into(),
                "[P] preset · [N] prototype · +/- sides".into(),
            ];
            if let Some(tiling) = &state.draft().tiling {
                lines.push(format!(
                    "period {:.3} × {:.3} · {} prototypes · {} tiles",
                    tiling.translation_a.length(),
                    tiling.translation_b.length(),
                    tiling.prototypes.len(),
                    tiling.instances.len()
                ));
                lines.extend(tiling.prototypes.iter().map(|prototype| {
                    let marker = if Some(prototype.id) == state.tiling_prototype() {
                        "▸"
                    } else {
                        " "
                    };
                    format!("{marker} {}  {:?}", prototype.name, prototype.shape)
                }));
                match crate::sim::tiling::validate_coverage(tiling) {
                    Ok(report) => lines.push(format!(
                        "✓ coverage {:.4} · overlap {:.2e}",
                        report.covered_area, report.overlap_area
                    )),
                    Err(errors) => lines.extend(
                        errors
                            .iter()
                            .take(3)
                            .map(|error| format!("! {}", error.message)),
                    ),
                }
                match crate::sim::tiling::canonical_half_edges(tiling, 1e-9) {
                    Ok(edges) => lines.push(format!("✓ paired seams {}", edges.len())),
                    Err(errors) => lines.push(format!("! seams: {}", errors.join("; "))),
                }
            } else {
                lines.push("no polygon draft · [P] create square preset".into());
            }
            lines
        }
        WorkbenchSection::Channels => std::iter::once("Channel compositor".to_string())
            .chain(state.draft().channels.iter().map(|channel| {
                let selected = channel.id == state.selected_channel();
                let color = crate::workbench::resolved_color(state.draft(), channel.id)
                    .map(|color| format!("#{:02x}{:02x}{:02x}", color.red, color.green, color.blue))
                    .unwrap_or_else(|| "—".into());
                format!(
                    "{} {}  {}  {}  {}",
                    if selected { "▸" } else { " " },
                    channel.name,
                    color,
                    if channel.display.visible {
                        "visible"
                    } else {
                        "hidden"
                    },
                    if channel.frozen { "frozen" } else { "active" }
                )
            }))
            .collect(),
        WorkbenchSection::Kernels => std::iter::once("Kernel routing editor".to_string())
            .chain(state.draft().kernels.iter().map(|kernel| {
                format!(
                    "{}  ch{} → ch{}  {}×{}  {:?}",
                    kernel.symbol,
                    kernel.source.0,
                    kernel.target.0,
                    kernel.definition.width,
                    kernel.definition.height,
                    kernel.definition.normalization
                )
            }))
            .collect(),
        WorkbenchSection::Growth => {
            let editor = state.growth_editor();
            let mut lines = vec![
                editor.signature().to_string(),
                if state.growth_editing() {
                    "EDITING source · Esc finish".into()
                } else {
                    "[E] edit source".into()
                },
            ];
            lines.extend(
                editor
                    .buffer()
                    .as_str()
                    .lines()
                    .take(8)
                    .map(|line| format!("  {line}")),
            );
            lines.push(format!(
                "plot {}{}",
                plot_sparkline(&editor.plot().data, 36),
                if editor.plot().stale { "  STALE" } else { "" }
            ));
            lines.extend(
                editor
                    .diagnostics()
                    .iter()
                    .take(3)
                    .map(|diagnostic| format!("! {diagnostic}")),
            );
            lines
        }
        WorkbenchSection::Experiment => vec![
            "Experiment review".into(),
            "Validate · Apply · Revert · Save · Load".into(),
            "Runtime changes only after Apply".into(),
        ],
    };
    let canvas_block = panel(" Canvas ", state.focus() == WorkbenchFocus::Canvas);
    let canvas_inner = canvas_block.inner(layout.canvas);
    frame.render_widget(canvas_block, layout.canvas);
    if matches!(state.section(), WorkbenchSection::World) {
        frame.render_widget(ChannelCanvas::new(state), canvas_inner);
    } else {
        frame.render_widget(
            Paragraph::new(canvas_lines.into_iter().map(Line::from).collect::<Vec<_>>())
                .wrap(Wrap { trim: false }),
            canvas_inner,
        );
    }
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
            Line::from("Click section · T section · Tab focus"),
            Line::from("Canvas: left paint · right erase"),
            Line::from("Wheel zoom · middle pan"),
            Line::from("A add · Del remove · ] select"),
            Line::from("V view · C color · X visible · F freeze"),
            Line::from("P tiling preset · E edit Growth"),
            Line::from("Tiling: N prototype · +/- sides"),
            Line::from("Ctrl+Z/Y undo/redo"),
            Line::from("Ctrl+Enter Apply"),
            Line::from("Ctrl+S active · Ctrl+E/O draft"),
            Line::from(app.workbench_notice().unwrap_or("")),
            Line::from("W leave Workbench · ? help"),
        ];
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(panel(
                    " Inspector ",
                    state.focus() == WorkbenchFocus::Inspector,
                )),
            inspector,
        );
    }
    if state.section() == WorkbenchSection::World {
        let (width, height) = match &state.draft().geometry {
            crate::sim::experiment_model::GeometrySpec::RasterGrid(grid) => {
                (grid.width as usize, grid.height as usize)
            }
        };
        app.set_viewport(canvas_inner, [width, height]);
    }
}

struct ChannelCanvas {
    width: usize,
    height: usize,
    planes: Vec<Vec<f32>>,
    colors: Vec<crate::render::channels::Rgb8>,
    visible: Vec<bool>,
    selected: usize,
    view: crate::workbench::ChannelView,
}

impl ChannelCanvas {
    fn new(state: &crate::workbench::WorkbenchState) -> Self {
        let (width, height) = match &state.draft().geometry {
            crate::sim::experiment_model::GeometrySpec::RasterGrid(grid) => {
                (grid.width as usize, grid.height as usize)
            }
        };
        let channels = &state.draft().channels;
        let colors = channels
            .iter()
            .map(|channel| {
                crate::workbench::resolved_color(state.draft(), channel.id)
                    .unwrap_or(crate::render::channels::Rgb8::new(255, 255, 255))
            })
            .collect::<Vec<_>>();
        let selected = channels
            .iter()
            .position(|channel| channel.id == state.selected_channel())
            .unwrap_or(0);
        Self {
            width,
            height,
            planes: channels
                .iter()
                .map(|channel| channel.initial.clone())
                .collect(),
            colors,
            visible: channels
                .iter()
                .map(|channel| channel.display.visible)
                .collect(),
            selected,
            view: state.channel_view(),
        }
    }
}

impl Widget for ChannelCanvas {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() || self.width == 0 || self.height == 0 {
            return;
        }
        for sy in 0..area.height {
            for sx in 0..area.width {
                let (plane, local_x, local_y, local_width, local_height) =
                    if self.view == crate::workbench::ChannelView::Grid {
                        let columns = (self.planes.len() as f32).sqrt().ceil().max(1.0) as usize;
                        let rows = self.planes.len().div_ceil(columns).max(1);
                        let cell_width = usize::from(area.width).div_ceil(columns).max(1);
                        let cell_height = usize::from(area.height).div_ceil(rows).max(1);
                        let column = usize::from(sx) / cell_width;
                        let row = usize::from(sy) / cell_height;
                        (
                            Some(row * columns + column),
                            usize::from(sx) % cell_width,
                            usize::from(sy) % cell_height,
                            cell_width,
                            cell_height,
                        )
                    } else {
                        (
                            None,
                            usize::from(sx),
                            usize::from(sy),
                            usize::from(area.width),
                            usize::from(area.height),
                        )
                    };
                let wx = local_x * self.width / local_width.max(1);
                let wy = local_y * self.height / local_height.max(1);
                let tile = wy * self.width + wx;
                let pixel = if let Some(index) = plane {
                    if index < self.planes.len() && self.visible[index] {
                        crate::render::channels::composite_pixel(
                            &[self.planes[index].get(tile).copied().unwrap_or(0.0)],
                            &[self.colors[index]],
                        )
                    } else {
                        crate::render::channels::Rgb8::new(0, 0, 0)
                    }
                } else {
                    let mut values = Vec::new();
                    let mut colors = Vec::new();
                    for index in 0..self.planes.len() {
                        let include = self.visible[index]
                            && (self.view == crate::workbench::ChannelView::Composite
                                || index == self.selected);
                        if include {
                            values.push(self.planes[index].get(tile).copied().unwrap_or(0.0));
                            colors.push(self.colors[index]);
                        }
                    }
                    crate::render::channels::composite_pixel(&values, &colors)
                };
                if let Some(cell) = buf.cell_mut((area.x + sx, area.y + sy)) {
                    cell.set_symbol(" ");
                    cell.set_bg(Color::Rgb(pixel.red, pixel.green, pixel.blue));
                }
            }
        }
    }
}

fn plot_sparkline(values: &[Option<f32>], width: usize) -> String {
    let finite = values.iter().flatten().copied().collect::<Vec<_>>();
    if finite.is_empty() {
        return "—".into();
    }
    let min = finite.iter().copied().fold(f32::INFINITY, f32::min);
    let max = finite.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let glyphs = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    values
        .iter()
        .take(width)
        .map(|value| {
            value.map_or('×', |value| {
                let ratio = if (max - min).abs() <= f32::EPSILON {
                    0.5
                } else {
                    ((value - min) / (max - min)).clamp(0.0, 1.0)
                };
                glyphs[(ratio * 7.0).round() as usize]
            })
        })
        .collect()
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
