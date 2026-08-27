//! The Channels workspace driven through its visible cards and actions.

use cellarium::gui::canvas::channels::{ChannelPreviewSource, ChannelView};
use cellarium::gui::{CellariumGui, Section, layout};
use cellarium::render::channels::Rgb8;
use cellarium::sim::experiment_model::{ChannelId, ExperimentSpec};
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

type Gui = Harness<'static, CellariumGui>;

const RED: Rgb8 = Rgb8 {
    red: 255,
    green: 0,
    blue: 0,
};
const GREEN: Rgb8 = Rgb8 {
    red: 0,
    green: 255,
    blue: 0,
};
const BLUE: Rgb8 = Rgb8 {
    red: 0,
    green: 0,
    blue: 255,
};

fn one_channel_gui() -> Gui {
    let mut app = CellariumGui::for_test(ExperimentSpec::single_channel_lenia(16, 16));
    app.navigation_mut().select(Section::Channels);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 720.0))
        .build_ui_state(|ui, app: &mut CellariumGui| layout::draw(app, ui), app);
    harness.run();
    harness
}

fn click(gui: &mut Gui, label: &str) {
    gui.get_by_label(label).click();
    gui.run();
}

/// Click a card by the name it shows, the way a user picks one out of the strip.
fn click_card(gui: &mut Gui, channel: ChannelId) {
    let name = card_name(gui, channel);
    gui.get_by_label(name.as_str()).click();
    gui.run();
}

/// Card actions are addressed per card, so two cards can offer "Hide" without
/// the test having to guess which one it reached.
fn click_card_action(gui: &mut Gui, channel: ChannelId, action: &str) {
    let name = card_name(gui, channel);
    gui.get_by_label(format!("{action} {name}").as_str())
        .click();
    gui.run();
}

fn card_name(gui: &Gui, channel: ChannelId) -> String {
    gui.state()
        .channel_cards()
        .into_iter()
        .find(|card| card.id == channel)
        .map(|card| card.name)
        .unwrap_or_else(|| panic!("channel {channel:?} has no card"))
}

fn card_names(gui: &Gui) -> Vec<String> {
    gui.state()
        .channel_cards()
        .into_iter()
        .map(|card| card.name)
        .collect()
}

fn card_colors(gui: &Gui) -> Vec<Rgb8> {
    gui.state()
        .channel_cards()
        .into_iter()
        .map(|card| card.color)
        .collect()
}

#[test]
fn cards_support_add_select_rgb_hide_freeze_delete_and_undo() {
    let mut gui = one_channel_gui();
    click(&mut gui, "Add channel");
    click(&mut gui, "Add channel");
    assert_eq!(card_names(&gui), ["state", "channel_2", "channel_3"]);
    assert_eq!(card_colors(&gui), [RED, GREEN, BLUE]);

    click_card(&mut gui, ChannelId(1));
    assert_eq!(gui.state().selected_channel(), ChannelId(1));

    click_card_action(&mut gui, ChannelId(1), "Hide");
    assert!(!gui.state().channel_cards()[1].visible);

    click_card_action(&mut gui, ChannelId(2), "Freeze");
    assert!(gui.state().channel_cards()[2].frozen);

    click_card_action(&mut gui, ChannelId(1), "Delete");
    assert_eq!(card_names(&gui), ["state", "channel_3"]);

    click(&mut gui, "Undo");
    assert_eq!(card_names(&gui), ["state", "channel_2", "channel_3"]);
    assert_eq!(gui.state().selected_channel(), ChannelId(1));
}

#[test]
fn every_card_offers_its_actions_and_the_last_channel_cannot_be_deleted() {
    let mut gui = one_channel_gui();
    // A single channel has nothing to fall back to, so Delete is refused rather
    // than leaving an experiment with no channels at all.
    let only = card_name(&gui, ChannelId(0));
    assert!(
        gui.get_by_label(format!("Delete {only}").as_str())
            .accesskit_node()
            .is_disabled(),
        "the last channel must not be deletable"
    );

    click(&mut gui, "Add channel");
    for card in gui.state().channel_cards() {
        for action in ["Hide", "Freeze", "Delete"] {
            gui.get_by_label(format!("{action} {}", card.name).as_str());
        }
    }
}

#[test]
fn hiding_and_freezing_are_reversible_from_the_same_control() {
    let mut gui = one_channel_gui();
    click(&mut gui, "Add channel");
    let name = card_name(&gui, ChannelId(1));

    click_card_action(&mut gui, ChannelId(1), "Hide");
    assert!(!gui.state().channel_cards()[1].visible);
    // The control renames itself so the reverse action is the same control.
    gui.get_by_label(format!("Show {name}").as_str()).click();
    gui.run();
    assert!(gui.state().channel_cards()[1].visible);

    click_card_action(&mut gui, ChannelId(1), "Freeze");
    assert!(gui.state().channel_cards()[1].frozen);
    gui.get_by_label(format!("Thaw {name}").as_str()).click();
    gui.run();
    assert!(!gui.state().channel_cards()[1].frozen);
}

#[test]
fn the_preview_never_passes_draft_initial_values_off_as_the_live_world() {
    let mut gui = one_channel_gui();
    assert_eq!(
        gui.state().channel_preview_source(),
        ChannelPreviewSource::Live
    );

    // Adding a channel changes the draft's structure. The live world still has
    // the old structure, so the preview must say which one it is showing.
    click(&mut gui, "Add channel");
    let preview = gui.state().channel_preview();
    assert!(
        preview.structure_stale,
        "a draft with new structure cannot be shown as the live world"
    );
    assert_eq!(
        preview.source,
        ChannelPreviewSource::DraftInitial,
        "a stale structure falls back to the draft's own initial values"
    );
    gui.get_by_label(preview.label);
    // The way out is offered, not merely implied.
    gui.get_by_label("Apply this draft");
}

#[test]
fn the_view_tabs_choose_how_channels_are_composited() {
    let mut gui = one_channel_gui();
    click(&mut gui, "Add channel");
    for (label, view) in [
        ("Composite", ChannelView::Composite),
        ("Solo", ChannelView::Solo),
        ("Grid", ChannelView::Grid),
    ] {
        click(&mut gui, label);
        assert_eq!(gui.state().channel_view(), view);
    }
}

#[test]
fn a_colour_can_be_set_exactly_and_returned_to_automatic() {
    let mut gui = one_channel_gui();
    click(&mut gui, "Add channel");
    click_card(&mut gui, ChannelId(1));

    gui.state_mut().set_selected_channel_rgb(18, 200, 77);
    gui.run();
    assert_eq!(gui.state().channel_cards()[1].color, Rgb8::new(18, 200, 77));

    click(&mut gui, "Automatic colour");
    let palette = cellarium::render::channels::automatic_palette(2);
    assert_eq!(
        gui.state().channel_cards()[1].color,
        palette[1],
        "automatic returns the channel to its palette slot"
    );
}
