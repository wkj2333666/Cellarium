# Cellarium Interactive Editor Design System

## Product and platform

Cellarium is a keyboard-and-mouse driven Rust/Ratatui terminal application for GPU cellular automata. The target is a desktop terminal, typically Kitty, at approximately 160–240 columns and 40–70 rows. Every design must remain expressible as terminal cells, Ratatui blocks, text, simple Unicode symbols, and a Kitty pixel viewport. Do not design a browser dashboard, floating cards, rounded controls, shadows, animation, or proportional-font UI.

The primary job is to move rapidly between simulation and a transactional experiment workbench. Users design the world/domain, periodic polygon tiling, channels, convolution kernels, and growth program without editing external files. Draft changes update previews immediately but affect the running simulation only after Apply (`Ctrl+Enter`). Escape/Revert restores the last applied configuration.

## Visual foundation

- Background: near-black navy `#080d19`; editor surfaces `#0a0f1c`; status background `#0c1220`.
- Border and active accent: Cellarium blue `#6090dc` / bright focus `#52a8ff`.
- Primary text: cool white `#d8e5f7`; secondary text `#bed7ff`; dim hints `#8094b4`.
- Viewport exterior: existing dark navy `#080c18`; zero-valued pixels inside the active domain are pure black `#000000`.
- Channel color: one channel uses near-white, three channels use red/green/blue, and other channel counts use a high-contrast accessible automatic palette. Per-channel custom colors remain pinned.
- Success: `#62c073`; warning: pale amber; error: muted red.
- Typography: monospace only. Preserve one-cell alignment. No font-size hierarchy; hierarchy comes from borders, uppercase section labels, color, whitespace, and markers such as `▸`, `●`, `○`, `×`.
- Corners: square. Borders: one terminal cell, single-line box drawing by default.

## Responsive shell

- Normal simulation mode: large viewport left, contextual inspector right when width permits, and a fixed two-row footer.
- Footer row one shows mode, run state, tick, selection, and draft state. Row two shows the 3–5 commands relevant to the current focus and `[?] Help`. Detailed rates remain in the Statistics panel or a diagnostics overlay. Whole segments disappear at narrow widths; text is never cut through a glyph.
- At narrow widths, hide the inspector and open it as a full-screen panel. The viewport always retains a usable minimum size.
- Editing mode uses the selected outline-first layout: persistent experiment outline at left, contextual canvas in the center, and property inspector at right. The outline identifies `World`, `Tiling`, `Channels`, `Kernels`, `Growth`, and `Experiment`.

## Interaction rules

- Keyboard and mouse are peers: every mouse action has a key equivalent and every focusable value is reachable by keyboard.
- `Tab`/`Shift+Tab` moves focus; arrows move/select; Enter edits or confirms a local field; `Ctrl+Enter` validates and applies the whole draft; `Esc` cancels the current edit or returns to simulation; `?` opens context help.
- Draft state is explicit: `APPLIED`, `MODIFIED`, `VALIDATING`, or `ERROR`. Never silently apply structural changes.
- Errors stay adjacent to the field/canvas element that caused them, with a concise summary in the inspector.
- Mouse wheel zooms visual canvases; middle-drag pans; left click/drag selects or paints; right click/drag erases where meaningful.

## Workbench editors

### World/domain

Large expanded-tiling canvas for rectangular, masked, and sparse domains. Paint/erase actual polygon tiles, use rectangle/fill tools, resize dimensions, choose channel-specific boundary policy, and preview boundary behavior. Inspector shows domain type, dimensions, active tile count, seed, and boundary.

### Periodic tiling

Large real-space canvas for laying regular or custom straight-sided polygon tiles. Users translate and rotate tile instances, snap complete edges, edit reusable prototypes, confirm two periodic translations, and inspect repeated copies, seams, gaps, overlaps, derived adjacency, and compiled CSR. The system derives a fundamental patch from the user-confirmed translations instead of forcing a rectangular unit-cell editor. The first release supports mixed periodic tilings such as regular octagons and squares, but not curves, holes, or aperiodic tilings.

### Channels

New experiments start with one channel. Painting changes the selected channel while Composite, Solo, and Grid views update immediately. Automatic colors prioritize clarity on the dark viewport; exactly three channels use red, green, and blue. Users can pin custom colors. Channels remain distinct from tile types and lattice sites.

### Kernel

Large zoomable heatmap/graph view with numeric weights and spatial or graph-distance profiles. Provide pencil, erase, fill, symmetry, normalize, import/generator tools. Inspector edits source and target channels, cutoff, normalization, named parameters, selected weight, and formula-generated versus explicit values.

### Growth

Split editor: a multi-line source editor and always-updating plot/heatmap. The generated signature has exactly one ordinary input per kernel targeting the selected channel plus implicit `self`. A restricted Rust-like language provides immutable `let`, `if/else`, booleans, comments, and whitelisted scalar functions without loops or external effects. One- and two-variable plots become curves and heatmaps; higher-dimensional programs use selectable slices and pinned-input sliders. Hover exposes local bindings, selected branches, and final result. CUDA compilation happens only on Apply.

### Experiment

Review all draft sections, compatibility/backend implications, dirty fields, validation errors, metadata, and load/save/export actions. Applying is atomic: validate model, compile topology, rebuild CPU/CUDA backend, then swap; failure preserves the last running experiment.

## Design constraints

- Use only the colors, monospace typography, square borders, spacing, and component language defined here.
- Keep simulation and editor visually continuous; this is one terminal application, not separate pages.
- Prefer information density with clear focus over decorative whitespace.
- No unsupported claims: a control shown in the UI must correspond to an implementable operation in the planned Rust model.
- The screenshot reference is ground truth for the existing shell, palette, border style, and viewport/editor proportion. Improve navigation and overflow without restyling the product.

## Style reference adaptation

The selected `ascii-hero` reference contributes rigorous monospace data presentation, high-contrast dark surfaces, square controls, blue active states, and explicit status indicators. Its Web-only elements—proportional headlines, pill chips, large type, gradients, cards, and landing-page composition—are intentionally excluded.
