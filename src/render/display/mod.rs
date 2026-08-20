pub mod half_block;

use image::{DynamicImage, ImageBuffer, Rgba};

use crate::render::raster::Framebuffer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayProtocol {
    Kitty,
    Sixel,
    Iterm2,
    HalfBlock,
}

impl DisplayProtocol {
    pub const fn is_pixel_protocol(self) -> bool {
        !matches!(self, Self::HalfBlock)
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Kitty => "Kitty graphics",
            Self::Sixel => "Sixel",
            Self::Iterm2 => "iTerm2 graphics",
            Self::HalfBlock => "half-block fallback",
        }
    }
}

pub enum ViewportDisplay {
    HalfBlock,
    Pixel(ratatui_image::picker::Picker),
}

impl ViewportDisplay {
    pub fn detect() -> Self {
        let term = std::env::var("TERM").unwrap_or_default();
        let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();
        let sixel = std::env::var("SIXEL").unwrap_or_default();
        Self::from_protocol(detect_protocol(&term, &term_program, &sixel))
    }

    fn from_protocol(protocol: DisplayProtocol) -> Self {
        if protocol == DisplayProtocol::HalfBlock {
            return Self::HalfBlock;
        }

        let picker_protocol = match protocol {
            DisplayProtocol::Kitty => ratatui_image::picker::ProtocolType::Kitty,
            DisplayProtocol::Sixel => ratatui_image::picker::ProtocolType::Sixel,
            DisplayProtocol::Iterm2 => ratatui_image::picker::ProtocolType::Iterm2,
            DisplayProtocol::HalfBlock => ratatui_image::picker::ProtocolType::Halfblocks,
        };
        let mut picker = ratatui_image::picker::Picker::halfblocks();
        picker.set_protocol_type(picker_protocol);
        Self::Pixel(picker)
    }

    pub fn framebuffer_size(&self, area: ratatui::layout::Rect) -> (usize, usize) {
        match self {
            Self::HalfBlock => (area.width as usize, area.height as usize * 2),
            Self::Pixel(picker) => {
                let font = picker.font_size();
                (
                    area.width as usize * font.width as usize,
                    area.height as usize * font.height as usize,
                )
            }
        }
    }

    pub fn protocol(&self) -> DisplayProtocol {
        match self {
            Self::HalfBlock => DisplayProtocol::HalfBlock,
            Self::Pixel(picker) => match picker.protocol_type() {
                ratatui_image::picker::ProtocolType::Kitty => DisplayProtocol::Kitty,
                ratatui_image::picker::ProtocolType::Sixel => DisplayProtocol::Sixel,
                ratatui_image::picker::ProtocolType::Iterm2 => DisplayProtocol::Iterm2,
                ratatui_image::picker::ProtocolType::Halfblocks => DisplayProtocol::HalfBlock,
            },
        }
    }

    pub fn render(
        &self,
        frame: &mut ratatui::Frame,
        area: ratatui::layout::Rect,
        framebuffer: &Framebuffer,
    ) {
        if let Self::Pixel(picker) = self {
            let image = framebuffer_to_dynamic_image(framebuffer);
            let size = ratatui::layout::Size::new(area.width, area.height);
            if let Ok(protocol) = picker.new_protocol(image, size, ratatui_image::Resize::Fit(None))
            {
                frame.render_widget(
                    ratatui_image::Image::new(&protocol).allow_clipping(true),
                    area,
                );
                return;
            }
        }

        frame.render_widget(
            ratatui::widgets::Paragraph::new(half_block::half_block_lines(framebuffer)),
            area,
        );
    }
}

pub fn detect_protocol(term: &str, term_program: &str, sixel: &str) -> DisplayProtocol {
    if term.contains("kitty") || term_program == "kitty" {
        DisplayProtocol::Kitty
    } else if term_program == "iTerm.app" || term_program == "vscode" {
        DisplayProtocol::Iterm2
    } else if sixel == "1" || term.contains("sixel") {
        DisplayProtocol::Sixel
    } else {
        DisplayProtocol::HalfBlock
    }
}

pub fn framebuffer_to_dynamic_image(framebuffer: &Framebuffer) -> DynamicImage {
    ImageBuffer::from_fn(
        framebuffer.width() as u32,
        framebuffer.height() as u32,
        |x, y| {
            let color = framebuffer.get(x as usize, y as usize);
            Rgba([color.red, color.green, color.blue, 255])
        },
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::raster::{Framebuffer, Rgb8};
    use image::GenericImageView;

    #[test]
    fn detects_pixel_protocols_and_falls_back_to_half_blocks() {
        assert_eq!(
            detect_protocol("xterm-kitty", "", ""),
            DisplayProtocol::Kitty
        );
        assert_eq!(
            detect_protocol("xterm-256color", "iTerm.app", ""),
            DisplayProtocol::Iterm2
        );
        assert_eq!(
            detect_protocol("xterm-256color", "WezTerm", "1"),
            DisplayProtocol::Sixel
        );
        assert_eq!(
            detect_protocol("xterm-256color", "", ""),
            DisplayProtocol::HalfBlock
        );
        assert!(DisplayProtocol::Kitty.is_pixel_protocol());
        assert!(!DisplayProtocol::HalfBlock.is_pixel_protocol());
    }

    #[test]
    fn converts_framebuffer_pixels_without_reordering() {
        let mut frame = Framebuffer::new(2, 1);
        frame.set(0, 0, Rgb8::new(1, 2, 3));
        frame.set(1, 0, Rgb8::new(4, 5, 6));

        let image = framebuffer_to_dynamic_image(&frame);
        assert_eq!(image.width(), 2);
        assert_eq!(image.height(), 1);
        assert_eq!(image.get_pixel(0, 0), image::Rgba([1, 2, 3, 255]));
        assert_eq!(image.get_pixel(1, 0), image::Rgba([4, 5, 6, 255]));
    }

    #[test]
    fn viewport_display_uses_cell_dimensions_for_halfblocks_and_pixels() {
        let area = ratatui::layout::Rect::new(0, 0, 10, 5);
        let halfblocks = ViewportDisplay::HalfBlock;
        assert_eq!(
            halfblocks.framebuffer_size(area),
            (10, 10),
            "half-block rows represent two framebuffer pixels"
        );

        let picker = ratatui_image::picker::Picker::halfblocks();
        let pixel_display = ViewportDisplay::Pixel(picker);
        assert_eq!(pixel_display.framebuffer_size(area), (100, 100));
    }

    #[test]
    fn viewport_display_uses_pixel_protocols_without_querying_stdio() {
        let display = ViewportDisplay::from_protocol(DisplayProtocol::Sixel);
        assert_eq!(display.protocol(), DisplayProtocol::Sixel);

        let display = ViewportDisplay::from_protocol(DisplayProtocol::HalfBlock);
        assert_eq!(display.protocol(), DisplayProtocol::HalfBlock);
    }

    #[test]
    fn viewport_display_renders_pixel_protocols_into_the_frame() {
        let framebuffer = Framebuffer::new(10, 20);
        let area = ratatui::layout::Rect::new(0, 0, 1, 1);
        let display = ViewportDisplay::Pixel(ratatui_image::picker::Picker::halfblocks());
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(1, 1)).unwrap();

        terminal
            .draw(|frame| display.render(frame, area, &framebuffer))
            .unwrap();
        assert_eq!(display.protocol(), DisplayProtocol::HalfBlock);
    }
}
