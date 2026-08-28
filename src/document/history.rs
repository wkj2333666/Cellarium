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
    /// Number of transactions that can still be undone.
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// Number of transactions that can still be redone.
    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }

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

    /// Run a command, folding it into the previous entry when `merge` allows.
    ///
    /// Typing produces one command per keystroke. Recorded individually, undoing
    /// a typed expression costs one click per character, which is not an undo a
    /// person can use. Folding keeps the entry's original inverse, so undoing
    /// the merged run returns to before the first keystroke of it.
    ///
    /// `merge` is only consulted when there is a previous entry, and the caller
    /// decides using both that entry's command and its own idea of whether the
    /// two edits belong together.
    pub fn coalesce_execute(
        &mut self,
        draft: &mut ExperimentSpec,
        forward: DraftCommand,
        merge: impl FnOnce(&DraftCommand) -> bool,
    ) -> Result<(), HistoryError> {
        let inverse = forward.apply(draft).map_err(HistoryError::Edit)?;
        match self.undo.last_mut() {
            Some(entry) if merge(&entry.forward) => entry.forward = forward,
            _ => {
                if self.undo.len() == MAX_HISTORY {
                    self.undo.remove(0);
                }
                self.undo.push(Entry { forward, inverse });
            }
        }
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

    #[test]
    fn coalesced_replace_commands_undo_as_one_pointer_drag() {
        let mut draft = ExperimentSpec::single_channel_lenia(2, 2);
        let before = draft.clone();
        let mut history = History::default();
        let mut first = draft.clone();
        first.channels[0].initial[0] = 0.25;
        history
            .execute(&mut draft, DraftCommand::ReplaceDraft(Box::new(first)))
            .unwrap();
        let mut last = draft.clone();
        last.channels[0].initial[0] = 0.75;
        history
            .coalesce_execute(
                &mut draft,
                DraftCommand::ReplaceDraft(Box::new(last.clone())),
                |previous| matches!(previous, DraftCommand::ReplaceDraft(_)),
            )
            .unwrap();

        history.undo(&mut draft).unwrap();
        assert_eq!(draft, before);
        history.redo(&mut draft).unwrap();
        assert_eq!(draft, last);
    }

    #[test]
    fn a_refused_merge_keeps_the_two_edits_separate() {
        let mut draft = ExperimentSpec::single_channel_lenia(2, 2);
        let mut history = History::default();
        let mut first = draft.clone();
        first.channels[0].initial[0] = 0.25;
        history
            .execute(&mut draft, DraftCommand::ReplaceDraft(Box::new(first)))
            .unwrap();
        let mut last = draft.clone();
        last.channels[0].initial[0] = 0.75;
        history
            .coalesce_execute(
                &mut draft,
                DraftCommand::ReplaceDraft(Box::new(last)),
                |_| false,
            )
            .unwrap();
        assert_eq!(
            history.undo_depth(),
            2,
            "a caller that declines the merge gets two undoable edits"
        );
    }
}
