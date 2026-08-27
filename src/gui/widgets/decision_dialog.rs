//! A decision the user has to make before a destructive edit proceeds.
//!
//! The dialog exists because some deletions cannot be carried out without also
//! rewriting something the user wrote. Showing the exact rewrite before it
//! happens is the difference between a choice and a surprise.

use eframe::egui::{self, RichText, Ui};

use crate::gui::theme;

/// A consequence the user is being asked to accept.
#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub title: String,
    /// One sentence saying what will happen, in the user's terms.
    pub summary: String,
    /// The exact before/after of any text that would be rewritten.
    pub diff: Option<DecisionDiff>,
    /// Label of the action that proceeds. Never just "OK": the button says
    /// what it does, so a glance is enough to know what is about to happen.
    pub confirm: String,
    pub confirm_hint: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecisionDiff {
    pub caption: String,
    pub before: String,
    pub after: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionOutcome {
    Confirmed,
    Cancelled,
}

pub fn decision_dialog(ui: &mut Ui, decision: &Decision) -> Option<DecisionOutcome> {
    let mut outcome = None;
    egui::Frame::popup(ui.style())
        .stroke(egui::Stroke::new(
            1.5,
            theme::state_color(theme::State::Invalid),
        ))
        .show(ui, |ui| {
            ui.label(RichText::new(&decision.title).strong());
            ui.label(&decision.summary);
            if let Some(diff) = &decision.diff {
                ui.separator();
                ui.label(RichText::new(&diff.caption).weak());
                // Before and after are both shown in full. A diff that only
                // showed the new text would ask the user to remember the old.
                ui.label(
                    RichText::new(format!("before: {}", diff.before))
                        .monospace()
                        .color(theme::state_color(theme::State::Stale)),
                );
                ui.label(
                    RichText::new(format!("after:  {}", diff.after))
                        .monospace()
                        .color(theme::state_color(theme::State::Live)),
                );
            }
            ui.separator();
            ui.horizontal(|ui| {
                // Cancel comes first, because the safe choice should not be the
                // one the pointer has to travel furthest to reach.
                if ui
                    .button("Cancel")
                    .on_hover_text("Change nothing")
                    .clicked()
                {
                    outcome = Some(DecisionOutcome::Cancelled);
                }
                if ui
                    .button(&decision.confirm)
                    .on_hover_text(&decision.confirm_hint)
                    .clicked()
                {
                    outcome = Some(DecisionOutcome::Confirmed);
                }
            });
        });
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision() -> Decision {
        Decision {
            title: "Delete kernel k1".into(),
            summary: "The growth program uses k1. Deleting it replaces that reference with 0."
                .into(),
            diff: Some(DecisionDiff {
                caption: "Growth source".into(),
                before: "k1 * 2.0".into(),
                after: "0.0 * 2.0".into(),
            }),
            confirm: "Replace references with 0 and remove".into(),
            confirm_hint: "Remove the kernel and rewrite the growth program".into(),
        }
    }

    #[test]
    fn the_confirming_button_says_what_it_does() {
        let decision = decision();
        assert!(
            decision.confirm.len() > 2 && decision.confirm.to_lowercase() != "ok",
            "the action must name itself: {}",
            decision.confirm
        );
        assert!(!decision.confirm_hint.is_empty());
    }

    #[test]
    fn a_rewrite_is_shown_before_and_after_not_only_after() {
        let diff = decision().diff.expect("this decision rewrites the source");
        assert_ne!(diff.before, diff.after);
        assert!(!diff.before.is_empty());
        assert!(!diff.caption.is_empty());
    }

    #[test]
    fn a_decision_without_a_rewrite_still_states_its_consequence() {
        let mut plain = decision();
        plain.diff = None;
        assert!(!plain.summary.is_empty());
    }
}
