pub mod half_block;

use image::{DynamicImage, ImageBuffer, Rgba};
use std::sync::{
    Arc, Mutex,
    mpsc::{SyncSender, sync_channel},
};
use std::thread::JoinHandle;

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

pub struct PixelDisplay {
    picker: ratatui_image::picker::Picker,
    protocol: Arc<Mutex<Option<ratatui_image::protocol::Protocol>>>,
    tx: Option<SyncSender<(DynamicImage, ratatui::layout::Size)>>,
    worker: Option<JoinHandle<()>>,
}

impl PixelDisplay {
    fn new(picker: ratatui_image::picker::Picker) -> Self {
        let (tx, rx) = sync_channel::<(DynamicImage, ratatui::layout::Size)>(1);
        let protocol = Arc::new(Mutex::new(None));
        let worker_protocol = Arc::clone(&protocol);
        let worker_picker = picker.clone();
        let worker = std::thread::spawn(move || {
            while let Ok((image, size)) = rx.recv() {
                if let Ok(encoded) =
                    worker_picker.new_protocol(image, size, ratatui_image::Resize::Fit(None))
                {
                    if let Ok(mut slot) = worker_protocol.lock() {
                        *slot = Some(encoded);
                    }
                }
            }
        });
        Self {
            picker,
            protocol,
            tx: Some(tx),
            worker: Some(worker),
        }
    }

    fn submit(&self, image: DynamicImage, size: ratatui::layout::Size) {
        if let Some(tx) = &self.tx {
            let _ = tx.try_send((image, size));
        }
    }

    fn render(&self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) -> bool {
        let Ok(protocol) = self.protocol.lock() else {
            return false;
        };
        let Some(protocol) = protocol.as_ref() else {
            return false;
        };
        frame.render_widget(
            ratatui_image::Image::new(protocol).allow_clipping(true),
            area,
        );
        true
    }
}

impl Drop for PixelDisplay {
    fn drop(&mut self) {
        self.tx.take();
        // Encoding a high-resolution frame can outlive the terminal session.
        // Joining here would make shutdown wait behind an obsolete frame and
        // prevent the quit key from returning control to the shell.
        let _ = self.worker.take();
    }
}

pub enum ViewportDisplay {
    HalfBlock,
    Pixel(PixelDisplay),
}

impl ViewportDisplay {
    pub fn detect() -> Self {
        let term = std::env::var("TERM").unwrap_or_default();
        let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();
        let sixel = std::env::var("SIXEL").unwrap_or_default();
        let remote =
            std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_TTY").is_some();
        let remote_graphics = std::env::var("CELLARIUM_REMOTE_GRAPHICS")
            .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
        let protocol = detect_protocol_for_connection(
            &term,
            &term_program,
            &sixel,
            remote && !remote_graphics,
        );
        Self::from_protocol_and_cell_size(protocol, cell_size_from_environment())
    }

    pub fn from_protocol_and_cell_size(
        protocol: DisplayProtocol,
        cell_size: Option<(u16, u16)>,
    ) -> Self {
        if protocol == DisplayProtocol::HalfBlock {
            return Self::HalfBlock;
        }

        let Some((width, height)) = cell_size.filter(|(width, height)| *width > 0 && *height > 0)
        else {
            return Self::HalfBlock;
        };

        let picker_protocol = match protocol {
            DisplayProtocol::Kitty => ratatui_image::picker::ProtocolType::Kitty,
            DisplayProtocol::Sixel => ratatui_image::picker::ProtocolType::Sixel,
            DisplayProtocol::Iterm2 => ratatui_image::picker::ProtocolType::Iterm2,
            DisplayProtocol::HalfBlock => ratatui_image::picker::ProtocolType::Halfblocks,
        };
        #[allow(deprecated)]
        let mut picker = ratatui_image::picker::Picker::from_fontsize(
            ratatui_image::FontSize::new(width, height),
        );
        picker.set_protocol_type(picker_protocol);
        Self::Pixel(PixelDisplay::new(picker))
    }

    pub fn framebuffer_size(&self, area: ratatui::layout::Rect) -> (usize, usize) {
        match self {
            Self::HalfBlock => (area.width as usize, area.height as usize * 2),
            Self::Pixel(pixel) => {
                let font = pixel.picker.font_size();
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
            Self::Pixel(pixel) => match pixel.picker.protocol_type() {
                ratatui_image::picker::ProtocolType::Kitty => DisplayProtocol::Kitty,
                ratatui_image::picker::ProtocolType::Sixel => DisplayProtocol::Sixel,
                ratatui_image::picker::ProtocolType::Iterm2 => DisplayProtocol::Iterm2,
                ratatui_image::picker::ProtocolType::Halfblocks => DisplayProtocol::HalfBlock,
            },
        }
    }

    pub fn uses_async_output(&self) -> bool {
        let remote =
            std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_TTY").is_some();
        let remote_graphics = std::env::var("CELLARIUM_REMOTE_GRAPHICS")
            .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
        should_use_async_output(self.protocol(), remote, remote_graphics)
    }

    pub fn render(
        &self,
        frame: &mut ratatui::Frame,
        area: ratatui::layout::Rect,
        framebuffer: &Framebuffer,
    ) {
        if let Self::Pixel(pixel) = self {
            let image = framebuffer_to_dynamic_image(framebuffer);
            let size = ratatui::layout::Size::new(area.width, area.height);
            pixel.submit(image, size);
            if pixel.render(frame, area) {
                return;
            }
        }

        frame.render_widget(
            ratatui::widgets::Paragraph::new(half_block::half_block_lines(framebuffer)),
            area,
        );
    }
}

pub fn should_use_async_output(
    protocol: DisplayProtocol,
    remote: bool,
    remote_graphics: bool,
) -> bool {
    remote && remote_graphics && protocol.is_pixel_protocol()
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

pub fn detect_protocol_for_connection(
    term: &str,
    term_program: &str,
    sixel: &str,
    remote_without_graphics: bool,
) -> DisplayProtocol {
    if remote_without_graphics {
        DisplayProtocol::HalfBlock
    } else {
        detect_protocol(term, term_program, sixel)
    }
}

fn cell_size_from_environment() -> Option<(u16, u16)> {
    if let (Some(width), Some(height)) = (
        env_cell_dimension("CELLARIUM_CELL_WIDTH"),
        env_cell_dimension("CELLARIUM_CELL_HEIGHT"),
    ) {
        return Some((width, height));
    }

    let size = crossterm::terminal::window_size().ok()?;
    let width = size.width.checked_div(size.columns)?;
    let height = size.height.checked_div(size.rows)?;
    (width > 0 && height > 0).then_some((width, height))
}

fn env_cell_dimension(name: &str) -> Option<u16> {
    std::env::var(name)
        .ok()?
        .parse::<u16>()
        .ok()
        .filter(|value| *value > 0)
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
    fn remote_kitty_connections_default_to_halfblocks_for_responsive_input() {
        assert_eq!(
            detect_protocol_for_connection("xterm-kitty", "kitty", "", true),
            DisplayProtocol::HalfBlock
        );
        assert_eq!(
            detect_protocol_for_connection("xterm-kitty", "kitty", "", false),
            DisplayProtocol::Kitty
        );
    }

    #[test]
    fn async_output_is_reserved_for_remote_graphics() {
        assert!(should_use_async_output(DisplayProtocol::Kitty, true, true));
        assert!(should_use_async_output(DisplayProtocol::Sixel, true, true));
        assert!(!should_use_async_output(
            DisplayProtocol::HalfBlock,
            true,
            true
        ));
        assert!(!should_use_async_output(
            DisplayProtocol::Kitty,
            true,
            false
        ));
        assert!(!should_use_async_output(
            DisplayProtocol::Kitty,
            false,
            true
        ));
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
        let pixel_display = ViewportDisplay::Pixel(PixelDisplay::new(picker));
        assert_eq!(pixel_display.framebuffer_size(area), (100, 100));
    }

    #[test]
    fn viewport_display_uses_pixel_protocols_without_querying_stdio() {
        let display =
            ViewportDisplay::from_protocol_and_cell_size(DisplayProtocol::Sixel, Some((10, 20)));
        assert_eq!(display.protocol(), DisplayProtocol::Sixel);

        let display =
            ViewportDisplay::from_protocol_and_cell_size(DisplayProtocol::HalfBlock, None);
        assert_eq!(display.protocol(), DisplayProtocol::HalfBlock);
    }

    #[test]
    fn pixel_protocols_require_real_cell_dimensions() {
        let display =
            ViewportDisplay::from_protocol_and_cell_size(DisplayProtocol::Kitty, Some((8, 16)));
        assert_eq!(display.protocol(), DisplayProtocol::Kitty);
        assert_eq!(
            display.framebuffer_size(ratatui::layout::Rect::new(0, 0, 10, 5)),
            (80, 80)
        );

        let display = ViewportDisplay::from_protocol_and_cell_size(DisplayProtocol::Kitty, None);
        assert_eq!(display.protocol(), DisplayProtocol::HalfBlock);
    }

    #[test]
    fn viewport_display_renders_pixel_protocols_into_the_frame() {
        let framebuffer = Framebuffer::new(10, 20);
        let area = ratatui::layout::Rect::new(0, 0, 1, 1);
        let display = ViewportDisplay::Pixel(PixelDisplay::new(
            ratatui_image::picker::Picker::halfblocks(),
        ));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(1, 1)).unwrap();

        terminal
            .draw(|frame| display.render(frame, area, &framebuffer))
            .unwrap();
        assert_eq!(display.protocol(), DisplayProtocol::HalfBlock);
    }
}
