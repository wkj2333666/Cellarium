use crate::sim::experiment_model::ExperimentSpec;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const DRAFT_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DraftEnvelope {
    pub format_version: u32,
    pub base_revision: u64,
    pub draft: ExperimentSpec,
}

pub fn encode_draft(base_revision: u64, draft: &ExperimentSpec) -> Result<String, String> {
    ron::ser::to_string_pretty(
        &DraftEnvelope {
            format_version: DRAFT_FORMAT_VERSION,
            base_revision,
            draft: draft.clone(),
        },
        ron::ser::PrettyConfig::default(),
    )
    .map_err(|error| error.to_string())
}

pub fn decode_draft(source: &str) -> Result<DraftEnvelope, String> {
    let envelope: DraftEnvelope = ron::from_str(source).map_err(|error| error.to_string())?;
    if envelope.format_version != DRAFT_FORMAT_VERSION {
        return Err(format!(
            "unsupported draft format version {}",
            envelope.format_version
        ));
    }
    Ok(envelope)
}

pub fn export_draft(
    path: impl AsRef<Path>,
    base_revision: u64,
    draft: &ExperimentSpec,
) -> Result<(), String> {
    let encoded = encode_draft(base_revision, draft)?;
    std::fs::write(path.as_ref(), encoded).map_err(|error| error.to_string())
}

pub fn load_draft(path: impl AsRef<Path>) -> Result<DraftEnvelope, String> {
    let source = std::fs::read_to_string(path.as_ref()).map_err(|error| error.to_string())?;
    decode_draft(&source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_load_is_rejected_and_invalid_growth_is_recoverable() {
        assert!(decode_draft("not ron").is_err());
        let mut draft = ExperimentSpec::single_channel_lenia(4, 4);
        draft.growth[0].source = "if potential {".into();
        let encoded = encode_draft(7, &draft).unwrap();
        let loaded = decode_draft(&encoded).unwrap();
        assert_eq!(loaded.base_revision, 7);
        assert_eq!(loaded.draft.growth[0].source, "if potential {");
    }
}
