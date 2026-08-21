use crate::sim::experiment_model::{ChannelId, DisplayColor, ExperimentSpec};

#[derive(Clone, Debug, PartialEq)]
pub enum DraftCommand {
    SetChannelValue {
        channel: ChannelId,
        tile: usize,
        value: f32,
    },
    RenameChannel {
        channel: ChannelId,
        name: String,
    },
    SetChannelColor {
        channel: ChannelId,
        color: DisplayColor,
    },
    SetChannelVisible {
        channel: ChannelId,
        visible: bool,
    },
    SetChannelFrozen {
        channel: ChannelId,
        frozen: bool,
    },
    ReplaceDraft(Box<ExperimentSpec>),
}

impl DraftCommand {
    pub fn apply(&self, draft: &mut ExperimentSpec) -> Result<Self, String> {
        match self {
            Self::SetChannelValue {
                channel,
                tile,
                value,
            } => {
                if !value.is_finite() || !(0.0..=1.0).contains(value) {
                    return Err("channel value must be finite and within 0..=1".into());
                }
                let target = draft
                    .channels
                    .iter_mut()
                    .find(|entry| entry.id == *channel)
                    .ok_or_else(|| "unknown channel".to_string())?;
                let previous = *target
                    .initial
                    .get(*tile)
                    .ok_or_else(|| "tile index is outside the channel".to_string())?;
                target.initial[*tile] = *value;
                Ok(Self::SetChannelValue {
                    channel: *channel,
                    tile: *tile,
                    value: previous,
                })
            }
            Self::RenameChannel { channel, name } => {
                let trimmed = name.trim();
                if trimmed.is_empty()
                    || draft
                        .channels
                        .iter()
                        .any(|entry| entry.id != *channel && entry.name == trimmed)
                {
                    return Err("channel name must be non-empty and unique".into());
                }
                let target = draft
                    .channels
                    .iter_mut()
                    .find(|entry| entry.id == *channel)
                    .ok_or_else(|| "unknown channel".to_string())?;
                let previous = std::mem::replace(&mut target.name, trimmed.to_string());
                Ok(Self::RenameChannel {
                    channel: *channel,
                    name: previous,
                })
            }
            Self::SetChannelColor { channel, color } => {
                let target = draft
                    .channels
                    .iter_mut()
                    .find(|entry| entry.id == *channel)
                    .ok_or_else(|| "unknown channel".to_string())?;
                let previous = std::mem::replace(&mut target.display.color, color.clone());
                Ok(Self::SetChannelColor {
                    channel: *channel,
                    color: previous,
                })
            }
            Self::SetChannelVisible { channel, visible } => {
                let target = draft
                    .channels
                    .iter_mut()
                    .find(|entry| entry.id == *channel)
                    .ok_or_else(|| "unknown channel".to_string())?;
                let previous = std::mem::replace(&mut target.display.visible, *visible);
                Ok(Self::SetChannelVisible {
                    channel: *channel,
                    visible: previous,
                })
            }
            Self::SetChannelFrozen { channel, frozen } => {
                let target = draft
                    .channels
                    .iter_mut()
                    .find(|entry| entry.id == *channel)
                    .ok_or_else(|| "unknown channel".to_string())?;
                let previous = std::mem::replace(&mut target.frozen, *frozen);
                Ok(Self::SetChannelFrozen {
                    channel: *channel,
                    frozen: previous,
                })
            }
            Self::ReplaceDraft(replacement) => {
                let previous = std::mem::replace(draft, replacement.as_ref().clone());
                Ok(Self::ReplaceDraft(Box::new(previous)))
            }
        }
    }
}
