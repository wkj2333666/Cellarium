# Stable extension model

Experiment files are the versioned interchange boundary for Cellarium. The
top-level `format_version` is mandatory and loaders reject versions they do not
understand before entering the terminal UI. Rule programs, kernels, and lattice
descriptions are data-only RON values; they do not carry CUDA handles or host
callbacks.

The compatibility rules are deliberately conservative:

1. A loader validates world dimensions, finite state values, kernel definitions,
   rule-program symbols, and optional lattice topology before construction.
2. A newer format version is rejected with an explicit error instead of being
   partially interpreted.
3. Future migrations should add a pure version-to-version conversion before
   validation. The runtime model should only receive the current schema.
4. Backend selection remains an implementation detail. The same experiment can
   be rebuilt on CPU or CUDA without changing its serialized rule.

The `ExperimentMetadata` fields (`name`, `description`, `author`, and `tags`)
are descriptive and safe to preserve across migrations. External tools should
write RON using the public data types and should not depend on private backend
or TUI fields.
