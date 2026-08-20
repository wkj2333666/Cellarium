use crate::sim::kernel::{KernelDefinition, KernelValues, Normalization, ring_definition};

pub use crate::sim::kernel::Kernel;

#[derive(Clone, Debug, PartialEq)]
pub enum Rule {
    Conway,
    Lenia { mu: f32, sigma: f32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct SimulationSpec {
    pub rule: Rule,
    pub kernel: Kernel,
    pub dt: f32,
}

impl SimulationSpec {
    pub fn conway() -> Self {
        Self {
            rule: Rule::Conway,
            kernel: empty_kernel(),
            dt: 1.0,
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
        }
    }
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
}
