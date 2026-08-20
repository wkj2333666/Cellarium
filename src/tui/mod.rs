use crate::app::App;
use crate::render::display::ViewportDisplay;
use crate::render::raster::rasterize_world;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

pub fn draw(frame: &mut ratatui::Frame, app: &mut App, display: &ViewportDisplay) {
    let outer = frame.area();
    let chunks = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Min(3),
        ratatui::layout::Constraint::Length(2),
    ])
    .split(outer);

    let block = Block::default()
        .title(" Cellarium — GPU Cellular Automata ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(96, 140, 220)));
    let viewport = block.inner(chunks[0]);
    let (frame_width, frame_height) = display.framebuffer_size(viewport);
    app.set_viewport(viewport, [frame_width, frame_height]);

    if viewport.width > 0 && viewport.height > 0 {
        let camera = *app.camera();
        let framebuffer = rasterize_world(app.world(), &camera, frame_width, frame_height);
        frame.render_widget(block, chunks[0]);
        display.render(frame, viewport, &framebuffer);
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
    fn status_includes_backend_errors() {
        let app = App::new(SimulationSpec::lenia_orbium(), 8, 8);
        let status = status_text_with_error(
            &app,
            &ViewportDisplay::HalfBlock,
            Some("CUDA driver error: device reset"),
        );

        assert!(status.contains("error CUDA driver error: device reset"));
    }
}
