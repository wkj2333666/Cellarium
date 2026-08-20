use crate::render::raster::{self, Rgb8};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

pub fn half_block_lines(frame: &raster::Framebuffer) -> Vec<Line<'static>> {
    let mut lines = Vec::with_capacity(frame.height().div_ceil(2));
    for y in (0..frame.height()).step_by(2) {
        let mut spans = Vec::with_capacity(frame.width());
        for x in 0..frame.width() {
            let top = frame.get(x, y);
            let bottom = if y + 1 < frame.height() {
                frame.get(x, y + 1)
            } else {
                top
            };
            spans.push(Span::styled(
                "▀",
                Style::new().fg(color(bottom)).bg(color(top)),
            ));
        }
        lines.push(Line::from(spans));
    }
    lines
}

fn color(value: Rgb8) -> Color {
    Color::Rgb(value.red, value.green, value.blue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    #[test]
    fn renders_two_vertical_pixels_as_one_half_block() {
        let mut frame = raster::Framebuffer::new(1, 2);
        frame.set(0, 0, Rgb8::new(255, 0, 0));
        frame.set(0, 1, Rgb8::new(0, 255, 0));

        let lines = half_block_lines(&frame);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0],
            ratatui::text::Line::from(ratatui::text::Span::styled(
                "\u{2580}",
                Style::new()
                    .fg(Color::Rgb(0, 255, 0))
                    .bg(Color::Rgb(255, 0, 0)),
            ))
        );
    }
}
