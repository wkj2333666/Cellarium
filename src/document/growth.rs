//! Pure experiment transforms for the growth program of a binding.
//!
//! The growth source, its signature and the kernels it may read are three
//! views of one thing. Keeping them here means the Workbench, the editor and
//! the plot all read the same answer rather than each deriving its own.

use crate::sim::experiment_model::{ExperimentSpec, KernelId, UpdateMode};
use crate::sim::growth::typecheck;
use crate::sim::growth::types::ExternalSymbols;
use crate::sim::ruleset::BindingKey;

/// The signature a growth program is compiled against.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GrowthSignature {
    /// Kernel symbols the program may read, in declaration order.
    pub kernel_inputs: Vec<String>,
    /// Kernel ids parallel to `kernel_inputs`.
    pub kernel_ids: Vec<KernelId>,
    /// Named constants the program may read.
    pub parameters: Vec<String>,
}

impl GrowthSignature {
    pub fn externals(&self) -> ExternalSymbols {
        ExternalSymbols {
            kernel_inputs: self.kernel_inputs.clone(),
            parameters: self.parameters.clone(),
        }
    }

    /// How the signature reads in the editor, e.g. `f(k0, k1, self)`.
    pub fn rendered(&self) -> String {
        let mut names = self.kernel_inputs.clone();
        names.push("self".into());
        names.extend(self.parameters.iter().cloned());
        format!("f({})", names.join(", "))
    }

    pub fn kernel_id_of(&self, symbol: &str) -> Option<KernelId> {
        self.kernel_inputs
            .iter()
            .position(|name| name == symbol)
            .and_then(|index| self.kernel_ids.get(index).copied())
    }
}

/// One diagnostic from compiling a growth program, in source coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct GrowthDiagnostic {
    pub code: String,
    pub start: usize,
    pub end: usize,
}

/// The growth source of a binding.
pub fn source_of(spec: &ExperimentSpec, binding: BindingKey) -> Option<String> {
    if let Some(rule_set) = crate::document::kernels::rule_set_for(spec, binding)
        && let Some(rule) = spec.rules.get(rule_set)
    {
        return Some(rule.growth.source.clone());
    }
    spec.growth
        .iter()
        .find(|growth| growth.target == binding.output)
        .map(|growth| growth.source.clone())
}

/// The update mode of a binding.
pub fn mode_of(spec: &ExperimentSpec, binding: BindingKey) -> Option<UpdateMode> {
    if let Some(rule_set) = crate::document::kernels::rule_set_for(spec, binding)
        && let Some(rule) = spec.rules.get(rule_set)
    {
        return Some(rule.growth.mode);
    }
    spec.growth
        .iter()
        .find(|growth| growth.target == binding.output)
        .map(|growth| growth.mode)
}

/// The signature the binding's program is compiled against.
///
/// The kernel inputs come from the binding's own kernels, so adding a kernel
/// widens the signature immediately: an editor that showed a stale arity would
/// reject source the model would have accepted.
pub fn signature_of(spec: &ExperimentSpec, binding: BindingKey) -> GrowthSignature {
    if let Some(rule_set) = crate::document::kernels::rule_set_for(spec, binding)
        && let Some(rule) = spec.rules.get(rule_set)
    {
        let mut kernel_inputs = Vec::new();
        let mut kernel_ids = Vec::new();
        for id in &rule.growth.kernel_inputs {
            if let Some(kernel) = rule.kernels.iter().find(|kernel| kernel.id == *id) {
                kernel_inputs.push(kernel.symbol.clone());
                kernel_ids.push(kernel.id);
            }
        }
        return GrowthSignature {
            kernel_inputs,
            kernel_ids,
            parameters: rule.growth.parameters.keys().cloned().collect(),
        };
    }

    let Some(growth) = spec
        .growth
        .iter()
        .find(|growth| growth.target == binding.output)
    else {
        return GrowthSignature::default();
    };
    let mut kernel_inputs = Vec::new();
    let mut kernel_ids = Vec::new();
    for id in &growth.kernel_inputs {
        if let Some(kernel) = spec.kernels.iter().find(|kernel| kernel.id == *id) {
            kernel_inputs.push(kernel.symbol.clone());
            kernel_ids.push(kernel.id);
        }
    }
    GrowthSignature {
        kernel_inputs,
        kernel_ids,
        parameters: growth.parameters.keys().cloned().collect(),
    }
}

/// Compile the binding's program, returning the symbols it truly reads.
pub fn analyze(
    spec: &ExperimentSpec,
    binding: BindingKey,
) -> Result<Vec<String>, Vec<GrowthDiagnostic>> {
    let signature = signature_of(spec, binding);
    let source = source_of(spec, binding).unwrap_or_default();
    typecheck::compile(&source, &signature.externals())
        .map(|program| program.referenced_kernel_inputs())
        .map_err(|diagnostics| {
            diagnostics
                .into_iter()
                .map(|diagnostic| GrowthDiagnostic {
                    code: diagnostic.code.to_string(),
                    start: diagnostic.span.start,
                    end: diagnostic.span.end,
                })
                .collect()
        })
}

/// Replace the growth source of a binding.
///
/// The source is stored even when it does not compile: an editor that refused
/// to hold invalid text would delete the user's work in progress the moment it
/// became temporarily unbalanced. Validity is reported, not enforced here.
pub fn set_source(
    spec: &ExperimentSpec,
    binding: BindingKey,
    source: &str,
) -> Result<ExperimentSpec, String> {
    let mut next = spec.clone();
    if !next.rules.is_empty() {
        let rule_set = next
            .rules
            .detach(binding)
            .map_err(|error| error.to_string())?;
        let rule = next
            .rules
            .get_mut(rule_set)
            .ok_or("the selected rule-set is missing")?;
        rule.growth.source = source.to_string();
        return Ok(next);
    }
    let growth = next
        .growth
        .iter_mut()
        .find(|growth| growth.target == binding.output)
        .ok_or("this channel has no growth program")?;
    growth.source = source.to_string();
    Ok(next)
}

/// Switch a binding between producing a rate and producing a value.
pub fn set_mode(
    spec: &ExperimentSpec,
    binding: BindingKey,
    mode: UpdateMode,
) -> Result<ExperimentSpec, String> {
    let mut next = spec.clone();
    if !next.rules.is_empty() {
        let rule_set = next
            .rules
            .detach(binding)
            .map_err(|error| error.to_string())?;
        let rule = next
            .rules
            .get_mut(rule_set)
            .ok_or("the selected rule-set is missing")?;
        rule.growth.mode = mode;
        rule.validate().map_err(|error| error.to_string())?;
        return Ok(next);
    }
    let growth = next
        .growth
        .iter_mut()
        .find(|growth| growth.target == binding.output)
        .ok_or("this channel has no growth program")?;
    growth.mode = mode;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::kernels;
    use crate::sim::tiling::BasisId;

    fn spec() -> ExperimentSpec {
        ExperimentSpec::single_channel_lenia(8, 8)
            .normalize_rules()
            .expect("the fixture normalizes")
    }

    fn binding(spec: &ExperimentSpec) -> BindingKey {
        BindingKey {
            basis: spec.basis_ids().first().copied().unwrap_or(BasisId(0)),
            output: spec.channels[0].id,
        }
    }

    #[test]
    fn the_signature_lists_the_bindings_own_kernels() {
        let spec = spec();
        let key = binding(&spec);
        let signature = signature_of(&spec, key);
        let cards = kernels::binding_kernels(&spec, key, None);
        assert_eq!(signature.kernel_inputs.len(), cards.len());
        for card in &cards {
            assert!(signature.kernel_inputs.contains(&card.symbol));
            assert_eq!(signature.kernel_id_of(&card.symbol), Some(card.id));
        }
    }

    #[test]
    fn adding_a_kernel_widens_the_signature_immediately() {
        let spec = spec();
        let key = binding(&spec);
        let before = signature_of(&spec, key).kernel_inputs.len();
        let (next, _) = kernels::add_kernel(&spec, key).unwrap();
        assert_eq!(signature_of(&next, key).kernel_inputs.len(), before + 1);
    }

    #[test]
    fn the_rendered_signature_always_offers_self() {
        let spec = spec();
        let rendered = signature_of(&spec, binding(&spec)).rendered();
        assert!(rendered.starts_with("f("), "{rendered}");
        assert!(rendered.contains("self"), "{rendered}");
    }

    #[test]
    fn analysis_reports_only_the_kernels_the_program_actually_reads() {
        let spec = spec();
        let key = binding(&spec);
        let (mut spec, _) = kernels::add_kernel(&spec, key).unwrap();
        let signature = signature_of(&spec, key);
        assert!(signature.kernel_inputs.len() >= 2);
        let first = signature.kernel_inputs[0].clone();

        spec = set_source(&spec, key, &format!("{first} * 2.0")).unwrap();
        let referenced = analyze(&spec, key).unwrap();
        assert_eq!(
            referenced,
            vec![first],
            "a declared but unused kernel is not a referenced one"
        );
    }

    #[test]
    fn a_program_that_reads_nothing_reports_no_referenced_kernels() {
        let spec = spec();
        let key = binding(&spec);
        let spec = set_source(&spec, key, "0.5").unwrap();
        assert_eq!(analyze(&spec, key).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn invalid_source_is_kept_and_reported_rather_than_discarded() {
        let spec = spec();
        let key = binding(&spec);
        let spec = set_source(&spec, key, "gauss(").unwrap();
        assert_eq!(
            source_of(&spec, key).as_deref(),
            Some("gauss("),
            "work in progress must survive being temporarily unbalanced"
        );
        let diagnostics = analyze(&spec, key).unwrap_err();
        assert!(!diagnostics.is_empty());
        assert!(!diagnostics[0].code.is_empty());
    }

    #[test]
    fn the_update_mode_can_be_read_and_changed() {
        let spec = spec();
        let key = binding(&spec);
        let before = mode_of(&spec, key).expect("the fixture has a growth program");
        let other = match before {
            UpdateMode::GrowthRate => UpdateMode::DirectUpdate,
            UpdateMode::DirectUpdate => UpdateMode::GrowthRate,
        };
        let next = set_mode(&spec, key, other).unwrap();
        assert_eq!(mode_of(&next, key), Some(other));
    }
}
