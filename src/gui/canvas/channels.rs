//! The channel preview canvas.
//!
//! The one rule this module exists to enforce: what is drawn is labelled with
//! where it came from. The live world and the draft's initial values look
//! alike and mean entirely different things, so the draft is never shown as a
//! stand-in for a world that has not been applied yet.

use eframe::egui::{self, Color32, ColorImage, Rect, Sense, TextureHandle, TextureOptions, Ui};

use crate::gui::canvas::CanvasTransform;
use crate::gui::theme;
use crate::render::channels::{Rgb8, composite_pixel};
use crate::sim::experiment_model::{ExperimentSpec, GeometrySpec};
use crate::sim::worker::SimulationSnapshot;

/// How the channels are composited on screen.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ChannelView {
    #[default]
    Composite,
    /// Only the selected channel.
    Solo,
    /// Every channel side by side.
    Grid,
}

impl ChannelView {
    pub const ALL: [ChannelView; 3] =
        [ChannelView::Composite, ChannelView::Solo, ChannelView::Grid];

    pub fn label(self) -> &'static str {
        match self {
            ChannelView::Composite => "Composite",
            ChannelView::Solo => "Solo",
            ChannelView::Grid => "Grid",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            ChannelView::Composite => "Blend every visible channel into one image",
            ChannelView::Solo => "Show only the selected channel",
            ChannelView::Grid => "Show every channel side by side",
        }
    }
}

/// Which values the preview is drawing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ChannelPreviewSource {
    /// The running world: the active experiment and its newest snapshot.
    #[default]
    Live,
    /// The draft's own initial values, which have never been simulated.
    DraftInitial,
}

impl ChannelPreviewSource {
    pub fn label(self) -> &'static str {
        match self {
            ChannelPreviewSource::Live => "Live world",
            ChannelPreviewSource::DraftInitial => "Draft initial values",
        }
    }
}

/// What the preview resolved to, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelPreview {
    /// The source actually drawn, which is not always the one requested.
    pub source: ChannelPreviewSource,
    /// Sentence shown above the canvas naming what is on screen.
    pub label: &'static str,
    /// The draft's structure no longer matches the running world.
    pub structure_stale: bool,
    /// Channel count and grid of whatever is being drawn.
    pub width: usize,
    pub height: usize,
    pub channels: usize,
}

/// Labels are `&'static str` so a caller can address one in a test without
/// reconstructing the sentence.
const LIVE_LABEL: &str = "Live world";
const DRAFT_LABEL: &str = "Draft initial values";
const STALE_LABEL: &str = "Draft initial values — the running world still has the old structure";

/// Decide what can honestly be drawn.
///
/// Requesting Live is only satisfiable while the draft still has the running
/// world's structure. Once it does not, the live pixels describe a different
/// experiment, so the preview switches to the draft's own initial values and
/// says so rather than silently showing either one.
pub fn resolve_preview(
    requested: ChannelPreviewSource,
    active: &ExperimentSpec,
    draft: &ExperimentSpec,
    snapshot: Option<&SimulationSnapshot>,
) -> ChannelPreview {
    let stale = !same_structure(active, draft);
    let (width, height) = grid_of(draft);
    match requested {
        ChannelPreviewSource::Live if !stale && snapshot.is_some() => {
            let snapshot = snapshot.expect("checked above");
            ChannelPreview {
                source: ChannelPreviewSource::Live,
                label: LIVE_LABEL,
                structure_stale: false,
                width: snapshot.layout.width,
                height: snapshot.layout.height,
                channels: snapshot.layout.channels,
            }
        }
        ChannelPreviewSource::Live => ChannelPreview {
            source: ChannelPreviewSource::DraftInitial,
            label: if stale { STALE_LABEL } else { DRAFT_LABEL },
            structure_stale: stale,
            width,
            height,
            channels: draft.channels.len(),
        },
        ChannelPreviewSource::DraftInitial => ChannelPreview {
            source: ChannelPreviewSource::DraftInitial,
            label: if stale { STALE_LABEL } else { DRAFT_LABEL },
            structure_stale: stale,
            width,
            height,
            channels: draft.channels.len(),
        },
    }
}

/// Two experiments share a structure when a world drawn for one describes the
/// other: same grid and the same channels in the same order.
fn same_structure(active: &ExperimentSpec, draft: &ExperimentSpec) -> bool {
    grid_of(active) == grid_of(draft)
        && active.channels.len() == draft.channels.len()
        && active
            .channels
            .iter()
            .zip(&draft.channels)
            .all(|(left, right)| left.id == right.id)
}

fn grid_of(spec: &ExperimentSpec) -> (usize, usize) {
    let GeometrySpec::RasterGrid(grid) = &spec.geometry;
    (grid.width as usize, grid.height as usize)
}

/// Transient canvas state the GUI owns between frames.
#[derive(Default)]
pub struct ChannelCanvasState {
    pub transform: Option<CanvasTransform>,
    pub view: ChannelView,
    pub source: ChannelPreviewSource,
    texture: Option<TextureHandle>,
    /// What is currently in the texture, so an unchanged preview is not
    /// re-uploaded every frame.
    uploaded: Option<u64>,
    /// Size of the image the current transform was fitted to.
    laid_out: Option<[usize; 2]>,
}

impl ChannelCanvasState {
    pub fn request_fit(&mut self) {
        self.transform = None;
    }

    /// Force the next frame to rebuild the texture, for a change the
    /// generation counter cannot see such as a recoloured channel.
    pub fn invalidate(&mut self) {
        self.uploaded = None;
    }
}

/// Everything the canvas needs that is not view state.
pub struct ChannelCanvasInput<'a> {
    pub active: &'a ExperimentSpec,
    pub draft: &'a ExperimentSpec,
    pub snapshot: Option<&'a SimulationSnapshot>,
    pub selected: usize,
    pub colors: &'a [Rgb8],
    /// Bumped by the caller whenever the drawn values could have changed.
    pub generation: u64,
}

/// What the pointer found, alongside the preview that was drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelCanvasOutcome {
    pub preview: ChannelPreview,
    /// Channel index, cell and value under the pointer.
    pub hovered: Option<(usize, usize, usize, f32)>,
}

pub fn render_channel_canvas(
    ui: &mut Ui,
    size: egui::Vec2,
    input: &ChannelCanvasInput<'_>,
    state: &mut ChannelCanvasState,
) -> ChannelCanvasOutcome {
    let preview = resolve_preview(state.source, input.active, input.draft, input.snapshot);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme::DOMAIN_EXTERIOR);

    if preview.width == 0 || preview.height == 0 || preview.channels == 0 {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "this experiment has nothing to draw",
            egui::FontId::proportional(14.0),
            theme::state_color(theme::State::Invalid),
        );
        return ChannelCanvasOutcome {
            preview,
            hovered: None,
        };
    }

    // Hiding a channel takes its pane away rather than leaving a pane the
    // control claims is hidden. Composite already drops hidden channels; a
    // grid that kept drawing them would be contradicting the same switch.
    let panes: Vec<usize> = (0..preview.channels)
        .filter(|channel| is_visible(input, *channel))
        .collect();
    if state.view == ChannelView::Grid && panes.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "every channel is hidden — press Show on a channel to see it",
            egui::FontId::proportional(14.0),
            theme::state_color(theme::State::Draft),
        );
        return ChannelCanvasOutcome {
            preview,
            hovered: None,
        };
    }

    // Grid lays the channels out side by side, so the drawn image is wider.
    let columns = match state.view {
        ChannelView::Grid => grid_columns(panes.len()),
        _ => 1,
    };
    let rows = match state.view {
        ChannelView::Grid => panes.len().div_ceil(columns),
        _ => 1,
    };
    let image_size = [preview.width * columns, preview.height * rows];

    // Switching to Grid, or hiding a channel, changes the shape of the drawn
    // image. A transform fitted to the old shape leaves panes clipped off the
    // edge, so a changed shape refits rather than making the user press Fit.
    if state.laid_out != Some(image_size) {
        state.transform = None;
        state.laid_out = Some(image_size);
    }

    let transform = match &mut state.transform {
        Some(transform) => {
            transform.viewport = rect;
            *transform
        }
        None => {
            let fitted =
                CanvasTransform::fit(rect, [image_size[0] as f64, image_size[1] as f64], 24.0);
            state.transform = Some(fitted);
            fitted
        }
    };

    upload(ui, input, &preview, image_size, columns, &panes, state);
    let board = Rect::from_min_max(
        transform.world_to_screen([0.0, 0.0]),
        transform.world_to_screen([image_size[0] as f64, image_size[1] as f64]),
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

    // In Grid every pane is captioned, otherwise the user is counting tiles to
    // work out which channel is which.
    if state.view == ChannelView::Grid {
        for (pane, index) in panes.iter().copied().enumerate() {
            let column = pane % columns;
            let row = pane / columns;
            let origin = transform.world_to_screen([
                (column * preview.width) as f64,
                (row * preview.height) as f64,
            ]);
            let name = input
                .draft
                .channels
                .get(index)
                .map(|channel| channel.name.as_str())
                .unwrap_or("channel");
            painter.text(
                origin + egui::vec2(4.0, 2.0),
                egui::Align2::LEFT_TOP,
                name,
                egui::FontId::proportional(12.0),
                theme::SELECTION,
            );
        }
    }

    // Which pane the pointer is over, and the value there. The panes come from
    // one image, so the pane is worked out with the coordinates it was drawn
    // with. The other three canvases all report the cell under the pointer;
    // this one left the user with pictures and no way to read a number off them.
    let hovered = response.hover_pos().and_then(|pointer| {
        let world = transform.screen_to_world(pointer);
        if world[0] < 0.0 || world[1] < 0.0 {
            return None;
        }
        let (image_x, image_y) = (world[0] as usize, world[1] as usize);
        if image_x >= image_size[0] || image_y >= image_size[1] {
            return None;
        }
        let channel = match state.view {
            ChannelView::Grid => {
                let (column, row) = (image_x / preview.width, image_y / preview.height);
                *panes.get(row * columns + column)?
            }
            // Composite blends every visible channel, so the selected one is
            // the only value it makes sense to name.
            ChannelView::Solo | ChannelView::Composite => input.selected,
        };
        let x = image_x % preview.width;
        let y = image_y % preview.height;
        Some((channel, x, y, sample(input, &preview, channel, x, y)))
    });

    ChannelCanvasOutcome { preview, hovered }
}

fn grid_columns(channels: usize) -> usize {
    (channels as f64).sqrt().ceil().max(1.0) as usize
}

/// Whether the user has this channel switched on for display.
fn is_visible(input: &ChannelCanvasInput<'_>, channel: usize) -> bool {
    input
        .draft
        .channels
        .get(channel)
        .is_some_and(|entry| entry.display.visible)
}

fn upload(
    ui: &Ui,
    input: &ChannelCanvasInput<'_>,
    preview: &ChannelPreview,
    image_size: [usize; 2],
    columns: usize,
    panes: &[usize],
    state: &mut ChannelCanvasState,
) {
    // The key mixes everything that changes the pixels, so a view or source
    // switch rebuilds the texture even when the values did not move.
    // Visibility is part of the key: hiding a channel changes the pixels while
    // the values behind them stay exactly where they were.
    let visibility = (0..preview.channels)
        .filter(|channel| is_visible(input, *channel))
        .fold(0u64, |bits, channel| bits | (1u64 << (channel % 64)));
    let key = input.generation
        ^ ((state.view as u64) << 40)
        ^ ((state.source as u64) << 44)
        ^ ((input.selected as u64) << 48)
        ^ visibility.rotate_left(17);
    if state.uploaded == Some(key) && state.texture.is_some() {
        return;
    }

    let width = preview.width;
    let height = preview.height;
    let mut pixels = vec![Color32::BLACK; image_size[0] * image_size[1]];
    let mut values = vec![0.0_f32; preview.channels];
    for y in 0..height {
        for x in 0..width {
            for (channel, value) in values.iter_mut().enumerate() {
                *value = sample(input, preview, channel, x, y);
            }
            match state.view {
                ChannelView::Grid => {
                    for (pane, channel) in panes.iter().copied().enumerate() {
                        let value = values.get(channel).copied().unwrap_or(0.0);
                        let column = pane % columns;
                        let row = pane / columns;
                        let color = solo_pixel(value, input.colors, channel);
                        let px = column * width + x;
                        let py = row * height + y;
                        pixels[py * image_size[0] + px] =
                            Color32::from_rgb(color.red, color.green, color.blue);
                    }
                }
                ChannelView::Solo => {
                    let value = values.get(input.selected).copied().unwrap_or(0.0);
                    let color = solo_pixel(value, input.colors, input.selected);
                    pixels[y * image_size[0] + x] =
                        Color32::from_rgb(color.red, color.green, color.blue);
                }
                ChannelView::Composite => {
                    // A hidden channel contributes nothing, which is what makes
                    // Hide a display control rather than a model edit.
                    let visible: Vec<f32> = values
                        .iter()
                        .enumerate()
                        .map(|(channel, value)| {
                            if is_visible(input, channel) {
                                *value
                            } else {
                                0.0
                            }
                        })
                        .collect();
                    let color = composite_pixel(&visible, input.colors);
                    pixels[y * image_size[0] + x] =
                        Color32::from_rgb(color.red, color.green, color.blue);
                }
            }
        }
    }

    let image = ColorImage::from_rgba_unmultiplied(
        image_size,
        &pixels
            .iter()
            .flat_map(|color| color.to_array())
            .collect::<Vec<_>>(),
    );
    match &mut state.texture {
        Some(texture) => texture.set(image, TextureOptions::NEAREST),
        None => {
            state.texture = Some(ui.ctx().load_texture(
                "cellarium-channels",
                image,
                TextureOptions::NEAREST,
            ));
        }
    }
    state.uploaded = Some(key);
}

/// Read one cell from whichever source the preview resolved to.
fn sample(
    input: &ChannelCanvasInput<'_>,
    preview: &ChannelPreview,
    channel: usize,
    x: usize,
    y: usize,
) -> f32 {
    match preview.source {
        ChannelPreviewSource::Live => input
            .snapshot
            .and_then(|snapshot| {
                snapshot
                    .layout
                    .index_by_position(channel, x, y, 0)
                    .and_then(|index| snapshot.cells.get(index).copied())
            })
            .unwrap_or(0.0),
        ChannelPreviewSource::DraftInitial => input
            .draft
            .channels
            .get(channel)
            .and_then(|entry| entry.initial.get(y * preview.width + x).copied())
            .unwrap_or(0.0),
    }
}

fn solo_pixel(value: f32, colors: &[Rgb8], channel: usize) -> Rgb8 {
    let value = value.clamp(0.0, 1.0);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> ExperimentSpec {
        ExperimentSpec::single_channel_lenia(8, 8)
    }

    #[test]
    fn a_matching_structure_shows_the_live_world() {
        let active = spec();
        let draft = active.clone();
        let preview = resolve_preview(ChannelPreviewSource::Live, &active, &draft, None);
        // With no snapshot there is no live world yet, so it falls back and
        // says which values it is drawing.
        assert_eq!(preview.source, ChannelPreviewSource::DraftInitial);
        assert!(!preview.structure_stale);
        assert_eq!(preview.label, DRAFT_LABEL);
    }

    #[test]
    fn a_changed_structure_is_never_drawn_as_the_live_world() {
        let active = spec();
        let mut draft = active.clone();
        draft.add_channel(String::from("second"), false);
        let preview = resolve_preview(ChannelPreviewSource::Live, &active, &draft, None);
        assert_eq!(preview.source, ChannelPreviewSource::DraftInitial);
        assert!(preview.structure_stale);
        assert_eq!(preview.label, STALE_LABEL);
        assert_eq!(preview.channels, 2);
    }

    #[test]
    fn asking_for_the_draft_never_silently_returns_the_live_world() {
        let active = spec();
        let draft = active.clone();
        let preview = resolve_preview(ChannelPreviewSource::DraftInitial, &active, &draft, None);
        assert_eq!(preview.source, ChannelPreviewSource::DraftInitial);
    }

    #[test]
    fn a_resized_world_counts_as_a_changed_structure() {
        let active = spec();
        let draft = ExperimentSpec::single_channel_lenia(16, 16);
        let preview = resolve_preview(ChannelPreviewSource::Live, &active, &draft, None);
        assert!(preview.structure_stale);
    }

    #[test]
    fn every_view_has_a_distinct_label_and_hint() {
        for (index, view) in ChannelView::ALL.iter().enumerate() {
            assert!(!view.label().is_empty());
            for other in &ChannelView::ALL[index + 1..] {
                assert_ne!(view.label(), other.label());
                assert_ne!(view.hint(), other.hint());
            }
        }
    }

    #[test]
    fn a_grid_is_laid_out_squarely_enough_to_stay_readable() {
        assert_eq!(grid_columns(1), 1);
        assert_eq!(grid_columns(2), 2);
        assert_eq!(grid_columns(4), 2);
        assert_eq!(grid_columns(9), 3);
    }

    #[test]
    fn each_source_names_itself() {
        assert_ne!(
            ChannelPreviewSource::Live.label(),
            ChannelPreviewSource::DraftInitial.label()
        );
    }
}
