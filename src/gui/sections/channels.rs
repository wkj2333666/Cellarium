//! The Channels workspace: object cards, view tabs, colour and the preview.

use eframe::egui::{self, Color32, RichText, Ui};

use crate::document::DocumentCommand;
use crate::document::channel_cards::channel_cards;
use crate::gui::app::CellariumGui;
use crate::gui::canvas::channels::{
    ChannelCanvasInput, ChannelPreviewSource, ChannelView, render_channel_canvas,
};
use crate::gui::theme;
use crate::gui::widgets::object_strip::{CardAction, ObjectCard, StripHit, object_strip};
use crate::render::channels::automatic_palette;
use crate::sim::experiment_model::ChannelId;

/// Presets offered in the colour popover, so a usable colour is one click away
/// and the exact fields are there when one click is not enough.
const PRESETS: [(&str, [u8; 3]); 6] = [
    ("Red", [255, 0, 0]),
    ("Green", [0, 255, 0]),
    ("Blue", [0, 0, 255]),
    ("Amber", [226, 178, 66]),
    ("Cyan", [64, 208, 216]),
    ("White", [236, 240, 246]),
];

pub fn draw(app: &mut CellariumGui, ui: &mut Ui) {
    cards(app, ui);
    ui.separator();
    toolbar(app, ui);
    ui.separator();
    canvas(app, ui);
}

fn cards(app: &mut CellariumGui, ui: &mut Ui) {
    let selected = app.selected_channel();
    let models = channel_cards(app.spec(), selected);
    let deletable = models.len() > 1;
    let cards: Vec<ObjectCard> = models
        .iter()
        .map(|model| {
            ObjectCard::new(u64::from(model.id.0), &model.name)
                .swatch(Color32::from_rgb(
                    model.color.red,
                    model.color.green,
                    model.color.blue,
                ))
                .selected(model.selected)
                .dimmed(!model.visible)
                .action(if model.visible {
                    CardAction::new("Hide", "Stop drawing this channel")
                } else {
                    CardAction::new("Show", "Draw this channel again")
                })
                .action(if model.frozen {
                    CardAction::new("Thaw", "Let this channel be updated again")
                } else {
                    CardAction::new("Freeze", "Hold this channel's values still")
                })
                .action(
                    CardAction::new(
                        "Delete",
                        if deletable {
                            "Remove this channel and its rules"
                        } else {
                            "an experiment must keep at least one channel"
                        },
                    )
                    .enabled(deletable),
                )
        })
        .collect();

    if let Some(hit) = object_strip(ui, "channel_cards", &cards, Some("Add channel")) {
        match hit {
            StripHit::Add => app.dispatch_document(DocumentCommand::AddChannel),
            StripHit::Select(key) => {
                app.dispatch_document(DocumentCommand::SelectChannel(ChannelId(key as u32)));
            }
            StripHit::Action { key, verb } => {
                let channel = ChannelId(key as u32);
                // Acting on a card selects it first, so the action and the
                // thing the user is looking at cannot drift apart.
                app.dispatch_document(DocumentCommand::SelectChannel(channel));
                let command = match verb.as_str() {
                    "Hide" => Some(DocumentCommand::SetSelectedChannelVisible(false)),
                    "Show" => Some(DocumentCommand::SetSelectedChannelVisible(true)),
                    "Freeze" => Some(DocumentCommand::SetSelectedChannelFrozen(true)),
                    "Thaw" => Some(DocumentCommand::SetSelectedChannelFrozen(false)),
                    "Delete" => Some(DocumentCommand::DeleteSelectedChannel),
                    _ => None,
                };
                if let Some(command) = command {
                    app.dispatch_document(command);
                    app.channel_canvas_mut().invalidate();
                }
            }
        }
    }
}

fn toolbar(app: &mut CellariumGui, ui: &mut Ui) {
    ui.horizontal_wrapped(|ui| {
        let mut view = app.channel_view();
        for candidate in ChannelView::ALL {
            if ui
                .add(egui::Button::selectable(
                    view == candidate,
                    candidate.label(),
                ))
                .on_hover_text(candidate.hint())
                .clicked()
            {
                view = candidate;
            }
        }
        if view != app.channel_view() {
            app.set_channel_view(view);
        }

        ui.separator();
        // The source is a deliberate choice, never an automatic fallback.
        let mut source = app.channel_preview_source();
        for candidate in [
            ChannelPreviewSource::Live,
            ChannelPreviewSource::DraftInitial,
        ] {
            if ui
                .add(egui::Button::selectable(
                    source == candidate,
                    candidate.label(),
                ))
                .on_hover_text(match candidate {
                    ChannelPreviewSource::Live => "Show the world that is actually running",
                    ChannelPreviewSource::DraftInitial => "Show the values a run would start from",
                })
                .clicked()
            {
                source = candidate;
            }
        }
        if source != app.channel_preview_source() {
            app.set_channel_preview_source(source);
        }

        ui.separator();
        colour(app, ui);
        ui.separator();
        if ui
            .button("Fit channels")
            .on_hover_text("Fit the whole preview in view")
            .clicked()
        {
            app.channel_canvas_mut().request_fit();
        }
    });
}

fn colour(app: &mut CellariumGui, ui: &mut Ui) {
    let selected = app.selected_channel();
    let current = app
        .spec()
        .channels
        .iter()
        .find(|channel| channel.id == selected)
        .map(|channel| channel.display.color.clone());
    ui.menu_button("Colour", |ui| {
        for (name, [red, green, blue]) in PRESETS {
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, 2.0, Color32::from_rgb(red, green, blue));
                if ui.button(name).clicked() {
                    app.set_selected_channel_rgb(red, green, blue);
                    ui.close();
                }
            });
        }
        ui.separator();
        // Exact values, for a colour no preset covers.
        let mut rgb = app.channel_colour_draft();
        let mut changed = false;
        for (label, component) in [("R", 0), ("G", 1), ("B", 2)] {
            changed |= ui
                .add(
                    egui::DragValue::new(&mut rgb[component])
                        .range(0..=255)
                        .prefix(format!("{label} ")),
                )
                .changed();
        }
        if changed {
            app.set_channel_colour_draft(rgb);
        }
        if ui.button("Set exact colour").clicked() {
            app.set_selected_channel_rgb(rgb[0], rgb[1], rgb[2]);
            ui.close();
        }
    })
    .response
    .on_hover_text("Choose how this channel is drawn");
    if ui
        .add_enabled(
            !matches!(
                current,
                Some(crate::sim::experiment_model::DisplayColor::Auto) | None
            ),
            egui::Button::new("Automatic colour"),
        )
        .on_hover_text("Return this channel to its slot in the palette")
        .clicked()
    {
        app.set_selected_channel_automatic_colour();
    }
}

fn canvas(app: &mut CellariumGui, ui: &mut Ui) {
    let snapshot = app.snapshot();
    let colors = automatic_palette(app.spec().channels.len());
    let colors: Vec<_> = app
        .spec()
        .channels
        .iter()
        .enumerate()
        .map(|(index, channel)| match channel.display.color {
            crate::sim::experiment_model::DisplayColor::Custom(rgb) => {
                crate::render::channels::Rgb8::new(rgb.red, rgb.green, rgb.blue)
            }
            crate::sim::experiment_model::DisplayColor::Auto => colors[index],
        })
        .collect();
    let selected = app
        .spec()
        .channels
        .iter()
        .position(|channel| channel.id == app.selected_channel())
        .unwrap_or(0);
    let generation = app.document().generation()
        ^ snapshot
            .as_ref()
            .map(|snapshot| snapshot.generation << 20)
            .unwrap_or(0);

    let label_height =
        ui.text_style_height(&egui::TextStyle::Body) + ui.spacing().item_spacing.y * 2.0;
    let size = egui::vec2(
        ui.available_width(),
        (ui.available_height() - label_height * 2.0).max(64.0),
    );

    let preview = {
        let active = app.document().active().clone();
        let draft = app.spec().clone();
        let input = ChannelCanvasInput {
            active: &active,
            draft: &draft,
            snapshot: snapshot.as_deref(),
            selected,
            colors: &colors,
            generation,
        };
        let state = app.channel_canvas_mut();
        render_channel_canvas(ui, size, &input, state)
    };

    // Naming the source is the whole point: draft initial values and a running
    // world look alike and mean different things.
    let state = if preview.structure_stale {
        theme::State::Stale
    } else {
        theme::State::Live
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(preview.label).color(theme::state_color(state)));
        if preview.structure_stale {
            // The way out is offered here rather than left to be discovered.
            if ui
                .button("Apply this draft")
                .on_hover_text("Make the draft the running experiment")
                .clicked()
            {
                app.dispatch(crate::gui::app::ShellAction::ApplyAndRun);
            }
        }
    });
    ui.label(
        RichText::new(format!(
            "{} channels, {} of {} visible",
            preview.channels,
            app.spec()
                .channels
                .iter()
                .filter(|channel| channel.display.visible)
                .count(),
            app.spec().channels.len()
        ))
        .weak(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_names_a_distinct_colour() {
        for (index, (name, rgb)) in PRESETS.iter().enumerate() {
            assert!(!name.is_empty());
            for (other_name, other_rgb) in &PRESETS[index + 1..] {
                assert_ne!(name, other_name);
                assert_ne!(rgb, other_rgb);
            }
        }
    }
}
