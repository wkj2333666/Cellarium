use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::sim::expression::KernelExpression;
use crate::sim::kernel::{KernelDefinition, KernelValues, Normalization, ring_definition};
use crate::sim::parser::{ParseError, parse_and_validate};
use crate::sim::program::{RuleProgram, RuleProgramError};

pub use crate::sim::kernel::Kernel;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Rule {
    Conway,
    Lenia { mu: f32, sigma: f32 },
    Program(RuleProgram),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimulationSpec {
    pub rule: Rule,
    pub kernel: Kernel,
    pub dt: f32,
    pub(crate) growth: Option<KernelExpression>,
}

impl SimulationSpec {
    pub fn conway() -> Self {
        Self {
            rule: Rule::Conway,
            kernel: empty_kernel(),
            dt: 1.0,
            growth: None,
        }
    }

    pub fn lenia_orbium() -> Self {
        Self {
            rule: Rule::Lenia {
                mu: 0.135,
                sigma: 0.015,
            },
            kernel: ring_definition(13, 0.5, 0.5)
                .build()
                .expect("the built-in ring kernel is valid"),
            dt: 0.1,
            growth: Some(default_growth_expression()),
        }
    }

    pub fn growth_expression(&self) -> Option<&KernelExpression> {
        self.growth.as_ref()
    }

    pub fn with_growth_expression(mut self, source: &str) -> Result<Self, RuleConfigError> {
        self.set_growth_expression(source)?;
        Ok(self)
    }

    pub fn set_growth_expression(&mut self, source: &str) -> Result<(), RuleConfigError> {
        if !matches!(self.rule, Rule::Lenia { .. }) {
            return Err(RuleConfigError::GrowthUnsupported);
        }
        self.growth = Some(parse_and_validate(source, &growth_symbols())?);
        Ok(())
    }

    pub fn custom_program(program: RuleProgram, dt: f32) -> Self {
        let kernel = program
            .primary_kernel()
            .cloned()
            .unwrap_or_else(empty_kernel);
        Self {
            rule: Rule::Program(program),
            kernel,
            dt,
            growth: None,
        }
    }
}

fn growth_symbols() -> BTreeSet<String> {
    ["mu", "potential", "sigma"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn default_growth_expression() -> KernelExpression {
    parse_and_validate(
        "2 * exp(-((potential - mu) / sigma) ^ 2) - 1",
        &growth_symbols(),
    )
    .expect("the built-in growth expression is valid")
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuleConfigError {
    #[error("growth expressions are only supported by continuous rules")]
    GrowthUnsupported,
    #[error(transparent)]
    Parse(#[from] ParseError),
    #[error(transparent)]
    Program(#[from] RuleProgramError),
}

fn empty_kernel() -> Kernel {
    KernelDefinition {
        name: "none".to_string(),
        width: 1,
        height: 1,
        anchor_x: 0,
        anchor_y: 0,
        mask: Some(vec![false]),
        normalization: Normalization::None,
        parameters: Default::default(),
        values: KernelValues::Explicit(vec![0.0]),
    }
    .build()
    .expect("the empty kernel is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lenia_ring_kernel_is_finite_and_normalized() {
        let spec = SimulationSpec::lenia_orbium();
        assert!(spec.kernel.values.iter().all(|value| value.is_finite()));
        assert!((spec.kernel.values.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert_eq!(spec.kernel.radius(), 12);
        assert_eq!(spec.kernel.values.len(), 27 * 27);
    }

    #[test]
    fn conway_rule_has_no_convolution_kernel() {
        let spec = SimulationSpec::conway();
        assert_eq!(spec.rule, Rule::Conway);
        assert_eq!(spec.kernel.name, "none");
        assert_eq!(spec.kernel.mask, Some(vec![false]));
        assert_eq!(spec.kernel.values, [0.0]);
        assert_eq!(spec.kernel.radius(), 0);
        assert_eq!(spec.dt, 1.0);
    }

    #[test]
    fn lenia_parameters_stay_in_a_stable_range() {
        let spec = SimulationSpec::lenia_orbium();
        assert_eq!(
            spec.rule,
            Rule::Lenia {
                mu: 0.135,
                sigma: 0.015
            }
        );
        assert_eq!(spec.dt, 0.1);
    }

    #[test]
    fn growth_expression_can_be_replaced_without_rebuilding_the_program() {
        let spec = SimulationSpec::lenia_orbium()
            .with_growth_expression("clamp(potential - mu, -1, 1) / sigma")
            .unwrap();

        assert_eq!(
            crate::sim::parser::format_expression(spec.growth_expression().unwrap()),
            "clamp(potential - mu, -1, 1) / sigma"
        );
        assert!(
            SimulationSpec::lenia_orbium()
                .with_growth_expression("unknown + potential")
                .is_err()
        );
    }
}
