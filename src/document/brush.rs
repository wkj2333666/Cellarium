//! What the pointer paints with.
//!
//! A brush is a shape, a strength and a target value. Keeping the arithmetic
//! here rather than in the canvas means the profile a stroke lays down can be
//! tested without a window, and that the readout beside the controls describes
//! the same numbers the stroke actually uses.

/// The kinds of mark a user can make.
///
/// These are named for what they do to the world, not for the maths behind
/// them: someone reaching for "pencil" wants one crisp cell, and someone
/// reaching for "airbrush" wants to build a value up gradually.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BrushKind {
    /// A single cell, always fully painted.
    Pencil,
    /// A hard-edged disc.
    #[default]
    Pen,
    /// A disc that fades towards its edge.
    Brush,
    /// A soft disc that builds up as the pointer lingers.
    Airbrush,
    /// A hard-edged disc that paints zero.
    Eraser,
}

impl BrushKind {
    pub const ALL: [BrushKind; 5] = [
        BrushKind::Pencil,
        BrushKind::Pen,
        BrushKind::Brush,
        BrushKind::Airbrush,
        BrushKind::Eraser,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BrushKind::Pencil => "Pencil",
            BrushKind::Pen => "Pen",
            BrushKind::Brush => "Brush",
            BrushKind::Airbrush => "Airbrush",
            BrushKind::Eraser => "Eraser",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            BrushKind::Pencil => "One cell, hard edge, painted fully",
            BrushKind::Pen => "A hard-edged disc, painted fully",
            BrushKind::Brush => "A disc that fades out towards its edge",
            BrushKind::Airbrush => "A soft disc that builds up as you hold the button",
            BrushKind::Eraser => "A hard-edged disc that paints zero",
        }
    }

    /// Whether the size control applies to this kind.
    pub fn has_radius(self) -> bool {
        self != BrushKind::Pencil
    }

    /// The strength this kind starts at, as a fraction of the paint value.
    ///
    /// An airbrush that laid its full value down on the first frame would be a
    /// pen; the low flow is the whole character of the tool.
    pub fn default_flow(self) -> f32 {
        match self {
            BrushKind::Airbrush => 0.12,
            _ => 1.0,
        }
    }

    /// How much of the stamp the pointer covers at distance `t` from the
    /// centre, where `t` is 1.0 at the edge.
    fn profile(self, t: f32) -> f32 {
        if t > 1.0 {
            return 0.0;
        }
        match self {
            // Hard edges: every cell inside the radius gets the same weight.
            BrushKind::Pencil | BrushKind::Pen | BrushKind::Eraser => 1.0,
            // Smooth falloff, full in the middle and zero at the rim.
            BrushKind::Brush => (1.0 - t * t).clamp(0.0, 1.0),
            // Softer still, so repeated passes accumulate a gradient.
            BrushKind::Airbrush => (1.0 - t).clamp(0.0, 1.0).powf(1.5),
        }
    }
}

/// Where a stroke is written.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BrushTarget {
    /// Every channel, which is what a single-channel world wants.
    #[default]
    AllChannels,
    /// One channel, by index.
    Channel(usize),
}

/// The brush as the user has set it up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushSettings {
    pub kind: BrushKind,
    /// Radius in cells. Zero paints a single cell.
    pub radius: u32,
    /// Share of the value laid down per application, 0.0 to 1.0.
    pub flow: f32,
    /// The value a full-strength stroke moves the cell towards.
    pub value: f32,
    pub target: BrushTarget,
}

impl Default for BrushSettings {
    fn default() -> Self {
        Self {
            kind: BrushKind::default(),
            radius: 2,
            flow: 1.0,
            value: 1.0,
            target: BrushTarget::default(),
        }
    }
}

/// One cell of a stamp: an offset from the centre and how strongly it applies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushSample {
    pub dx: i64,
    pub dy: i64,
    /// Fraction of the way from the cell's current value to the target.
    pub alpha: f32,
}

impl BrushSettings {
    /// Radius actually used, which a pencil forces to a single cell.
    pub fn effective_radius(&self) -> u32 {
        if self.kind.has_radius() {
            self.radius
        } else {
            0
        }
    }

    /// The value a stroke moves cells towards.
    pub fn target_value(&self, erase: bool) -> f32 {
        if erase || self.kind == BrushKind::Eraser {
            0.0
        } else {
            self.value
        }
    }

    /// The cells this brush touches and how strongly.
    ///
    /// Computed once per stroke position rather than per channel, because a
    /// multi-channel world would otherwise pay for the same geometry twice.
    pub fn stamp(&self) -> Vec<BrushSample> {
        let radius = self.effective_radius() as i64;
        let flow = self.flow.clamp(0.0, 1.0);
        let mut samples = Vec::new();
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let distance = ((dx * dx + dy * dy) as f32).sqrt();
                // A zero radius is a single cell, and dividing by it would make
                // the one cell it does cover NaN.
                let t = if radius == 0 {
                    0.0
                } else {
                    distance / radius as f32
                };
                if t > 1.0 {
                    continue;
                }
                let alpha = flow * self.kind.profile(t);
                if alpha <= 0.0 {
                    continue;
                }
                samples.push(BrushSample { dx, dy, alpha });
            }
        }
        samples
    }

    /// The value one application writes into a cell that currently holds
    /// `current`.
    ///
    /// Blending rather than assigning is what makes a partial strength mean
    /// anything: at 70% the cell ends up 70% of the way to the target, so a
    /// second pass takes it further and a soft edge stays soft.
    pub fn blend(&self, current: f32, alpha: f32, erase: bool) -> f32 {
        let target = self.target_value(erase);
        (current + (target - current) * alpha.clamp(0.0, 1.0)).clamp(0.0, 1.0)
    }

    /// Whether this channel index is painted.
    pub fn paints_channel(&self, channel: usize) -> bool {
        match self.target {
            BrushTarget::AllChannels => true,
            BrushTarget::Channel(only) => only == channel,
        }
    }

    /// Adopt a kind, taking its natural strength with it.
    pub fn select_kind(&mut self, kind: BrushKind) {
        self.kind = kind;
        self.flow = kind.default_flow();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pencil_paints_exactly_one_cell_whatever_the_radius() {
        let brush = BrushSettings {
            kind: BrushKind::Pencil,
            radius: 9,
            ..BrushSettings::default()
        };
        let stamp = brush.stamp();
        assert_eq!(stamp.len(), 1);
        assert_eq!((stamp[0].dx, stamp[0].dy), (0, 0));
        assert_eq!(stamp[0].alpha, 1.0);
    }

    #[test]
    fn a_pen_covers_its_disc_evenly() {
        let brush = BrushSettings {
            kind: BrushKind::Pen,
            radius: 3,
            ..BrushSettings::default()
        };
        let stamp = brush.stamp();
        assert!(stamp.len() > 1);
        assert!(
            stamp.iter().all(|sample| sample.alpha == 1.0),
            "a hard edge means no cell inside the disc is weaker than another"
        );
    }

    #[test]
    fn a_brush_fades_towards_its_edge() {
        let brush = BrushSettings {
            kind: BrushKind::Brush,
            radius: 4,
            ..BrushSettings::default()
        };
        let stamp = brush.stamp();
        let alpha_at = |dx: i64, dy: i64| {
            stamp
                .iter()
                .find(|sample| sample.dx == dx && sample.dy == dy)
                .map(|sample| sample.alpha)
        };
        // Cells exactly on the rim fade to nothing and are left out of the
        // stamp entirely, so the claim is about the cells that are painted.
        let centre = alpha_at(0, 0).expect("the centre is always painted");
        let middle = alpha_at(2, 0).expect("a cell inside the disc is painted");
        let outer = alpha_at(3, 0).expect("a cell nearer the rim is painted");
        assert!(
            centre > middle && middle > outer,
            "strength falls off with distance: {centre} {middle} {outer}"
        );
        assert!(
            alpha_at(4, 0).is_none(),
            "the rim itself contributes nothing"
        );
    }

    #[test]
    fn seventy_percent_strength_moves_a_cell_seventy_percent_of_the_way() {
        let brush = BrushSettings {
            kind: BrushKind::Pen,
            flow: 0.7,
            value: 1.0,
            ..BrushSettings::default()
        };
        let stamp = brush.stamp();
        let alpha = stamp[0].alpha;
        assert!((alpha - 0.7).abs() < 1e-6);
        assert!((brush.blend(0.0, alpha, false) - 0.7).abs() < 1e-6);
    }

    #[test]
    fn repeated_partial_passes_build_up_without_passing_the_target() {
        let brush = BrushSettings {
            kind: BrushKind::Airbrush,
            flow: 0.3,
            value: 1.0,
            ..BrushSettings::default()
        };
        let mut value = 0.0;
        for _ in 0..40 {
            value = brush.blend(value, 0.3, false);
        }
        assert!(value > 0.9, "an airbrush builds up: {value}");
        assert!(value <= 1.0, "and never exceeds the value it paints");
    }

    #[test]
    fn an_eraser_paints_zero_whatever_the_value_says() {
        let brush = BrushSettings {
            kind: BrushKind::Eraser,
            value: 1.0,
            ..BrushSettings::default()
        };
        assert_eq!(brush.target_value(false), 0.0);
        assert_eq!(brush.blend(1.0, 1.0, false), 0.0);
    }

    #[test]
    fn the_right_button_erases_with_any_brush() {
        let brush = BrushSettings::default();
        assert_eq!(brush.target_value(true), 0.0);
    }

    #[test]
    fn choosing_a_kind_takes_its_natural_strength() {
        let mut brush = BrushSettings::default();
        brush.select_kind(BrushKind::Airbrush);
        assert!(brush.flow < 1.0, "an airbrush starts gentle");
        brush.select_kind(BrushKind::Pen);
        assert_eq!(brush.flow, 1.0, "a pen starts solid");
    }

    #[test]
    fn a_targeted_brush_paints_only_its_channel() {
        let brush = BrushSettings {
            target: BrushTarget::Channel(1),
            ..BrushSettings::default()
        };
        assert!(!brush.paints_channel(0));
        assert!(brush.paints_channel(1));
        assert!(BrushSettings::default().paints_channel(3));
    }

    #[test]
    fn blending_never_leaves_the_unit_range() {
        let brush = BrushSettings {
            value: 1.0,
            ..BrushSettings::default()
        };
        assert!((0.0..=1.0).contains(&brush.blend(0.9, 2.0, false)));
        assert!((0.0..=1.0).contains(&brush.blend(0.1, -1.0, true)));
    }
}
