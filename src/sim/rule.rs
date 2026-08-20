#[derive(Clone, Debug, PartialEq)]
pub enum Rule {
    Conway,
    Lenia { mu: f32, sigma: f32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Kernel {
    pub radius: usize,
    pub values: Vec<f32>,
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
            kernel: Kernel {
                radius: 0,
                values: Vec::new(),
            },
            dt: 1.0,
        }
    }

    pub fn lenia_orbium() -> Self {
        Self {
            rule: Rule::Lenia {
                mu: 0.135,
                sigma: 0.015,
            },
            kernel: ring_kernel(13),
            dt: 0.1,
        }
    }
}

fn ring_kernel(radius: usize) -> Kernel {
    let diameter = 2 * radius + 1;
    let mut values = Vec::with_capacity(diameter * diameter);
    let radius_f = radius as f32;

    for y in 0..diameter {
        for x in 0..diameter {
            let dx = x as f32 - radius_f;
            let dy = y as f32 - radius_f;
            let distance = (dx * dx + dy * dy).sqrt() / radius_f;
            let value = if (0.0..1.0).contains(&distance) {
                (-4.0 * distance * (1.0 - distance)).exp()
            } else {
                0.0
            };
            values.push(value);
        }
    }

    let sum: f32 = values.iter().sum();
    for value in &mut values {
        *value /= sum;
    }
    Kernel { radius, values }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lenia_ring_kernel_is_finite_and_normalized() {
        let spec = SimulationSpec::lenia_orbium();
        assert!(spec.kernel.values.iter().all(|value| value.is_finite()));
        assert!((spec.kernel.values.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert_eq!(
            spec.kernel.values.len(),
            (2 * spec.kernel.radius + 1).pow(2)
        );
    }

    #[test]
    fn conway_rule_has_no_convolution_kernel() {
        let spec = SimulationSpec::conway();
        assert_eq!(spec.rule, Rule::Conway);
        assert_eq!(spec.kernel.radius, 0);
        assert!(spec.kernel.values.is_empty());
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
