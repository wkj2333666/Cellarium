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
pub const KERNEL_ACTIVE_ZERO: Color32 = Color32::from_rgb(58, 62, 70);
pub const KERNEL_ANCHOR: Color32 = Color32::from_rgb(226, 178, 66);

pub const SELECTION: Color32 = Color32::WHITE;
pub const DRAFT: Color32 = Color32::from_rgb(226, 178, 66);
pub const LIVE: Color32 = Color32::from_rgb(74, 206, 108);
pub const INVALID: Color32 = Color32::from_rgb(226, 74, 74);
pub const STALE: Color32 = Color32::from_rgb(226, 178, 66);

/// Color is never the only state indicator; each state also carries this glyph
/// so the interface stays readable without color discrimination.
pub fn state_glyph(state: State) -> &'static str {
    match state {
        State::Draft => "◐",
        State::Live => "●",
        State::Invalid => "✖",
        State::Stale => "◌",
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
