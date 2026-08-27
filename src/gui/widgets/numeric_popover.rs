//! Typing an exact number into a cell.
//!
//! Painting reaches a value quickly and approximately; this reaches one
//! exactly. A user who needs 0.137 should not have to arrive at it by
//! accumulating wheel steps.

use eframe::egui::{self, Ui};

/// An exact-value entry in progress.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NumericPopover {
    /// What the value belongs to, for the caller to interpret.
    pub target: Option<(usize, usize)>,
    pub text: String,
    /// Why the typed text is not a number yet.
    pub error: Option<String>,
}

impl NumericPopover {
    pub fn open(&mut self, target: (usize, usize), current: f32) {
        self.target = Some(target);
        // Pre-filling with the current value makes a small correction a small
        // edit rather than a retype.
        self.text = format!("{current}");
        self.error = None;
    }

    pub fn close(&mut self) {
        self.target = None;
        self.text.clear();
        self.error = None;
    }

    pub fn is_open(&self) -> bool {
        self.target.is_some()
    }

    /// Parse the typed text, or say why it is not a number.
    pub fn parse(&self) -> Result<f32, String> {
        let trimmed = self.text.trim();
        if trimmed.is_empty() {
            return Err("type a number".into());
        }
        let value: f32 = trimmed
            .parse()
            .map_err(|_| format!("`{trimmed}` is not a number"))?;
        if !value.is_finite() {
            return Err("a kernel weight must be finite".into());
        }
        Ok(value)
    }
}

/// What the user did with the popover.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NumericOutcome {
    Accepted { x: usize, y: usize, value: f32 },
    Cancelled,
}

pub fn numeric_popover(ui: &mut Ui, state: &mut NumericPopover) -> Option<NumericOutcome> {
    let (x, y) = state.target?;
    let mut outcome = None;
    egui::Frame::popup(ui.style()).show(ui, |ui| {
        ui.label(format!("Exact value for cell ({x}, {y})"));
        let response = ui.add(
            egui::TextEdit::singleline(&mut state.text)
                .desired_width(120.0)
                .hint_text("for example 0.137"),
        );
        // Enter accepts, because a dialog that only accepts by mouse is slower
        // than the painting it exists to improve on.
        let submitted =
            response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        if response.changed() {
            state.error = state.parse().err();
        }
        if let Some(error) = &state.error {
            ui.label(
                egui::RichText::new(error).color(crate::gui::theme::state_color(
                    crate::gui::theme::State::Invalid,
                )),
            );
        }
        ui.horizontal(|ui| {
            let accept = ui
                .button("Set value")
                .on_hover_text("Write this exact value into the cell")
                .clicked();
            if accept || submitted {
                match state.parse() {
                    Ok(value) => outcome = Some(NumericOutcome::Accepted { x, y, value }),
                    Err(error) => state.error = Some(error),
                }
            }
            if ui
                .button("Cancel value")
                .on_hover_text("Leave the cell as it is")
                .clicked()
            {
                outcome = Some(NumericOutcome::Cancelled);
            }
        });
    });
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_prefills_the_current_value_so_a_small_change_is_a_small_edit() {
        let mut popover = NumericPopover::default();
        assert!(!popover.is_open());
        popover.open((2, 3), 0.25);
        assert!(popover.is_open());
        assert_eq!(popover.text, "0.25");
        assert_eq!(popover.parse().unwrap(), 0.25);
    }

    #[test]
    fn text_that_is_not_a_number_is_refused_with_the_text_quoted_back() {
        let mut popover = NumericPopover::default();
        popover.open((0, 0), 1.0);
        popover.text = "0.1.2".into();
        let error = popover.parse().unwrap_err();
        assert!(error.contains("0.1.2"), "{error}");

        popover.text = "   ".into();
        assert!(popover.parse().unwrap_err().contains("type a number"));
    }

    #[test]
    fn a_non_finite_value_is_refused() {
        let mut popover = NumericPopover::default();
        popover.open((0, 0), 1.0);
        for text in ["inf", "-inf", "NaN"] {
            popover.text = text.into();
            assert!(popover.parse().is_err(), "{text} must be refused");
        }
    }

    #[test]
    fn negative_and_exponent_forms_are_accepted() {
        let mut popover = NumericPopover::default();
        popover.open((0, 0), 0.0);
        popover.text = "-0.75".into();
        assert_eq!(popover.parse().unwrap(), -0.75);
        popover.text = "1e-3".into();
        assert!((popover.parse().unwrap() - 0.001).abs() < 1e-9);
    }

    #[test]
    fn closing_clears_the_target_so_a_stale_edit_cannot_be_committed() {
        let mut popover = NumericPopover::default();
        popover.open((4, 5), 1.0);
        popover.close();
        assert!(!popover.is_open());
        assert!(popover.text.is_empty());
    }
}
