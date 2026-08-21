pub mod half_block;

use image::{DynamicImage, ImageBuffer, Rgba};
#[cfg(unix)]
use std::collections::VecDeque;
#[cfg(unix)]
use std::ffi::CString;
#[cfg(unix)]
use std::fs::File;
#[cfg(all(unix, test))]
use std::io::Read;
#[cfg(unix)]
use std::io::Write;
#[cfg(unix)]
use std::os::fd::FromRawFd;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
#[cfg(unix)]
use std::time::{Duration, Instant};

use crate::render::raster::Framebuffer;
use crate::render::{camera::Camera, raster::rasterize_world_into_while};
use crate::sim::world::World;

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
    ready_sequence: Arc<AtomicU64>,
    displayed_sequence: AtomicU64,
    queue: LatestWorkQueue<(DynamicImage, ratatui::layout::Size)>,
    worker: Option<JoinHandle<()>>,
}

#[derive(Clone, Copy)]
struct RenderStatus {
    rendered: bool,
    fresh: bool,
}

struct LatestWorkState<T> {
    value: Option<T>,
    closed: bool,
}

struct LatestWorkQueue<T> {
    state: Arc<(Mutex<LatestWorkState<T>>, Condvar)>,
}

struct RasterRequest {
    generation: u64,
    world_width: usize,
    world_height: usize,
    cells: Vec<f32>,
    camera: Camera,
    frame_width: usize,
    frame_height: usize,
    terminal_size: ratatui::layout::Size,
}

pub struct AsyncRasterizer {
    queue: LatestWorkQueue<RasterRequest>,
    ready: Arc<Mutex<Option<(DynamicImage, ratatui::layout::Size)>>>,
    latest_generation: Arc<AtomicU64>,
    worker: Option<JoinHandle<()>>,
}

impl AsyncRasterizer {
    pub fn new() -> Self {
        let queue: LatestWorkQueue<RasterRequest> = LatestWorkQueue::new();
        let worker_queue = queue.clone();
        let ready = Arc::new(Mutex::new(None));
        let worker_ready = Arc::clone(&ready);
        let latest_generation = Arc::new(AtomicU64::new(0));
        let worker_generation = Arc::clone(&latest_generation);
        let worker = std::thread::spawn(move || {
            while let Some(request) = worker_queue.recv() {
                let mut world = World::new(request.world_width, request.world_height);
                world.replace_cells(&request.cells);
                let mut framebuffer = Framebuffer::new(request.frame_width, request.frame_height);
                let completed =
                    rasterize_world_into_while(&world, &request.camera, &mut framebuffer, || {
                        worker_generation.load(Ordering::Acquire) == request.generation
                    });
                if !completed || worker_generation.load(Ordering::Acquire) != request.generation {
                    continue;
                }
                let image = framebuffer_to_dynamic_image(&framebuffer);
                if worker_generation.load(Ordering::Acquire) != request.generation {
                    continue;
                }
                if let Ok(mut slot) = worker_ready.lock() {
                    *slot = Some((image, request.terminal_size));
                }
            }
        });
        Self {
            queue,
            ready,
            latest_generation,
            worker: Some(worker),
        }
    }

    pub fn submit(
        &self,
        world: &World,
        camera: Camera,
        frame_width: usize,
        frame_height: usize,
        terminal_size: ratatui::layout::Size,
        priority_generation: u64,
    ) {
        self.latest_generation
            .store(priority_generation, Ordering::Release);
        self.queue.submit(RasterRequest {
            generation: priority_generation,
            world_width: world.width(),
            world_height: world.height(),
            cells: world.cells().to_vec(),
            camera,
            frame_width,
            frame_height,
            terminal_size,
        });
    }

    fn take_ready(&self) -> Option<(DynamicImage, ratatui::layout::Size)> {
        self.ready.lock().ok()?.take()
    }
}

impl Default for AsyncRasterizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AsyncRasterizer {
    fn drop(&mut self) {
        self.queue.close();
        // Large raster work is allowed to finish in the detached worker; exit
        // must not block behind a frame that the terminal will never display.
        let _ = self.worker.take();
    }
}

impl<T> Clone for LatestWorkQueue<T> {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
        }
    }
}

impl<T> LatestWorkQueue<T> {
    fn new() -> Self {
        Self {
            state: Arc::new((
                Mutex::new(LatestWorkState {
                    value: None,
                    closed: false,
                }),
                Condvar::new(),
            )),
        }
    }

    fn submit(&self, value: T) {
        let (lock, wake) = &*self.state;
        if let Ok(mut state) = lock.lock()
            && !state.closed
        {
            state.value = Some(value);
            wake.notify_one();
        }
    }

    fn recv(&self) -> Option<T> {
        let (lock, wake) = &*self.state;
        let mut state = lock.lock().ok()?;
        while state.value.is_none() && !state.closed {
            state = wake.wait(state).ok()?;
        }
        state.value.take()
    }

    fn close(&self) {
        let (lock, wake) = &*self.state;
        if let Ok(mut state) = lock.lock() {
            state.closed = true;
            state.value = None;
            wake.notify_all();
        }
    }
}

#[cfg(unix)]
struct KittySharedState {
    ready: Option<KittySharedFrame>,
    displayed_id: Option<u32>,
    retained: VecDeque<KittySharedFrame>,
    failed: bool,
    next_image_id: u32,
}

#[cfg(unix)]
pub struct KittySharedDisplay {
    font_size: (u16, u16),
    state: Arc<Mutex<KittySharedState>>,
    queue: LatestWorkQueue<(DynamicImage, ratatui::layout::Size)>,
    worker: Option<JoinHandle<()>>,
    fallback: PixelDisplay,
}

#[cfg(unix)]
impl KittySharedDisplay {
    fn new(font_size: (u16, u16)) -> Self {
        let queue: LatestWorkQueue<(DynamicImage, ratatui::layout::Size)> = LatestWorkQueue::new();
        let state = Arc::new(Mutex::new(KittySharedState {
            ready: None,
            displayed_id: None,
            retained: VecDeque::new(),
            failed: false,
            next_image_id: rand::random::<u32>().max(1),
        }));
        let worker_state = Arc::clone(&state);
        let worker_queue = queue.clone();
        let worker = std::thread::spawn(move || {
            while let Some((image, area)) = worker_queue.recv() {
                let image_id = match worker_state.lock() {
                    Ok(mut state) => {
                        let image_id = state.next_image_id;
                        state.next_image_id = state.next_image_id.wrapping_add(1).max(1);
                        image_id
                    }
                    Err(_) => break,
                };
                let rgba = image.into_rgba8();
                let frame = KittySharedFrame::new(
                    rgba.as_raw(),
                    rgba.width(),
                    rgba.height(),
                    area.width,
                    area.height,
                    image_id,
                );
                let Ok(frame) = frame else {
                    if let Ok(mut state) = worker_state.lock() {
                        state.failed = true;
                    }
                    break;
                };
                if let Ok(mut state) = worker_state.lock() {
                    state.ready = Some(frame);
                }
            }
        });
        Self {
            font_size,
            state,
            queue,
            worker: Some(worker),
            fallback: kitty_pixel_display(font_size),
        }
    }

    fn submit(&self, image: DynamicImage, size: ratatui::layout::Size) {
        if self.state.lock().map_or(true, |state| state.failed) {
            self.fallback.submit(image, size);
            return;
        }
        self.queue.submit((image, size));
    }

    fn render(&self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) -> RenderStatus {
        let Ok(mut state) = self.state.lock() else {
            return self.fallback.render(frame, area);
        };
        state
            .retained
            .retain(|shared| !shared.was_consumed_by_terminal());
        if state
            .retained
            .front()
            .is_some_and(|shared| shared.created_at.elapsed() >= Duration::from_secs(2))
        {
            state.failed = true;
            state.ready = None;
            state.retained.clear();
        }
        if state.failed {
            if let Some(image_id) = state.displayed_id.take() {
                let command = kitty_delete_image_command(image_id);
                drop(state);
                frame.render_widget(
                    KittySharedWidget {
                        command: Some(&command),
                    },
                    area,
                );
                return RenderStatus {
                    rendered: true,
                    fresh: false,
                };
            }
            drop(state);
            return self.fallback.render(frame, area);
        }
        let Some(shared) = state.ready.take() else {
            frame.render_widget(KittySharedWidget { command: None }, area);
            return RenderStatus {
                rendered: state.displayed_id.is_some(),
                fresh: false,
            };
        };
        let image_id = shared.image_id;
        let mut command = shared.command.clone();
        if let Some(previous_id) = state.displayed_id {
            command.push_str(&kitty_delete_image_command(previous_id));
        }
        command.push_str("\x1b[u");
        frame.render_widget(
            KittySharedWidget {
                command: Some(&command),
            },
            area,
        );
        state.displayed_id = Some(image_id);
        state.retained.push_back(shared);
        RenderStatus {
            rendered: true,
            fresh: true,
        }
    }
}

#[cfg(unix)]
impl Drop for KittySharedDisplay {
    fn drop(&mut self) {
        self.queue.close();
        // A high-resolution write may still be in progress. Detach rather than
        // making q/exit wait behind a frame that will never be displayed.
        let _ = self.worker.take();
    }
}

#[cfg(unix)]
struct KittySharedWidget<'a> {
    command: Option<&'a str>,
}

#[cfg(unix)]
impl ratatui::widgets::Widget for KittySharedWidget<'_> {
    fn render(self, area: ratatui::layout::Rect, buffer: &mut ratatui::buffer::Buffer) {
        use ratatui::buffer::CellDiffOption;
        use std::num::NonZeroU16;

        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if let Some(cell) = buffer.cell_mut((x, y)) {
                    cell.set_diff_option(CellDiffOption::Skip);
                }
            }
        }
        if let (Some(command), Some(cell)) = (self.command, buffer.cell_mut(area.as_position())) {
            cell.set_symbol(command)
                .set_diff_option(CellDiffOption::ForcedWidth(
                    NonZeroU16::new(1).expect("one is non-zero"),
                ));
        }
    }
}

impl PixelDisplay {
    fn new(picker: ratatui_image::picker::Picker) -> Self {
        let queue: LatestWorkQueue<(DynamicImage, ratatui::layout::Size)> = LatestWorkQueue::new();
        let protocol = Arc::new(Mutex::new(None));
        let ready_sequence = Arc::new(AtomicU64::new(0));
        let worker_protocol = Arc::clone(&protocol);
        let worker_ready_sequence = Arc::clone(&ready_sequence);
        let worker_picker = picker.clone();
        let worker_queue = queue.clone();
        let worker = std::thread::spawn(move || {
            while let Some((image, size)) = worker_queue.recv() {
                if let Ok(encoded) =
                    worker_picker.new_protocol(image, size, ratatui_image::Resize::Fit(None))
                    && let Ok(mut slot) = worker_protocol.lock()
                {
                    *slot = Some(encoded);
                    worker_ready_sequence.fetch_add(1, Ordering::Release);
                }
            }
        });
        Self {
            picker,
            protocol,
            ready_sequence,
            displayed_sequence: AtomicU64::new(0),
            queue,
            worker: Some(worker),
        }
    }

    fn submit(&self, image: DynamicImage, size: ratatui::layout::Size) {
        self.queue.submit((image, size));
    }

    fn render(&self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) -> RenderStatus {
        let Ok(protocol) = self.protocol.lock() else {
            return RenderStatus {
                rendered: false,
                fresh: false,
            };
        };
        let Some(protocol) = protocol.as_ref() else {
            return RenderStatus {
                rendered: false,
                fresh: false,
            };
        };
        frame.render_widget(
            ratatui_image::Image::new(protocol).allow_clipping(true),
            area,
        );
        let ready = self.ready_sequence.load(Ordering::Acquire);
        let displayed = self.displayed_sequence.swap(ready, Ordering::AcqRel);
        RenderStatus {
            rendered: true,
            fresh: ready != displayed,
        }
    }
}

#[cfg(unix)]
fn kitty_pixel_display(font_size: (u16, u16)) -> PixelDisplay {
    #[allow(deprecated)]
    let mut picker = ratatui_image::picker::Picker::from_fontsize(ratatui_image::FontSize::new(
        font_size.0,
        font_size.1,
    ));
    picker.set_protocol_type(ratatui_image::picker::ProtocolType::Kitty);
    PixelDisplay::new(picker)
}

#[cfg(unix)]
fn kitty_delete_image_command(image_id: u32) -> String {
    format!("\x1b_Ga=d,d=I,i={image_id},q=1\x1b\\")
}

impl Drop for PixelDisplay {
    fn drop(&mut self) {
        self.queue.close();
        // Encoding a high-resolution frame can outlive the terminal session.
        // Joining here would make shutdown wait behind an obsolete frame and
        // prevent the quit key from returning control to the shell.
        let _ = self.worker.take();
    }
}

pub enum ViewportDisplay {
    HalfBlock,
    Pixel(PixelDisplay),
    #[cfg(unix)]
    KittyShared(KittySharedDisplay),
}

#[cfg(unix)]
struct KittySharedFrame {
    command: String,
    name: CString,
    image_id: u32,
    created_at: Instant,
}

#[cfg(unix)]
impl KittySharedFrame {
    fn new(
        rgba: &[u8],
        width: u32,
        height: u32,
        columns: u16,
        rows: u16,
        image_id: u32,
    ) -> std::io::Result<Self> {
        use base64::Engine;

        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Kitty RGBA dimensions overflow",
                )
            })?;
        if rgba.len() != expected {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Kitty RGBA dimensions do not match the pixel data",
            ));
        }

        let name = create_shared_memory(rgba)?;
        let encoded_name = base64::engine::general_purpose::STANDARD.encode(name.as_bytes());
        let command = format!(
            "\x1b[s\x1b_Ga=T,f=32,t=s,s={width},v={height},S={},i={image_id},p=1,c={columns},r={rows},q=1;{encoded_name}\x1b\\",
            rgba.len()
        );
        Ok(Self {
            command,
            name,
            image_id,
            created_at: Instant::now(),
        })
    }

    fn was_consumed_by_terminal(&self) -> bool {
        let descriptor = unsafe { libc::shm_open(self.name.as_ptr(), libc::O_RDONLY, 0) };
        if descriptor >= 0 {
            unsafe {
                libc::close(descriptor);
            }
            false
        } else {
            std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound
        }
    }

    #[cfg(test)]
    fn read_pixels_for_test(&self) -> std::io::Result<Vec<u8>> {
        let descriptor = unsafe { libc::shm_open(self.name.as_ptr(), libc::O_RDONLY, 0) };
        if descriptor < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut file = unsafe { File::from_raw_fd(descriptor) };
        let mut pixels = Vec::new();
        file.read_to_end(&mut pixels)?;
        Ok(pixels)
    }
}

#[cfg(unix)]
impl Drop for KittySharedFrame {
    fn drop(&mut self) {
        unsafe {
            libc::shm_unlink(self.name.as_ptr());
        }
    }
}

#[cfg(unix)]
fn create_shared_memory(bytes: &[u8]) -> std::io::Result<CString> {
    for _ in 0..16 {
        let name = CString::new(format!(
            "/clrm-{:x}-{:x}",
            std::process::id(),
            rand::random::<u32>()
        ))
        .expect("shared memory name contains no NUL bytes");
        let descriptor = unsafe {
            libc::shm_open(
                name.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                (libc::S_IRUSR | libc::S_IWUSR) as libc::c_uint,
            )
        };
        if descriptor < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                continue;
            }
            return Err(error);
        }

        let mut file = unsafe { File::from_raw_fd(descriptor) };
        if let Err(error) = (|| {
            let length = i64::try_from(bytes.len()).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Kitty frame is too large for shared memory",
                )
            })?;
            if unsafe { libc::ftruncate(descriptor, length) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
            file.write_all(bytes)
        })() {
            unsafe {
                libc::shm_unlink(name.as_ptr());
            }
            return Err(error);
        }
        return Ok(name);
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate a unique Kitty shared memory object",
    ))
}

pub fn should_use_kitty_shared_memory(
    protocol: DisplayProtocol,
    remote: bool,
    native_kitty: bool,
) -> bool {
    cfg!(unix) && protocol == DisplayProtocol::Kitty && !remote && native_kitty
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
        let native_kitty = std::env::var_os("KITTY_WINDOW_ID").is_some()
            || term_program.eq_ignore_ascii_case("kitty");
        let protocol = detect_protocol_for_connection(
            &term,
            &term_program,
            &sixel,
            remote && !remote_graphics,
        );
        Self::from_protocol_and_cell_size_for_connection(
            protocol,
            cell_size_from_environment(),
            remote,
            native_kitty,
        )
    }

    pub fn from_protocol_and_cell_size(
        protocol: DisplayProtocol,
        cell_size: Option<(u16, u16)>,
    ) -> Self {
        Self::from_protocol_and_cell_size_for_connection(protocol, cell_size, false, true)
    }

    fn from_protocol_and_cell_size_for_connection(
        protocol: DisplayProtocol,
        cell_size: Option<(u16, u16)>,
        remote: bool,
        native_kitty: bool,
    ) -> Self {
        #[cfg(not(unix))]
        let _ = (remote, native_kitty);

        if protocol == DisplayProtocol::HalfBlock {
            return Self::HalfBlock;
        }

        let Some((width, height)) = cell_size.filter(|(width, height)| *width > 0 && *height > 0)
        else {
            return Self::HalfBlock;
        };

        #[cfg(unix)]
        if should_use_kitty_shared_memory(protocol, remote, native_kitty) {
            return Self::KittyShared(KittySharedDisplay::new((width, height)));
        }

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
            #[cfg(unix)]
            Self::KittyShared(display) => (
                area.width as usize * display.font_size.0 as usize,
                area.height as usize * display.font_size.1 as usize,
            ),
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
            #[cfg(unix)]
            Self::KittyShared(_) => DisplayProtocol::Kitty,
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
    ) -> bool {
        if let Self::Pixel(pixel) = self {
            let image = framebuffer_to_dynamic_image(framebuffer);
            let size = ratatui::layout::Size::new(area.width, area.height);
            pixel.submit(image, size);
            let status = pixel.render(frame, area);
            if status.rendered {
                return status.fresh;
            }
        }

        #[cfg(unix)]
        if let Self::KittyShared(display) = self {
            let image = framebuffer_to_dynamic_image(framebuffer);
            let size = ratatui::layout::Size::new(area.width, area.height);
            display.submit(image, size);
            let status = display.render(frame, area);
            if status.rendered {
                return status.fresh;
            }
        }

        frame.render_widget(
            ratatui::widgets::Paragraph::new(half_block::half_block_lines(framebuffer)),
            area,
        );
        true
    }

    pub fn render_async(
        &self,
        frame: &mut ratatui::Frame,
        area: ratatui::layout::Rect,
        world: &World,
        camera: Camera,
        rasterizer: &AsyncRasterizer,
        priority_generation: u64,
    ) -> bool {
        let (frame_width, frame_height) = self.framebuffer_size(area);
        let terminal_size = ratatui::layout::Size::new(area.width, area.height);
        rasterizer.submit(
            world,
            camera,
            frame_width,
            frame_height,
            terminal_size,
            priority_generation,
        );
        if let Some((image, size)) = rasterizer.take_ready() {
            match self {
                Self::Pixel(display) => display.submit(image, size),
                #[cfg(unix)]
                Self::KittyShared(display) => display.submit(image, size),
                Self::HalfBlock => return false,
            }
        }
        match self {
            Self::Pixel(display) => display.render(frame, area).fresh,
            #[cfg(unix)]
            Self::KittyShared(display) => display.render(frame, area).fresh,
            Self::HalfBlock => false,
        }
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
    use std::time::{Duration, Instant};

    #[test]
    fn latest_image_queue_replaces_pending_frames_instead_of_preserving_stale_work() {
        let queue = LatestWorkQueue::new();
        queue.submit(1_u8);
        queue.submit(2_u8);

        assert_eq!(queue.recv(), Some(2));
        queue.close();
        assert_eq!(queue.recv(), None);
    }

    #[test]
    fn async_rasterizer_produces_a_pixel_frame_without_blocking_the_caller() {
        let mut world = World::new(2, 2);
        world.replace_cells(&[0.0, 1.0, 0.0, 1.0]);
        let rasterizer = AsyncRasterizer::new();
        rasterizer.submit(
            &world,
            Camera::new([1.0, 1.0], 1.0),
            4,
            4,
            ratatui::layout::Size::new(2, 2),
            0,
        );

        let deadline = Instant::now() + Duration::from_secs(1);
        let (image, size) = loop {
            if let Some(ready) = rasterizer.take_ready() {
                break ready;
            }
            assert!(Instant::now() < deadline, "raster worker did not publish");
            std::thread::sleep(Duration::from_millis(1));
        };

        assert_eq!((image.width(), image.height()), (4, 4));
        assert_eq!(size, ratatui::layout::Size::new(2, 2));
    }

    #[cfg(unix)]
    #[test]
    fn shared_display_marks_reused_placement_as_not_fresh() {
        let display = KittySharedDisplay::new((8, 16));
        display.state.lock().unwrap().displayed_id = Some(41);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(1, 1)).unwrap();
        let mut fresh = true;

        terminal
            .draw(|frame| {
                fresh = display
                    .render(frame, ratatui::layout::Rect::new(0, 0, 1, 1))
                    .fresh;
            })
            .unwrap();

        assert!(!fresh);
    }

    #[cfg(unix)]
    #[test]
    fn shared_display_deletes_the_image_displayed_at_presentation_time() {
        let display = KittySharedDisplay::new((8, 16));
        let pixels = [1_u8, 2, 3, 255];
        let ready = KittySharedFrame::new(&pixels, 1, 1, 1, 1, 41).unwrap();
        {
            let mut state = display.state.lock().unwrap();
            state.displayed_id = Some(99);
            state.ready = Some(ready);
        }
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(1, 1)).unwrap();

        terminal
            .draw(|frame| {
                display.render(frame, ratatui::layout::Rect::new(0, 0, 1, 1));
            })
            .unwrap();

        let symbol = terminal.backend().buffer().cell((0, 0)).unwrap().symbol();
        assert!(symbol.contains("a=T"));
        assert!(symbol.contains("i=41"));
        assert!(symbol.contains("a=d,d=I,i=99"));
        assert_eq!(display.state.lock().unwrap().displayed_id, Some(41));
    }

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

    #[cfg(unix)]
    #[test]
    fn local_kitty_frames_use_a_small_shared_memory_command() {
        let pixels = [1, 2, 3, 255, 4, 5, 6, 255];
        let frame = KittySharedFrame::new(&pixels, 2, 1, 12, 4, 41).unwrap();

        assert!(frame.command.contains("a=T"));
        assert!(frame.command.contains("t=s"));
        assert!(frame.command.contains("f=32"));
        assert!(frame.command.contains("s=2,v=1"));
        assert!(frame.command.contains("c=12,r=4"));
        assert!(frame.command.contains("i=41,p=1"));
        assert!(frame.command.contains("q=1"));
        assert!(!frame.command.contains("q=2"));
        assert!(!frame.command.contains("a=d,d=I"));
        assert!(frame.command.len() < 256);
        assert_eq!(frame.read_pixels_for_test().unwrap(), pixels);
    }

    #[test]
    fn shared_memory_is_only_selected_for_a_local_kitty_terminal() {
        assert_eq!(
            should_use_kitty_shared_memory(DisplayProtocol::Kitty, false, true),
            cfg!(unix)
        );
        assert!(!should_use_kitty_shared_memory(
            DisplayProtocol::Kitty,
            true,
            true
        ));
        assert!(!should_use_kitty_shared_memory(
            DisplayProtocol::Kitty,
            false,
            false
        ));
        assert!(!should_use_kitty_shared_memory(
            DisplayProtocol::Sixel,
            false,
            true
        ));
    }

    #[cfg(unix)]
    #[test]
    fn shared_memory_frame_is_reaped_only_after_terminal_unlinks_it() {
        let pixels = [1, 2, 3, 255];
        let frame = KittySharedFrame::new(&pixels, 1, 1, 1, 1, 41).unwrap();

        assert!(!frame.was_consumed_by_terminal());
        assert_eq!(unsafe { libc::shm_unlink(frame.name.as_ptr()) }, 0);
        assert!(frame.was_consumed_by_terminal());
    }

    #[cfg(unix)]
    #[test]
    fn shared_memory_failure_routes_future_frames_to_inline_kitty() {
        let display = KittySharedDisplay::new((8, 16));
        display.state.lock().unwrap().failed = true;
        display.submit(
            DynamicImage::new_rgba8(8, 16),
            ratatui::layout::Size::new(1, 1),
        );

        let deadline = Instant::now() + Duration::from_secs(1);
        while display.fallback.protocol.lock().unwrap().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(display.fallback.protocol.lock().unwrap().is_some());
    }

    #[cfg(unix)]
    #[test]
    fn shared_memory_fallback_deletes_the_last_real_kitty_placement() {
        let display = KittySharedDisplay::new((8, 16));
        {
            let mut state = display.state.lock().unwrap();
            state.failed = true;
            state.displayed_id = Some(41);
        }
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(1, 1)).unwrap();

        terminal
            .draw(|frame| {
                display.render(frame, ratatui::layout::Rect::new(0, 0, 1, 1));
            })
            .unwrap();

        let symbol = terminal.backend().buffer().cell((0, 0)).unwrap().symbol();
        assert!(symbol.contains("a=d,d=I,i=41,q=1"));
        assert_eq!(display.state.lock().unwrap().displayed_id, None);
    }

    #[cfg(unix)]
    #[test]
    fn non_native_kitty_compatibility_uses_inline_protocol() {
        let display = ViewportDisplay::from_protocol_and_cell_size_for_connection(
            DisplayProtocol::Kitty,
            Some((8, 16)),
            false,
            false,
        );
        assert!(matches!(display, ViewportDisplay::Pixel(_)));
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
            .draw(|frame| {
                display.render(frame, area, &framebuffer);
            })
            .unwrap();
        assert_eq!(display.protocol(), DisplayProtocol::HalfBlock);
    }

    #[cfg(unix)]
    #[test]
    fn local_kitty_viewport_emits_shared_memory_references() {
        let framebuffer = Framebuffer::new(8, 16);
        let area = ratatui::layout::Rect::new(0, 0, 1, 1);
        let display =
            ViewportDisplay::from_protocol_and_cell_size(DisplayProtocol::Kitty, Some((8, 16)));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(1, 1)).unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        let symbol = loop {
            terminal
                .draw(|frame| {
                    display.render(frame, area, &framebuffer);
                })
                .unwrap();
            let symbol = terminal
                .backend()
                .buffer()
                .cell((0, 0))
                .unwrap()
                .symbol()
                .to_string();
            if symbol.contains("t=s") || std::time::Instant::now() >= deadline {
                break symbol;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };

        assert!(
            symbol.contains("t=s"),
            "local Kitty output was not a shared-memory command: {symbol:?}"
        );
        assert!(symbol.len() < 256);
    }
}
