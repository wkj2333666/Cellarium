use crate::render::channels::{Rgb8, automatic_palette};
use crate::sim::experiment_model::{ChannelId, DisplayColor, ExperimentSpec, RgbColor};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ChannelView {
    #[default]
    Composite,
    Solo,
    Grid,
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
