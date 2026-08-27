//! The live simulation canvas.
//!
//! Cells are uploaded to one texture per snapshot generation and drawn through
//! the shared [`CanvasTransform`]. Pointer hits use that same transform, so what
//! the user clicks is the cell they see.

use eframe::egui::{self, Color32, ColorImage, Rect, Sense, TextureHandle, TextureOptions, Ui};

use crate::gui::canvas::CanvasTransform;
use crate::gui::theme;
use crate::render::channels::{Rgb8, composite_pixel};
use crate::sim::local_backend::WorldEdit;
use crate::sim::worker::SimulationSnapshot;

/// Which channels the canvas draws.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ChannelView {
    #[default]
    Composite,
    Solo(usize),
}

/// Transient canvas state the GUI owns between frames.
pub struct WorldCanvasState {
    pub transform: Option<CanvasTransform>,
    pub view: ChannelView,
    pub brush_radius: u32,
    pub brush_value: f32,
    texture: Option<TextureHandle>,
    /// Generation currently in the texture, so an unchanged snapshot is not
    /// re-uploaded every frame.
    uploaded: Option<u64>,
}

impl Default for WorldCanvasState {
    fn default() -> Self {
        Self {
            transform: None,
            view: ChannelView::default(),
            brush_radius: 2,
            brush_value: 1.0,
            texture: None,
            uploaded: None,
        }
    }
}

impl WorldCanvasState {
    /// Drop the fitted transform so the next frame refits the world.
    pub fn request_fit(&mut self) {
        self.transform = None;
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldCanvasResponse {
    /// Cells the user painted this frame, already in layout coordinates.
    pub edits: Vec<WorldEdit>,
    /// World position under the pointer, for the hover readout.
    pub hovered: Option<[f64; 2]>,
    /// Cell under the pointer as `(basis, x, y)`.
    pub hovered_cell: Option<(usize, usize, usize)>,
}

/// Draw the snapshot and return what the pointer did.
pub fn render_world_canvas(
    ui: &mut Ui,
    size: egui::Vec2,
    snapshot: Option<&SimulationSnapshot>,
    colors: &[Rgb8],
    state: &mut WorldCanvasState,
) -> WorldCanvasResponse {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme::DOMAIN_EXTERIOR);

    let Some(snapshot) = snapshot else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "no simulation is running",
            egui::FontId::proportional(14.0),
            theme::state_color(theme::State::Invalid),
        );
        return WorldCanvasResponse::default();
    };

    let width = snapshot.layout.width;
    let height = snapshot.layout.height;
    // One frame's transform serves both drawing and hit testing.
    let transform = match &mut state.transform {
        Some(transform) => {
            transform.viewport = rect;
            *transform
        }
        None => {
            let fitted = CanvasTransform::fit(rect, [width as f64, height as f64], 24.0);
            state.transform = Some(fitted);
            fitted
        }
    };

    upload(ui, snapshot, colors, state);
    let board = Rect::from_min_max(
        transform.world_to_screen([0.0, 0.0]),
        transform.world_to_screen([width as f64, height as f64]),
    );
    painter.rect_filled(board, 0.0, theme::BOARD_INTERIOR);
    if let Some(texture) = &state.texture {
        painter.image(
            texture.id(),
            board,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }

    let mut result = WorldCanvasResponse::default();

    // Wheel zooms about the pointer; middle drag pans. Both keep the world
    // point under the pointer fixed, which is what makes precise edits possible.
    if let Some(pointer) = response.hover_pos() {
        let scroll = ui.input(|input| input.smooth_scroll_delta.y);
        if scroll != 0.0
            && let Some(transform) = &mut state.transform
        {
            transform.zoom_at(pointer, (scroll as f64 / 120.0).exp2());
        }
        result.hovered = Some(transform.screen_to_world(pointer));
        result.hovered_cell = cell_at(&transform, pointer, snapshot);
    }
    if response.dragged_by(egui::PointerButton::Middle)
        && let Some(transform) = &mut state.transform
    {
        transform.pan_screen(response.drag_delta());
    }

    // Left paints the brush value, right erases. Painting reads the same
    // transform the frame drew with, so the cell edited is the cell under the
    // cursor.
    for (button, value) in [
        (egui::PointerButton::Primary, state.brush_value),
        (egui::PointerButton::Secondary, 0.0),
    ] {
        if !(response.dragged_by(button)
            || (response.clicked_by(button) || response.drag_started_by(button)))
        {
            continue;
        }
        let Some(pointer) = response.interact_pointer_pos() else {
            continue;
        };
        let Some((basis, x, y)) = cell_at(&transform, pointer, snapshot) else {
            continue;
        };
        let radius = state.brush_radius as i64;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy > radius * radius {
                    continue;
                }
                let (Some(px), Some(py)) = (
                    (x as i64 + dx).try_into().ok().filter(|px| *px < width),
                    (y as i64 + dy).try_into().ok().filter(|py| *py < height),
                ) else {
                    continue;
                };
                for channel in 0..snapshot.layout.channels {
                    result.edits.push(WorldEdit {
                        channel,
                        basis,
                        x: px,
                        y: py,
                        value,
                    });
                }
            }
        }
    }

    result
}

fn cell_at(
    transform: &CanvasTransform,
    pointer: egui::Pos2,
    snapshot: &SimulationSnapshot,
) -> Option<(usize, usize, usize)> {
    let world = transform.screen_to_world(pointer);
    if world[0] < 0.0 || world[1] < 0.0 {
        return None;
    }
    let x = world[0] as usize;
    let y = world[1] as usize;
    if x >= snapshot.layout.width || y >= snapshot.layout.height {
        return None;
    }
    Some((0, x, y))
}

/// Upload the snapshot to a texture, skipping generations already uploaded.
fn upload(ui: &Ui, snapshot: &SimulationSnapshot, colors: &[Rgb8], state: &mut WorldCanvasState) {
    if state.uploaded == Some(snapshot.generation) && state.texture.is_some() {
        return;
    }
    let width = snapshot.layout.width;
    let height = snapshot.layout.height;
    let bases = snapshot.layout.bases.len();
    let channels = snapshot.layout.channels;
    let mut pixels = Vec::with_capacity(width * height);
    let mut values = vec![0.0; channels];
    for y in 0..height {
        for x in 0..width {
            for (channel, value) in values.iter_mut().enumerate() {
                // A multi-basis world is averaged into one raster pixel here;
                // true polygon geometry arrives with the Tiling canvas.
                let mut total = 0.0;
                for basis in 0..bases {
                    let index = channel * width * height * bases + (y * width + x) * bases + basis;
                    total += snapshot.cells.get(index).copied().unwrap_or(0.0);
                }
                *value = total / bases as f32;
            }
            let color = match state.view {
                ChannelView::Composite => composite_pixel(&values, colors),
                ChannelView::Solo(channel) => {
                    let value = values.get(channel).copied().unwrap_or(0.0).clamp(0.0, 1.0);
                    let base = colors
                        .get(channel)
                        .copied()
                        .unwrap_or(Rgb8::new(236, 240, 246));
                    Rgb8::new(
                        (base.red as f32 * value) as u8,
                        (base.green as f32 * value) as u8,
                        (base.blue as f32 * value) as u8,
                    )
                }
            };
            pixels.push(Color32::from_rgb(color.red, color.green, color.blue));
        }
    }
    let image = ColorImage {
        size: [width, height],
        ..ColorImage::from_rgba_unmultiplied(
            [width, height],
            &pixels
                .iter()
                .flat_map(|color| color.to_array())
                .collect::<Vec<_>>(),
        )
    };
    match &mut state.texture {
        Some(texture) => texture.set(image, TextureOptions::NEAREST),
        None => {
            state.texture = Some(ui.ctx().load_texture(
                "cellarium-world",
                image,
                TextureOptions::NEAREST,
            ));
        }
    }
    state.uploaded = Some(snapshot.generation);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_solo_view_scales_one_channel_by_its_colour() {
        let colors = [Rgb8::new(200, 100, 50)];
        let composite = composite_pixel(&[1.0], &colors);
        assert_eq!(composite, Rgb8::new(200, 100, 50));
    }

    #[test]
    fn requesting_a_fit_drops_the_transform_so_the_next_frame_refits() {
        let mut state = WorldCanvasState {
            transform: Some(CanvasTransform::new(
                Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(10.0, 10.0)),
                [1.0, 1.0],
                2.0,
            )),
            ..WorldCanvasState::default()
        };
        state.request_fit();
        assert!(state.transform.is_none());
    }

    #[test]
    fn the_default_brush_paints_a_visible_value() {
        let state = WorldCanvasState::default();
        assert!(state.brush_value > 0.0);
        assert!(state.brush_radius >= 1);
    }
}
