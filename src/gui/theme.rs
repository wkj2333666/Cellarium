use eframe::egui::Color32;

/// Board interior. The simulated domain is pure black so channel colors carry
/// all visible signal.
pub const BOARD_INTERIOR: Color32 = Color32::BLACK;

/// Everything outside the periodic domain.
pub const DOMAIN_EXTERIOR: Color32 = Color32::from_rgb(8, 18, 46);

/// Single-channel experiments use a high-contrast neutral instead of red.
pub const SINGLE_CHANNEL: Color32 = Color32::from_rgb(236, 240, 246);

pub const CHANNEL_RGB: [Color32; 3] = [
    Color32::from_rgb(226, 74, 74),
    Color32::from_rgb(74, 206, 108),
    Color32::from_rgb(78, 138, 240),
];

pub const KERNEL_POSITIVE: Color32 = Color32::from_rgb(64, 208, 216);
pub const KERNEL_NEGATIVE: Color32 = Color32::from_rgb(226, 74, 74);
/// A cell that contributes but is currently worth nothing. It has to read
/// as clearly present: against the near-black of an absent cell, a dark
/// grey here made the two states indistinguishable at a glance.
pub const KERNEL_ACTIVE_ZERO: Color32 = Color32::from_rgb(122, 130, 146);
/// A cell switched off. Near-black, and outlined where it is drawn, so the
/// stencil's shape stays legible without the cell claiming to hold a value.
pub const KERNEL_INACTIVE: Color32 = Color32::from_rgb(18, 22, 34);
pub const KERNEL_ANCHOR: Color32 = Color32::from_rgb(226, 178, 66);

/// Tiling canvas. The unit cell is opaque and its periodic copies are
/// translucent, so what is editable is visibly distinct from what is context.
pub const CELL_FILL: Color32 = Color32::from_rgb(38, 54, 96);
pub const CELL_STROKE: Color32 = Color32::from_rgb(150, 176, 224);
pub const NEIGHBOR_FILL: Color32 = Color32::from_rgba_premultiplied(20, 32, 62, 150);
pub const NEIGHBOR_STROKE: Color32 = Color32::from_rgba_premultiplied(70, 92, 140, 170);
pub const LATTICE_VECTOR: Color32 = Color32::from_rgb(226, 178, 66);

pub const SELECTION: Color32 = Color32::WHITE;
pub const DRAFT: Color32 = Color32::from_rgb(226, 178, 66);
pub const LIVE: Color32 = Color32::from_rgb(74, 206, 108);
pub const INVALID: Color32 = Color32::from_rgb(226, 74, 74);
pub const STALE: Color32 = Color32::from_rgb(226, 178, 66);

/// Color is never the only state indicator; each state also carries this glyph
/// so the interface stays readable without color discrimination. The glyphs stay
/// inside Latin-1 because egui's bundled proportional font has no coverage for
/// geometric shapes such as U+25CF, which draw as a missing-glyph box.
pub fn state_glyph(state: State) -> &'static str {
    match state {
        State::Draft => "»",
        State::Live => "•",
        State::Invalid => "×",
        State::Stale => "·",
    }
}

pub fn state_color(state: State) -> Color32 {
    match state {
        State::Draft => DRAFT,
        State::Live => LIVE,
        State::Invalid => INVALID,
        State::Stale => STALE,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Draft,
    Live,
    Invalid,
    Stale,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// egui's bundled proportional font covers Latin-1 plus a small set of
    /// punctuation. A glyph outside that coverage draws as a missing-glyph box,
    /// which reads as an unexplained mark in the interface.
    #[test]
    fn every_glyph_stays_within_the_bundled_font_coverage() {
        for state in [State::Draft, State::Live, State::Invalid, State::Stale] {
            for character in state_glyph(state).chars() {
                assert!(
                    (character as u32) <= 0x2022,
                    "glyph {character:?} for {state:?} is outside the covered range"
                );
            }
        }
    }

    #[test]
    fn every_state_has_a_distinct_glyph_and_color() {
        let states = [State::Draft, State::Live, State::Invalid, State::Stale];
        for (index, state) in states.iter().enumerate() {
            for other in &states[index + 1..] {
                assert_ne!(state_glyph(*state), state_glyph(*other));
            }
        }
        assert_ne!(state_color(State::Draft), state_color(State::Live));
        assert_ne!(state_color(State::Live), state_color(State::Invalid));
    }
}
