#[derive(Clone, Debug, PartialEq)]
pub struct NumericEditor {
    label: String,
    original: f64,
    buffer: String,
    range: std::ops::RangeInclusive<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NumericError {
    InvalidNumber,
    NonFinite,
    OutOfRange { min: f64, max: f64 },
}

impl NumericEditor {
    pub fn begin(
        label: impl Into<String>,
        original: f64,
        range: std::ops::RangeInclusive<f64>,
    ) -> Self {
        Self {
            label: label.into(),
            original,
            buffer: format!("{original}"),
            range,
        }
    }

    pub fn label(&self) -> &str {
        &self.label
    }
    pub fn buffer(&self) -> &str {
        &self.buffer
    }
    pub fn original(&self) -> f64 {
        self.original
    }

    pub fn replace(&mut self, source: impl Into<String>) {
        self.buffer = source.into();
    }
    pub fn push(&mut self, character: char) {
        self.buffer.push(character);
    }
    pub fn backspace(&mut self) {
        self.buffer.pop();
    }

    pub fn commit(&self) -> Result<f64, NumericError> {
        let value = self
            .buffer
            .parse::<f64>()
            .map_err(|_| NumericError::InvalidNumber)?;
        if !value.is_finite() {
            return Err(NumericError::NonFinite);
        }
        if !self.range.contains(&value) {
            return Err(NumericError::OutOfRange {
                min: *self.range.start(),
                max: *self.range.end(),
            });
        }
        Ok(value)
    }

    pub fn cancel(self) -> f64 {
        self.original
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_decimal_commit_and_cancel_are_transactional() {
        let mut editor = NumericEditor::begin("weight", 0.25, -2.0..=2.0);
        editor.replace("-0.1375");
        assert_eq!(editor.commit(), Ok(-0.1375));
        assert_eq!(editor.cancel(), 0.25);
    }

    #[test]
    fn non_finite_and_out_of_range_values_are_rejected() {
        let mut editor = NumericEditor::begin("weight", 0.0, -1.0..=1.0);
        editor.replace("NaN");
        assert_eq!(editor.commit(), Err(NumericError::NonFinite));
        editor.replace("2.0");
        assert_eq!(
            editor.commit(),
            Err(NumericError::OutOfRange {
                min: -1.0,
                max: 1.0
            })
        );
    }
}
