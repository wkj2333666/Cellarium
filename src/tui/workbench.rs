use crate::app::App;
use crate::input::UiCommand;
use crate::render::display::ViewportDisplay;
use crate::render::workbench_graphics::{GraphicsScene, PlacementAction};
use crate::workbench::{WorkbenchFocus, WorkbenchSection};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorkbenchLayout {
    pub outline: Rect,
    pub canvas: Rect,
    pub inspector: Option<Rect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarAction {
    Ui(UiCommand),
    EditorKey(crossterm::event::KeyCode),
    ToggleGrowthEditor,
}

fn toolbar_segments(
    state: &crate::workbench::WorkbenchState,
) -> Vec<(&'static str, ToolbarAction)> {
    use crossterm::event::KeyCode;
    match state.section() {
        WorkbenchSection::World => vec![
            ("] Channel", ToolbarAction::Ui(UiCommand::SelectNext)),
            ("[V] View", ToolbarAction::Ui(UiCommand::CyclePresentation)),
        ],
        WorkbenchSection::Tiling => vec![
            ("[D] Shape", ToolbarAction::EditorKey(KeyCode::Char('d'))),
            ("[A] Add basis", ToolbarAction::Ui(UiCommand::ContextAdd)),
            ("[P] Preset", ToolbarAction::Ui(UiCommand::CyclePreset)),
            (
                "[S] Solve seams",
                ToolbarAction::EditorKey(KeyCode::Char('s')),
            ),
            ("[N] Next basis", ToolbarAction::Ui(UiCommand::ShapeNext)),
            ("[+] Sides", ToolbarAction::Ui(UiCommand::ShapeIncrease)),
            ("[-] Sides", ToolbarAction::Ui(UiCommand::ShapeDecrease)),
            ("[0] Fit", ToolbarAction::EditorKey(KeyCode::Char('0'))),
        ],
        WorkbenchSection::Channels => vec![
            ("[A] Add", ToolbarAction::Ui(UiCommand::ContextAdd)),
            ("[Del] Remove", ToolbarAction::Ui(UiCommand::ContextDelete)),
            ("] Select", ToolbarAction::Ui(UiCommand::SelectNext)),
            ("[V] View", ToolbarAction::Ui(UiCommand::CyclePresentation)),
            ("[C] Color", ToolbarAction::Ui(UiCommand::CycleColor)),
            (
                "[E] Exact color",
                ToolbarAction::EditorKey(KeyCode::Char('e')),
            ),
            (
                "[X] Visible",
                ToolbarAction::Ui(UiCommand::ToggleVisibility),
            ),
            ("[F] Freeze", ToolbarAction::Ui(UiCommand::ToggleFrozen)),
        ],
        WorkbenchSection::Kernels => vec![
            (
                match state.kernel_tool() {
                    crate::workbench::kernel_editor::KernelTool::Weights => "[M] Tool: Weights",
                    crate::workbench::kernel_editor::KernelTool::Support => "[M] Tool: Support",
                },
                ToolbarAction::EditorKey(KeyCode::Char('m')),
            ),
            (
                match state.kernel_sampling_metric() {
                    crate::sim::kernel_sampling::KernelSamplingMetric::LatticeAffine => {
                        "[Q] Metric: Affine"
                    }
                    crate::sim::kernel_sampling::KernelSamplingMetric::WorldEuclidean => {
                        "[Q] Metric: World"
                    }
                },
                ToolbarAction::EditorKey(KeyCode::Char('q')),
            ),
            ("[P] Gaussian", ToolbarAction::EditorKey(KeyCode::Char('p'))),
            ("[G] Sigma", ToolbarAction::EditorKey(KeyCode::Char('g'))),
            ("[A] Add kernel", ToolbarAction::Ui(UiCommand::ContextAdd)),
            ("[Del] Remove", ToolbarAction::Ui(UiCommand::ContextDelete)),
            ("] Kernel", ToolbarAction::Ui(UiCommand::SelectNext)),
            ("[S] Source", ToolbarAction::EditorKey(KeyCode::Char('s'))),
            ("[U] Output", ToolbarAction::EditorKey(KeyCode::Char('u'))),
            ("[R] Resize", ToolbarAction::EditorKey(KeyCode::Char('r'))),
            (
                "[E/Enter] Exact",
                ToolbarAction::EditorKey(KeyCode::Char('e')),
            ),
            ("[0] Fit", ToolbarAction::EditorKey(KeyCode::Char('0'))),
        ],
        WorkbenchSection::Growth => vec![
            (
                if state.growth_editing() {
                    "[Esc] Finish source"
                } else {
                    "[E] Edit source"
                },
                ToolbarAction::ToggleGrowthEditor,
            ),
            (
                match state.selected_growth_mode() {
                    Some(crate::sim::experiment_model::UpdateMode::GrowthRate) => "[M] Mode: Rate",
                    _ => "[M] Mode: Value",
                },
                ToolbarAction::EditorKey(KeyCode::Char('m')),
            ),
            ("[d] Plot min", ToolbarAction::EditorKey(KeyCode::Char('d'))),
            ("[D] Plot max", ToolbarAction::EditorKey(KeyCode::Char('D'))),
        ],
        WorkbenchSection::Experiment => vec![
            (
                "[Ctrl+Enter] Apply & Run",
                ToolbarAction::Ui(UiCommand::ApplyDraft),
            ),
            ("[Ctrl+R] Revert", ToolbarAction::Ui(UiCommand::RevertDraft)),
            ("[D] Edit dt", ToolbarAction::EditorKey(KeyCode::Char('d'))),
            ("[Ctrl+S] Save", ToolbarAction::Ui(UiCommand::SaveActive)),
        ],
    }
}

pub fn toolbar_text(state: &crate::workbench::WorkbenchState) -> String {
    toolbar_segments(state)
        .into_iter()
        .map(|(label, _)| label)
        .collect::<Vec<_>>()
        .join("  ")
}

fn static_canvas_header_lines(
    state: &crate::workbench::WorkbenchState,
    context: &str,
) -> Vec<String> {
    let toolbar = toolbar_text(state);
    if toolbar.is_empty() {
        vec![context.to_string()]
    } else {
        vec![toolbar, context.to_string()]
    }
}

pub fn toolbar_action_at(
    state: &crate::workbench::WorkbenchState,
    column: u16,
) -> Option<ToolbarAction> {
    let mut start = 0usize;
    let column = usize::from(column);
    for (label, action) in toolbar_segments(state) {
        let end = start + label.chars().count();
        if (start..end).contains(&column) {
            return Some(action);
        }
        start = end + 2;
    }
    None
}
pub fn workbench_layout(area: Rect) -> WorkbenchLayout {
    if area.width >= 120 {
        let regions = Layout::new(
            Direction::Horizontal,
            [
                Constraint::Length(24),
                Constraint::Min(60),
                Constraint::Length(36),
            ],
        )
        .split(area);
        WorkbenchLayout {
            outline: regions[0],
            canvas: regions[1],
            inspector: Some(regions[2]),
        }
    } else {
        let regions = Layout::new(
            Direction::Horizontal,
            [
                Constraint::Length(22.min(area.width / 3)),
                Constraint::Min(20),
            ],
        )
        .split(area);
        WorkbenchLayout {
            outline: regions[0],
            canvas: regions[1],
            inspector: None,
        }
    }
}

fn inspector_kernel_count(state: &crate::workbench::WorkbenchState) -> usize {
    state
        .selected_rule_set()
        .and_then(|id| state.draft().rules.get(id))
        .map_or_else(
            || {
                state
                    .draft()
                    .kernels
                    .iter()
                    .filter(|kernel| kernel.target == state.selected_channel())
                    .count()
            },
            |rule| rule.kernels.len(),
        )
}

fn effective_kernel_count(spec: &crate::sim::experiment_model::ExperimentSpec) -> usize {
    if spec.rules.is_empty() {
        spec.kernels.len()
    } else {
        spec.rules
            .bindings
            .iter()
            .filter_map(|binding| spec.rules.get(binding.rule_set))
            .map(|rule| rule.kernels.len())
            .sum()
    }
}

fn growth_program_count(spec: &crate::sim::experiment_model::ExperimentSpec) -> usize {
    if spec.rules.is_empty() {
        spec.growth.len()
    } else {
        spec.rules.bindings.len()
    }
}

fn growth_inspector_texts(state: &crate::workbench::WorkbenchState) -> Vec<String> {
    use crate::sim::experiment_model::UpdateMode;
    let mode = state
        .selected_growth_mode()
        .unwrap_or(UpdateMode::DirectUpdate);
    let mut lines = vec![
        "Growth function".into(),
        format!(
            "target: basis {} / channel {}",
            state.selected_basis().0,
            state.selected_channel().0,
        ),
        state.growth_editor().signature().to_string(),
        format!(
            "mode [M]: {}",
            match mode {
                UpdateMode::GrowthRate => "Rate (Euler step)",
                UpdateMode::DirectUpdate => "Value (direct step)",
            }
        ),
        match mode {
            UpdateMode::GrowthRate => "next = clamp(self + dt × result, 0, 1)".into(),
            UpdateMode::DirectUpdate => "next = clamp(result, 0, 1)".into(),
        },
        format!("dt = {}", state.draft().simulation_dt),
        format!(
            "plot domain [d/D] = [{:.3}, {:.3}] (editor only)",
            state.growth_editor().primary_axis_interval()[0],
            state.growth_editor().primary_axis_interval()[1]
        ),
        "self = current target cell value".into(),
        "result = final expression value".into(),
        "clamp means: below lo → lo; above hi → hi".into(),
        "".into(),
        "Language".into(),
        "final expression is the result (no trailing ;)".into(),
        "let name = expression;".into(),
        "if condition { expression } else { expression }".into(),
        "numbers · true/false · pi · e · // comment".into(),
        "+  -  *  /  ^".into(),
        "==  !=  <  <=  >  >=  &&  ||  !".into(),
        "".into(),
        "Built-ins".into(),
        "sqrt(x)  abs(x)  exp(x)  log(x)".into(),
        "sin(x)  cos(x)  tanh(x)".into(),
        "floor(x)  ceil(x)  round(x)  sign(x)".into(),
        "min(a,b)  max(a,b)  step(edge,x)".into(),
        "clamp(x, lo, hi)  smoothstep(lo, hi, x)".into(),
        "mix(a, b, t)  gauss(x, mu, sigma)".into(),
        "".into(),
        "Kernel arguments are convolution results.".into(),
    ];
    if let Some(rule) = state
        .selected_rule_set()
        .and_then(|id| state.draft().rules.get(id))
    {
        lines.push(format!(
            "inputs: {}",
            rule.kernels
                .iter()
                .map(|kernel| kernel.symbol.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        if !rule.growth.parameters.is_empty() {
            lines.push(format!(
                "parameters: {}",
                rule.growth
                    .parameters
                    .iter()
                    .map(|(name, value)| format!("{name}={value}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    lines.push("Wheel over this panel to scroll.".into());
    lines
}

fn experiment_review_lines(state: &crate::workbench::WorkbenchState) -> Vec<String> {
    let active = state.authoritative();
    let draft = state.draft();
    let validation = crate::sim::experiment_model::validate_structure(draft);
    let mut lines = vec![
        "Experiment review".into(),
        match &validation {
            Ok(()) => "Validation: VALID · ready to apply".into(),
            Err(errors) => format!("Validation: INVALID · {} problem(s)", errors.len()),
        },
        "".into(),
        "Active → draft".into(),
        format!(
            "Channels        {} → {}",
            active.channels.len(),
            draft.channels.len()
        ),
        format!(
            "Effective kernels {} → {}",
            effective_kernel_count(active),
            effective_kernel_count(draft)
        ),
        format!(
            "Growth programs  {} → {}",
            growth_program_count(active),
            growth_program_count(draft)
        ),
    ];
    for channel in draft
        .channels
        .iter()
        .filter(|channel| !active.channels.iter().any(|active| active.id == channel.id))
    {
        lines.push(format!("  + {}", channel.name));
    }
    for channel in active
        .channels
        .iter()
        .filter(|channel| !draft.channels.iter().any(|draft| draft.id == channel.id))
    {
        lines.push(format!("  − {}", channel.name));
    }
    if active.channels != draft.channels && active.channels.len() == draft.channels.len() {
        lines.push("  ~ channel metadata or initial field changed".into());
    }
    if active.kernels != draft.kernels || active.rules != draft.rules {
        lines.push("  ~ kernel weights or routing changed".into());
    }
    if active.growth != draft.growth || active.rules != draft.rules {
        lines.push("  ~ growth programs changed".into());
    }
    if active.tiling != draft.tiling {
        lines.push("  ~ periodic tiling changed".into());
    }
    if active.geometry != draft.geometry {
        lines.push("  ~ world geometry changed".into());
    }
    if active == draft {
        lines.push("No unapplied changes.".into());
    }
    if let Err(errors) = validation {
        lines.push("".into());
        lines.push("Problems".into());
        lines.extend(
            errors
                .into_iter()
                .take(6)
                .map(|error| format!("  ! {error}")),
        );
    }
    lines.extend([
        "".into(),
        "Apply & Run restarts the runtime from the draft initial field.".into(),
        "Ctrl+Enter Apply & Run · Ctrl+R Revert · Ctrl+S Save workspace".into(),
    ]);
    lines
}

pub fn draw_workbench(
    frame: &mut ratatui::Frame,
    app: &mut App,
    display: &ViewportDisplay,
    area: Rect,
) {
    let layout = workbench_layout(area);
    app.set_workbench_area(area);
    let canvas_block = panel(
        " Canvas ",
        app.workbench().focus() == WorkbenchFocus::Canvas,
    );
    let canvas_content = canvas_block.inner(layout.canvas);
    let header_height = canvas_content.height.min(2);
    let canvas_header = Rect::new(
        canvas_content.x,
        canvas_content.y,
        canvas_content.width,
        header_height,
    );
    let canvas_inner = Rect::new(
        canvas_content.x,
        canvas_content.y.saturating_add(header_height),
        canvas_content.width,
        canvas_content.height.saturating_sub(header_height),
    );
    let graphics_area = if app.workbench().section() == WorkbenchSection::Growth {
        let source_height = canvas_inner.height.saturating_mul(48) / 100;
        Rect::new(
            canvas_inner.x,
            canvas_inner.y.saturating_add(source_height),
            canvas_inner.width,
            canvas_inner.height.saturating_sub(source_height),
        )
    } else {
        canvas_inner
    };
    let (pixel_width, pixel_height) = display.framebuffer_size(graphics_area);
    let (placement_action, scene_generation) = app.prepare_workbench_scene(
        graphics_area,
        [pixel_width as u32, pixel_height as u32],
        display.protocol(),
    );
    if matches!(
        placement_action,
        PlacementAction::DeleteBeforePresent | PlacementAction::DeleteOnly
    ) {
        display.invalidate_pending_graphics();
    }
    let state = app.workbench();
    let outline_lines = WorkbenchSection::ALL
        .into_iter()
        .map(|section| {
            Line::from(format!(
                "{} {}",
                if section == state.section() {
                    "▸"
                } else {
                    " "
                },
                section.label()
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(outline_lines).block(panel(
            " Experiment ",
            state.focus() == WorkbenchFocus::Outline,
        )),
        layout.outline,
    );
    let canvas_lines: Vec<String> = match state.section() {
        WorkbenchSection::World => vec![
            "Initial field editor".into(),
            "Paint selected channel on the canvas".into(),
            "Mouse: left paint · right erase".into(),
        ],
        WorkbenchSection::Tiling => {
            let mut lines = vec![
                "Periodic tiling editor".into(),
                "[P] preset · [N] prototype · +/- sides".into(),
            ];
            if let Some(tiling) = &state.draft().tiling {
                lines.push(format!(
                    "period {:.3} × {:.3} · {} prototypes · {} tiles",
                    tiling.translation_a.length(),
                    tiling.translation_b.length(),
                    tiling.prototypes.len(),
                    tiling.instances.len()
                ));
                lines.extend(tiling.prototypes.iter().map(|prototype| {
                    let marker = if Some(prototype.id) == state.tiling_prototype() {
                        "▸"
                    } else {
                        " "
                    };
                    format!("{marker} {}  {:?}", prototype.name, prototype.shape)
                }));
                match crate::sim::tiling::validate_coverage(tiling) {
                    Ok(report) => lines.push(format!(
                        "✓ coverage {:.4} · overlap {:.2e}",
                        report.covered_area, report.overlap_area
                    )),
                    Err(errors) => lines.extend(
                        errors
                            .iter()
                            .take(3)
                            .map(|error| format!("! {}", error.message)),
                    ),
                }
                match crate::sim::tiling::canonical_half_edges(tiling, 1e-9) {
                    Ok(edges) => lines.push(format!("✓ paired seams {}", edges.len())),
                    Err(errors) => lines.push(format!("! seams: {}", errors.join("; "))),
                }
            } else {
                lines.push("no polygon draft · [A] draw basis · [P] preset".into());
            }
            lines
        }
        WorkbenchSection::Channels => std::iter::once("Channel compositor".to_string())
            .chain(state.draft().channels.iter().map(|channel| {
                let selected = channel.id == state.selected_channel();
                let color = crate::workbench::resolved_color(state.draft(), channel.id)
                    .map(|color| format!("#{:02x}{:02x}{:02x}", color.red, color.green, color.blue))
                    .unwrap_or_else(|| "—".into());
                format!(
                    "{} {}  {}  {}  {}",
                    if selected { "▸" } else { " " },
                    channel.name,
                    color,
                    if channel.display.visible {
                        "visible"
                    } else {
                        "hidden"
                    },
                    if channel.frozen { "frozen" } else { "active" }
                )
            }))
            .collect(),
        WorkbenchSection::Kernels => std::iter::once("Kernel routing editor".to_string())
            .chain(state.draft().kernels.iter().map(|kernel| {
                format!(
                    "{}  ch{} → ch{}  {}×{}  {:?}",
                    kernel.symbol,
                    kernel.source.0,
                    kernel.target.0,
                    kernel.definition.width,
                    kernel.definition.height,
                    kernel.definition.normalization
                )
            }))
            .collect(),
        WorkbenchSection::Growth => {
            let editor = state.growth_editor();
            let mut lines = vec![
                editor.signature().to_string(),
                if state.growth_editing() {
                    "EDITING source · Esc finish".into()
                } else {
                    "[E] edit source".into()
                },
            ];
            lines.extend(
                editor
                    .buffer()
                    .as_str()
                    .lines()
                    .take(8)
                    .map(|line| format!("  {line}")),
            );
            lines.push(format!(
                "plot {}{}",
                plot_sparkline(&editor.plot().data, 36),
                if editor.plot().stale { "  STALE" } else { "" }
            ));
            lines.extend(
                editor
                    .diagnostics()
                    .iter()
                    .take(3)
                    .map(|diagnostic| format!("! {diagnostic}")),
            );
            lines
        }
        WorkbenchSection::Experiment => experiment_review_lines(state),
    };
    frame.render_widget(canvas_block, layout.canvas);
    let header = match state.section() {
        WorkbenchSection::Tiling => format!(
            "[D] Edit selected  [A] Draw new basis  [P] Next preset  [N] Next basis   tool: {:?}{}",
            state.tiling_tool(),
            if state.is_drawing_new_basis() {
                " · NEW BASIS"
            } else {
                ""
            },
        ),
        WorkbenchSection::Kernels => {
            "Click select · drag paint · wheel value · E exact · middle pan · empty wheel zoom"
                .into()
        }
        WorkbenchSection::Growth => {
            "Source editor and pixel plot · E edit · Esc finish · diagnostics update live".into()
        }
        WorkbenchSection::World => "Left paint · right erase · middle pan · wheel zoom".into(),
        WorkbenchSection::Channels => {
            "Add/remove channels · visibility · color · composite view".into()
        }
        WorkbenchSection::Experiment => "Review changes; Ctrl+Enter applies explicitly".into(),
    };
    let mut header_lines = static_canvas_header_lines(state, &header)
        .into_iter()
        .map(|line| Line::styled(line, Style::default().fg(Color::Rgb(150, 190, 240))))
        .collect::<Vec<_>>();
    if let Some(editor) = state.numeric_editor() {
        header_lines.truncate(1);
        header_lines.push(Line::from(vec![
            Span::styled(
                format!("Exact {} = ", editor.label()),
                Style::default()
                    .fg(Color::Rgb(255, 220, 130))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                editor.buffer().to_string(),
                Style::default()
                    .fg(Color::Rgb(255, 255, 255))
                    .bg(Color::Rgb(55, 85, 145)),
            ),
            Span::styled(
                "▌  Enter commit · Esc cancel",
                Style::default().fg(Color::White),
            ),
        ]));
    } else if let Some(buffer) = state.kernel_resize_editor() {
        header_lines.truncate(1);
        header_lines.push(Line::from(vec![
            Span::styled(
                "Resize width,height,anchor_x,anchor_y = ",
                Style::default()
                    .fg(Color::Rgb(255, 220, 130))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                buffer.to_string(),
                Style::default()
                    .fg(Color::Rgb(255, 255, 255))
                    .bg(Color::Rgb(55, 85, 145)),
            ),
            Span::styled(
                "▌  Enter commit · Esc cancel",
                Style::default().fg(Color::White),
            ),
        ]));
    }
    frame.render_widget(Paragraph::new(header_lines), canvas_header);
    if matches!(state.section(), WorkbenchSection::World) {
        let (width, height) = display.framebuffer_size(graphics_area);
        let mut graphics = app
            .workbench_initial_basis_scene(state.channel_view(), scene_generation)
            .map(|scene| scene.render_frame(width as u32, height as u32))
            .unwrap_or_else(|| {
                initial_field_graphics(state, *app.camera(), width as u32, height as u32)
            });
        graphics.generation = scene_generation;
        display.render_graphics(frame, graphics_area, &graphics);
    } else if state.section() == WorkbenchSection::Tiling {
        if let Some(tiling) = &state.draft().tiling {
            let scene = crate::workbench::tiling_editor::TilingScene::new(tiling.clone());
            let scene = scene
                .with_selected_basis(state.selected_basis())
                .with_camera(state.tiling_camera())
                .with_selected_vertex(state.tiling_selected_vertex().map(|(_, vertex)| vertex))
                .with_construction(state.tiling_construction().to_vec())
                .with_pointer(state.tiling_pointer());
            let (width, height) = display.framebuffer_size(graphics_area);
            let mut graphics = scene.render_rgba(width as u32, height as u32);
            graphics.generation = scene_generation;
            display.render_graphics(frame, graphics_area, &graphics);
        } else if state.is_drawing_new_basis() || !state.tiling_construction().is_empty() {
            let scene = crate::workbench::tiling_editor::TilingScene::empty(state.tiling_camera())
                .with_construction(state.tiling_construction().to_vec())
                .with_pointer(state.tiling_pointer());
            let (width, height) = display.framebuffer_size(graphics_area);
            let mut graphics = scene.render_rgba(width as u32, height as u32);
            graphics.generation = scene_generation;
            display.render_graphics(frame, graphics_area, &graphics);
        } else {
            frame.render_widget(
                Paragraph::new(canvas_lines.into_iter().map(Line::from).collect::<Vec<_>>())
                    .wrap(Wrap { trim: false }),
                canvas_inner,
            );
        }
    } else if state.section() == WorkbenchSection::Kernels {
        if let (Some(tiling), Some(rule_kernel)) = (
            state.draft().tiling.clone(),
            state.selected_rule_kernel().cloned(),
        ) && let crate::sim::ruleset::KernelSpatialDefinition::Periodic(definition) =
            rule_kernel.spatial
        {
            let scene = crate::workbench::kernel_editor::PeriodicKernelScene::new(
                tiling,
                definition,
                state.selected_basis(),
            )
            .with_view(state.kernel_view())
            .with_selected(state.periodic_kernel_selection());
            let (width, height) = display.framebuffer_size(graphics_area);
            let mut graphics = scene.render_rgba(width as u32, height as u32);
            graphics.generation = scene_generation;
            display.render_graphics(frame, graphics_area, &graphics);
        } else if let Some(definition) = state.selected_raster_kernel_definition() {
            let scene = crate::workbench::kernel_editor::KernelScene::new(definition.clone())
                .with_view(state.kernel_view())
                .with_selected(state.kernel_selection());
            let (width, height) = display.framebuffer_size(graphics_area);
            let mut graphics = scene.render_rgba(width as u32, height as u32);
            graphics.generation = scene_generation;
            display.render_graphics(frame, graphics_area, &graphics);
        }
    } else if state.section() == WorkbenchSection::Growth {
        let source_height = canvas_inner.height.saturating_sub(graphics_area.height);
        let source_area = Rect::new(
            canvas_inner.x,
            canvas_inner.y,
            canvas_inner.width,
            source_height,
        );
        let editor = state.growth_editor();
        let mut source_lines = vec![
            Line::styled(
                format!(
                    "target: basis {} / channel {}",
                    state.selected_basis().0,
                    state.selected_channel().0
                ),
                Style::default().fg(Color::Rgb(120, 170, 230)),
            ),
            Line::styled(
                editor.signature().to_string(),
                Style::default()
                    .fg(Color::Rgb(255, 220, 130))
                    .add_modifier(Modifier::BOLD),
            ),
            Line::styled(
                editor.plot_caption(),
                Style::default().fg(Color::Rgb(145, 195, 235)),
            ),
        ];
        source_lines.extend(growth_source_preview(editor));
        if !editor.diagnostics().is_empty() {
            source_lines.push(Line::styled(
                editor.diagnostics().join(" · "),
                Style::default().fg(Color::Rgb(255, 95, 105)),
            ));
        }
        frame.render_widget(
            Paragraph::new(source_lines)
                .wrap(Wrap { trim: false })
                .block(
                    Block::default()
                        .title(if state.growth_editing() {
                            " Source — EDITING "
                        } else {
                            " Source — press E "
                        })
                        .borders(Borders::BOTTOM),
                ),
            source_area,
        );
        let scene = crate::workbench::growth_graph::GrowthScene::from_editor(state.growth_editor());
        let (width, height) = display.framebuffer_size(graphics_area);
        let mut graphics = scene.render_rgba(width as u32, height as u32);
        graphics.generation = scene_generation;
        display.render_graphics(frame, graphics_area, &graphics);
    } else if state.section() == WorkbenchSection::Channels {
        let (width, height) = display.framebuffer_size(graphics_area);
        let runtime_generation =
            scene_generation.wrapping_add(app.render_generation().rotate_left(23));
        let mut graphics = authoritative_channel_graphics(
            app,
            state,
            width as u32,
            height as u32,
            runtime_generation,
        );
        graphics.generation = runtime_generation;
        display.render_graphics(frame, graphics_area, &graphics);
    } else {
        frame.render_widget(
            Paragraph::new(canvas_lines.into_iter().map(Line::from).collect::<Vec<_>>())
                .wrap(Wrap { trim: false }),
            canvas_inner,
        );
    }
    display.apply_placement_action(frame, graphics_area, placement_action);
    if let Some(inspector) = layout.inspector {
        let selected = state
            .draft()
            .channels
            .iter()
            .find(|channel| channel.id == state.selected_channel());
        let mut lines = vec![
            Line::from(format!("section: {}", state.section().label())),
            Line::from(format!("draft: {:?}", state.status())),
            Line::from(format!("channels: {}", state.draft().channels.len())),
            Line::from(format!("kernels: {}", inspector_kernel_count(state))),
            Line::from(format!(
                "selected: {}",
                selected.map_or("—", |c| c.name.as_str())
            )),
            Line::from(format!("view: {:?}", state.channel_view())),
            Line::from(""),
            Line::from("Click section · T section · Tab focus"),
        ];
        match state.section() {
            WorkbenchSection::World => {
                lines.push(Line::from("Canvas: left paint · right erase"));
                lines.push(Line::from("Wheel zoom · middle pan"));
            }
            WorkbenchSection::Tiling => {
                lines.push(Line::from(format!("tool: {:?}", state.tiling_tool())));
                lines.push(Line::from(format!(
                    "construction vertices: {}",
                    state.tiling_construction().len()
                )));
                lines.push(Line::from("D redraw selected · A draw a new basis"));
                lines.push(Line::from(
                    "Click vertices · click first/Enter close · Esc cancel",
                ));
                lines.push(Line::from("Select: drag vertex · right remove"));
                lines.push(Line::from("P preset · N basis · +/- regular sides"));
                lines.push(Line::from(format!(
                    "S solve full-edge seams · linked seams: {}",
                    state.tiling_constraint_count()
                )));
                lines.push(Line::from("Wheel zoom · middle pan"));
                lines.push(Line::from(""));
                lines.extend(tiling_inspector_texts(state).into_iter().map(Line::from));
            }
            WorkbenchSection::Channels => {
                lines.push(Line::from("A add · Del remove · ] select"));
                lines.push(Line::from(
                    "V view · C palette · E exact color · X visible · F freeze",
                ));
                if let Some(color) = state.color_editor() {
                    lines.push(Line::from(format!("color = {color}▌")));
                    lines.push(Line::from("Enter commit · Esc cancel"));
                }
                lines.push(Line::from(""));
                lines.extend(channel_inspector_texts(state).into_iter().map(Line::from));
            }
            WorkbenchSection::Kernels => {
                if let Some(rule) = state
                    .selected_rule_set()
                    .and_then(|id| state.draft().rules.get(id))
                {
                    lines.push(Line::from(format!(
                        "target: basis {} · channel {}",
                        state.selected_basis().0,
                        state.selected_channel().0,
                    )));
                    lines.push(Line::from(format!(
                        "rule-set {} · {} · {} kernel(s)",
                        rule.id.0,
                        rule.shared_name.as_deref().unwrap_or("local/default"),
                        rule.kernels.len(),
                    )));
                    let default = state
                        .draft()
                        .rules
                        .defaults
                        .get(&state.selected_channel())
                        .is_some_and(|default| *default == rule.id);
                    let sharing = state
                        .draft()
                        .rules
                        .bindings
                        .iter()
                        .filter(|binding| binding.rule_set == rule.id)
                        .count();
                    lines.push(Line::from(format!(
                        "sharing: {} · {} binding(s)",
                        if default {
                            "channel default"
                        } else {
                            "local override"
                        },
                        sharing,
                    )));
                    if let Some(kernel) = state.selected_rule_kernel() {
                        let source_name = state
                            .draft()
                            .channels
                            .iter()
                            .find(|channel| channel.id == kernel.source_channel)
                            .map_or("—", |channel| channel.name.as_str());
                        lines.push(Line::from(format!(
                            "kernel {} `{}` · source {} ({})",
                            kernel.id.0, kernel.symbol, kernel.source_channel.0, source_name,
                        )));
                        match &kernel.spatial {
                            crate::sim::ruleset::KernelSpatialDefinition::Periodic(definition) => {
                                lines.push(Line::from(format!(
                                    "stencil {}×{} · anchor {},{} · {} source basis plane(s)",
                                    definition.width,
                                    definition.height,
                                    definition.anchor_x,
                                    definition.anchor_y,
                                    definition.planes.len(),
                                )));
                                lines.push(Line::from(format!(
                                    "source bases: {}",
                                    definition
                                        .planes
                                        .keys()
                                        .map(|basis| basis.0.to_string())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                )));
                                let active_values = definition
                                    .planes
                                    .values()
                                    .flat_map(|plane| {
                                        plane.values.iter().enumerate().filter_map(
                                            move |(index, value)| {
                                                plane
                                                    .mask
                                                    .as_ref()
                                                    .is_none_or(|mask| mask[index])
                                                    .then_some(*value)
                                            },
                                        )
                                    })
                                    .collect::<Vec<_>>();
                                let sum = active_values.iter().copied().sum::<f32>();
                                let absolute_sum =
                                    active_values.iter().map(|value| value.abs()).sum::<f32>();
                                let minimum = active_values
                                    .iter()
                                    .copied()
                                    .reduce(f32::min)
                                    .unwrap_or(0.0);
                                let maximum = active_values
                                    .iter()
                                    .copied()
                                    .reduce(f32::max)
                                    .unwrap_or(0.0);
                                lines.push(Line::from(format!(
                                    "raw Σ={sum:.4} · |Σ|={absolute_sum:.4} · min={minimum:.4} · max={maximum:.4}",
                                )));
                            }
                            crate::sim::ruleset::KernelSpatialDefinition::Raster(definition) => {
                                lines.push(Line::from(format!(
                                    "raster stencil {}×{} · anchor {},{} · {:?}",
                                    definition.width,
                                    definition.height,
                                    definition.anchor_x,
                                    definition.anchor_y,
                                    definition.normalization,
                                )));
                            }
                        }
                    }
                } else if let Some(kernel) = state.selected_legacy_kernel() {
                    lines.push(Line::from(format!(
                        "legacy kernel {} `{}` · source ch{} → target ch{}",
                        kernel.id.0, kernel.symbol, kernel.source.0, kernel.target.0,
                    )));
                    lines.push(Line::from(format!(
                        "stencil {}×{} · anchor {},{} · {:?}",
                        kernel.definition.width,
                        kernel.definition.height,
                        kernel.definition.anchor_x,
                        kernel.definition.anchor_y,
                        kernel.definition.normalization,
                    )));
                }
                match state.kernel_tool() {
                    crate::workbench::kernel_editor::KernelTool::Weights => {
                        lines.push(Line::from(
                            "Weights: left/drag paint · right zero · inactive is locked",
                        ));
                        lines.push(Line::from("Active wheel ±0.05 · Shift ±0.005 · Ctrl ±0.5"));
                        lines.push(Line::from(
                            "Inactive/empty wheel zoom · middle pan · E exact value",
                        ));
                    }
                    crate::workbench::kernel_editor::KernelTool::Support => {
                        lines.push(Line::from(
                            "Support: left/drag activate · right/drag deactivate",
                        ));
                        lines.push(Line::from(
                            "Deactivation clears weight · wheel zoom · middle pan",
                        ));
                    }
                }
                lines.push(Line::from("A add kernel · Del remove · ] next kernel"));
                lines.push(Line::from("S source channel · U output channel"));
                lines.push(Line::from("R resize stencil/anchor"));
                lines.push(Line::from(format!(
                    "Q metric {:?} · G sigma {:.6} · P apply Gaussian",
                    state.kernel_sampling_metric(),
                    state.kernel_gaussian_sigma(),
                )));
                lines.push(Line::from("Channel count: Channels section A/Del"));
                lines.push(Line::from(format!(
                    "paint value: {:.4}",
                    state.kernel_paint_value()
                )));
                if let Some(point) = state.kernel_selection() {
                    lines.push(Line::from(format!(
                        "selected cell: {}, {}",
                        point.x, point.y
                    )));
                }
                if let Some(selection) = state.periodic_kernel_selection() {
                    lines.push(Line::from(format!(
                        "selected: offset [{},{}] · source basis {}",
                        selection.offset[0], selection.offset[1], selection.source_basis.0,
                    )));
                    if let Some(kernel) = state.selected_rule_kernel()
                        && let crate::sim::ruleset::KernelSpatialDefinition::Periodic(definition) =
                            &kernel.spatial
                    {
                        let active = definition
                            .is_active(selection.offset, selection.source_basis)
                            .unwrap_or(false);
                        lines.push(Line::from(format!(
                            "active: {}",
                            if active { "yes" } else { "no" }
                        )));
                        lines.push(Line::from(if active {
                            format!(
                                "weight: {:.6}",
                                definition
                                    .raw_weight(selection.offset, selection.source_basis)
                                    .unwrap_or(0.0)
                            )
                        } else {
                            "weight: —".to_string()
                        }));
                    }
                }
                if let Some(editor) = state.numeric_editor() {
                    lines.push(Line::from(format!(
                        "{} = {}▌",
                        editor.label(),
                        editor.buffer()
                    )));
                    lines.push(Line::from("Enter commit · Esc cancel"));
                }
            }
            WorkbenchSection::Growth => {
                lines.extend(growth_inspector_texts(state).into_iter().map(Line::from));
            }
            WorkbenchSection::Experiment => {
                lines.push(Line::from(format!(
                    "simulation dt: {} · D edit",
                    state.draft().simulation_dt
                )));
                lines.push(Line::from("Ctrl+Enter Apply & Run"));
                if let Some(editor) = state.numeric_editor() {
                    lines.push(Line::from(format!(
                        "{} = {}▌",
                        editor.label(),
                        editor.buffer()
                    )));
                    lines.push(Line::from("Enter commit · Esc cancel"));
                }
            }
        }
        lines.extend([
            Line::from("Ctrl+Z/Y undo/redo · Ctrl+Enter Apply & Run"),
            Line::from("Ctrl+S workspace · Ctrl+E/L draft"),
            Line::from(app.workbench_notice().unwrap_or("")),
            Line::from("W leave Workbench · ? help"),
        ]);
        frame.render_widget(
            Paragraph::new(lines)
                .scroll((
                    if state.section() == WorkbenchSection::Growth {
                        state.growth_help_scroll()
                    } else {
                        0
                    },
                    0,
                ))
                .wrap(Wrap { trim: false })
                .block(panel(
                    if state.section() == WorkbenchSection::Growth {
                        " Inspector · syntax "
                    } else {
                        " Inspector "
                    },
                    state.focus() == WorkbenchFocus::Inspector,
                )),
            inspector,
        );
    }
    let (frame_width, frame_height) = display.framebuffer_size(graphics_area);
    app.set_viewport(graphics_area, [frame_width, frame_height]);
}

fn channel_inspector_texts(state: &crate::workbench::WorkbenchState) -> Vec<String> {
    state
        .draft()
        .channels
        .iter()
        .map(|channel| {
            let color = crate::workbench::resolved_color(state.draft(), channel.id)
                .map(|color| format!("#{:02X}{:02X}{:02X}", color.red, color.green, color.blue))
                .unwrap_or_else(|| "#??????".into());
            format!(
                "{} {}  {}  {}  {}",
                if channel.id == state.selected_channel() {
                    "▸"
                } else {
                    " "
                },
                channel.name,
                color,
                if channel.display.visible {
                    "visible"
                } else {
                    "hidden"
                },
                if channel.frozen { "frozen" } else { "active" },
            )
        })
        .collect()
}

fn tiling_inspector_texts(state: &crate::workbench::WorkbenchState) -> Vec<String> {
    let Some(tiling) = &state.draft().tiling else {
        return vec![
            "No periodic tiling yet".into(),
            "Click [A] to draw a basis or [P] for a preset".into(),
        ];
    };
    let selected_prototype = state.tiling_prototype();
    let selected_name = selected_prototype
        .and_then(|id| {
            tiling
                .prototypes
                .iter()
                .find(|prototype| prototype.id == id)
        })
        .map_or("unnamed", |prototype| prototype.name.as_str());
    let mut lines = vec![
        format!(
            "central cell: {} {} · {} shape {}",
            tiling.instances.len(),
            if tiling.instances.len() == 1 {
                "polygon"
            } else {
                "polygons"
            },
            tiling.prototypes.len(),
            if tiling.prototypes.len() == 1 {
                "type"
            } else {
                "types"
            },
        ),
        format!(
            "editing polygon: basis {} · shape {} ({selected_name})",
            state.selected_basis().0,
            selected_prototype.map_or_else(|| "—".into(), |id| id.0.to_string())
        ),
        format!(
            "a=({:.3}, {:.3})",
            tiling.translation_a.x, tiling.translation_a.y
        ),
        format!(
            "b=({:.3}, {:.3})",
            tiling.translation_b.x, tiling.translation_b.y
        ),
        format!(
            "linked full-edge seams: {}",
            state.tiling_constraint_count()
        ),
    ];
    match crate::sim::tiling::validate_coverage(tiling) {
        Ok(report) => {
            let neighbors = report
                .neighbor_ring
                .get(&state.selected_basis())
                .map_or(0, Vec::len);
            lines.push("✓ exact edge-to-edge tiling".into());
            lines.push(format!(
                "area {:.4} · atomic edges {}",
                report.patch_area, report.atomic_edges
            ));
            lines.push(format!(
                "Euler {} · neighbor seams {}",
                report.euler_characteristic, neighbors
            ));
        }
        Err(errors) => {
            lines.push("! not a valid periodic tiling".into());
            let mut guidance = Vec::new();
            for error in errors {
                let message = tiling_diagnostic_guidance(error.code);
                if !guidance.contains(&message) {
                    guidance.push(message);
                }
                if guidance.len() == 4 {
                    break;
                }
            }
            lines.extend(guidance.into_iter().map(|message| format!("! {message}")));
        }
    }
    lines
}

fn tiling_diagnostic_guidance(code: &str) -> &'static str {
    match code {
        "coverage_gap" => {
            "Gap: the polygons do not fill one period. Extend an edge or add the missing polygon."
        }
        "coverage_overlap" | "coverage_multiplicity" => {
            "Overlap: polygons cover part of the period more than once. Move or reshape them."
        }
        "proper_crossing" | "self_intersection" => {
            "Crossing edges: boundary edges may meet, but must not cross through each other."
        }
        "unmatched_atomic_edge" => {
            "Open seam: this edge has no matching opposite edge in a neighboring cell."
        }
        "competing_twins" | "incompatible_collinear_overlap" => {
            "Ambiguous seam: an edge matches multiple boundaries. Remove the overlap."
        }
        "invalid_period" => {
            "Invalid period: the two translation arrows must be finite and non-parallel."
        }
        "non_ccw_or_degenerate" | "zero_edge" | "zero_length_fragment" => {
            "Invalid polygon: keep at least three distinct corners in counter-clockwise order."
        }
        "too_few_vertices" => "Incomplete polygon: add at least three corners, then close it.",
        "too_many_vertices" => "Polygon is too complex: use at most 64 corners.",
        "unknown_tile" | "unknown_prototype" | "empty_arrangement" => {
            "Missing shape: add or select a polygon for this periodic cell."
        }
        code if code.starts_with("budget_") || code.ends_with("_overflow") => {
            "Geometry is too large or dense to validate safely. Simplify it or increase its scale."
        }
        _ => "Geometry could not be validated. Check polygon order, seams, gaps, and overlaps.",
    }
}

fn initial_field_graphics(
    state: &crate::workbench::WorkbenchState,
    camera: crate::render::camera::Camera,
    width: u32,
    height: u32,
) -> crate::render::workbench_graphics::GraphicsFrame {
    let width = width.max(1);
    let height = height.max(1);
    let (grid_width, grid_height) = match &state.draft().geometry {
        crate::sim::experiment_model::GeometrySpec::RasterGrid(grid) => (grid.width, grid.height),
    };
    let colors = state
        .draft()
        .channels
        .iter()
        .map(|channel| {
            crate::workbench::resolved_color(state.draft(), channel.id)
                .unwrap_or(crate::render::channels::Rgb8::new(255, 255, 255))
        })
        .collect::<Vec<_>>();
    let selected = state.selected_channel();
    let mut rgba = vec![0_u8; width as usize * height as usize * 4];
    for y in 0..height {
        for x in 0..width {
            let world = camera.screen_to_world(
                [x as f32 + 0.5, y as f32 + 0.5],
                width as usize,
                height as usize,
            );
            let wx = (world[0].floor() as isize).rem_euclid(grid_width as isize) as usize;
            let wy = (world[1].floor() as isize).rem_euclid(grid_height as isize) as usize;
            let tile = wy * grid_width as usize + wx;
            let mut values = Vec::new();
            let mut active_colors = Vec::new();
            for (index, channel) in state.draft().channels.iter().enumerate() {
                if channel.display.visible
                    && (state.channel_view() == crate::workbench::ChannelView::Composite
                        || channel.id == selected)
                {
                    values.push(channel.initial.get(tile).copied().unwrap_or(0.0));
                    active_colors.push(colors[index]);
                }
            }
            let pixel = crate::render::channels::composite_pixel(&values, &active_colors);
            let offset = (y as usize * width as usize + x as usize) * 4;
            rgba[offset..offset + 4].copy_from_slice(&[pixel.red, pixel.green, pixel.blue, 255]);
        }
    }
    crate::render::workbench_graphics::GraphicsFrame::new(width, height, rgba, 0)
        .expect("initial field dimensions are valid")
}

fn channel_graphics(
    state: &crate::workbench::WorkbenchState,
    width: u32,
    height: u32,
) -> crate::render::workbench_graphics::GraphicsFrame {
    let width = width.max(1);
    let height = height.max(1);
    let (grid_width, grid_height) = match &state.draft().geometry {
        crate::sim::experiment_model::GeometrySpec::RasterGrid(grid) => {
            (grid.width as usize, grid.height as usize)
        }
    };
    let channels = &state.draft().channels;
    let colors = channels
        .iter()
        .map(|channel| {
            crate::workbench::resolved_color(state.draft(), channel.id)
                .unwrap_or(crate::render::channels::Rgb8::new(255, 255, 255))
        })
        .collect::<Vec<_>>();
    let selected = channels
        .iter()
        .position(|channel| channel.id == state.selected_channel())
        .unwrap_or(0);
    let columns = (channels.len() as f64).sqrt().ceil().max(1.0) as usize;
    let rows = channels.len().div_ceil(columns).max(1);
    let mut rgba = vec![0_u8; width as usize * height as usize * 4];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let (panel, local_x, local_y, panel_width, panel_height) =
                if state.channel_view() == crate::workbench::ChannelView::Grid {
                    let panel_width = (width as usize).div_ceil(columns).max(1);
                    let panel_height = (height as usize).div_ceil(rows).max(1);
                    let column = x / panel_width;
                    let row = y / panel_height;
                    (
                        Some(row * columns + column),
                        x % panel_width,
                        y % panel_height,
                        panel_width,
                        panel_height,
                    )
                } else {
                    (None, x, y, width as usize, height as usize)
                };
            let wx = local_x * grid_width / panel_width.max(1);
            let wy = local_y * grid_height / panel_height.max(1);
            let tile = wy.min(grid_height.saturating_sub(1)) * grid_width
                + wx.min(grid_width.saturating_sub(1));
            let mut values = Vec::new();
            let mut active_colors = Vec::new();
            for index in 0..channels.len() {
                let include = channels[index].display.visible
                    && match panel {
                        Some(panel) => panel == index,
                        None => {
                            state.channel_view() == crate::workbench::ChannelView::Composite
                                || index == selected
                        }
                    };
                if include {
                    values.push(channels[index].initial.get(tile).copied().unwrap_or(0.0));
                    active_colors.push(colors[index]);
                }
            }
            let pixel = crate::render::channels::composite_pixel(&values, &active_colors);
            let offset = (y * width as usize + x) * 4;
            rgba[offset..offset + 4].copy_from_slice(&[pixel.red, pixel.green, pixel.blue, 255]);
        }
    }
    crate::render::workbench_graphics::GraphicsFrame::new(width, height, rgba, 0)
        .expect("channel graphics dimensions are valid")
}

fn authoritative_channel_graphics(
    app: &crate::app::App,
    state: &crate::workbench::WorkbenchState,
    width: u32,
    height: u32,
    generation: u64,
) -> crate::render::workbench_graphics::GraphicsFrame {
    let width = width.max(1);
    let height = height.max(1);
    if state.channel_view() != crate::workbench::ChannelView::Grid {
        if let Some(scene) = app.workbench_basis_scene(state.channel_view(), generation) {
            return scene.render_frame(width, height);
        }
        return channel_graphics(state, width, height);
    }

    let channel_count = state.draft().channels.len().max(1);
    let columns = (channel_count as f64).sqrt().ceil().max(1.0) as usize;
    let rows = channel_count.div_ceil(columns).max(1);
    let mut rgba = vec![0_u8; width as usize * height as usize * 4];
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[
            crate::render::channels::OUTSIDE_DOMAIN.red,
            crate::render::channels::OUTSIDE_DOMAIN.green,
            crate::render::channels::OUTSIDE_DOMAIN.blue,
            255,
        ]);
    }
    for channel in 0..channel_count {
        let column = channel % columns;
        let row = channel / columns;
        let left = column * width as usize / columns;
        let right = (column + 1) * width as usize / columns;
        let top = row * height as usize / rows;
        let bottom = (row + 1) * height as usize / rows;
        let panel_width = right.saturating_sub(left).max(1);
        let panel_height = bottom.saturating_sub(top).max(1);
        let Some(scene) = app.workbench_basis_scene_channel(channel, generation) else {
            continue;
        };
        let panel = scene.render_frame(panel_width as u32, panel_height as u32);
        for py in 0..panel_height {
            for px in 0..panel_width {
                let source = (py * panel_width + px) * 4;
                let target = ((top + py) * width as usize + left + px) * 4;
                rgba[target..target + 4].copy_from_slice(&panel.rgba[source..source + 4]);
            }
        }
    }
    crate::render::workbench_graphics::GraphicsFrame::new(width, height, rgba, generation)
        .expect("channel grid dimensions are valid")
}

fn growth_source_preview(editor: &crate::workbench::GrowthEditorState) -> Vec<Line<'static>> {
    let text = editor.buffer().as_str();
    let cursor = editor.buffer().cursor();
    let selection = editor
        .buffer()
        .selection()
        .map(|selection| selection.range());
    let mut lines = Vec::new();
    let mut offset = 0usize;
    for (line_number, source_line) in text.split('\n').take(12).enumerate() {
        let end = offset + source_line.len();
        let mut spans = vec![Span::styled(
            format!("{:>3} ", line_number + 1),
            Style::default().fg(Color::Rgb(80, 95, 120)),
        )];
        for (relative, character) in source_line.char_indices() {
            let absolute = offset + relative;
            if editor.buffer().cursor_is_char_boundary() && cursor == absolute {
                spans.push(Span::styled(
                    "▌",
                    Style::default().fg(Color::Rgb(255, 220, 90)),
                ));
            }
            let selected = selection
                .as_ref()
                .is_some_and(|range| range.contains(&absolute));
            let style = if selected {
                Style::default()
                    .fg(Color::Rgb(255, 255, 255))
                    .bg(Color::Rgb(55, 85, 145))
            } else {
                growth_source_style(source_line, relative)
            };
            spans.push(Span::styled(character.to_string(), style));
        }
        if cursor == end {
            spans.push(Span::styled(
                "▌",
                Style::default().fg(Color::Rgb(255, 220, 90)),
            ));
        }
        lines.push(Line::from(spans));
        offset = end.saturating_add(1);
    }
    if lines.is_empty() {
        lines.push(Line::from("  1 ▌"));
    }
    lines
}

fn growth_source_style(line: &str, byte: usize) -> Style {
    if line.get(..byte).is_some_and(|prefix| prefix.contains("//")) {
        return Style::default().fg(Color::Rgb(100, 140, 110));
    }
    let is_word = |character: char| character == '_' || character.is_alphanumeric();
    let start = line[..byte]
        .char_indices()
        .rev()
        .take_while(|(_, character)| is_word(*character))
        .last()
        .map_or(byte, |(index, _)| index);
    let end = line[byte..]
        .char_indices()
        .take_while(|(_, character)| is_word(*character))
        .last()
        .map_or(byte, |(index, character)| {
            byte + index + character.len_utf8()
        });
    let token = &line[start..end];
    if matches!(token, "let" | "if" | "else" | "true" | "false") {
        Style::default()
            .fg(Color::Rgb(215, 145, 255))
            .add_modifier(Modifier::BOLD)
    } else if line[byte..]
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_digit())
    {
        Style::default().fg(Color::Rgb(255, 190, 105))
    } else {
        Style::default().fg(Color::Rgb(220, 225, 235))
    }
}

#[allow(dead_code)]
struct ChannelCanvas {
    width: usize,
    height: usize,
    planes: Vec<Vec<f32>>,
    colors: Vec<crate::render::channels::Rgb8>,
    visible: Vec<bool>,
    selected: usize,
    view: crate::workbench::ChannelView,
}

impl ChannelCanvas {
    #[allow(dead_code)]
    fn new(state: &crate::workbench::WorkbenchState) -> Self {
        let (width, height) = match &state.draft().geometry {
            crate::sim::experiment_model::GeometrySpec::RasterGrid(grid) => {
                (grid.width as usize, grid.height as usize)
            }
        };
        let channels = &state.draft().channels;
        let colors = channels
            .iter()
            .map(|channel| {
                crate::workbench::resolved_color(state.draft(), channel.id)
                    .unwrap_or(crate::render::channels::Rgb8::new(255, 255, 255))
            })
            .collect::<Vec<_>>();
        let selected = channels
            .iter()
            .position(|channel| channel.id == state.selected_channel())
            .unwrap_or(0);
        Self {
            width,
            height,
            planes: channels
                .iter()
                .map(|channel| channel.initial.clone())
                .collect(),
            colors,
            visible: channels
                .iter()
                .map(|channel| channel.display.visible)
                .collect(),
            selected,
            view: state.channel_view(),
        }
    }
}

impl Widget for ChannelCanvas {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() || self.width == 0 || self.height == 0 {
            return;
        }
        for sy in 0..area.height {
            for sx in 0..area.width {
                let (plane, local_x, local_y, local_width, local_height) =
                    if self.view == crate::workbench::ChannelView::Grid {
                        let columns = (self.planes.len() as f32).sqrt().ceil().max(1.0) as usize;
                        let rows = self.planes.len().div_ceil(columns).max(1);
                        let cell_width = usize::from(area.width).div_ceil(columns).max(1);
                        let cell_height = usize::from(area.height).div_ceil(rows).max(1);
                        let column = usize::from(sx) / cell_width;
                        let row = usize::from(sy) / cell_height;
                        (
                            Some(row * columns + column),
                            usize::from(sx) % cell_width,
                            usize::from(sy) % cell_height,
                            cell_width,
                            cell_height,
                        )
                    } else {
                        (
                            None,
                            usize::from(sx),
                            usize::from(sy),
                            usize::from(area.width),
                            usize::from(area.height),
                        )
                    };
                let wx = local_x * self.width / local_width.max(1);
                let wy = local_y * self.height / local_height.max(1);
                let tile = wy * self.width + wx;
                let pixel = if let Some(index) = plane {
                    if index < self.planes.len() && self.visible[index] {
                        crate::render::channels::composite_pixel(
                            &[self.planes[index].get(tile).copied().unwrap_or(0.0)],
                            &[self.colors[index]],
                        )
                    } else {
                        crate::render::channels::Rgb8::new(0, 0, 0)
                    }
                } else {
                    let mut values = Vec::new();
                    let mut colors = Vec::new();
                    for index in 0..self.planes.len() {
                        let include = self.visible[index]
                            && (self.view == crate::workbench::ChannelView::Composite
                                || index == self.selected);
                        if include {
                            values.push(self.planes[index].get(tile).copied().unwrap_or(0.0));
                            colors.push(self.colors[index]);
                        }
                    }
                    crate::render::channels::composite_pixel(&values, &colors)
                };
                if let Some(cell) = buf.cell_mut((area.x + sx, area.y + sy)) {
                    cell.set_symbol(" ");
                    cell.set_bg(Color::Rgb(pixel.red, pixel.green, pixel.blue));
                }
            }
        }
    }
}

fn plot_sparkline(values: &[Option<f32>], width: usize) -> String {
    let finite = values.iter().flatten().copied().collect::<Vec<_>>();
    if finite.is_empty() {
        return "—".into();
    }
    let min = finite.iter().copied().fold(f32::INFINITY, f32::min);
    let max = finite.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let glyphs = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    values
        .iter()
        .take(width)
        .map(|value| {
            value.map_or('×', |value| {
                let ratio = if (max - min).abs() <= f32::EPSILON {
                    0.5
                } else {
                    ((value - min) / (max - min)).clamp(0.0, 1.0)
                };
                glyphs[(ratio * 7.0).round() as usize]
            })
        })
        .collect()
}

fn panel(title: &'static str, focused: bool) -> Block<'static> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(
            Style::default()
                .fg(if focused {
                    Color::Rgb(245, 190, 90)
                } else {
                    Color::Rgb(96, 140, 220)
                })
                .add_modifier(if focused {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wide_has_three_regions_and_narrow_two() {
        assert!(
            workbench_layout(Rect::new(0, 0, 180, 50))
                .inspector
                .is_some()
        );
        assert!(
            workbench_layout(Rect::new(0, 0, 80, 30))
                .inspector
                .is_none()
        );
    }
    #[test]
    fn draft_status_debug_is_stable() {
        assert_eq!(
            format!("{:?}", crate::workbench::DraftStatus::Dirty),
            "Dirty"
        );
    }

    #[test]
    fn three_channel_inspector_explains_the_rgb_composite() {
        let mut state = crate::workbench::WorkbenchState::new(
            crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(4, 4),
        );
        state.add_channel().unwrap();
        state.add_channel().unwrap();
        let text = channel_inspector_texts(&state).join("\n");
        assert!(text.contains("state  #FF0000"));
        assert!(text.contains("channel_2  #00FF00"));
        assert!(text.contains("channel_3  #0000FF"));
        assert!(text.contains("visible"));
        assert!(text.contains("active"));
    }

    #[test]
    fn inspector_kernel_count_uses_the_selected_normalized_rule_set() {
        let spec = crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(4, 4)
            .normalize_rules()
            .unwrap();
        let state = crate::workbench::WorkbenchState::new(spec);

        assert_eq!(inspector_kernel_count(&state), 1);
    }

    #[test]
    fn inspector_kernel_count_uses_the_selected_legacy_output() {
        let mut state = crate::workbench::WorkbenchState::new(
            crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(4, 4),
        );
        state.add_channel().unwrap();

        assert_eq!(state.draft().kernels.len(), 2);
        assert_eq!(inspector_kernel_count(&state), 1);
    }

    #[test]
    fn world_graphics_respects_the_selected_solo_channel_color() {
        let mut spec = crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(1, 1);
        spec.channels[0].initial[0] = 1.0;
        spec.add_channel("green", false);
        let blue = spec.add_channel("blue", false);
        spec.channels
            .iter_mut()
            .find(|channel| channel.id == blue)
            .unwrap()
            .initial[0] = 1.0;
        let mut state = crate::workbench::WorkbenchState::new(spec);
        state.select_next_channel();
        state.select_next_channel();
        state.set_channel_view(crate::workbench::ChannelView::Solo);

        let frame = initial_field_graphics(
            &state,
            crate::render::camera::Camera::new([0.5, 0.5], 1.0),
            1,
            1,
        );

        assert_eq!(&frame.rgba[..4], &[0, 0, 255, 255]);
    }

    #[test]
    fn visible_toolbar_labels_have_clickable_hit_targets() {
        let mut state = crate::workbench::WorkbenchState::new(
            crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(4, 4),
        );
        state.select_section(WorkbenchSection::Channels);
        let text = toolbar_text(&state);
        assert!(text.starts_with("[A] Add"));
        assert_eq!(
            toolbar_action_at(&state, 2),
            Some(ToolbarAction::Ui(UiCommand::ContextAdd))
        );
        let color = text.find("[C] Color").unwrap() as u16 + 2;
        assert_eq!(
            toolbar_action_at(&state, color),
            Some(ToolbarAction::Ui(UiCommand::CycleColor))
        );
    }

    #[test]
    fn world_toolbar_exposes_channel_selection_and_view() {
        let mut state = crate::workbench::WorkbenchState::new(
            crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(4, 4),
        );
        state.select_section(WorkbenchSection::World);
        let text = toolbar_text(&state);
        assert!(text.contains("] Channel"));
        assert!(text.contains("[V] View"));
        assert_eq!(
            toolbar_action_at(&state, 2),
            Some(ToolbarAction::Ui(UiCommand::SelectNext))
        );
    }

    #[test]
    fn experiment_toolbar_exposes_clickable_apply_and_revert_actions() {
        let mut state = crate::workbench::WorkbenchState::new(
            crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(4, 4),
        );
        state.select_section(WorkbenchSection::Experiment);
        let text = toolbar_text(&state);
        assert!(text.contains("[Ctrl+Enter] Apply"));
        assert!(text.contains("[Ctrl+R] Revert"));
        let apply = text.find("[Ctrl+Enter] Apply").unwrap() as u16 + 2;
        let revert = text.find("[Ctrl+R] Revert").unwrap() as u16 + 2;
        assert_eq!(
            toolbar_action_at(&state, apply),
            Some(ToolbarAction::Ui(UiCommand::ApplyDraft))
        );
        assert_eq!(
            toolbar_action_at(&state, revert),
            Some(ToolbarAction::Ui(UiCommand::RevertDraft))
        );
    }

    #[test]
    fn growth_toolbar_and_inspector_explain_mode_rate_and_language() {
        let mut state = crate::workbench::WorkbenchState::new(
            crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(4, 4),
        );
        state.select_section(WorkbenchSection::Growth);

        assert!(toolbar_text(&state).contains("[M] Mode: Rate"));
        let text = growth_inspector_texts(&state).join("\n");
        assert!(text.contains("next = clamp(self + dt × result, 0, 1)"));
        assert!(text.contains("dt = 0.1"));
        assert!(text.contains("final expression is the result"));
        assert!(text.contains("if condition { expression } else { expression }"));
        assert!(text.contains("gauss(x, mu, sigma)"));
        assert!(text.contains("clamp(x, lo, hi)"));
        assert!(text.contains("clamp means: below lo → lo; above hi → hi"));
    }

    #[test]
    fn experiment_review_reports_real_differences_and_apply_consequences() {
        let mut state = crate::workbench::WorkbenchState::new(
            crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(4, 4),
        );
        state.add_channel().unwrap();

        let text = experiment_review_lines(&state).join("\n");

        assert!(
            text.contains("VALID · ready to apply"),
            "review was:\n{text}"
        );
        assert!(
            text.contains("Channels        1 → 2"),
            "review was:\n{text}"
        );
        assert!(text.contains("+ channel_2"), "review was:\n{text}");
        assert!(
            !text.contains("(channel 1)"),
            "internal zero-based channel ids should not leak into the review:\n{text}"
        );
        assert!(
            text.contains("Effective kernels 1 → 2"),
            "review was:\n{text}"
        );
        assert!(
            text.contains("Growth programs  1 → 2"),
            "review was:\n{text}"
        );
        assert!(
            text.contains("Apply & Run restarts the runtime from the draft initial field"),
            "review was:\n{text}"
        );
        assert!(
            text.contains("Ctrl+S Save workspace"),
            "the primary persistent workspace action must be visible:\n{text}"
        );
    }

    #[test]
    fn experiment_review_surfaces_validation_failures_in_the_canvas() {
        let mut spec = crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(4, 4);
        spec.channels[0].initial.clear();
        let state = crate::workbench::WorkbenchState::new(spec);

        let text = experiment_review_lines(&state).join("\n");

        assert!(
            text.contains("INVALID · 1 problem(s)"),
            "review was:\n{text}"
        );
        assert!(text.contains("!"), "review was:\n{text}");
        assert!(text.contains("initial"), "review was:\n{text}");
    }

    #[test]
    fn canvas_header_separates_actions_from_context_without_duplication() {
        let mut state = crate::workbench::WorkbenchState::new(
            crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(4, 4),
        );
        state.select_section(WorkbenchSection::Tiling);
        let lines = static_canvas_header_lines(&state, "Drag a vertex · wheel zoom");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], toolbar_text(&state));
        assert_eq!(lines[1], "Drag a vertex · wheel zoom");
        assert_eq!(lines.join("\n").matches("[A] Add basis").count(), 1);
    }

    #[test]
    fn tiling_inspector_reports_authoritative_topology_or_diagnostics() {
        let mut spec = crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(4, 4);
        spec.tiling = Some(crate::sim::tiling::build_preset(
            crate::sim::tiling::TilingPreset::RegularHexagon,
            1.0,
        ));
        let state = crate::workbench::WorkbenchState::new(spec);
        let text = tiling_inspector_texts(&state).join("\n");
        assert!(text.contains("exact edge-to-edge tiling"));
        assert!(text.contains("Euler 0"));
        assert!(text.contains("neighbor seams 6"));
    }

    #[test]
    fn tiling_inspector_distinguishes_the_complete_cell_from_the_edited_polygon() {
        let mut spec = crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(4, 4);
        spec.tiling = Some(crate::sim::tiling::build_preset(
            crate::sim::tiling::TilingPreset::OctagonSquare,
            1.0,
        ));
        let state = crate::workbench::WorkbenchState::new(spec);
        let text = tiling_inspector_texts(&state).join("\n");
        assert!(
            text.contains("central cell: 2 polygons · 2 shape types"),
            "inspector must name the complete unit-cell composition: {text}"
        );
        assert!(
            text.contains("editing polygon:"),
            "the selected polygon must be labeled as an editing target, not as the cell: {text}"
        );
    }

    #[test]
    fn tiling_inspector_translates_internal_geometry_errors_into_user_guidance() {
        let mut spec = crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(4, 4);
        spec.tiling = Some(crate::sim::tiling::build_preset(
            crate::sim::tiling::TilingPreset::Square,
            1.0,
        ));
        let tiling = spec.tiling.as_mut().unwrap();
        tiling.translation_a.x = 2.0;
        let state = crate::workbench::WorkbenchState::new(spec);

        let text = tiling_inspector_texts(&state).join("\n");

        assert!(text.contains("not a valid periodic tiling"));
        assert!(
            text.contains("gap") || text.contains("seam"),
            "diagnostic should explain what the user needs to repair: {text}"
        );
        assert!(!text.contains("ShapeEdgeRef"));
        assert!(!text.contains("BasisId("));
        assert!(!text.contains("offset ["));
    }

    #[test]
    fn empty_tiling_guidance_mentions_free_drawing_and_presets() {
        let state = crate::workbench::WorkbenchState::new(
            crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(4, 4),
        );
        let text = tiling_inspector_texts(&state).join("\n");
        assert!(text.contains("[A]"));
        assert!(text.contains("draw"));
        assert!(text.contains("[P]"));
    }

    #[test]
    fn spatial_editors_expose_a_fit_view_action() {
        let mut state = crate::workbench::WorkbenchState::new(
            crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(4, 4),
        );
        for section in [WorkbenchSection::Tiling, WorkbenchSection::Kernels] {
            state.select_section(section);
            let text = toolbar_text(&state);
            assert!(text.contains("[0] Fit"), "{section:?} toolbar was {text:?}");
            let column = text.find("[0] Fit").unwrap() as u16 + 2;
            assert_eq!(
                toolbar_action_at(&state, column),
                Some(ToolbarAction::EditorKey(crossterm::event::KeyCode::Char(
                    '0'
                )))
            );
        }
    }

    #[test]
    fn kernel_toolbar_exposes_independent_kernel_selection() {
        let mut state = crate::workbench::WorkbenchState::new(
            crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(4, 4),
        );
        state.add_kernel_for_selected().unwrap();
        state.select_section(WorkbenchSection::Kernels);
        let text = toolbar_text(&state);
        assert!(text.contains("] Kernel"), "kernel toolbar was {text:?}");
    }

    #[test]
    fn kernel_toolbar_exposes_source_and_output_channel_controls() {
        let mut state = crate::workbench::WorkbenchState::new(
            crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(4, 4),
        );
        state.select_section(WorkbenchSection::Kernels);
        let text = toolbar_text(&state);
        for (label, key) in [
            ("[S] Source", 's'),
            ("[U] Output", 'u'),
            ("[R] Resize", 'r'),
        ] {
            assert!(text.contains(label), "kernel toolbar was {text:?}");
            let column = text.find(label).unwrap() as u16 + 2;
            assert_eq!(
                toolbar_action_at(&state, column),
                Some(ToolbarAction::EditorKey(crossterm::event::KeyCode::Char(
                    key
                )))
            );
        }
    }

    #[test]
    fn kernel_toolbar_exposes_the_current_weights_or_support_tool() {
        let mut state = crate::workbench::WorkbenchState::new(
            crate::sim::experiment_model::ExperimentSpec::single_channel_lenia(4, 4),
        );
        state.select_section(WorkbenchSection::Kernels);

        assert!(toolbar_text(&state).contains("[M] Tool: Weights"));
        state.cycle_kernel_tool();
        let text = toolbar_text(&state);
        assert!(
            text.contains("[M] Tool: Support"),
            "kernel toolbar was {text:?}"
        );
        let column = text.find("[M] Tool: Support").unwrap() as u16 + 2;
        assert_eq!(
            toolbar_action_at(&state, column),
            Some(ToolbarAction::EditorKey(crossterm::event::KeyCode::Char(
                'm'
            )))
        );
        assert!(text.contains("[Q] Metric: Affine"));
        assert!(text.contains("[P] Gaussian"));
    }
}
