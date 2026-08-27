#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecisionChoice {
    pub id: String,
    pub label: String,
}

impl DecisionChoice {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecisionPanel {
    pub title: String,
    pub detail: String,
    pub choices: Vec<DecisionChoice>,
    selected: usize,
}

impl DecisionPanel {
    pub fn new(
        title: impl Into<String>,
        detail: impl Into<String>,
        choices: Vec<DecisionChoice>,
    ) -> Self {
        assert!(
            !choices.is_empty(),
            "a decision requires at least one choice"
        );
        Self {
            title: title.into(),
            detail: detail.into(),
            choices,
            selected: 0,
        }
    }

    pub fn selected_choice(&self) -> &DecisionChoice {
        &self.choices[self.selected]
    }

    pub fn select_next(&mut self) {
        self.selected = (self.selected + 1) % self.choices.len();
    }

    pub fn choose(&self, id: &str) -> Result<&DecisionChoice, String> {
        self.choices
            .iter()
            .find(|choice| choice.id == id)
            .ok_or_else(|| format!("unknown decision choice `{id}`"))
    }
}

#[cfg(test)]
mod tests {
    use super::{DecisionChoice, DecisionPanel};

    #[test]
    fn decision_panel_keeps_error_context_until_explicit_choice_or_cancel() {
        let mut panel = DecisionPanel::new(
            "Kernel is referenced",
            "Growth reads k1",
            vec![
                DecisionChoice::new("replace-zero-remove", "Replace k1 with 0 and remove"),
                DecisionChoice::new("cancel", "Cancel"),
            ],
        );

        panel.select_next();
        assert_eq!(panel.selected_choice().id, "cancel");
        assert_eq!(
            panel.choose("replace-zero-remove").unwrap().id,
            "replace-zero-remove"
        );
        assert_eq!(panel.detail, "Growth reads k1");
        assert!(panel.choose("missing").is_err());
    }
}
