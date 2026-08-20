# Configurable Kernel System Design

## Scope

Phase P5 makes convolution kernels data-driven while preserving the existing
CPU/CUDA backend boundary. A kernel author can load a text-defined kernel at
startup, inspect and tune named scalar parameters in the TUI, regenerate the
kernel, and run the result through either backend without editing Rust source.

This phase does not add the full P6 text expression parser, NVRTC rule
compilation, multiple channels, or custom lattices.

## Chosen architecture

The design separates a editable `KernelDefinition` from the validated,
backend-ready `Kernel`:

```text
RON kernel file / built-in definition
        |
KernelDefinition + named parameters
        |
safe expression evaluation and validation
        |
Kernel: dimensions, anchor, mask, normalized values
        |
SimulationBackend -> CPU stencil / CUDA stencil
```

Definitions own editable intent: name, rectangular dimensions, anchor, shape
mask, normalization mode, named scalar parameters, and an expression AST.
Kernels own immutable evaluated data and enforce all invariants. Backends
consume only evaluated `Kernel` data, so neither backend knows about files,
parameter editing, or expressions.

This is preferred over putting formulas directly in the backends or parsing a
new language per backend. It keeps GPU execution simple and data-parallel while
allowing P6 to replace the RON AST syntax with a parser that emits the same
evaluation representation.

## Kernel model

`KernelDefinition` supports:

- positive rectangular `width` and `height`, bounded to 129 cells per axis;
- an `anchor` inside the rectangle, allowing non-centered or asymmetric stencils;
- an optional rectangular mask with exactly `width * height` boolean entries;
- `Normalization::None` or `Normalization::Sum`;
- a finite named scalar parameter map;
- a `KernelExpression` used to generate masked values;
- optional explicit values for definition formats that do not use expressions.

`KernelExpression` is a small typed AST with constants, named parameters,
geometric variables (`x`, `y`, `radius`, and normalized distance `r`), basic
arithmetic, exponentiation, absolute value, minimum/maximum, square root, and
clamping. Evaluation is deterministic and returns `Result`; division by zero,
square roots of negative values, and non-finite outputs are kernel errors.

The generated ring preset uses named `center` and `width` parameters, making it
editable without source changes.

`Kernel::try_from(definition)` validates dimensions, anchor, mask/value
lengths, parameter and expression finiteness, then applies normalization over
unmasked entries. The sum normalization denominator must be finite and greater
than `1e-12` in magnitude. Invalid definitions never reach a backend.

For compatibility with the original square API, `Kernel::radius()` is the
maximum Chebyshev distance from the anchor to any included rectangle cell. It
is metadata for status and bounds checks; execution uses width, height, and
anchor directly.

## Text format

Kernel files use RON because it is human-editable and matches the project's
long-term serialization direction. A file contains one `KernelDefinition`.
Example:

```ron
(
    name: "wide-ring",
    width: 33,
    height: 21,
    anchor: (16, 10),
    normalization: Sum,
    parameters: {
        "center": 0.48,
        "width": 0.17,
    },
    value: Exp(
        Neg(
        Pow(
            Div(Sub(Variable("r"), Parameter("center")), Parameter("width")),
            Constant(2.0),
        )),
    ),
    mask: None,
)
```

A file may alternatively provide `values: Some([...])`. Explicit values obey
the same validation and normalization as generated values. Loading is fallible
and reports both file/parse and kernel-validation errors.

## CPU and CUDA execution

Both backends iterate the actual rectangle and skip masked entries. CPU uses
the anchor to convert a kernel coordinate to a world offset. The CUDA kernel
receives `kernel_width`, `kernel_height`, `anchor_x`, and `anchor_y` instead of
assuming a square radius. CUDA parity tests cover:

- the centered square ring;
- a non-square rectangle;
- a masked/asymmetric stencil;
- sum and no-normalization modes.

The existing CUDA stream synchronization remains before host state replacement.

## Application and TUI interaction

The app owns a kernel catalog containing built-in presets and any kernel loaded
from `--kernel <path>`. Commands are explicit:

- `K`: select the next kernel in the catalog;
- `Tab`: cycle the selected editable parameter;
- `+`/`=` and `-`/`_`: increase or decrease the selected parameter;
- `G`: regenerate the active kernel and recreate the backend;
- `V`: toggle kernel preview.

Changing a parameter only edits `KernelDefinition`. `G` re-evaluates,
validates, updates `SimulationSpec`, and recreates the backend. Invalid edits
preserve the previous active kernel and display a validation error in the
status bar.

The preview is a bounded textual view rendered over the simulation viewport. It
shows the kernel name, dimensions, anchor, radius, normalization, selected
parameter and value, value range, active-entry count, and a compact sampled
representation using dim and bright half-block characters. It deliberately
avoids a full modal editor until P12.

The binary accepts `--kernel <path>` before entering the TUI. Load failure
prints a concise diagnostic and exits without changing terminal state.

## Error handling

`KernelError` distinguishes malformed definitions, expression errors, and
normalization errors. The loader wraps file and RON errors while preserving the
underlying message. Backend construction and stepping retain their existing
error boundary. TUI parameter adjustments clamp to finite `f32` values and do
not mutate the active kernel until regeneration succeeds.

## Testing

Unit tests cover dimension/anchor/mask invariants, normalization, explicit
values, expression variables, named parameters, malformed expressions, radius
metadata, file loading, and preset construction. Backend tests compare CPU and
CUDA results on rectangular, asymmetric, and masked kernels. App tests cover
selection, parameter editing, regeneration failure rollback, and backend
recreation. TUI tests cover command hints, status fields, and preview content.
The PTY startup regression remains part of the full gate.

## Completion criteria

- A kernel author can edit and load a RON kernel without changing Rust source.
- Kernel files and TUI editing support width, height, radius metadata, masks,
  arbitrary values, normalization, expression-generated values, and named
  parameters.
- CPU and CUDA produce parity results for non-square/asymmetric kernels.
- The TUI can select, inspect, tune, regenerate, and preview kernels.
- The full format, test, Clippy, release-build, and PTY gates pass.
