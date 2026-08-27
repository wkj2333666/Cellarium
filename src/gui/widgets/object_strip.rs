//! A scrolling strip of object cards.
//!
//! Channels, kernels and any other list of stable-identity objects are picked
//! from one of these. Every object present is reachable by scrolling: nothing
//! is hidden behind a "next" control that only reveals one neighbour at a time.

use eframe::egui::{self, Color32, RichText, Ui};

use crate::gui::theme;

/// One action offered on a card. The label is what the user reads and what a
/// test addresses, so it is qualified by the card's own title.
#[derive(Clone, Debug, PartialEq)]
pub struct CardAction {
    /// Verb shown on the button, e.g. "Hide".
    pub verb: String,
    pub tooltip: String,
    pub enabled: bool,
}

impl CardAction {
    pub fn new(verb: impl Into<String>, tooltip: impl Into<String>) -> Self {
        Self {
            verb: verb.into(),
            tooltip: tooltip.into(),
            enabled: true,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// One object in the strip.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectCard {
    /// Caller's stable identity for the object, returned in hits.
    pub key: u64,
    pub title: String,
    /// Small line under the title, e.g. an ordinal or a source.
    pub subtitle: Option<String>,
    /// Colour swatch drawn beside the title.
    pub swatch: Option<Color32>,
    pub selected: bool,
    /// Drawn dimmed when the object is not contributing.
    pub dimmed: bool,
    pub actions: Vec<CardAction>,
}

impl ObjectCard {
    pub fn new(key: u64, title: impl Into<String>) -> Self {
        Self {
            key,
            title: title.into(),
            subtitle: None,
            swatch: None,
            selected: false,
            dimmed: false,
            actions: Vec::new(),
        }
    }

    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    pub fn swatch(mut self, swatch: Color32) -> Self {
        self.swatch = Some(swatch);
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn dimmed(mut self, dimmed: bool) -> Self {
        self.dimmed = dimmed;
        self
    }

    pub fn action(mut self, action: CardAction) -> Self {
        self.actions.push(action);
        self
    }
}

/// What the pointer did to the strip.
#[derive(Clone, Debug, PartialEq)]
pub enum StripHit {
    /// A card body was clicked; select this object.
    Select(u64),
    /// An action on a card was clicked.
    Action { key: u64, verb: String },
    /// The trailing add card was clicked.
    Add,
}

/// Draw the strip and report the single thing the pointer did.
pub fn object_strip(
    ui: &mut Ui,
    id: &str,
    cards: &[ObjectCard],
    add: Option<&str>,
) -> Option<StripHit> {
    let mut hit = None;
    egui::ScrollArea::horizontal()
        .id_salt(id)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for card in cards {
                    if let Some(card_hit) = draw_card(ui, card) {
                        hit = Some(card_hit);
                    }
                }
                if let Some(add) = add {
                    // Add is trailing so the objects keep stable positions as
                    // the list grows.
                    if ui
                        .button(RichText::new(add).strong())
                        .on_hover_text("Add a new object to this list")
                        .clicked()
                    {
                        hit = Some(StripHit::Add);
                    }
                }
            });
        });
    hit
}

fn draw_card(ui: &mut Ui, card: &ObjectCard) -> Option<StripHit> {
    let mut hit = None;
    let stroke = if card.selected {
        egui::Stroke::new(2.0, theme::state_color(theme::State::Live))
    } else {
        egui::Stroke::new(1.0, theme::CELL_STROKE)
    };
    egui::Frame::group(ui.style())
        .stroke(stroke)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    if let Some(swatch) = card.swatch {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 2.0, swatch);
                    }
                    // The title is the select control, so picking a card is
                    // clicking the thing that names it.
                    let mut title = RichText::new(&card.title);
                    if card.dimmed {
                        title = title.weak();
                    }
                    if card.selected {
                        title = title.strong();
                    }
                    if ui
                        .add(egui::Button::new(title).frame(false))
                        .on_hover_text("Select this object")
                        .clicked()
                    {
                        hit = Some(StripHit::Select(card.key));
                    }
                });
                if let Some(subtitle) = &card.subtitle {
                    ui.label(RichText::new(subtitle).weak().small());
                }
                ui.horizontal(|ui| {
                    for action in &card.actions {
                        // The button reads as the bare verb, but its accessible
                        // name is qualified by the card's title: without that,
                        // three cards offer three controls all called "Hide"
                        // and neither a test nor a screen reader can say which
                        // one it means.
                        let label = format!("{} {}", action.verb, card.title);
                        let response = ui
                            .add_enabled(
                                action.enabled,
                                egui::Button::new(RichText::new(&action.verb).small()),
                            )
                            .on_hover_text(&action.tooltip);
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                action.enabled,
                                &label,
                            )
                        });
                        if response.clicked() {
                            hit = Some(StripHit::Action {
                                key: card.key,
                                verb: action.verb.clone(),
                            });
                        }
                    }
                });
            });
        });
    hit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_carries_its_identity_and_actions() {
        let card = ObjectCard::new(7, "state")
            .subtitle("channel 1")
            .swatch(Color32::RED)
            .selected(true)
            .action(CardAction::new("Hide", "Stop drawing this channel"))
            .action(CardAction::new("Delete", "Remove it").enabled(false));
        assert_eq!(card.key, 7);
        assert_eq!(card.actions.len(), 2);
        assert!(!card.actions[1].enabled);
    }

    #[test]
    fn a_disabled_action_stays_visible_so_the_reason_can_be_shown() {
        let action = CardAction::new("Delete", "the last channel cannot be removed").enabled(false);
        assert!(!action.enabled);
        assert!(!action.tooltip.is_empty());
    }
}
