use crate::sim::expression::{BinaryOp, ExpressionVariable, Function, KernelExpression, UnaryOp};

const MAX_CODEGEN_DEPTH: usize = 256;
const ENTRY_POINT: &str = "cellarium_step";
const GROWTH_MARKER: &str = "/*__CELLARIUM_GROWTH__*/";

#[derive(Clone, Debug, PartialEq, Eq)]
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
}

pub fn generate_cuda_source(
    expression: &KernelExpression,
) -> Result<GeneratedCudaSource, CodegenError> {
    let expression = generate_expression(expression, 0)?;
    Ok(GeneratedCudaSource {
        source: CUDA_TEMPLATE.replace(GROWTH_MARKER, &expression),
        entry_point: ENTRY_POINT,
    })
}

fn generate_expression(
    expression: &KernelExpression,
    depth: usize,
) -> Result<String, CodegenError> {
    if depth > MAX_CODEGEN_DEPTH {
        return Err(CodegenError::TooDeep);
    }
    let child_depth = depth + 1;
    match expression {
        KernelExpression::Constant(value) => cuda_float(*value),
        KernelExpression::Parameter(name) => match name.as_str() {
            "potential" | "mu" | "sigma" => Ok(name.clone()),
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
            let lhs = generate_expression(lhs, child_depth)?;
            let rhs = generate_expression(rhs, child_depth)?;
            Ok(match op {
                BinaryOp::Add => format!("({lhs} + {rhs})"),
                BinaryOp::Subtract => format!("({lhs} - {rhs})"),
                BinaryOp::Multiply => format!("({lhs} * {rhs})"),
                BinaryOp::Divide => format!("({lhs} / {rhs})"),
                BinaryOp::Power => format!("powf({lhs}, {rhs})"),
            })
        }
        KernelExpression::Unary { op, operand } => {
            let operand = generate_expression(operand, child_depth)?;
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
                .map(|argument| generate_expression(argument, child_depth))
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::sim::expression::KernelExpression;
    use crate::sim::parser::parse_and_validate;

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
}
