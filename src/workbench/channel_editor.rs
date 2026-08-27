use crate::render::channels::{Rgb8, automatic_palette};
use crate::sim::experiment_model::{ChannelId, DisplayColor, ExperimentSpec, RgbColor};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ChannelView {
    #[default]
    Composite,
    Solo,
    Grid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelCardModel {
    pub id: ChannelId,
    pub name: String,
    pub color: Rgb8,
    pub visible: bool,
    pub frozen: bool,
    pub selected: bool,
}

impl ChannelCardModel {
    pub fn new(
        id: ChannelId,
        name: impl Into<String>,
        color: Rgb8,
        visible: bool,
        frozen: bool,
        selected: bool,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            color,
            visible,
            frozen,
            selected,
        }
    }
}

pub fn add_channel(
    draft: &mut ExperimentSpec,
    name: impl Into<String>,
    frozen: bool,
) -> Result<ChannelId, String> {
    let name = name.into();
    if name.trim().is_empty() || draft.channels.iter().any(|c| c.name == name) {
        return Err("channel name must be non-empty and unique".into());
    }
    Ok(draft.add_channel(name, frozen))
}
pub fn resolved_color(draft: &ExperimentSpec, channel: ChannelId) -> Option<Rgb8> {
    let index = draft
        .channels
        .iter()
        .position(|entry| entry.id == channel)?;
    let channel = &draft.channels[index];
    Some(match channel.display.color {
        DisplayColor::Custom(RgbColor { red, green, blue }) => Rgb8::new(red, green, blue),
        DisplayColor::Auto => automatic_palette(draft.channels.len())[index],
    })
}

pub fn channel_cards(draft: &ExperimentSpec, selected: ChannelId) -> Vec<ChannelCardModel> {
    draft
        .channels
        .iter()
        .filter_map(|channel| {
            Some(ChannelCardModel::new(
                channel.id,
                &channel.name,
                resolved_color(draft, channel.id)?,
                channel.display.visible,
                channel.frozen,
                channel.id == selected,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{ChannelCardModel, channel_cards};
    use crate::render::channels::Rgb8;
    use crate::sim::experiment_model::{ChannelId, ExperimentSpec};

    #[test]
    fn channel_cards_expose_rgb_selection_visibility_and_frozen_state() {
        let mut spec = ExperimentSpec::single_channel_lenia(4, 4);
        spec.add_channel("green", false);
        spec.add_channel("blue", false);
        spec.channels[1].display.visible = false;
        spec.channels[2].frozen = true;

        let cards = channel_cards(&spec, ChannelId(1));

        assert_eq!(
            cards,
            vec![
                ChannelCardModel::new(
                    ChannelId(0),
                    "state",
                    Rgb8::new(255, 0, 0),
                    true,
                    false,
                    false
                ),
                ChannelCardModel::new(
                    ChannelId(1),
                    "green",
                    Rgb8::new(0, 255, 0),
                    false,
                    false,
                    true
                ),
                ChannelCardModel::new(
                    ChannelId(2),
                    "blue",
                    Rgb8::new(0, 0, 255),
                    true,
                    true,
                    false
                ),
            ]
        );
    }
}
