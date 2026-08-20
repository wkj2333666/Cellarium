use crate::app::App;
use crate::render::display::ViewportDisplay;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

const MINIMUM_KERNEL_PREVIEW_ROWS: usize = 8;

pub fn draw(frame: &mut ratatui::Frame, app: &mut App, display: &ViewportDisplay) {
    let outer = frame.area();
    let chunks = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Min(3),
        ratatui::layout::Constraint::Length(2),
    ])
    .split(outer);
    let content = ratatui::layout::Layout::horizontal([
        ratatui::layout::Constraint::Percentage(72),
        ratatui::layout::Constraint::Min(28),
    ])
    .split(chunks[0]);
    let viewport_area = if outer.width >= 96 {
        content[0]
    } else {
        chunks[0]
    };

    let block = Block::default()
        .title(" Cellarium — GPU Cellular Automata ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(96, 140, 220)));
    let viewport = block.inner(viewport_area);
    let (frame_width, frame_height) = display.framebuffer_size(viewport);
    app.set_viewport(viewport, [frame_width, frame_height]);

    if viewport.width > 0 && viewport.height > 0 {
        let framebuffer = app.render_framebuffer(frame_width, frame_height);
        frame.render_widget(block, viewport_area);
        display.render(frame, viewport, framebuffer);
    }

    if outer.width >= 96 {
        render_editor_panel(frame, app, content[1]);
    }

    if app.kernel_preview_enabled() {
        render_kernel_preview(frame, app, outer);
    }

    let status = Line::from(vec![
        Span::styled(
            status_text(app, display),
            Style::default().fg(Color::Rgb(190, 215, 255)),
        ),
        Span::raw("  "),
        Span::styled(
            "[Space] pause  [N] step  [1] Conway  [2] Lenia  [R] reset  [A] random  [C] clear  [Q] quit  L-drag paint  R-drag erase  M-drag pan  wheel zoom",
            Style::default()
                .fg(Color::Rgb(128, 148, 180))
                .add_modifier(Modifier::DIM),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(status).style(Style::default().bg(Color::Rgb(12, 18, 32))),
        chunks[1],
    );
}

fn render_kernel_preview(frame: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let content_width = area.width.saturating_sub(2).min(64) as usize;
    let content_height = area.height.saturating_sub(2).min(16) as usize;
    let lines = kernel_preview_lines(app, content_width, content_height);
    if lines.is_empty() || area.width < 2 || area.height < 2 {
        return;
    }

    let text_width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or_default()
        .saturating_add(2) as u16;
    let width = text_width.min(area.width);
    let height = (lines.len() as u16).saturating_add(2).min(area.height);
    let x = area.x + (area.width - width) / 2;
    let y = area.y + (area.height - height) / 2;
    let area = ratatui::layout::Rect::new(x, y, width, height);

    let block = Block::default()
        .title(" Kernel ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(150, 190, 255)));
    let text = lines.into_iter().map(Line::from).collect::<Vec<_>>();
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(text)
            .block(block)
            .style(Style::default().bg(Color::Rgb(10, 15, 28))),
        area,
    );
}

fn kernel_preview_lines(app: &App, max_width: usize, max_height: usize) -> Vec<String> {
    if max_width == 0 || max_height < MINIMUM_KERNEL_PREVIEW_ROWS {
        return Vec::new();
    }

    let mut lines = Vec::new();
    let dimensions = app.selected_kernel_dimensions();
    let anchor = app.selected_kernel_anchor();
    lines.push(truncate_chars(
        &format!(
            "Kernel {} {}×{}",
            app.selected_kernel_name(),
            dimensions.0,
            dimensions.1
        ),
        max_width,
    ));
    lines.push(truncate_chars(
        &format!(
            "anchor ({},{}) · radius {}",
            anchor.0,
            anchor.1,
            app.selected_kernel_radius()
        ),
        max_width,
    ));
    lines.push(truncate_chars(
        &format!(
            "normalization {}",
            normalization_label(app.selected_kernel_normalization())
        ),
        max_width,
    ));
    lines.push(truncate_chars(
        &app.selected_kernel_parameter()
            .map_or("parameter —".to_string(), |(name, value)| {
                format!("parameter {name} {value:.3}")
            }),
        max_width,
    ));

    let kernel = &app.spec().kernel;
    let active_values = active_kernel_values(kernel);
    let value_range = if active_values.is_empty() {
        (0.0, 0.0)
    } else {
        (
            active_values.iter().copied().fold(f32::INFINITY, f32::min),
            active_values
                .iter()
                .copied()
                .fold(f32::NEG_INFINITY, f32::max),
        )
    };
    lines.push(truncate_chars(
        &format!(
            "value range {:.3}–{:.3} · active {}",
            value_range.0,
            value_range.1,
            active_values.len()
        ),
        max_width,
    ));

    let sample_rows = max_height
        .saturating_sub(lines.len() + 2)
        .min(4)
        .min(kernel.height);
    if sample_rows > 0 {
        let sample_width = max_width
            .saturating_sub("sample ".len())
            .min(12)
            .min(kernel.width);
        let maximum = active_values
            .iter()
            .fold(0.0_f32, |maximum, value| maximum.max(value.abs()));
        for sample_y in 0..sample_rows {
            let y = sample_index(sample_y, sample_rows, kernel.height);
            let mut sample = String::from("sample ");
            for sample_x in 0..sample_width {
                let x = sample_index(sample_x, sample_width, kernel.width);
                let value = kernel.values[y * kernel.width + x];
                sample.push(sample_symbol(value.abs(), maximum));
            }
            lines.push(truncate_chars(&sample, max_width));
        }
    }

    lines.push(truncate_chars(
        "[K] kernel  [Tab] param  [V] close",
        max_width,
    ));
    lines.push(truncate_chars("[+/-] edit  [G] regenerate", max_width));
    lines.truncate(max_height);
    lines
}

fn active_kernel_values(kernel: &crate::sim::kernel::Kernel) -> Vec<f32> {
    kernel.mask.as_ref().map_or(kernel.values.clone(), |mask| {
        kernel
            .values
            .iter()
            .enumerate()
            .filter(|(index, _)| mask.get(*index).is_some_and(|active| *active))
            .map(|(_, value)| *value)
            .collect()
    })
}

fn sample_index(sample: usize, sample_count: usize, source_count: usize) -> usize {
    if sample_count <= 1 || source_count <= 1 {
        0
    } else {
        sample * (source_count - 1) / (sample_count - 1)
    }
}

fn sample_symbol(value: f32, maximum: f32) -> char {
    if maximum <= 0.0 {
        return '·';
    }
    let ratio = value / maximum;
    if ratio >= 0.75 {
        '█'
    } else if ratio >= 0.5 {
        '▓'
    } else if ratio >= 0.25 {
        '▒'
    } else {
        '░'
    }
}

fn normalization_label(normalization: crate::sim::kernel::Normalization) -> &'static str {
    match normalization {
        crate::sim::kernel::Normalization::None => "none",
        crate::sim::kernel::Normalization::Sum => "sum",
    }
}

fn editor_panel_lines(app: &App, max_width: usize, max_height: usize) -> Vec<String> {
    if max_width == 0 || max_height == 0 {
        return Vec::new();
    }
    let (simulation_rate, render_rate) = app.rates();
    let performance = app.performance();
    let world = app.world();
    let mut lines = vec![
        format!("{} WORLD", panel_marker(app, crate::app::Panel::Overview)),
        format!("size {}×{} · scalar channel", world.width(), world.height()),
        "boundary periodic · editable viewport".to_string(),
        format!("{} RULE", panel_marker(app, crate::app::Panel::Rule)),
        crate::app::rule_name(app.spec()).to_string(),
        if app.expression_editing() {
            format!("edit: {}", app.expression_buffer())
        } else {
            format!(
                "expression: {} · [E] edit",
                app.spec()
                    .growth_expression()
                    .map(crate::sim::parser::format_expression)
                    .unwrap_or_else(|| "n/a".to_string())
            )
        },
        format!("{} KERNEL", panel_marker(app, crate::app::Panel::Kernel)),
        format!(
            "{} {}×{} · anchor ({},{})",
            app.selected_kernel_name(),
            app.selected_kernel_dimensions().0,
            app.selected_kernel_dimensions().1,
            app.selected_kernel_anchor().0,
            app.selected_kernel_anchor().1
        ),
        "[K] select · [G] regenerate · [V] preview".to_string(),
        format!(
            "{} TOPOLOGY",
            panel_marker(app, crate::app::Panel::Topology)
        ),
        "square lattice · dense periodic CSR-ready".to_string(),
        "custom lattice/domain editor available".to_string(),
        "STATISTICS".to_string(),
        format!("tick {} · sim {:.1}/s", app.tick(), simulation_rate),
        format!(
            "render {:.1}/s · inspect {:?}",
            render_rate,
            app.inspected()
        ),
        format!(
            "step {:.2}/{:.2} ms · render {:.2}/{:.2} ms",
            performance.last_step_ms,
            performance.average_step_ms,
            performance.last_render_ms,
            performance.average_render_ms
        ),
        format!("{} ERRORS", panel_marker(app, crate::app::Panel::Errors)),
        app.backend_error().unwrap_or("none").to_string(),
        "[T] next panel · mouse targets viewport".to_string(),
    ];
    lines.truncate(max_height);
    lines
        .into_iter()
        .map(|line| truncate_chars(&line, max_width))
        .collect()
}

fn panel_marker(app: &App, panel: crate::app::Panel) -> &'static str {
    if app.active_panel() == panel {
        "▸"
    } else {
        " "
    }
}

fn render_editor_panel(frame: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let lines = editor_panel_lines(
        app,
        area.width.saturating_sub(2) as usize,
        area.height.saturating_sub(2) as usize,
    )
    .into_iter()
    .map(Line::from)
    .collect::<Vec<_>>();
    let block = Block::default()
        .title(" Editor [T] next panel ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(110, 160, 220)));
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .style(Style::default().bg(Color::Rgb(10, 15, 28))),
        area,
    );
}

fn truncate_chars(value: &str, maximum: usize) -> String {
    if value.chars().count() <= maximum {
        value.to_string()
    } else {
        value.chars().take(maximum).collect()
    }
}

pub fn status_text(app: &App, display: &ViewportDisplay) -> String {
    status_text_with_error(app, display, app.backend_error())
}

fn status_text_with_error(
    app: &App,
    display: &ViewportDisplay,
    backend_error: Option<&str>,
) -> String {
    let (simulation_rate, render_rate) = app.rates();
    let world = app.world();
    let inspected = app
        .inspected()
        .map_or("—".to_string(), |value| format!("{value:.3}"));
    let status = format!(
        "{} · {} · {} · tick {} · {}×{} · zoom {:.1}× · inspect {} · display {} · sim {:.1}/s · render {:.1}/s",
        app.backend_name(),
        crate::app::rule_name(app.spec()),
        if app.paused() { "paused" } else { "running" },
        app.tick(),
        world.width(),
        world.height(),
        app.camera().zoom(),
        inspected,
        display.protocol().label(),
        simulation_rate,
        render_rate,
    );
    if let Some(error) = backend_error {
        format!("{status} · error {error}")
    } else {
        status
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::rule::SimulationSpec;

    #[test]
    fn status_includes_backend_rule_and_independent_rates() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 8, 8);
        app.handle_command(crate::input::Command::TogglePause);
        app.set_rates(12.5, 47.0);
        let status = status_text(&app, &ViewportDisplay::HalfBlock);

        assert!(status.contains("CPU"));
        assert!(status.contains("Lenia"));
        assert!(status.contains("paused"));
        assert!(status.contains("sim 12.5/s"));
        assert!(status.contains("render 47.0/s"));
        assert!(status.contains("8×8"));
        assert!(status.contains("display half-block fallback"));
    }

    #[test]
    fn kernel_preview_is_bounded_and_includes_metadata_sample_and_hints() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 8, 8);
        app.handle_command(crate::input::Command::NextKernelParameter);
        let lines = kernel_preview_lines(&app, 44, 12);
        let text = lines.join("\n");

        assert!(lines.len() <= 12);
        assert!(lines.iter().all(|line| line.chars().count() <= 44));
        assert!(text.contains("ring 27×27"));
        assert!(text.contains("anchor (13,13)"));
        assert!(text.contains("radius 12"));
        assert!(text.contains("normalization sum"));
        assert!(text.contains("parameter center 0.500"));
        assert!(text.contains("value range"));
        assert!(text.contains("active 517"));
        assert!(text.contains("sample"));
        assert!(text.contains("[K] kernel  [Tab] param"));
        assert!(text.contains("[+/-] edit  [G] regenerate"));
    }

    #[test]
    fn kernel_preview_at_the_minimum_height_includes_one_sample_and_both_hints() {
        let mut app = App::new(SimulationSpec::lenia_orbium(), 8, 8);
        app.handle_command(crate::input::Command::NextKernelParameter);
        let lines = kernel_preview_lines(&app, 44, 8);

        assert_eq!(lines.len(), 8);
        assert!(lines[0].contains("ring 27×27"));
        assert!(lines[1].contains("anchor (13,13)"));
        assert!(lines[2].contains("normalization sum"));
        assert!(lines[3].contains("parameter center 0.500"));
        assert!(lines[4].contains("value range"));
        assert!(lines[5].starts_with("sample "));
        assert!(lines[6].contains("[K] kernel"));
        assert!(lines[7].contains("[G] regenerate"));
    }

    #[test]
    fn kernel_preview_below_the_minimum_height_is_suppressed() {
        let app = App::new(SimulationSpec::lenia_orbium(), 8, 8);

        for max_height in 0..8 {
            assert!(
                kernel_preview_lines(&app, 44, max_height).is_empty(),
                "height {max_height} must not render a partial preview"
            );
        }
    }

    #[test]
    fn status_includes_backend_errors() {
        let app = App::new(SimulationSpec::lenia_orbium(), 8, 8);
        let status = status_text_with_error(
            &app,
            &ViewportDisplay::HalfBlock,
            Some("CUDA driver error: device reset"),
        );

        assert!(status.contains("error CUDA driver error: device reset"));
    }

    #[test]
    fn editor_panel_exposes_rule_kernel_topology_stats_and_errors() {
        let app = App::new(SimulationSpec::lenia_orbium(), 8, 8);
        let lines = editor_panel_lines(&app, 48, 32);
        let text = lines.join("\n");

        assert!(text.contains("WORLD"));
        assert!(text.contains("RULE"));
        assert!(text.contains("KERNEL"));
        assert!(text.contains("TOPOLOGY"));
        assert!(text.contains("STATISTICS"));
        assert!(text.contains("ERRORS"));
        assert!(text.contains("periodic"));
        assert!(text.contains("Lenia/Orbium"));
        assert!(lines.iter().all(|line| line.chars().count() <= 48));
    }
}
