//! Shared Workbench graphics surface.
//!
//! Workbench editors (tiling, kernel, growth plots) rasterize themselves into
//! CPU-side RGBA frames through [`GraphicsScene`]. [`GraphicsSurface`] gates
//! presentation so preview work stays dirty-generation driven instead of
//! running at simulation frame rate, and one accepted frame feeds both the
//! Kitty pixel presenters and the half-block fallback.

/// A complete RGBA preview frame produced by a [`GraphicsScene`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphicsFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub generation: u64,
}

/// Reasons a [`GraphicsFrame`] cannot be used for presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicsFrameError {
    /// Zero-width or zero-height frames have no terminal representation.
    ZeroSized,
    /// The RGBA payload length does not match `width * height * 4`.
    RgbaLengthMismatch { expected: usize, actual: usize },
}

impl GraphicsFrame {
    /// Exact byte length of an RGBA payload covering `width * height`.
    pub fn pixel_len(width: u32, height: u32) -> Option<usize> {
        (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
    }

    pub fn new(
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        generation: u64,
    ) -> Result<Self, GraphicsFrameError> {
        if width == 0 || height == 0 {
            return Err(GraphicsFrameError::ZeroSized);
        }
        let expected = Self::pixel_len(width, height).ok_or(GraphicsFrameError::ZeroSized)?;
        if rgba.len() != expected {
            return Err(GraphicsFrameError::RgbaLengthMismatch {
                expected,
                actual: rgba.len(),
            });
        }
        Ok(Self {
            width,
            height,
            rgba,
            generation,
        })
    }
}

impl std::fmt::Display for GraphicsFrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroSized => write!(f, "graphics frame must have non-zero size"),
            Self::RgbaLengthMismatch { expected, actual } => write!(
                f,
                "graphics frame payload is {actual} bytes, expected {expected}"
            ),
        }
    }
}

impl std::error::Error for GraphicsFrameError {}

/// A scene that rasterizes itself into an RGBA frame of the requested size.
///
/// Implementations stay independent of terminal output; presenters translate
/// the returned frame into Kitty placements or half-block cells.
#[allow(dead_code)]
pub trait GraphicsScene {
    fn render_rgba(&self, width: u32, height: u32) -> GraphicsFrame;
}

/// Outcome of presenting a frame through a [`GraphicsSurface`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentResult {
    /// A new image reached the presenter for this generation.
    Fresh,
    /// Nothing was presented: the surface was clean or the frame generation
    /// did not advance past what was already presented.
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacementAction {
    Keep,
    Present,
    DeleteBeforePresent,
    DeleteOnly,
}

/// Whether the current Workbench section owns a raster placement.
///
/// Empty and Text are distinct UI states, but both must invalidate any
/// previously presented raster image.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScenePresence {
    Pixels,
    #[default]
    Empty,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneKey {
    pub section: crate::workbench::WorkbenchSection,
    pub selected_basis: crate::sim::tiling::BasisId,
    pub selected_channel: crate::sim::experiment_model::ChannelId,
    pub selected_kernel: Option<crate::sim::experiment_model::KernelId>,
    pub display_mode: crate::render::display::DisplayProtocol,
    pub placement_generation: u64,
    pub transform_generation: u64,
    pub draft_scene_generation: u64,
}

#[cfg(test)]
impl SceneKey {
    fn test(
        section: u8,
        placement_generation: u64,
        transform_generation: u64,
        draft_scene_generation: u64,
    ) -> Self {
        Self {
            section: crate::workbench::WorkbenchSection::ALL[section as usize],
            selected_basis: crate::sim::tiling::BasisId(0),
            selected_channel: crate::sim::experiment_model::ChannelId(0),
            selected_kernel: None,
            display_mode: crate::render::display::DisplayProtocol::Kitty,
            placement_generation,
            transform_generation,
            draft_scene_generation,
        }
    }
}

/// Dirty-generation gate between Workbench editor scenes and the display
/// presenters.
///
/// Scenes mark the surface dirty whenever their content changes; the draw
/// loop presents at most one frame per redraw and only while dirty. Older
/// generations never replace newer presented frames.
#[derive(Debug, Default)]
pub struct GraphicsSurface {
    dirty: bool,
    presented_generation: Option<u64>,
    max_dimensions: Option<(u32, u32)>,
    scene: Option<SceneKey>,
    presence: ScenePresence,
    fresh_presentations: u64,
}

impl GraphicsSurface {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_limits(max_width: u32, max_height: u32) -> Self {
        Self {
            dirty: false,
            presented_generation: None,
            max_dimensions: Some((max_width.max(1), max_height.max(1))),
            scene: None,
            presence: ScenePresence::Empty,
            fresh_presentations: 0,
        }
    }

    /// Schedule the next redraw to present a frame.
    pub fn mark_dirty(&mut self) {
        // Standalone surfaces used by non-Workbench callers predate explicit
        // scene keys; marking them dirty still means they own pixel output.
        self.presence = ScenePresence::Pixels;
        self.dirty = true;
    }

    /// Whether a pending frame should be presented on the next redraw.
    pub fn needs_present(&self) -> bool {
        self.dirty
    }

    /// Generation of the most recently presented frame, if any.
    pub fn presented_generation(&self) -> Option<u64> {
        self.presented_generation
    }

    pub fn transition(&mut self, scene: SceneKey) -> PlacementAction {
        self.transition_presence(ScenePresence::Pixels, Some(scene))
    }

    pub fn transition_presence(
        &mut self,
        presence: ScenePresence,
        scene: Option<SceneKey>,
    ) -> PlacementAction {
        debug_assert_eq!(presence == ScenePresence::Pixels, scene.is_some());
        if self.presence == presence && self.scene == scene {
            return PlacementAction::Keep;
        }
        let previous_presence = self.presence;
        let previous = self.scene;
        self.presence = presence;
        self.scene = scene;
        self.presented_generation = None;
        self.dirty = presence == ScenePresence::Pixels;
        if presence != ScenePresence::Pixels {
            return if previous_presence == ScenePresence::Pixels || previous.is_some() {
                PlacementAction::DeleteOnly
            } else {
                PlacementAction::Keep
            };
        }
        let scene = scene.expect("pixel presence requires a scene key");
        if previous_presence == ScenePresence::Pixels
            && previous.is_some_and(|previous| {
                previous.section != scene.section
                    || previous.selected_basis != scene.selected_basis
                    || previous.selected_channel != scene.selected_channel
                    || previous.selected_kernel != scene.selected_kernel
                    || previous.display_mode != scene.display_mode
                    || previous.placement_generation != scene.placement_generation
            })
        {
            PlacementAction::DeleteBeforePresent
        } else {
            PlacementAction::Present
        }
    }

    pub fn presence(&self) -> ScenePresence {
        self.presence
    }

    pub fn leave_scene(&mut self) -> PlacementAction {
        self.transition_presence(ScenePresence::Empty, None)
    }

    pub fn fresh_presentations(&self) -> u64 {
        self.fresh_presentations
    }

    /// Present `frame`, returning whether it became the fresh on-screen
    /// image. Presenting clears the dirty flag even when the generation is
    /// stale so a regressed caller cannot loop forever.
    pub fn present(&mut self, frame: GraphicsFrame) -> Result<PresentResult, GraphicsFrameError> {
        let expected = GraphicsFrame::pixel_len(frame.width, frame.height)
            .ok_or(GraphicsFrameError::ZeroSized)?;
        if frame.width == 0 || frame.height == 0 {
            return Err(GraphicsFrameError::ZeroSized);
        }
        if frame.rgba.len() != expected {
            return Err(GraphicsFrameError::RgbaLengthMismatch {
                expected,
                actual: frame.rgba.len(),
            });
        }
        if let Some((max_width, max_height)) = self.max_dimensions
            && (frame.width > max_width || frame.height > max_height)
        {
            return Err(GraphicsFrameError::RgbaLengthMismatch {
                expected: (max_width as usize)
                    .saturating_mul(max_height as usize)
                    .saturating_mul(4),
                actual: frame.rgba.len(),
            });
        }
        let fresh = self.presence == ScenePresence::Pixels
            && self.dirty
            && self
                .presented_generation
                .is_none_or(|generation| frame.generation > generation);
        self.dirty = false;
        if fresh {
            self.presented_generation = Some(frame.generation);
            self.fresh_presentations = self.fresh_presentations.saturating_add(1);
            Ok(PresentResult::Fresh)
        } else {
            Ok(PresentResult::Stale)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: u32, height: u32, generation: u64) -> GraphicsFrame {
        GraphicsFrame::new(
            width,
            height,
            vec![128; GraphicsFrame::pixel_len(width, height).unwrap()],
            generation,
        )
        .expect("test frame is valid")
    }

    #[test]
    fn accepts_frame_with_exact_rgba_length() {
        let frame = frame(3, 2, 1);
        assert_eq!(frame.rgba.len(), 3 * 2 * 4);
        assert_eq!(frame.width, 3);
        assert_eq!(frame.height, 2);
    }

    #[test]
    fn rejects_payload_length_mismatch() {
        assert_eq!(
            GraphicsFrame::new(2, 2, vec![0; 15], 1),
            Err(GraphicsFrameError::RgbaLengthMismatch {
                expected: 16,
                actual: 15
            })
        );
        assert_eq!(
            GraphicsFrame::new(2, 2, vec![0; 17], 1),
            Err(GraphicsFrameError::RgbaLengthMismatch {
                expected: 16,
                actual: 17
            })
        );
    }

    #[test]
    fn rejects_zero_sized_frames() {
        for (width, height) in [(0, 4), (4, 0), (0, 0)] {
            assert_eq!(
                GraphicsFrame::new(width, height, Vec::new(), 1),
                Err(GraphicsFrameError::ZeroSized),
                "({width}, {height}) must be rejected"
            );
        }
    }

    #[test]
    fn requires_dirty_before_presenting() {
        let mut surface = GraphicsSurface::new();
        surface.mark_dirty();
        assert_eq!(surface.present(frame(2, 2, 1)), Ok(PresentResult::Fresh));
        // A clean surface ignores even a newer generation.
        assert_eq!(surface.present(frame(2, 2, 2)), Ok(PresentResult::Stale));
        assert_eq!(surface.presented_generation(), Some(1));
    }

    #[test]
    fn newer_generation_replaces_older_pending() {
        let mut surface = GraphicsSurface::new();
        surface.mark_dirty();
        assert_eq!(surface.present(frame(2, 2, 1)), Ok(PresentResult::Fresh));
        surface.mark_dirty();
        assert_eq!(surface.present(frame(2, 2, 2)), Ok(PresentResult::Fresh));
        assert_eq!(surface.presented_generation(), Some(2));
    }

    #[test]
    fn older_generations_never_replace_presented_frames() {
        let mut surface = GraphicsSurface::new();
        surface.mark_dirty();
        assert_eq!(surface.present(frame(2, 2, 7)), Ok(PresentResult::Fresh));
        surface.mark_dirty();
        assert_eq!(surface.present(frame(2, 2, 6)), Ok(PresentResult::Stale));
        assert_eq!(surface.presented_generation(), Some(7));
    }

    #[test]
    fn generation_is_fresh_exactly_once() {
        let mut surface = GraphicsSurface::new();
        surface.mark_dirty();
        assert_eq!(surface.present(frame(2, 2, 3)), Ok(PresentResult::Fresh));
        surface.mark_dirty();
        // The same generation presented again must not report fresh.
        assert_eq!(surface.present(frame(2, 2, 3)), Ok(PresentResult::Stale));
        assert!(!surface.needs_present());
    }

    #[test]
    fn present_rejects_hand_built_invalid_frames() {
        let mut surface = GraphicsSurface::new();
        surface.mark_dirty();
        let bad = GraphicsFrame {
            width: 2,
            height: 2,
            rgba: vec![0; 4],
            generation: 1,
        };
        assert_eq!(
            surface.present(bad),
            Err(GraphicsFrameError::RgbaLengthMismatch {
                expected: 16,
                actual: 4
            })
        );
        assert!(
            surface.needs_present(),
            "an invalid frame must not consume the dirty flag"
        );
    }

    #[test]
    fn content_updates_keep_the_old_placement_until_the_new_frame_is_ready() {
        let mut surface = GraphicsSurface::new();
        let first = SceneKey::test(1, 1, 10, 5);
        assert_eq!(surface.transition(first), PlacementAction::Present);
        assert_eq!(surface.present(frame(2, 2, 5)), Ok(PresentResult::Fresh));
        assert_eq!(surface.fresh_presentations(), 1);
        assert_eq!(surface.transition(first), PlacementAction::Keep);
        assert_eq!(surface.present(frame(2, 2, 5)), Ok(PresentResult::Stale));
        assert_eq!(surface.fresh_presentations(), 1);

        let edited = SceneKey::test(1, 1, 10, 6);
        assert_eq!(
            surface.transition(edited),
            PlacementAction::Present,
            "content edits must not blank the current Kitty image while its replacement is encoded"
        );
        assert!(surface.needs_present());

        let mut other_surface = GraphicsSurface::new();
        assert_eq!(other_surface.transition(first), PlacementAction::Present);
        let different_editor = SceneKey::test(2, 1, 10, 6);
        assert_eq!(
            other_surface.transition(different_editor),
            PlacementAction::DeleteBeforePresent,
            "switching editors must not expose an unrelated delayed graphics frame"
        );

        let zoomed = SceneKey::test(1, 1, 11, 6);
        assert_eq!(
            surface.transition(zoomed),
            PlacementAction::Present,
            "camera changes must keep the old placement until the zoomed frame is ready"
        );
        assert!(surface.needs_present());
        let resized = SceneKey::test(1, 2, 12, 6);
        assert_eq!(
            surface.transition(resized),
            PlacementAction::DeleteBeforePresent,
            "a placement geometry change must remove the image at its old terminal rectangle"
        );
        assert_eq!(surface.leave_scene(), PlacementAction::DeleteOnly);
        assert_eq!(surface.leave_scene(), PlacementAction::Keep);
    }

    #[test]
    fn pixels_to_empty_deletes_the_existing_placement() {
        let mut surface = GraphicsSurface::new();
        let key = SceneKey::test(1, 1, 10, 5);
        assert_eq!(
            surface.transition_presence(ScenePresence::Pixels, Some(key)),
            PlacementAction::Present
        );
        assert_eq!(
            surface.transition_presence(ScenePresence::Empty, None),
            PlacementAction::DeleteOnly
        );
        assert_eq!(surface.presence(), ScenePresence::Empty);
        assert!(!surface.needs_present());
    }

    #[test]
    fn empty_presence_rejects_a_late_pixel_frame() {
        let mut surface = GraphicsSurface::new();
        let key = SceneKey::test(1, 1, 10, 5);
        assert_eq!(
            surface.transition_presence(ScenePresence::Pixels, Some(key)),
            PlacementAction::Present
        );
        surface.transition_presence(ScenePresence::Empty, None);
        assert_eq!(surface.present(frame(2, 2, 5)), Ok(PresentResult::Stale));
        assert_eq!(surface.presented_generation(), None);
        assert_eq!(surface.fresh_presentations(), 0);
    }
}
