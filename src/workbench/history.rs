use super::DraftCommand;
use crate::sim::experiment_model::ExperimentSpec;

const MAX_HISTORY: usize = 1024;

#[derive(Clone, Debug)]
struct Entry {
    forward: DraftCommand,
    inverse: DraftCommand,
}

#[derive(Clone, Debug, Default)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum HistoryError {
    #[error("nothing to undo")]
    NothingToUndo,
    #[error("nothing to redo")]
    NothingToRedo,
    #[error("draft edit failed: {0}")]
    Edit(String),
}

impl History {
    pub fn execute(
        &mut self,
        draft: &mut ExperimentSpec,
        forward: DraftCommand,
    ) -> Result<(), HistoryError> {
        let inverse = forward.apply(draft).map_err(HistoryError::Edit)?;
        if self.undo.len() == MAX_HISTORY {
            self.undo.remove(0);
        }
        self.undo.push(Entry { forward, inverse });
        self.redo.clear();
        Ok(())
    }
    pub fn undo(&mut self, draft: &mut ExperimentSpec) -> Result<(), HistoryError> {
        let entry = self.undo.pop().ok_or(HistoryError::NothingToUndo)?;
        let redo = entry.inverse.apply(draft).map_err(HistoryError::Edit)?;
        self.redo.push(Entry {
            forward: redo,
            inverse: entry.inverse,
        });
        Ok(())
    }
    pub fn redo(&mut self, draft: &mut ExperimentSpec) -> Result<(), HistoryError> {
        let entry = self.redo.pop().ok_or(HistoryError::NothingToRedo)?;
        let inverse = entry.forward.apply(draft).map_err(HistoryError::Edit)?;
        self.undo.push(Entry {
            forward: entry.forward,
            inverse,
        });
        Ok(())
    }
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::experiment_model::ChannelId;
    #[test]
    fn paint_roundtrips_through_undo_and_redo() {
        let mut draft = ExperimentSpec::single_channel_lenia(2, 2);
        let before = draft.clone();
        let mut history = History::default();
        history
            .execute(
                &mut draft,
                DraftCommand::SetChannelValue {
                    channel: ChannelId(0),
                    tile: 3,
                    value: 1.0,
                },
            )
            .unwrap();
        history.undo(&mut draft).unwrap();
        assert_eq!(draft, before);
        history.redo(&mut draft).unwrap();
        assert_eq!(draft.channels[0].initial[3], 1.0);
    }
}
