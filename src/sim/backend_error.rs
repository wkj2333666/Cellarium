use super::cuda_codegen::CodegenError;
use super::expression::KernelExpressionError;

#[cfg(feature = "cuda")]
use cudarc::driver::DriverError;
#[cfg(feature = "cuda")]
use cudarc::nvrtc::CompileError;

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[cfg(feature = "cuda")]
    #[error("CUDA driver error: {0}")]
    Driver(#[from] DriverError),
    #[cfg(feature = "cuda")]
    #[error("CUDA compilation error: {0}")]
    Compile(#[from] CompileError),
    #[error("rule evaluation error: {0}")]
    RuleEvaluation(#[from] KernelExpressionError),
    #[error("CUDA code generation error: {0}")]
    Codegen(#[from] CodegenError),
    #[error("the runtime compilation cache is unavailable")]
    CompilationCachePoisoned,
    #[error("world dimensions must fit a CUDA launch")]
    InvalidWorld,
    #[error("compiled topology has inconsistent CSR arrays")]
    InvalidTopology,
    #[error("CUDA support was not compiled in")]
    CudaNotCompiled,
    #[error("CUDA runtime libraries are unavailable")]
    CudaUnavailable,
    #[error("multi-channel runtime error: {0}")]
    Runtime(#[from] crate::sim::runtime::RuntimeError),
}
