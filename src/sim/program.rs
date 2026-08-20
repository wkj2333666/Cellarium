use std::collections::{BTreeMap, BTreeSet};

use crate::sim::expression::{
    ExpressionContext, ExpressionVariable, KernelExpression, KernelExpressionError, evaluate,
};
use crate::sim::kernel::Kernel;
use crate::sim::parser::{ParseError, validate_symbols};
use crate::sim::world::{ChannelWorld, World};

#[derive(Clone, Debug, PartialEq)]
pub enum InputSource {
    State,
    ChannelState { channel: usize },
    Convolution { kernel: Kernel },
    ChannelConvolution { channel: usize, kernel: Kernel },
}

#[derive(Clone, Debug, PartialEq)]
pub struct RuleInput {
    pub name: String,
    pub source: InputSource,
}

impl RuleInput {
    pub fn state(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            source: InputSource::State,
        }
    }

    pub fn convolution(name: impl Into<String>, kernel: Kernel) -> Self {
        Self {
            name: name.into(),
            source: InputSource::Convolution { kernel },
        }
    }

    pub fn channel_state(name: impl Into<String>, channel: usize) -> Self {
        Self {
            name: name.into(),
            source: InputSource::ChannelState { channel },
        }
    }

    pub fn channel_convolution(name: impl Into<String>, channel: usize, kernel: Kernel) -> Self {
        Self {
            name: name.into(),
            source: InputSource::ChannelConvolution { channel, kernel },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RuleProgram {
    pub inputs: Vec<RuleInput>,
    pub parameters: BTreeMap<String, f32>,
    pub update: KernelExpression,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuleProgramError {
    #[error("input name {0} is duplicated")]
    DuplicateInput(String),
    #[error("input or parameter name {0} is reserved")]
    ReservedSymbol(String),
    #[error("input or parameter name {0} is not a valid identifier")]
    InvalidSymbol(String),
    #[error("input and parameter share the symbol {0}")]
    ConflictingSymbol(String),
    #[error("unknown symbol {0} in update expression")]
    UnknownSymbol(String),
    #[error("geometry variable {0} is unavailable in a rule program")]
    GeometryVariable(&'static str),
    #[error("rule parameter {0} is not finite")]
    NonFiniteParameter(String),
    #[error(transparent)]
    Parse(#[from] ParseError),
}

impl RuleProgram {
    pub fn new(
        inputs: Vec<RuleInput>,
        parameters: BTreeMap<String, f32>,
        update: KernelExpression,
    ) -> Result<Self, RuleProgramError> {
        let mut symbols = BTreeSet::new();
        for input in &inputs {
            validate_name(&input.name)?;
            if !symbols.insert(input.name.clone()) {
                return Err(RuleProgramError::DuplicateInput(input.name.clone()));
            }
        }
        for (name, value) in &parameters {
            validate_name(name)?;
            if !value.is_finite() {
                return Err(RuleProgramError::NonFiniteParameter(name.clone()));
            }
            if !symbols.insert(name.clone()) {
                return Err(RuleProgramError::ConflictingSymbol(name.clone()));
            }
        }
        validate_expression(&update, &symbols)?;

        Ok(Self {
            inputs,
            parameters,
            update,
        })
    }

    pub fn primary_kernel(&self) -> Option<&Kernel> {
        self.inputs.iter().find_map(|input| match &input.source {
            InputSource::State | InputSource::ChannelState { .. } => None,
            InputSource::Convolution { kernel }
            | InputSource::ChannelConvolution { kernel, .. } => Some(kernel),
        })
    }

    pub fn evaluate(
        &self,
        world: &World,
        x: isize,
        y: isize,
    ) -> Result<f32, KernelExpressionError> {
        let mut values = self.parameters.clone();
        self.populate_inputs(world, x, y, &mut values);
        evaluate(
            &self.update,
            &ExpressionContext {
                x: 0.0,
                y: 0.0,
                radius: 0.0,
                distance: 0.0,
                parameters: &values,
            },
        )
    }

    pub fn evaluate_channels(
        &self,
        world: &ChannelWorld,
        x: isize,
        y: isize,
    ) -> Result<f32, KernelExpressionError> {
        let mut values = self.parameters.clone();
        self.populate_channel_inputs(world, x, y, &mut values);
        evaluate(
            &self.update,
            &ExpressionContext {
                x: 0.0,
                y: 0.0,
                radius: 0.0,
                distance: 0.0,
                parameters: &values,
            },
        )
    }

    pub(crate) fn populate_inputs(
        &self,
        world: &World,
        x: isize,
        y: isize,
        values: &mut BTreeMap<String, f32>,
    ) {
        values.clear();
        values.extend(
            self.parameters
                .iter()
                .map(|(name, value)| (name.clone(), *value)),
        );
        for input in &self.inputs {
            let value = match &input.source {
                InputSource::State => world.get(x, y),
                InputSource::ChannelState { .. } => 0.0,
                InputSource::Convolution { kernel } => convolve(world, x, y, kernel),
                InputSource::ChannelConvolution { kernel, .. } => convolve(world, x, y, kernel),
            };
            values.insert(input.name.clone(), value);
        }
    }
    pub(crate) fn populate_channel_inputs(
        &self,
        world: &ChannelWorld,
        x: isize,
        y: isize,
        values: &mut BTreeMap<String, f32>,
    ) {
        values.clear();
        values.extend(
            self.parameters
                .iter()
                .map(|(name, value)| (name.clone(), *value)),
        );
        for input in &self.inputs {
            let value = match &input.source {
                InputSource::State => world.get(0, x, y),
                InputSource::ChannelState { channel } => world.get(*channel, x, y),
                InputSource::Convolution { kernel } => convolve_channel(world, 0, x, y, kernel),
                InputSource::ChannelConvolution { channel, kernel } => {
                    convolve_channel(world, *channel, x, y, kernel)
                }
            };
            values.insert(input.name.clone(), value);
        }
    }
}

fn validate_name(name: &str) -> Result<(), RuleProgramError> {
    if matches!(name, "x" | "y" | "radius" | "distance" | "r") {
        return Err(RuleProgramError::ReservedSymbol(name.to_string()));
    }
    let mut characters = name.chars();
    let valid_start = characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_');
    let valid_rest =
        characters.all(|character| character.is_ascii_alphanumeric() || character == '_');
    if !valid_start || !valid_rest {
        return Err(RuleProgramError::InvalidSymbol(name.to_string()));
    }
    Ok(())
}

fn validate_expression(
    expression: &KernelExpression,
    symbols: &BTreeSet<String>,
) -> Result<(), RuleProgramError> {
    match expression {
        KernelExpression::Variable(variable) => {
            let name = match variable {
                ExpressionVariable::X => "x",
                ExpressionVariable::Y => "y",
                ExpressionVariable::Radius => "radius",
                ExpressionVariable::Distance => "distance",
            };
            Err(RuleProgramError::GeometryVariable(name))
        }
        KernelExpression::Parameter(_name) => {
            validate_symbols(expression, symbols).map_err(|error| match error {
                ParseError::UnknownParameter { name, .. } => RuleProgramError::UnknownSymbol(name),
                other => RuleProgramError::Parse(other),
            })
        }
        KernelExpression::Constant(_) => Ok(()),
        KernelExpression::Binary { lhs, rhs, .. } => {
            validate_expression(lhs, symbols)?;
            validate_expression(rhs, symbols)
        }
        KernelExpression::Unary { operand, .. } => validate_expression(operand, symbols),
        KernelExpression::Call { arguments, .. } => arguments
            .iter()
            .try_for_each(|argument| validate_expression(argument, symbols)),
    }
}

fn convolve(world: &World, x: isize, y: isize, kernel: &Kernel) -> f32 {
    let mut value = 0.0;
    for kernel_y in 0..kernel.height {
        for kernel_x in 0..kernel.width {
            let index = kernel_y * kernel.width + kernel_x;
            if kernel.mask.as_ref().is_some_and(|mask| !mask[index]) {
                continue;
            }
            let offset_x = kernel_x as isize - kernel.anchor_x as isize;
            let offset_y = kernel_y as isize - kernel.anchor_y as isize;
            value += kernel.values[index] * world.get(x + offset_x, y + offset_y);
        }
    }
    value
}

fn convolve_channel(
    world: &ChannelWorld,
    channel: usize,
    x: isize,
    y: isize,
    kernel: &Kernel,
) -> f32 {
    let mut value = 0.0;
    for kernel_y in 0..kernel.height {
        for kernel_x in 0..kernel.width {
            let index = kernel_y * kernel.width + kernel_x;
            if kernel.mask.as_ref().is_some_and(|mask| !mask[index]) {
                continue;
            }
            let offset_x = kernel_x as isize - kernel.anchor_x as isize;
            let offset_y = kernel_y as isize - kernel.anchor_y as isize;
            value += kernel.values[index] * world.get(channel, x + offset_x, y + offset_y);
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::sim::expression::KernelExpression;
    use crate::sim::kernel::{KernelDefinition, KernelValues, Normalization};
    use crate::sim::world::World;

    fn identity_kernel(name: &str, value: f32) -> Kernel {
        KernelDefinition {
            name: name.to_string(),
            width: 1,
            height: 1,
            anchor_x: 0,
            anchor_y: 0,
            mask: None,
            normalization: Normalization::None,
            parameters: BTreeMap::new(),
            values: KernelValues::Explicit(vec![value]),
        }
        .build()
        .unwrap()
    }

    #[test]
    fn evaluates_named_state_and_multiple_convolution_inputs() {
        let program = RuleProgram::new(
            vec![
                RuleInput::state("self"),
                RuleInput::convolution("food", identity_kernel("food-kernel", 1.0)),
                RuleInput::convolution("crowd", identity_kernel("crowd-kernel", 2.0)),
            ],
            BTreeMap::from([("gain".to_string(), 0.5)]),
            crate::sim::parser::parse_expression("(self + food + crowd) * gain").unwrap(),
        )
        .unwrap();
        let mut world = World::new(3, 2);
        world.replace_cells(&[0.2, 0.4, 0.6, 0.8, 0.1, 0.3]);

        let value = program.evaluate(&world, 1, 0).unwrap();

        assert!((value - 0.8).abs() < 1e-6);
    }

    #[test]
    fn evaluates_cross_channel_inputs() {
        let program = RuleProgram::new(
            vec![
                RuleInput::channel_state("signal", 1),
                RuleInput::channel_convolution("neighbor", 1, identity_kernel("identity", 1.0)),
            ],
            BTreeMap::new(),
            crate::sim::parser::parse_expression("signal + neighbor").unwrap(),
        )
        .unwrap();
        let mut world = ChannelWorld::new(2, 1, 2);
        world.set(1, 0, 0, 0.25);

        assert!((program.evaluate_channels(&world, 0, 0).unwrap() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn rejects_duplicate_or_unknown_program_symbols() {
        let duplicate = RuleProgram::new(
            vec![RuleInput::state("self"), RuleInput::state("self")],
            BTreeMap::new(),
            KernelExpression::Constant(0.0),
        )
        .unwrap_err();
        assert!(matches!(duplicate, RuleProgramError::DuplicateInput(_)));

        let unknown = RuleProgram::new(
            vec![RuleInput::state("self")],
            BTreeMap::new(),
            KernelExpression::Parameter("missing".to_string()),
        )
        .unwrap_err();
        assert!(matches!(unknown, RuleProgramError::UnknownSymbol(_)));
    }

    #[test]
    fn rejects_geometry_variables_and_non_finite_parameters() {
        let geometry = RuleProgram::new(
            vec![RuleInput::state("self")],
            BTreeMap::new(),
            KernelExpression::Variable(crate::sim::expression::ExpressionVariable::X),
        )
        .unwrap_err();
        assert!(matches!(geometry, RuleProgramError::GeometryVariable(_)));

        let parameter = RuleProgram::new(
            vec![RuleInput::state("self")],
            BTreeMap::from([("gain".to_string(), f32::NAN)]),
            KernelExpression::Parameter("gain".to_string()),
        )
        .unwrap_err();
        assert!(matches!(parameter, RuleProgramError::NonFiniteParameter(_)));
    }
}
