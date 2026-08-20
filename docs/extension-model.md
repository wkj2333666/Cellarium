# Stable extension model

Experiment files are the versioned interchange boundary for Cellarium. The
top-level `format_version` is mandatory and loaders reject versions they do not
understand before entering the terminal UI. Rule programs, kernels, and lattice
descriptions are data-only RON values; they do not carry CUDA handles or host
callbacks.

The compatibility rules are deliberately conservative:

1. A loader validates world dimensions, finite state values, kernel definitions,
   rule-program symbols, and optional lattice topology before construction.
2. Version `0` is migrated in memory to the current version with default
   metadata; newer unknown versions are rejected with an explicit error instead
   of being partially interpreted.
3. Future migrations should add another pure version-to-version conversion
   before validation. The runtime model should only receive the current schema.
4. Backend selection remains an implementation detail. The same experiment can
   be rebuilt on CPU or CUDA without changing its serialized rule.

Callers that need to inspect an asset without constructing it can use the
`CompatibilityReport` API. It returns a supported flag plus human-readable
issues, which is suitable for a preset browser or import preview.

The `ExperimentMetadata` fields (`name`, `description`, `author`, and `tags`)
are descriptive and safe to preserve across migrations. External tools should
write RON using the public data types and should not depend on private backend
or TUI fields.
