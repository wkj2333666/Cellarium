use crate::sim::expression::{
    BinaryOp, ExpressionContext, KernelExpression, KernelExpressionError, evaluate,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ops::Deref;

const MAX_KERNEL_AXIS: usize = 129;
const MIN_NORMALIZATION_SUM: f32 = 1e-12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Normalization {
    None,
    Sum,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum KernelValues {
    Explicit(Vec<f32>),
    Expression(KernelExpression),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KernelDefinition {
    pub name: String,
    pub width: usize,
    pub height: usize,
    pub anchor_x: usize,
    pub anchor_y: usize,
    pub mask: Option<Vec<bool>>,
    pub normalization: Normalization,
    pub parameters: BTreeMap<String, f32>,
    pub values: KernelValues,
}

impl KernelDefinition {
    pub fn build(&self) -> Result<Kernel, KernelError> {
        self.clone().try_into()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Kernel {
    pub name: String,
    pub width: usize,
    pub height: usize,
    pub anchor_x: usize,
    pub anchor_y: usize,
    pub mask: Option<Vec<bool>>,
    pub normalization: Normalization,
    pub parameters: BTreeMap<String, f32>,
    pub values: Vec<f32>,
    legacy_shape: LegacyKernelShape,
}

impl Kernel {
    pub fn radius(&self) -> usize {
        included_radius(
            self.mask.as_deref(),
            self.width,
            self.height,
            self.anchor_x,
            self.anchor_y,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LegacyKernelShape {
    pub(crate) radius: usize,
}

impl Deref for Kernel {
    type Target = LegacyKernelShape;

    fn deref(&self) -> &Self::Target {
        &self.legacy_shape
    }
}

fn included_radius(
    mask: Option<&[bool]>,
    width: usize,
    height: usize,
    anchor_x: usize,
    anchor_y: usize,
) -> usize {
    mask.map(|mask| {
        mask.iter()
            .enumerate()
            .filter(|(_, active)| **active)
            .map(|(index, _)| {
                let x = index % width;
                let y = index / width;
                (x as isize - anchor_x as isize)
                    .abs()
                    .max((y as isize - anchor_y as isize).abs())
            })
            .max()
            .unwrap_or(0) as usize
    })
    .unwrap_or_else(|| {
        (width - 1 - anchor_x)
            .max(anchor_x)
            .max(height - 1 - anchor_y)
            .max(anchor_y)
    })
}

impl TryFrom<KernelDefinition> for Kernel {
    type Error = KernelError;

    fn try_from(definition: KernelDefinition) -> Result<Self, Self::Error> {
        if definition.width == 0
            || definition.height == 0
            || definition.width > MAX_KERNEL_AXIS
            || definition.height > MAX_KERNEL_AXIS
        {
            return Err(KernelError::InvalidDimensions);
        }
        if definition.anchor_x >= definition.width || definition.anchor_y >= definition.height {
            return Err(KernelError::InvalidAnchor);
        }

        let cell_count = definition.width * definition.height;
        if let Some(mask) = &definition.mask {
            if mask.len() != cell_count {
                return Err(KernelError::InvalidMaskLength);
            }
        }
        if definition
            .parameters
            .values()
            .any(|value| !value.is_finite())
        {
            return Err(KernelError::NonFiniteParameter);
        }

        let included = |index: usize| definition.mask.as_ref().is_none_or(|mask| mask[index]);
        let mut values = vec![0.0; cell_count];
        match &definition.values {
            KernelValues::Explicit(explicit) => {
                if explicit.len() != cell_count {
                    return Err(KernelError::InvalidValuesLength);
                }
                for index in 0..cell_count {
                    if included(index) {
                        if explicit[index].is_finite() {
                            values[index] = explicit[index];
                        } else {
                            return Err(KernelError::NonFiniteValue);
                        }
                    }
                }
            }
            KernelValues::Expression(expression) => {
                for index in 0..cell_count {
                    if !included(index) {
                        continue;
                    }
                    let x = index % definition.width;
                    let y = index / definition.width;
                    let geometry = normalized_geometry(
                        definition.width,
                        definition.height,
                        definition.anchor_x,
                        definition.anchor_y,
                        x,
                        y,
                    );
                    let context = ExpressionContext {
                        x: geometry.x,
                        y: geometry.y,
                        radius: geometry.radius,
                        distance: geometry.distance,
                        parameters: &definition.parameters,
                    };
                    values[index] = evaluate(expression, &context)?;
                }
            }
        }

        if definition.normalization == Normalization::Sum {
            let sum = values.iter().sum::<f32>();
            if !sum.is_finite() || sum.abs() <= MIN_NORMALIZATION_SUM {
                return Err(KernelError::InvalidNormalizationSum);
            }
            for value in &mut values {
                *value /= sum;
            }
        }

        let legacy_shape = LegacyKernelShape {
            radius: included_radius(
                definition.mask.as_deref(),
                definition.width,
                definition.height,
                definition.anchor_x,
                definition.anchor_y,
            ),
        };

        Ok(Self {
            name: definition.name,
            width: definition.width,
            height: definition.height,
            anchor_x: definition.anchor_x,
            anchor_y: definition.anchor_y,
            mask: definition.mask,
            normalization: definition.normalization,
            parameters: definition.parameters,
            values,
            legacy_shape,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    #[error("kernel dimensions must be between 1 and 129 cells")]
    InvalidDimensions,
    #[error("kernel anchor must lie inside the kernel rectangle")]
    InvalidAnchor,
    #[error("kernel mask must contain exactly width * height entries")]
    InvalidMaskLength,
    #[error("explicit kernel values must contain exactly width * height entries")]
    InvalidValuesLength,
    #[error("kernel parameters must be finite")]
    NonFiniteParameter,
    #[error("kernel values must be finite")]
    NonFiniteValue,
    #[error(transparent)]
    Expression(#[from] KernelExpressionError),
    #[error("sum-normalized kernels must have a finite sum larger than 1e-12 in magnitude")]
    InvalidNormalizationSum,
}

impl PartialEq for KernelError {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::InvalidDimensions, Self::InvalidDimensions)
                | (Self::InvalidAnchor, Self::InvalidAnchor)
                | (Self::InvalidMaskLength, Self::InvalidMaskLength)
                | (Self::InvalidValuesLength, Self::InvalidValuesLength)
                | (Self::NonFiniteParameter, Self::NonFiniteParameter)
                | (Self::NonFiniteValue, Self::NonFiniteValue)
                | (Self::Expression(_), Self::Expression(_))
                | (Self::InvalidNormalizationSum, Self::InvalidNormalizationSum)
        )
    }
}

#[derive(Clone, Copy)]
struct NormalizedGeometry {
    x: f32,
    y: f32,
    radius: f32,
    distance: f32,
}

fn normalized_geometry(
    width: usize,
    height: usize,
    anchor_x: usize,
    anchor_y: usize,
    x: usize,
    y: usize,
) -> NormalizedGeometry {
    let x_scale = anchor_x.max(width - 1 - anchor_x).max(1) as f32;
    let y_scale = anchor_y.max(height - 1 - anchor_y).max(1) as f32;
    let x = (x as f32 - anchor_x as f32) / x_scale;
    let y = (y as f32 - anchor_y as f32) / y_scale;
    NormalizedGeometry {
        x,
        y,
        radius: x_scale.max(y_scale),
        distance: (x * x + y * y).sqrt(),
    }
}

pub fn ring_definition(radius: usize, center: f32, width: f32) -> KernelDefinition {
    let diameter = 2 * radius + 1;
    let mut mask = Vec::with_capacity(diameter * diameter);
    for y in 0..diameter {
        for x in 0..diameter {
            let dx = x as f32 - radius as f32;
            let dy = y as f32 - radius as f32;
            let distance = (dx * dx + dy * dy).sqrt() / radius as f32;
            mask.push(distance < 1.0);
        }
    }

    KernelDefinition {
        name: "ring".to_string(),
        width: diameter,
        height: diameter,
        anchor_x: radius,
        anchor_y: radius,
        mask: Some(mask),
        normalization: Normalization::Sum,
        parameters: BTreeMap::from([("center".to_string(), center), ("width".to_string(), width)]),
        values: KernelValues::Expression(KernelExpression::Unary {
            op: crate::sim::expression::UnaryOp::Exp,
            operand: Box::new(KernelExpression::Unary {
                op: crate::sim::expression::UnaryOp::Neg,
                operand: Box::new(KernelExpression::Binary {
                    op: BinaryOp::Power,
                    lhs: Box::new(KernelExpression::Binary {
                        op: BinaryOp::Divide,
                        lhs: Box::new(KernelExpression::Binary {
                            op: BinaryOp::Subtract,
                            lhs: Box::new(KernelExpression::Variable(
                                crate::sim::expression::ExpressionVariable::Distance,
                            )),
                            rhs: Box::new(KernelExpression::Parameter("center".to_string())),
                        }),
                        rhs: Box::new(KernelExpression::Parameter("width".to_string())),
                    }),
                    rhs: Box::new(KernelExpression::Constant(2.0)),
                }),
            }),
        }),
    }
}

pub fn render_definition(width: usize, height: usize) -> KernelDefinition {
    KernelDefinition {
        name: "render".to_string(),
        width,
        height,
        anchor_x: width.saturating_sub(1) / 2,
        anchor_y: height.saturating_sub(1) / 2,
        mask: Some(vec![true; width * height]),
        normalization: Normalization::Sum,
        parameters: BTreeMap::new(),
        values: KernelValues::Explicit(vec![1.0; width * height]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::expression::{BinaryOp, ExpressionVariable, KernelExpression};
    use std::collections::BTreeMap;

    fn parameters(entries: &[(&str, f32)]) -> BTreeMap<String, f32> {
        entries
            .iter()
            .map(|(name, value)| ((*name).to_string(), *value))
            .collect()
    }

    fn explicit_definition() -> KernelDefinition {
        KernelDefinition {
            name: "masked".to_string(),
            width: 3,
            height: 2,
            anchor_x: 2,
            anchor_y: 0,
            mask: Some(vec![true, false, true, true, true, false]),
            normalization: Normalization::Sum,
            parameters: BTreeMap::new(),
            values: KernelValues::Explicit(vec![2.0, f32::NAN, 4.0, 6.0, 3.0, f32::NAN]),
        }
    }

    fn geometry_definition(variable: ExpressionVariable) -> KernelDefinition {
        KernelDefinition {
            name: "geometry".to_string(),
            width: 3,
            height: 2,
            anchor_x: 2,
            anchor_y: 0,
            mask: None,
            normalization: Normalization::None,
            parameters: BTreeMap::new(),
            values: KernelValues::Expression(KernelExpression::Variable(variable)),
        }
    }

    #[test]
    fn kernel_definition_surface_is_serde_derivable() {
        fn assert_serde<T: serde::Serialize + serde::de::DeserializeOwned>() {}

        assert_serde::<KernelDefinition>();
        assert_serde::<KernelValues>();
        assert_serde::<Normalization>();
    }

    #[test]
    fn explicit_non_square_masked_kernel_normalizes_only_active_cells() {
        let kernel = explicit_definition().build().unwrap();

        assert_eq!(kernel.name, "masked");
        assert_eq!((kernel.width, kernel.height), (3, 2));
        assert_eq!((kernel.anchor_x, kernel.anchor_y), (2, 0));
        assert_eq!(
            kernel.mask.as_deref(),
            Some(&[true, false, true, true, true, false][..])
        );
        assert_eq!(kernel.normalization, Normalization::Sum);
        assert_eq!(kernel.radius(), 2);
        let expected = [2.0 / 15.0, 0.0, 4.0 / 15.0, 6.0 / 15.0, 3.0 / 15.0, 0.0];
        assert_eq!(kernel.values, expected);
        assert!((kernel.values.iter().sum::<f32>() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn explicit_values_can_remain_unnormalized_and_masked_values_are_zero() {
        let mut definition = explicit_definition();
        definition.normalization = Normalization::None;
        definition.values = KernelValues::Explicit(vec![2.0, f32::NAN, 4.0, 6.0, 3.0, f32::NAN]);

        let kernel = definition.build().unwrap();

        assert_eq!(kernel.values, [2.0, 0.0, 4.0, 6.0, 3.0, 0.0]);
    }

    #[test]
    fn expressions_receive_normalized_anchor_relative_geometry() {
        let x = geometry_definition(ExpressionVariable::X)
            .build()
            .unwrap()
            .values;
        assert_eq!(x, [-1.0, -0.5, 0.0, -1.0, -0.5, 0.0]);

        let y = geometry_definition(ExpressionVariable::Y)
            .build()
            .unwrap()
            .values;
        assert_eq!(y, [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);

        let distance = geometry_definition(ExpressionVariable::Distance)
            .build()
            .unwrap()
            .values;
        assert_eq!(
            distance,
            [1.0, 0.5, 0.0, 2.0_f32.sqrt(), 1.25_f32.sqrt(), 1.0]
        );

        let radius = geometry_definition(ExpressionVariable::Radius)
            .build()
            .unwrap()
            .values;
        assert_eq!(radius, [2.0; 6]);
    }

    #[test]
    fn expressions_remain_finite_on_one_cell_axes() {
        let definition = KernelDefinition {
            name: "narrow".to_string(),
            width: 1,
            height: 2,
            anchor_x: 0,
            anchor_y: 0,
            mask: None,
            normalization: Normalization::None,
            parameters: BTreeMap::new(),
            values: KernelValues::Expression(KernelExpression::Variable(
                ExpressionVariable::Distance,
            )),
        };

        let kernel = definition.build().unwrap();

        assert_eq!(kernel.values, [0.0, 1.0]);
    }

    #[test]
    fn expressions_use_finite_named_parameters() {
        let definition = KernelDefinition {
            name: "scaled".to_string(),
            width: 2,
            height: 1,
            anchor_x: 0,
            anchor_y: 0,
            mask: None,
            normalization: Normalization::None,
            parameters: parameters(&[("scale", 3.0)]),
            values: KernelValues::Expression(KernelExpression::Binary {
                op: BinaryOp::Multiply,
                lhs: Box::new(KernelExpression::Parameter("scale".to_string())),
                rhs: Box::new(KernelExpression::Constant(2.0)),
            }),
        };

        let kernel = definition.build().unwrap();

        assert_eq!(kernel.parameters, parameters(&[("scale", 3.0)]));
        assert_eq!(kernel.values, [6.0, 6.0]);
    }

    #[test]
    fn large_non_square_asymmetric_kernel_reports_radius_and_full_value_count() {
        let definition = KernelDefinition {
            name: "wide".to_string(),
            width: 33,
            height: 21,
            anchor_x: 16,
            anchor_y: 10,
            mask: None,
            normalization: Normalization::None,
            parameters: BTreeMap::new(),
            values: KernelValues::Expression(KernelExpression::Constant(1.0)),
        };
        let kernel = definition.build().unwrap();

        assert_eq!((kernel.width, kernel.height), (33, 21));
        assert_eq!((kernel.anchor_x, kernel.anchor_y), (16, 10));
        assert_eq!(kernel.radius(), 16);
        assert_eq!(kernel.values.len(), 693);
        assert!(kernel.values.iter().all(|value| *value == 1.0));
    }

    #[test]
    fn ring_preset_uses_parameters_and_a_circular_mask() {
        let definition = ring_definition(13, 0.5, 0.5);
        assert_eq!(definition.name, "ring");
        assert_eq!((definition.width, definition.height), (27, 27));
        assert_eq!((definition.anchor_x, definition.anchor_y), (13, 13));
        assert_eq!(
            definition.parameters,
            parameters(&[("center", 0.5), ("width", 0.5)])
        );
        assert_eq!(definition.normalization, Normalization::Sum);

        let kernel = definition.build().unwrap();
        assert_eq!(kernel.values.len(), 27 * 27);
        assert_eq!(kernel.radius(), 12);
        assert!(kernel.values.iter().all(|value| value.is_finite()));
        assert!((kernel.values.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        assert!(
            kernel
                .mask
                .iter()
                .flatten()
                .filter(|active| **active)
                .count()
                < 27 * 27
        );
        assert_eq!(
            kernel.values[kernel
                .mask
                .iter()
                .flatten()
                .position(|active| !*active)
                .unwrap()],
            0.0
        );
    }

    #[test]
    fn render_preset_is_a_normalized_rectangle() {
        let definition = render_definition(3, 5);
        assert_eq!(definition.name, "render");
        assert_eq!((definition.width, definition.height), (3, 5));
        assert_eq!((definition.anchor_x, definition.anchor_y), (1, 2));
        assert_eq!(definition.normalization, Normalization::Sum);

        let kernel = definition.build().unwrap();
        assert_eq!(kernel.mask, Some(vec![true; 15]));
        assert_eq!(kernel.values, vec![1.0 / 15.0; 15]);
        assert_eq!(kernel.radius(), 2);
    }

    #[test]
    fn render_preset_with_invalid_dimensions_reports_model_validation() {
        let definition = render_definition(0, 0);

        assert_eq!(definition.build(), Err(KernelError::InvalidDimensions));
    }

    #[test]
    fn rejects_invalid_dimensions_anchors_and_lengths() {
        let mut zero_width = explicit_definition();
        zero_width.width = 0;
        assert_eq!(zero_width.build(), Err(KernelError::InvalidDimensions));

        let mut too_wide = explicit_definition();
        too_wide.width = 130;
        too_wide.mask = None;
        too_wide.values = KernelValues::Explicit(Vec::new());
        assert_eq!(too_wide.build(), Err(KernelError::InvalidDimensions));

        let mut anchor = explicit_definition();
        anchor.anchor_x = 3;
        assert_eq!(anchor.build(), Err(KernelError::InvalidAnchor));

        let mut mask = explicit_definition();
        mask.mask = Some(vec![true; 5]);
        assert_eq!(mask.build(), Err(KernelError::InvalidMaskLength));

        let mut values = explicit_definition();
        values.values = KernelValues::Explicit(vec![1.0; 5]);
        assert_eq!(values.build(), Err(KernelError::InvalidValuesLength));
    }

    #[test]
    fn rejects_non_finite_parameters_values_and_expression_results() {
        let mut parameter = explicit_definition();
        parameter.parameters = parameters(&[("bad", f32::NAN)]);
        assert_eq!(parameter.build(), Err(KernelError::NonFiniteParameter));

        let mut explicit = explicit_definition();
        explicit.values = KernelValues::Explicit(vec![f32::NAN, 0.0, 0.0, 0.0, 0.0, 0.0]);
        explicit.mask = None;
        assert_eq!(explicit.build(), Err(KernelError::NonFiniteValue));

        let mut expression = explicit_definition();
        expression.mask = None;
        expression.values = KernelValues::Expression(KernelExpression::Binary {
            op: BinaryOp::Multiply,
            lhs: Box::new(KernelExpression::Constant(f32::MAX)),
            rhs: Box::new(KernelExpression::Constant(f32::MAX)),
        });
        assert!(matches!(
            expression.build(),
            Err(KernelError::Expression(_))
        ));
    }

    #[test]
    fn rejects_sums_that_are_non_finite_or_too_close_to_zero() {
        let mut zero = explicit_definition();
        zero.values = KernelValues::Explicit(vec![0.0; 6]);
        zero.mask = None;
        assert_eq!(zero.build(), Err(KernelError::InvalidNormalizationSum));

        let mut tiny = explicit_definition();
        tiny.values = KernelValues::Explicit(vec![1e-13; 6]);
        tiny.mask = None;
        assert_eq!(tiny.build(), Err(KernelError::InvalidNormalizationSum));
    }
}
