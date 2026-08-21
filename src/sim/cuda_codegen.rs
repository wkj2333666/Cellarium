use std::collections::BTreeSet;

use crate::sim::expression::{BinaryOp, ExpressionVariable, Function, KernelExpression, UnaryOp};
use crate::sim::program::{InputSource, RuleProgram};

const MAX_CODEGEN_DEPTH: usize = 256;
const ENTRY_POINT: &str = "cellarium_step";
const GROWTH_MARKER: &str = "/*__CELLARIUM_GROWTH__*/";

#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedCudaSource {
    pub source: String,
    pub entry_point: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CodegenError {
    #[error("unknown growth symbol `{0}`")]
    UnknownSymbol(String),
    #[error("geometry variable `{0}` is unavailable in a growth expression")]
    UnsupportedVariable(&'static str),
    #[error("expression exceeds the maximum code-generation depth")]
    TooDeep,
    #[error("expression contains a non-finite constant")]
    NonFiniteConstant,
    #[error("built-in function has an invalid argument count")]
    InvalidFunctionArity,
    #[error("program kernels exceed the CUDA metadata range")]
    InvalidKernelLayout,
}

pub fn generate_cuda_source(
    expression: &KernelExpression,
) -> Result<GeneratedCudaSource, CodegenError> {
    let expression = generate_expression(expression, 0, &growth_symbols())?;
    Ok(GeneratedCudaSource {
        source: CUDA_TEMPLATE.replace(GROWTH_MARKER, &expression),
        entry_point: ENTRY_POINT,
    })
}

pub fn generate_cuda_expression(
    expression: &KernelExpression,
    symbols: &BTreeSet<String>,
) -> Result<String, CodegenError> {
    generate_expression(expression, 0, symbols)
}

fn generate_expression(
    expression: &KernelExpression,
    depth: usize,
    symbols: &BTreeSet<String>,
) -> Result<String, CodegenError> {
    if depth > MAX_CODEGEN_DEPTH {
        return Err(CodegenError::TooDeep);
    }
    let child_depth = depth + 1;
    match expression {
        KernelExpression::Constant(value) => cuda_float(*value),
        KernelExpression::Parameter(name) => match name.as_str() {
            _ if symbols.contains(name) => Ok(name.clone()),
            _ => Err(CodegenError::UnknownSymbol(name.clone())),
        },
        KernelExpression::Variable(variable) => {
            let name = match variable {
                ExpressionVariable::X => "x",
                ExpressionVariable::Y => "y",
                ExpressionVariable::Radius => "radius",
                ExpressionVariable::Distance => "distance",
            };
            Err(CodegenError::UnsupportedVariable(name))
        }
        KernelExpression::Binary { op, lhs, rhs } => {
            let lhs = generate_expression(lhs, child_depth, symbols)?;
            let rhs = generate_expression(rhs, child_depth, symbols)?;
            Ok(match op {
                BinaryOp::Add => format!("({lhs} + {rhs})"),
                BinaryOp::Subtract => format!("({lhs} - {rhs})"),
                BinaryOp::Multiply => format!("({lhs} * {rhs})"),
                BinaryOp::Divide => format!("({lhs} / {rhs})"),
                BinaryOp::Power => format!("powf({lhs}, {rhs})"),
            })
        }
        KernelExpression::Unary { op, operand } => {
            let operand = generate_expression(operand, child_depth, symbols)?;
            Ok(match op {
                UnaryOp::Neg => format!("(-({operand}))"),
                UnaryOp::Sqrt => format!("sqrtf({operand})"),
                UnaryOp::Abs => format!("fabsf({operand})"),
                UnaryOp::Exp => format!("expf({operand})"),
                UnaryOp::Sin => format!("sinf({operand})"),
                UnaryOp::Cos => format!("cosf({operand})"),
            })
        }
        KernelExpression::Call {
            function,
            arguments,
        } => {
            let arguments = arguments
                .iter()
                .map(|argument| generate_expression(argument, child_depth, symbols))
                .collect::<Result<Vec<_>, _>>()?;
            match (function, arguments.as_slice()) {
                (Function::Min, [lhs, rhs]) => Ok(format!("fminf({lhs}, {rhs})")),
                (Function::Max, [lhs, rhs]) => Ok(format!("fmaxf({lhs}, {rhs})")),
                (Function::Clamp, [value, minimum, maximum]) => {
                    Ok(format!("fminf({maximum}, fmaxf({minimum}, {value}))"))
                }
                _ => Err(CodegenError::InvalidFunctionArity),
            }
        }
    }
}

fn growth_symbols() -> BTreeSet<String> {
    BTreeSet::from([
        "mu".to_string(),
        "potential".to_string(),
        "sigma".to_string(),
    ])
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProgramKernelData {
    pub values: Vec<f32>,
    pub masks: Vec<i32>,
    pub offsets: Vec<i32>,
    pub widths: Vec<i32>,
    pub heights: Vec<i32>,
    pub anchor_x: Vec<i32>,
    pub anchor_y: Vec<i32>,
    pub channels: Vec<i32>,
}

pub fn program_kernel_data(program: &RuleProgram) -> Result<ProgramKernelData, CodegenError> {
    let mut data = ProgramKernelData {
        values: Vec::new(),
        masks: Vec::new(),
        offsets: Vec::with_capacity(program.inputs.len()),
        widths: Vec::with_capacity(program.inputs.len()),
        heights: Vec::with_capacity(program.inputs.len()),
        anchor_x: Vec::with_capacity(program.inputs.len()),
        anchor_y: Vec::with_capacity(program.inputs.len()),
        channels: Vec::with_capacity(program.inputs.len()),
    };
    for input in &program.inputs {
        match &input.source {
            InputSource::State => {
                data.offsets.push(0);
                data.widths.push(0);
                data.heights.push(0);
                data.anchor_x.push(0);
                data.anchor_y.push(0);
                data.channels.push(0);
            }
            InputSource::ChannelState { channel } => {
                data.offsets.push(0);
                data.widths.push(0);
                data.heights.push(0);
                data.anchor_x.push(0);
                data.anchor_y.push(0);
                data.channels
                    .push(i32::try_from(*channel).map_err(|_| CodegenError::InvalidKernelLayout)?);
            }
            InputSource::Convolution { kernel } => {
                let offset = i32::try_from(data.values.len())
                    .map_err(|_| CodegenError::InvalidKernelLayout)?;
                let width =
                    i32::try_from(kernel.width).map_err(|_| CodegenError::InvalidKernelLayout)?;
                let height =
                    i32::try_from(kernel.height).map_err(|_| CodegenError::InvalidKernelLayout)?;
                let anchor_x = i32::try_from(kernel.anchor_x)
                    .map_err(|_| CodegenError::InvalidKernelLayout)?;
                let anchor_y = i32::try_from(kernel.anchor_y)
                    .map_err(|_| CodegenError::InvalidKernelLayout)?;
                data.offsets.push(offset);
                data.widths.push(width);
                data.heights.push(height);
                data.anchor_x.push(anchor_x);
                data.anchor_y.push(anchor_y);
                data.channels.push(0);
                data.values.extend_from_slice(&kernel.values);
                if let Some(mask) = &kernel.mask {
                    data.masks
                        .extend(mask.iter().map(|active| i32::from(*active)));
                } else {
                    data.masks
                        .extend(std::iter::repeat_n(1, kernel.values.len()));
                }
            }
            InputSource::ChannelConvolution { channel, kernel } => {
                let offset = i32::try_from(data.values.len())
                    .map_err(|_| CodegenError::InvalidKernelLayout)?;
                data.offsets.push(offset);
                data.widths.push(
                    i32::try_from(kernel.width).map_err(|_| CodegenError::InvalidKernelLayout)?,
                );
                data.heights.push(
                    i32::try_from(kernel.height).map_err(|_| CodegenError::InvalidKernelLayout)?,
                );
                data.anchor_x.push(
                    i32::try_from(kernel.anchor_x)
                        .map_err(|_| CodegenError::InvalidKernelLayout)?,
                );
                data.anchor_y.push(
                    i32::try_from(kernel.anchor_y)
                        .map_err(|_| CodegenError::InvalidKernelLayout)?,
                );
                data.channels
                    .push(i32::try_from(*channel).map_err(|_| CodegenError::InvalidKernelLayout)?);
                data.values.extend_from_slice(&kernel.values);
                if let Some(mask) = &kernel.mask {
                    data.masks
                        .extend(mask.iter().map(|active| i32::from(*active)));
                } else {
                    data.masks
                        .extend(std::iter::repeat_n(1, kernel.values.len()));
                }
            }
        }
    }
    if data.values.is_empty() {
        data.values.push(0.0);
        data.masks.push(0);
    }
    Ok(data)
}

pub fn generate_program_cuda_source(
    program: &RuleProgram,
) -> Result<GeneratedCudaSource, CodegenError> {
    let mut symbols = program.parameters.keys().cloned().collect::<BTreeSet<_>>();
    symbols.extend(program.inputs.iter().map(|input| input.name.clone()));
    let expression = generate_expression(&program.update, 0, &symbols)?;
    let arguments = program
        .inputs
        .iter()
        .map(|input| format!("float {}", input.name))
        .chain(
            program
                .parameters
                .keys()
                .map(|name| format!("float {name}")),
        )
        .collect::<Vec<_>>()
        .join(", ");
    let call_arguments = program
        .inputs
        .iter()
        .map(|input| input.name.as_str())
        .chain(program.parameters.keys().map(String::as_str))
        .collect::<Vec<_>>()
        .join(", ");
    let input_code = program
        .inputs
        .iter()
        .enumerate()
        .map(|(index, input)| match &input.source {
            InputSource::State | InputSource::ChannelState { .. } => format!(
                "float {name} = current[kernel_channels[{index}] * cell_count + linear];",
                name = input.name
            ),
            InputSource::Convolution { .. } | InputSource::ChannelConvolution { .. } => format!(
                "float {name} = 0.0f; for (int ky = 0; ky < kernel_heights[{index}]; ++ky) {{ for (int kx = 0; kx < kernel_widths[{index}]; ++kx) {{ int ki = kernel_offsets[{index}] + ky * kernel_widths[{index}] + kx; if (kernel_masks[ki] == 0) continue; int nx = cellarium_wrap(x + kx - kernel_anchor_x[{index}], width); int ny = cellarium_wrap(y + ky - kernel_anchor_y[{index}], height); {name} += kernel_values[ki] * current[kernel_channels[{index}] * cell_count + ny * width + nx]; }} }}",
                name = input.name
            ),
        })
        .collect::<Vec<_>>()
        .join("\n        ");
    let parameter_signature = program
        .parameters
        .keys()
        .map(|name| format!(", float {name}"))
        .collect::<String>();
    let source = PROGRAM_CUDA_TEMPLATE
        .replace("/*__PROGRAM_ARGS__*/", &arguments)
        .replace(
            "/*__PROGRAM_KERNEL_PARAMETER_SIGNATURE__*/",
            &parameter_signature,
        )
        .replace("/*__PROGRAM_CALL_ARGS__*/", &call_arguments)
        .replace("/*__PROGRAM_INPUTS__*/", &input_code)
        .replace("/*__PROGRAM_EXPRESSION__*/", &expression);
    Ok(GeneratedCudaSource {
        source,
        entry_point: ENTRY_POINT,
    })
}

pub fn generate_topology_cuda_source() -> GeneratedCudaSource {
    GeneratedCudaSource {
        source: TOPOLOGY_CUDA_TEMPLATE.to_string(),
        entry_point: "cellarium_topology_step",
    }
}

fn cuda_float(value: f32) -> Result<String, CodegenError> {
    if !value.is_finite() {
        return Err(CodegenError::NonFiniteConstant);
    }
    let mut literal = format!("{value:?}");
    if !literal.contains(['.', 'e', 'E']) {
        literal.push_str(".0");
    }
    literal.push('f');
    Ok(literal)
}

const CUDA_TEMPLATE: &str = r#"
extern "C" __device__ int cellarium_wrap(int value, int size) {
    int wrapped = value % size;
    return wrapped < 0 ? wrapped + size : wrapped;
}

extern "C" __device__ float cellarium_growth(float potential, float mu, float sigma) {
#line 1 "cellarium-growth-expression"
    return /*__CELLARIUM_GROWTH__*/;
#line 1 "cellarium-generated-kernel"
}

extern "C" __global__ void cellarium_step(
    float* next,
    const float* current,
    const float* kernel,
    int width,
    int height,
    int kernel_width,
    int kernel_height,
    int kernel_anchor_x,
    int kernel_anchor_y,
    const int* kernel_mask,
    int mode,
    float dt,
    float mu,
    float sigma
) {
    int linear = blockIdx.x * blockDim.x + threadIdx.x;
    int cell_count = width * height;
    if (linear >= cell_count) return;

    int x = linear % width;
    int y = linear / width;
    if (mode == 0) {
        int neighbors = 0;
        for (int dy = -1; dy <= 1; ++dy) {
            for (int dx = -1; dx <= 1; ++dx) {
                if (dx == 0 && dy == 0) continue;
                int nx = cellarium_wrap(x + dx, width);
                int ny = cellarium_wrap(y + dy, height);
                neighbors += current[ny * width + nx] > 0.5f ? 1 : 0;
            }
        }
        float self_state = current[linear];
        bool alive = self_state > 0.5f;
        bool survives = alive && (neighbors == 2 || neighbors == 3);
        bool born = !alive && neighbors == 3;
        next[linear] = (survives || born) ? 1.0f : 0.0f;
    } else {
        float potential = 0.0f;
        for (int ky = 0; ky < kernel_height; ++ky) {
            for (int kx = 0; kx < kernel_width; ++kx) {
                int kernel_index = ky * kernel_width + kx;
                if (kernel_mask[kernel_index] == 0) continue;
                int nx = cellarium_wrap(x + kx - kernel_anchor_x, width);
                int ny = cellarium_wrap(y + ky - kernel_anchor_y, height);
                potential += kernel[kernel_index] * current[ny * width + nx];
            }
        }
        float growth = cellarium_growth(potential, mu, sigma);
        float updated = current[linear] + dt * growth;
        next[linear] = fminf(1.0f, fmaxf(0.0f, updated));
    }
}
"#;

const PROGRAM_CUDA_TEMPLATE: &str = r#"
extern "C" __device__ int cellarium_wrap(int value, int size) {
    int wrapped = value % size;
    return wrapped < 0 ? wrapped + size : wrapped;
}

extern "C" __device__ float cellarium_update(/*__PROGRAM_ARGS__*/) {
#line 1 "cellarium-program-expression"
    return /*__PROGRAM_EXPRESSION__*/;
#line 1 "cellarium-generated-program"
}

extern "C" __global__ void cellarium_step(
    float* next,
    const float* current,
    const float* kernel_values,
    const int* kernel_masks,
    const int* kernel_offsets,
    const int* kernel_widths,
    const int* kernel_heights,
    const int* kernel_anchor_x,
    const int* kernel_anchor_y,
    const int* kernel_channels,
    int width,
    int height,
    float dt/*__PROGRAM_KERNEL_PARAMETER_SIGNATURE__*/
) {
    int linear = blockIdx.x * blockDim.x + threadIdx.x;
    int cell_count = width * height;
    if (linear >= cell_count) return;
    int x = linear % width;
    int y = linear / width;
    /*__PROGRAM_INPUTS__*/
    float update = cellarium_update(/*__PROGRAM_CALL_ARGS__*/);
    next[linear] = fminf(1.0f, fmaxf(0.0f, current[linear] + dt * update));
}
"#;

const TOPOLOGY_CUDA_TEMPLATE: &str = r#"
extern "C" __global__ void cellarium_topology_step(
    float* next,
    const float* current,
    const unsigned int* offsets,
    const unsigned int* neighbors,
    const float* weights,
    float dt,
    unsigned int count
) {
    unsigned int site = blockIdx.x * blockDim.x + threadIdx.x;
    if (site >= count) return;
    float total = 0.0f;
    for (unsigned int edge = offsets[site]; edge < offsets[site + 1]; ++edge) {
        total += weights[edge] * current[neighbors[edge]];
    }
    next[site] = fminf(1.0f, fmaxf(0.0f, current[site] + dt * total));
}
"#;

#[cfg(test)]
mod tests {
    // multi-input CUDA code generation tests
    fn program() -> RuleProgram {
        let food_kernel = KernelDefinition {
            name: "food-kernel".to_string(),
            width: 1,
            height: 1,
            anchor_x: 0,
            anchor_y: 0,
            mask: None,
            normalization: Normalization::None,
            parameters: BTreeMap::new(),
            values: KernelValues::Explicit(vec![1.0]),
        }
        .build()
        .unwrap();
        RuleProgram::new(
            vec![
                RuleInput::state("self"),
                RuleInput::convolution("food", food_kernel),
            ],
            BTreeMap::from([("gain".to_string(), 0.5)]),
            parse_and_validate(
                "(self + food) * gain",
                &BTreeSet::from(["self".to_string(), "food".to_string(), "gain".to_string()]),
            )
            .unwrap(),
        )
        .unwrap()
    }
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
    use crate::sim::expression::KernelExpression;
    use crate::sim::kernel::{KernelDefinition, KernelValues, Normalization};
    use crate::sim::parser::parse_and_validate;
    use crate::sim::program::{RuleInput, RuleProgram};

    fn growth(source: &str) -> KernelExpression {
        let symbols = BTreeSet::from([
            "mu".to_string(),
            "potential".to_string(),
            "sigma".to_string(),
        ]);
        parse_and_validate(source, &symbols).unwrap()
    }

    #[test]
    fn generates_cuda_device_helper_and_update_kernel_from_ast() {
        let generated =
            generate_cuda_source(&growth("2 * exp(-((potential - mu) / sigma) ^ 2) - 1")).unwrap();

        assert!(
            generated
                .source
                .contains("__device__ float cellarium_growth")
        );
        assert!(generated.source.contains("powf("));
        assert!(generated.source.contains("expf("));
        assert!(
            generated
                .source
                .contains("#line 1 \"cellarium-growth-expression\"")
        );
        assert!(generated.source.contains("__global__ void cellarium_step"));
        assert_eq!(generated.entry_point, "cellarium_step");
    }

    #[test]
    fn rejects_symbols_that_are_not_growth_inputs() {
        let error = generate_cuda_source(&KernelExpression::Parameter(
            "not-an-identifier".to_string(),
        ))
        .unwrap_err();

        assert!(matches!(error, CodegenError::UnknownSymbol(_)));
    }

    #[test]
    fn rejects_malformed_function_arity_without_panicking() {
        let expression = KernelExpression::Call {
            function: crate::sim::expression::Function::Min,
            arguments: vec![KernelExpression::Constant(1.0)],
        };

        let error = generate_cuda_source(&expression).unwrap_err();

        assert!(matches!(error, CodegenError::InvalidFunctionArity));
    }

    #[test]
    fn generates_program_inputs_and_kernel_metadata() {
        let generated = generate_program_cuda_source(&program()).unwrap();
        assert!(generated.source.contains("float self"));
        assert!(generated.source.contains("float food"));
        assert!(generated.source.contains("float gain"));
        assert!(generated.source.contains("kernel_offsets[1]"));
        let metadata = program_kernel_data(&program()).unwrap();
        assert_eq!(metadata.offsets, vec![0, 0]);
        assert_eq!(metadata.widths, vec![0, 1]);
        assert_eq!(metadata.heights, vec![0, 1]);
        assert_eq!(metadata.values, vec![1.0]);
        assert_eq!(metadata.masks, vec![1]);
    }
}
