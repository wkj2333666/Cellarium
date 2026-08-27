use super::growth_editor::editor_for_basis;
use super::kernel_editor::{KernelPoint, KernelSelection, KernelTool};
use super::numeric_editor::NumericEditor;
use super::{ChannelView, DraftCommand, GrowthEditorState, History, HistoryError};
use crate::sim::experiment_model::{
    ChannelId, ChannelSpec, DisplayColor, ExperimentSpec, KernelId, KernelSlot, RgbColor,
    UpdateMode,
};
use crate::sim::kernel::KernelDefinition;
use crate::sim::ruleset::{BindingKey, RuleKernel, RuleSet, RuleSetId};
use crate::sim::tiling::{
    BasisId, PrototypeId, PrototypeShape, SeamConstraint, TilingPreset, build_preset,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AppMode {
    #[default]
    Simulation,
    Workbench,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WorkbenchSection {
    #[default]
    World,
    Tiling,
    Channels,
    Kernels,
    Growth,
    Experiment,
}
impl WorkbenchSection {
    pub const ALL: [Self; 6] = [
        Self::World,
        Self::Tiling,
        Self::Channels,
        Self::Kernels,
        Self::Growth,
        Self::Experiment,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::World => "World",
            Self::Tiling => "Tiling",
            Self::Channels => "Channels",
            Self::Kernels => "Kernels",
            Self::Growth => "Growth",
            Self::Experiment => "Experiment",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WorkbenchFocus {
    #[default]
    Outline,
    Canvas,
    Inspector,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DraftStatus {
    #[default]
    Clean,
    Dirty,
    Invalid,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeamSolveSummary {
    pub seams: usize,
    pub max_displacement: f64,
    pub max_residual: f64,
}

#[derive(Clone, Copy, Debug)]
struct WorkbenchSelection {
    channel: ChannelId,
    basis: BasisId,
    kernel: Option<KernelId>,
    prototype: Option<PrototypeId>,
}

#[derive(Clone, Debug)]
pub struct WorkbenchState {
    authoritative: ExperimentSpec,
    draft: ExperimentSpec,
    history: History,
    selection_undo: Vec<WorkbenchSelection>,
    selection_redo: Vec<WorkbenchSelection>,
    section: WorkbenchSection,
    focus: WorkbenchFocus,
    decision: Option<super::decision::DecisionPanel>,
    status: DraftStatus,
    selected_channel: ChannelId,
    selected_basis: BasisId,
    selected_rule_set: Option<RuleSetId>,
    selected_kernel: Option<KernelId>,
    channel_view: ChannelView,
    growth_editor: GrowthEditorState,
    growth_editing: bool,
    growth_help_scroll: u16,
    selected_prototype: Option<PrototypeId>,
    kernel_view: super::kernel_editor::KernelView,
    kernel_selection: Option<KernelPoint>,
    periodic_kernel_selection: Option<KernelSelection>,
    kernel_tool: KernelTool,
    kernel_sampling_metric: crate::sim::kernel_sampling::KernelSamplingMetric,
    kernel_paint_value: f32,
    numeric_editor: Option<NumericEditor>,
    simulation_dt_editing: bool,
    kernel_sigma_editing: bool,
    kernel_gaussian_sigma: f64,
    kernel_resize_editor: Option<String>,
    kernel_resize_replace_on_input: bool,
    kernel_resize_confirmed: bool,
    color_editor: Option<String>,
    color_editor_replace_on_input: bool,
    tiling_tool: super::tiling_editor::TilingTool,
    tiling_camera: super::tiling_editor::TilingCamera,
    tiling_selected_vertex: Option<(PrototypeId, usize)>,
    tiling_construction: Vec<crate::sim::tiling::Vec2>,
    tiling_pointer: Option<crate::sim::tiling::Vec2>,
    tiling_new_basis: bool,
    tiling_drag_active: bool,
    tiling_constraints: Vec<SeamConstraint>,
}

fn validate_growth_after_kernel_removal(
    source: &str,
    kernel_inputs: Vec<String>,
    parameters: Vec<String>,
    removed_symbol: &str,
) -> Result<(), String> {
    crate::sim::growth::typecheck::compile(
        source,
        &crate::sim::growth::types::ExternalSymbols {
            kernel_inputs,
            parameters,
        },
    )
    .map(|_| ())
    .map_err(|diagnostics| {
        let details = diagnostics
            .into_iter()
            .map(|diagnostic| {
                format!(
                    "{} at {}..{}",
                    diagnostic.code, diagnostic.span.start, diagnostic.span.end
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        format!("cannot remove kernel `{removed_symbol}`: Growth would become invalid ({details})")
    })
}

impl WorkbenchState {
    pub fn new(spec: ExperimentSpec) -> Self {
        let selected_channel = spec.channels.first().map_or(ChannelId(0), |c| c.id);
        let selected_basis = spec.basis_ids().first().copied().unwrap_or(BasisId(0));
        let selected_rule_set = spec
            .rules
            .binding(selected_basis, selected_channel)
            .map(|binding| binding.rule_set);
        let selected_kernel = selected_rule_set
            .and_then(|rule_set| spec.rules.get(rule_set))
            .and_then(|rule| rule.kernels.first())
            .map(|kernel| kernel.id)
            .or_else(|| {
                spec.kernels
                    .iter()
                    .find(|kernel| kernel.target == selected_channel)
                    .map(|kernel| kernel.id)
            });
        let growth_editor = editor_for_basis(&spec, selected_basis, selected_channel);
        let selected_prototype = spec
            .tiling
            .as_ref()
            .and_then(|tiling| tiling.prototypes.first().map(|prototype| prototype.id));
        Self {
            authoritative: spec.clone(),
            draft: spec,
            history: History::default(),
            selection_undo: Vec::new(),
            selection_redo: Vec::new(),
            section: WorkbenchSection::World,
            focus: WorkbenchFocus::Outline,
            decision: None,
            status: DraftStatus::Clean,
            selected_channel,
            selected_basis,
            selected_rule_set,
            selected_kernel,
            channel_view: ChannelView::Composite,
            growth_editor,
            growth_editing: false,
            growth_help_scroll: 0,
            selected_prototype,
            kernel_view: super::kernel_editor::KernelView::default(),
            kernel_selection: None,
            periodic_kernel_selection: None,
            kernel_tool: KernelTool::Weights,
            kernel_sampling_metric:
                crate::sim::kernel_sampling::KernelSamplingMetric::LatticeAffine,
            kernel_paint_value: 0.05,
            numeric_editor: None,
            simulation_dt_editing: false,
            kernel_sigma_editing: false,
            kernel_gaussian_sigma: 1.0,
            kernel_resize_editor: None,
            kernel_resize_replace_on_input: false,
            kernel_resize_confirmed: false,
            color_editor: None,
            color_editor_replace_on_input: false,
            tiling_tool: super::tiling_editor::TilingTool::Select,
            tiling_camera: super::tiling_editor::TilingCamera::default(),
            tiling_selected_vertex: None,
            tiling_construction: Vec::new(),
            tiling_pointer: None,
            tiling_new_basis: false,
            tiling_drag_active: false,
            tiling_constraints: Vec::new(),
        }
    }
    pub fn draft(&self) -> &ExperimentSpec {
        &self.draft
    }
    pub fn authoritative(&self) -> &ExperimentSpec {
        &self.authoritative
    }
    pub fn section(&self) -> WorkbenchSection {
        self.section
    }
    pub fn focus(&self) -> WorkbenchFocus {
        self.focus
    }
    pub fn set_focus(&mut self, focus: WorkbenchFocus) {
        self.focus = focus;
    }
    pub fn decision(&self) -> Option<&super::decision::DecisionPanel> {
        self.decision.as_ref()
    }
    pub fn present_decision(&mut self, decision: super::decision::DecisionPanel) {
        self.decision = Some(decision);
    }
    pub fn cancel_decision(&mut self) {
        self.decision = None;
    }
    pub fn choose_decision(&mut self, id: &str) -> Result<super::decision::DecisionChoice, String> {
        let choice = self
            .decision
            .as_ref()
            .ok_or_else(|| "no decision is active".to_string())?
            .choose(id)?
            .clone();
        self.decision = None;
        Ok(choice)
    }
    pub fn status(&self) -> DraftStatus {
        self.status
    }
    pub fn selected_channel(&self) -> ChannelId {
        self.selected_channel
    }
    pub fn selected_basis(&self) -> BasisId {
        self.selected_basis
    }
    pub fn selected_rule_set(&self) -> Option<RuleSetId> {
        self.selected_rule_set
    }
    pub fn selected_kernel(&self) -> Option<KernelId> {
        self.selected_kernel
    }
    pub fn rule_for(&self, basis: BasisId, output: ChannelId) -> Option<&RuleSet> {
        let id = self.draft.rules.binding(basis, output)?.rule_set;
        self.draft.rules.get(id)
    }
    pub fn set_selected_basis(&mut self, basis: BasisId) -> Result<(), String> {
        if !self.draft.basis_ids().contains(&basis) {
            return Err("unknown basis".into());
        }
        self.selected_basis = basis;
        self.selected_prototype = self
            .draft
            .tiling
            .as_ref()
            .and_then(|tiling| {
                tiling
                    .instances
                    .iter()
                    .find(|instance| instance.id == basis)
            })
            .map(|instance| instance.prototype)
            .or(self.selected_prototype);
        self.refresh_rule_selection();
        self.growth_editor =
            editor_for_basis(&self.draft, self.selected_basis, self.selected_channel);
        Ok(())
    }
    pub fn channel_view(&self) -> ChannelView {
        self.channel_view
    }
    pub fn kernel_view(&self) -> super::kernel_editor::KernelView {
        self.kernel_view
    }
    pub fn set_kernel_view(&mut self, view: super::kernel_editor::KernelView) {
        self.kernel_view = view;
    }
    pub fn kernel_selection(&self) -> Option<KernelPoint> {
        self.kernel_selection
    }
    pub fn select_kernel_point(&mut self, point: KernelPoint) {
        self.kernel_selection = Some(point);
        self.periodic_kernel_selection = None;
    }
    pub fn periodic_kernel_selection(&self) -> Option<KernelSelection> {
        self.periodic_kernel_selection
    }
    pub fn select_periodic_kernel(&mut self, selection: KernelSelection) {
        self.periodic_kernel_selection = Some(selection);
        self.kernel_selection = None;
    }
    pub fn kernel_tool(&self) -> KernelTool {
        self.kernel_tool
    }
    pub fn cycle_kernel_tool(&mut self) {
        self.kernel_tool = match self.kernel_tool {
            KernelTool::Weights => KernelTool::Support,
            KernelTool::Support => KernelTool::Weights,
        };
    }
    pub fn kernel_sampling_metric(&self) -> crate::sim::kernel_sampling::KernelSamplingMetric {
        self.kernel_sampling_metric
    }
    pub fn cycle_kernel_sampling_metric(
        &mut self,
    ) -> crate::sim::kernel_sampling::KernelSamplingMetric {
        self.kernel_sampling_metric = match self.kernel_sampling_metric {
            crate::sim::kernel_sampling::KernelSamplingMetric::LatticeAffine => {
                crate::sim::kernel_sampling::KernelSamplingMetric::WorldEuclidean
            }
            crate::sim::kernel_sampling::KernelSamplingMetric::WorldEuclidean => {
                crate::sim::kernel_sampling::KernelSamplingMetric::LatticeAffine
            }
        };
        self.kernel_sampling_metric
    }
    pub fn selected_rule_kernel(&self) -> Option<&RuleKernel> {
        let rule_set = self.selected_rule_set?;
        let kernel = self.selected_kernel?;
        self.draft
            .rules
            .get(rule_set)?
            .kernels
            .iter()
            .find(|entry| entry.id == kernel)
    }
    pub fn selected_raster_kernel_definition(&self) -> Option<&KernelDefinition> {
        if let Some(kernel) = self.selected_rule_kernel() {
            return match &kernel.spatial {
                crate::sim::ruleset::KernelSpatialDefinition::Raster(definition) => {
                    Some(definition)
                }
                crate::sim::ruleset::KernelSpatialDefinition::Periodic(_) => None,
            };
        }
        self.selected_legacy_kernel()
            .map(|kernel| &kernel.definition)
    }
    pub fn replace_selected_raster_kernel_definition(
        &mut self,
        definition: KernelDefinition,
    ) -> Result<(), HistoryError> {
        definition
            .build()
            .map_err(|error| HistoryError::Edit(error.to_string()))?;
        let selected_kernel = self
            .selected_kernel
            .ok_or_else(|| HistoryError::Edit("no selected kernel".into()))?;
        if self.selected_rule_set.is_some() {
            let binding = BindingKey {
                basis: self.selected_basis,
                output: self.selected_channel,
            };
            let mut next = self.draft.clone();
            let rule_set = next
                .rules
                .detach(binding)
                .map_err(|error| HistoryError::Edit(error.to_string()))?;
            let kernel = next
                .rules
                .get_mut(rule_set)
                .and_then(|rule| {
                    rule.kernels
                        .iter_mut()
                        .find(|kernel| kernel.id == selected_kernel)
                })
                .ok_or_else(|| HistoryError::Edit("selected rule kernel is missing".into()))?;
            if !matches!(
                kernel.spatial,
                crate::sim::ruleset::KernelSpatialDefinition::Raster(_)
            ) {
                return Err(HistoryError::Edit(
                    "selected kernel is not a raster kernel".into(),
                ));
            }
            kernel.spatial = crate::sim::ruleset::KernelSpatialDefinition::Raster(definition);
            next.rules
                .get(rule_set)
                .expect("detached rule-set must remain available")
                .validate()
                .map_err(|error| HistoryError::Edit(error.to_string()))?;
            self.execute(DraftCommand::ReplaceDraft(Box::new(next)))?;
            self.refresh_rule_selection();
            self.selected_kernel = Some(selected_kernel);
            return Ok(());
        }
        let mut next = self.draft.clone();
        let kernel = next
            .kernels
            .iter_mut()
            .find(|kernel| kernel.id == selected_kernel)
            .ok_or_else(|| HistoryError::Edit("selected legacy kernel is missing".into()))?;
        kernel.definition = definition;
        self.replace_draft(next)?;
        self.selected_kernel = Some(selected_kernel);
        Ok(())
    }
    pub fn kernel_paint_value(&self) -> f32 {
        self.kernel_paint_value
    }
    pub fn set_kernel_paint_value(&mut self, value: f32) -> Result<(), String> {
        if !value.is_finite() {
            return Err("kernel paint value must be finite".into());
        }
        self.kernel_paint_value = value.clamp(-1.0, 1.0);
        Ok(())
    }
    pub fn numeric_editor(&self) -> Option<&NumericEditor> {
        self.numeric_editor.as_ref()
    }
    pub fn numeric_editor_mut(&mut self) -> Option<&mut NumericEditor> {
        self.numeric_editor.as_mut()
    }
    pub fn begin_numeric_editor(&mut self, editor: NumericEditor) {
        self.numeric_editor = Some(editor);
        self.simulation_dt_editing = false;
        self.kernel_sigma_editing = false;
    }
    pub fn begin_simulation_dt_editor(&mut self) {
        self.numeric_editor = Some(NumericEditor::begin(
            "simulation dt",
            f64::from(self.draft.simulation_dt),
            0.000_001..=10.0,
        ));
        self.simulation_dt_editing = true;
        self.kernel_sigma_editing = false;
    }
    pub fn simulation_dt_editing(&self) -> bool {
        self.simulation_dt_editing
    }
    pub fn begin_kernel_sigma_editor(&mut self) {
        self.numeric_editor = Some(NumericEditor::begin(
            "Gaussian sigma",
            self.kernel_gaussian_sigma,
            0.000_001..=1_000_000.0,
        ));
        self.simulation_dt_editing = false;
        self.kernel_sigma_editing = true;
    }
    pub fn kernel_sigma_editing(&self) -> bool {
        self.kernel_sigma_editing
    }
    pub fn kernel_gaussian_sigma(&self) -> f64 {
        self.kernel_gaussian_sigma
    }
    pub fn set_kernel_gaussian_sigma(&mut self, sigma: f64) -> Result<(), String> {
        if !sigma.is_finite() || sigma <= 0.0 {
            return Err("Gaussian sigma must be finite and positive".into());
        }
        self.kernel_gaussian_sigma = sigma;
        Ok(())
    }
    pub fn take_numeric_editor(&mut self) -> Option<NumericEditor> {
        self.simulation_dt_editing = false;
        self.kernel_sigma_editing = false;
        self.numeric_editor.take()
    }
    pub fn restore_numeric_editor(
        &mut self,
        editor: NumericEditor,
        simulation_dt: bool,
        kernel_sigma: bool,
    ) {
        self.numeric_editor = Some(editor);
        self.simulation_dt_editing = simulation_dt;
        self.kernel_sigma_editing = kernel_sigma;
    }
    pub fn kernel_resize_editor(&self) -> Option<&str> {
        self.kernel_resize_editor.as_deref()
    }
    pub fn begin_selected_kernel_resize_editor(&mut self) -> Result<(), String> {
        let kernel = self
            .selected_rule_kernel()
            .ok_or_else(|| "no selected kernel".to_string())?;
        let crate::sim::ruleset::KernelSpatialDefinition::Periodic(definition) = &kernel.spatial
        else {
            return Err("selected kernel is not periodic".into());
        };
        self.kernel_resize_editor = Some(format!(
            "{},{},{},{}",
            definition.width, definition.height, definition.anchor_x, definition.anchor_y
        ));
        self.kernel_resize_replace_on_input = true;
        self.kernel_resize_confirmed = false;
        Ok(())
    }
    pub fn cancel_kernel_resize_editor(&mut self) {
        self.kernel_resize_editor = None;
        self.kernel_resize_replace_on_input = false;
        self.kernel_resize_confirmed = false;
    }
    pub fn kernel_resize_editor_select_all(&mut self) {
        if self.kernel_resize_editor.is_some() {
            self.kernel_resize_replace_on_input = true;
            self.kernel_resize_confirmed = false;
        }
    }
    pub fn kernel_resize_editor_insert(&mut self, character: char) -> bool {
        if !(character.is_ascii_digit() || character == ',' || character.is_ascii_whitespace()) {
            return false;
        }
        let Some(buffer) = self.kernel_resize_editor.as_mut() else {
            return false;
        };
        if self.kernel_resize_replace_on_input {
            buffer.clear();
            self.kernel_resize_replace_on_input = false;
        }
        buffer.push(character);
        self.kernel_resize_confirmed = false;
        true
    }
    pub fn kernel_resize_editor_backspace(&mut self) {
        if let Some(buffer) = self.kernel_resize_editor.as_mut() {
            if self.kernel_resize_replace_on_input {
                buffer.clear();
                self.kernel_resize_replace_on_input = false;
            } else {
                buffer.pop();
            }
            self.kernel_resize_confirmed = false;
        }
    }
    pub fn kernel_resize_confirmed(&self) -> bool {
        self.kernel_resize_confirmed
    }
    pub fn confirm_kernel_resize(&mut self) {
        self.kernel_resize_confirmed = true;
    }
    pub fn color_editor(&self) -> Option<&str> {
        self.color_editor.as_deref()
    }
    pub fn begin_selected_color_editor(&mut self) {
        let color = super::channel_editor::resolved_color(&self.draft, self.selected_channel)
            .unwrap_or(crate::render::channels::Rgb8::new(255, 255, 255));
        self.color_editor = Some(format!(
            "#{:02X}{:02X}{:02X}",
            color.red, color.green, color.blue
        ));
        self.color_editor_replace_on_input = true;
    }
    pub fn cancel_color_editor(&mut self) {
        self.color_editor = None;
        self.color_editor_replace_on_input = false;
    }
    pub fn color_editor_insert(&mut self, character: char) -> bool {
        if !(character == '#' || character.is_ascii_hexdigit()) {
            return false;
        }
        let Some(buffer) = self.color_editor.as_mut() else {
            return false;
        };
        if self.color_editor_replace_on_input {
            buffer.clear();
            self.color_editor_replace_on_input = false;
        }
        if buffer.len() < 7 {
            buffer.push(character.to_ascii_uppercase());
        }
        true
    }
    pub fn color_editor_backspace(&mut self) -> bool {
        let Some(buffer) = self.color_editor.as_mut() else {
            return false;
        };
        self.color_editor_replace_on_input = false;
        buffer.pop().is_some()
    }
    pub fn color_editor_select_all(&mut self) -> bool {
        if self.color_editor.is_none() {
            return false;
        }
        self.color_editor_replace_on_input = true;
        true
    }
    pub fn commit_color_editor(&mut self) -> Result<RgbColor, String> {
        let source = self
            .color_editor
            .as_deref()
            .ok_or_else(|| "color editor is not active".to_string())?;
        let hex = source.strip_prefix('#').unwrap_or(source);
        if hex.len() != 6 || !hex.chars().all(|character| character.is_ascii_hexdigit()) {
            return Err("use exactly six hexadecimal digits, for example #33AACC".into());
        }
        let color = RgbColor {
            red: u8::from_str_radix(&hex[0..2], 16).map_err(|error| error.to_string())?,
            green: u8::from_str_radix(&hex[2..4], 16).map_err(|error| error.to_string())?,
            blue: u8::from_str_radix(&hex[4..6], 16).map_err(|error| error.to_string())?,
        };
        self.execute(DraftCommand::SetChannelColor {
            channel: self.selected_channel,
            color: DisplayColor::Custom(color),
        })
        .map_err(|error| error.to_string())?;
        self.cancel_color_editor();
        self.cancel_decision();
        Ok(color)
    }
    pub fn tiling_tool(&self) -> super::tiling_editor::TilingTool {
        self.tiling_tool
    }
    pub fn tiling_camera(&self) -> super::tiling_editor::TilingCamera {
        self.tiling_camera
    }
    pub fn set_tiling_camera(&mut self, camera: super::tiling_editor::TilingCamera) {
        self.tiling_camera = camera;
    }
    pub fn tiling_selected_vertex(&self) -> Option<(PrototypeId, usize)> {
        self.tiling_selected_vertex
    }
    pub fn select_tiling_vertex(&mut self, prototype: PrototypeId, vertex: usize) {
        self.tiling_selected_vertex = Some((prototype, vertex));
    }
    pub fn clear_tiling_vertex(&mut self) {
        self.tiling_selected_vertex = None;
    }
    pub fn tiling_pointer(&self) -> Option<crate::sim::tiling::Vec2> {
        self.tiling_pointer
    }
    pub fn set_tiling_pointer(&mut self, pointer: Option<crate::sim::tiling::Vec2>) {
        self.tiling_pointer = pointer;
    }
    pub fn tiling_constraint_count(&self) -> usize {
        self.tiling_constraints.len()
    }
    pub fn clear_tiling_constraints(&mut self) {
        self.tiling_constraints.clear();
    }
    pub fn solve_tiling_seams(&mut self) -> Result<SeamSolveSummary, String> {
        let mut candidate = self.draft.clone();
        complete_single_triangle_cell(&mut candidate)?;
        let tiling = candidate
            .tiling
            .as_ref()
            .ok_or_else(|| "draw or choose at least one polygon first".to_string())?;
        let scale = tiling
            .translation_a
            .length()
            .max(tiling.translation_b.length())
            .max(1e-6);
        let proposals = crate::sim::tiling::propose_full_edge_seams(tiling, scale * 0.2)?;
        let edge_count = tiling
            .instances
            .iter()
            .map(|instance| {
                tiling
                    .prototypes
                    .iter()
                    .find(|prototype| prototype.id == instance.prototype)
                    .ok_or_else(|| "basis references a missing prototype".to_string())
                    .and_then(|prototype| {
                        crate::sim::tiling::polygon::prototype_vertices(&prototype.shape)
                            .map(|vertices| vertices.len())
                            .map_err(|issues| {
                                issues
                                    .into_iter()
                                    .map(|issue| issue.message)
                                    .collect::<Vec<_>>()
                                    .join("; ")
                            })
                    })
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .sum::<usize>();
        if proposals.len() * 2 != edge_count {
            return Err(format!(
                "found {} complete seam pairs for {edge_count} edges; move matching full edges closer, then solve again",
                proposals.len()
            ));
        }
        let constraints = proposals
            .into_iter()
            .map(|proposal| proposal.constraint)
            .collect::<Vec<_>>();
        let solved = crate::sim::tiling::solve_edge_constraints(tiling, &constraints, None)
            .map_err(|error| error.0)?;
        let summary = SeamSolveSummary {
            seams: constraints.len(),
            max_displacement: solved.max_displacement,
            max_residual: solved.max_seam_residual,
        };
        let mut next = candidate;
        next.tiling = Some(solved.draft);
        self.replace_draft(next)
            .map_err(|error| error.to_string())?;
        self.tiling_constraints = constraints;
        Ok(summary)
    }
    pub fn drag_constrained_tiling_vertex(
        &mut self,
        prototype: PrototypeId,
        vertex: usize,
        local_target: crate::sim::tiling::Vec2,
    ) -> Result<SeamSolveSummary, String> {
        if self.tiling_constraints.is_empty() {
            return Err("solve seams before using linked vertex dragging".into());
        }
        let tiling = self
            .draft
            .tiling
            .as_ref()
            .ok_or_else(|| "tiling draft is missing".to_string())?;
        let solved = crate::sim::tiling::solve_edge_constraints(
            tiling,
            &self.tiling_constraints,
            Some(crate::sim::tiling::DragTarget {
                prototype,
                vertex,
                to: local_target,
            }),
        )
        .map_err(|error| error.0)?;
        let summary = SeamSolveSummary {
            seams: self.tiling_constraints.len(),
            max_displacement: solved.max_displacement,
            max_residual: solved.max_seam_residual,
        };
        let mut next = self.draft.clone();
        next.tiling = Some(solved.draft);
        self.import_tiling_drag_draft(next)
            .map_err(|error| error.to_string())?;
        Ok(summary)
    }
    pub fn set_tiling_tool(&mut self, tool: super::tiling_editor::TilingTool) {
        self.tiling_tool = tool;
        if tool != super::tiling_editor::TilingTool::DrawPolygon {
            self.tiling_construction.clear();
            self.tiling_pointer = None;
            self.tiling_new_basis = false;
        }
    }
    pub fn begin_new_basis_polygon(&mut self) {
        self.tiling_tool = super::tiling_editor::TilingTool::DrawPolygon;
        self.tiling_construction.clear();
        self.tiling_pointer = None;
        self.tiling_new_basis = true;
    }
    pub fn is_drawing_new_basis(&self) -> bool {
        self.tiling_new_basis
    }
    pub fn tiling_construction(&self) -> &[crate::sim::tiling::Vec2] {
        &self.tiling_construction
    }
    pub fn push_tiling_vertex(&mut self, point: crate::sim::tiling::Vec2) -> Result<(), String> {
        crate::sim::tiling::polygon::validate_open_path_append(&self.tiling_construction, point)?;
        self.tiling_construction.push(point);
        Ok(())
    }
    pub fn pop_tiling_vertex(&mut self) -> Option<crate::sim::tiling::Vec2> {
        let removed = self.tiling_construction.pop();
        self.tiling_pointer = self.tiling_construction.last().copied();
        removed
    }
    pub fn cancel_tiling_construction(&mut self) {
        self.tiling_construction.clear();
        self.tiling_pointer = None;
        self.tiling_tool = super::tiling_editor::TilingTool::Select;
        self.tiling_new_basis = false;
    }
    pub fn finish_tiling_construction(&mut self) -> Result<(), String> {
        // The commit rules live in the document layer so the terminal Workbench
        // and the GUI produce byte-identical drafts from the same polygon.
        let target = if self.tiling_new_basis {
            crate::document::tiling::ConstructionTarget::NewBasis
        } else {
            crate::document::tiling::ConstructionTarget::ReplacePrototype(
                self.selected_prototype
                    .ok_or("select a basis polygon first")?,
            )
        };
        let commit = crate::document::tiling::finish_polygon(
            &self.draft,
            &self.tiling_construction,
            target,
        )?;
        self.selected_prototype = Some(commit.prototype);
        if let Some(basis) = commit.basis {
            self.selected_basis = basis;
        }
        self.replace_draft(commit.spec)
            .map_err(|error| error.to_string())?;
        self.tiling_construction.clear();
        self.tiling_pointer = None;
        self.tiling_tool = super::tiling_editor::TilingTool::Select;
        self.tiling_new_basis = false;
        self.tiling_constraints.clear();
        self.refresh_rule_selection();
        self.growth_editor =
            editor_for_basis(&self.draft, self.selected_basis, self.selected_channel);
        Ok(())
    }
    pub fn growth_editor(&self) -> &GrowthEditorState {
        &self.growth_editor
    }
    pub fn growth_editor_mut(&mut self) -> &mut GrowthEditorState {
        &mut self.growth_editor
    }
    pub fn growth_editing(&self) -> bool {
        self.growth_editing
    }
    pub fn toggle_growth_editing(&mut self) {
        self.growth_editing = !self.growth_editing;
    }
    pub fn stop_growth_editing(&mut self) {
        self.growth_editing = false;
    }
    pub fn growth_help_scroll(&self) -> u16 {
        self.growth_help_scroll
    }
    pub fn scroll_growth_help(&mut self, lines: i16) {
        self.growth_help_scroll = if lines < 0 {
            self.growth_help_scroll.saturating_sub(lines.unsigned_abs())
        } else {
            self.growth_help_scroll
                .saturating_add(lines as u16)
                .min(128)
        };
    }
    pub fn sync_growth_source(&mut self) {
        let source = self.growth_editor.buffer().as_str().to_string();
        let binding = BindingKey {
            basis: self.selected_basis,
            output: self.selected_channel,
        };
        if self
            .draft
            .rules
            .binding(binding.basis, binding.output)
            .is_some()
        {
            match self.draft.rules.detach(binding) {
                Ok(rule_set) => {
                    if let Some(rule) = self.draft.rules.get_mut(rule_set) {
                        rule.growth.source = source;
                    }
                    self.selected_rule_set = Some(rule_set);
                    self.selected_kernel = self
                        .draft
                        .rules
                        .get(rule_set)
                        .and_then(|rule| rule.kernels.first())
                        .map(|kernel| kernel.id);
                    self.status = if self.growth_editor.diagnostics().is_empty() {
                        DraftStatus::Dirty
                    } else {
                        DraftStatus::Invalid
                    };
                }
                Err(_) => self.status = DraftStatus::Invalid,
            }
            return;
        }
        if let Some(growth) = self
            .draft
            .growth
            .iter_mut()
            .find(|growth| growth.target == self.selected_channel)
        {
            growth.source = source;
            self.status = if self.growth_editor.diagnostics().is_empty() {
                DraftStatus::Dirty
            } else {
                DraftStatus::Invalid
            };
        }
    }
    pub fn selected_growth_mode(&self) -> Option<UpdateMode> {
        self.selected_rule_set
            .and_then(|id| self.draft.rules.get(id))
            .map(|rule| rule.growth.mode)
            .or_else(|| {
                self.draft
                    .growth
                    .iter()
                    .find(|growth| growth.target == self.selected_channel)
                    .map(|growth| growth.mode)
            })
    }
    pub fn set_selected_growth_mode(&mut self, mode: UpdateMode) -> Result<(), HistoryError> {
        let mut next = self.draft.clone();
        let binding = BindingKey {
            basis: self.selected_basis,
            output: self.selected_channel,
        };
        if next.rules.binding(binding.basis, binding.output).is_some() {
            let rule_set = next
                .rules
                .detach(binding)
                .map_err(|error| HistoryError::Edit(error.to_string()))?;
            next.rules
                .get_mut(rule_set)
                .ok_or_else(|| HistoryError::Edit("selected rule-set is missing".into()))?
                .growth
                .mode = mode;
        } else {
            next.growth
                .iter_mut()
                .find(|growth| growth.target == self.selected_channel)
                .ok_or_else(|| HistoryError::Edit("selected growth program is missing".into()))?
                .mode = mode;
        }
        self.execute(DraftCommand::ReplaceDraft(Box::new(next)))?;
        self.refresh_rule_selection();
        self.growth_editor =
            editor_for_basis(&self.draft, self.selected_basis, self.selected_channel);
        Ok(())
    }
    pub fn set_simulation_dt(&mut self, value: f32) -> Result<(), HistoryError> {
        if !value.is_finite() || value <= 0.0 || value > 10.0 {
            return Err(HistoryError::Edit(
                "simulation dt must be finite and in (0, 10]".into(),
            ));
        }
        let mut next = self.draft.clone();
        next.simulation_dt = value;
        self.execute(DraftCommand::ReplaceDraft(Box::new(next)))?;
        Ok(())
    }
    pub fn toggle_selected_growth_mode(&mut self) -> Result<UpdateMode, HistoryError> {
        let next = match self
            .selected_growth_mode()
            .unwrap_or(UpdateMode::DirectUpdate)
        {
            UpdateMode::GrowthRate => UpdateMode::DirectUpdate,
            UpdateMode::DirectUpdate => UpdateMode::GrowthRate,
        };
        self.set_selected_growth_mode(next)?;
        Ok(next)
    }
    pub fn execute(&mut self, command: DraftCommand) -> Result<(), HistoryError> {
        let selection = self.selection_snapshot();
        self.history.execute(&mut self.draft, command)?;
        self.selection_undo.push(selection);
        self.selection_redo.clear();
        self.status = DraftStatus::Dirty;
        Ok(())
    }
    pub fn undo(&mut self) -> Result<(), HistoryError> {
        self.finish_tiling_drag();
        self.tiling_constraints.clear();
        let current = self.selection_snapshot();
        self.history.undo(&mut self.draft)?;
        let restored = self.selection_undo.pop().unwrap_or(current);
        self.selection_redo.push(current);
        self.restore_selection(restored);
        self.status = if self.draft == self.authoritative {
            DraftStatus::Clean
        } else {
            DraftStatus::Dirty
        };
        Ok(())
    }
    pub fn redo(&mut self) -> Result<(), HistoryError> {
        self.finish_tiling_drag();
        self.tiling_constraints.clear();
        let current = self.selection_snapshot();
        self.history.redo(&mut self.draft)?;
        let restored = self.selection_redo.pop().unwrap_or(current);
        self.selection_undo.push(current);
        self.restore_selection(restored);
        self.status = DraftStatus::Dirty;
        Ok(())
    }
    pub fn revert(&mut self) {
        self.finish_tiling_drag();
        self.tiling_constraints.clear();
        self.draft = self.authoritative.clone();
        self.growth_editing = false;
        self.numeric_editor = None;
        self.simulation_dt_editing = false;
        self.kernel_sigma_editing = false;
        self.cancel_kernel_resize_editor();
        self.cancel_color_editor();
        self.cancel_decision();
        self.history.clear();
        self.selection_undo.clear();
        self.selection_redo.clear();
        self.status = DraftStatus::Clean;
        self.tiling_construction.clear();
        self.tiling_pointer = None;
        self.tiling_tool = super::tiling_editor::TilingTool::Select;
        self.tiling_new_basis = false;
        self.reconcile_selection_to_draft();
    }
    pub fn accept(&mut self, normalized: ExperimentSpec) {
        self.finish_tiling_drag();
        self.tiling_constraints.clear();
        self.authoritative = normalized.clone();
        self.draft = normalized;
        self.growth_editing = false;
        self.numeric_editor = None;
        self.simulation_dt_editing = false;
        self.kernel_sigma_editing = false;
        self.cancel_kernel_resize_editor();
        self.cancel_color_editor();
        self.history.clear();
        self.selection_undo.clear();
        self.selection_redo.clear();
        self.status = DraftStatus::Clean;
        self.reconcile_selection_to_draft();
    }
    pub fn select_section(&mut self, section: WorkbenchSection) {
        if self.section != section {
            self.close_section_editors();
        }
        self.section = section;
    }
    pub fn section_next(&mut self) {
        let index = WorkbenchSection::ALL
            .iter()
            .position(|value| *value == self.section)
            .unwrap_or(0);
        self.close_section_editors();
        self.section = WorkbenchSection::ALL[(index + 1) % WorkbenchSection::ALL.len()];
    }

    fn close_section_editors(&mut self) {
        self.finish_tiling_drag();
        // A pointer is meaningful only inside the currently visible Tiling
        // canvas. Keeping it across section changes can recreate an obsolete
        // construction segment when the user returns.
        self.tiling_pointer = None;
        self.growth_editing = false;
        self.numeric_editor = None;
        self.simulation_dt_editing = false;
        self.kernel_sigma_editing = false;
        self.cancel_color_editor();
    }
    pub fn focus_next(&mut self) {
        self.focus = match self.focus {
            WorkbenchFocus::Outline => WorkbenchFocus::Canvas,
            WorkbenchFocus::Canvas => WorkbenchFocus::Inspector,
            WorkbenchFocus::Inspector => WorkbenchFocus::Outline,
        };
    }
    pub fn focus_previous(&mut self) {
        self.focus = match self.focus {
            WorkbenchFocus::Outline => WorkbenchFocus::Inspector,
            WorkbenchFocus::Canvas => WorkbenchFocus::Outline,
            WorkbenchFocus::Inspector => WorkbenchFocus::Canvas,
        };
    }
    pub fn set_selected_channel(&mut self, channel: ChannelId) -> Result<(), String> {
        if self.draft.channels.iter().any(|entry| entry.id == channel) {
            self.selected_channel = channel;
            self.refresh_rule_selection();
            self.growth_editor =
                editor_for_basis(&self.draft, self.selected_basis, self.selected_channel);
            Ok(())
        } else {
            Err("unknown channel".into())
        }
    }

    fn refresh_rule_selection(&mut self) {
        self.selected_rule_set = self
            .draft
            .rules
            .binding(self.selected_basis, self.selected_channel)
            .map(|binding| binding.rule_set);
        let available = self
            .selected_rule_set
            .and_then(|rule_set| self.draft.rules.get(rule_set))
            .map(|rule| {
                rule.kernels
                    .iter()
                    .map(|kernel| kernel.id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| {
                self.draft
                    .kernels
                    .iter()
                    .filter(|kernel| kernel.target == self.selected_channel)
                    .map(|kernel| kernel.id)
                    .collect()
            });
        if !self
            .selected_kernel
            .is_some_and(|selected| available.contains(&selected))
        {
            self.selected_kernel = available.first().copied();
        }
        self.kernel_selection = None;
        self.periodic_kernel_selection = None;
    }

    fn reconcile_selection_to_draft(&mut self) {
        if !self
            .draft
            .channels
            .iter()
            .any(|channel| channel.id == self.selected_channel)
        {
            self.selected_channel = self
                .draft
                .channels
                .first()
                .map_or(ChannelId(0), |channel| channel.id);
        }
        let bases = self.draft.basis_ids();
        if !bases.contains(&self.selected_basis) {
            self.selected_basis = bases.first().copied().unwrap_or(BasisId(0));
        }
        let selected_prototype_is_valid = self.selected_prototype.is_some_and(|selected| {
            self.draft.tiling.as_ref().is_some_and(|tiling| {
                tiling
                    .prototypes
                    .iter()
                    .any(|prototype| prototype.id == selected)
            })
        });
        if !selected_prototype_is_valid {
            self.selected_prototype = self
                .draft
                .tiling
                .as_ref()
                .and_then(|tiling| tiling.prototypes.first().map(|prototype| prototype.id));
        }
        self.refresh_rule_selection();
        self.growth_editor =
            editor_for_basis(&self.draft, self.selected_basis, self.selected_channel);
    }

    fn selection_snapshot(&self) -> WorkbenchSelection {
        WorkbenchSelection {
            channel: self.selected_channel,
            basis: self.selected_basis,
            kernel: self.selected_kernel,
            prototype: self.selected_prototype,
        }
    }

    fn restore_selection(&mut self, selection: WorkbenchSelection) {
        self.selected_channel = selection.channel;
        self.selected_basis = selection.basis;
        self.selected_kernel = selection.kernel;
        self.selected_prototype = selection.prototype;
        self.reconcile_selection_to_draft();
    }

    pub fn selected_legacy_kernel(&self) -> Option<&KernelSlot> {
        self.selected_kernel
            .and_then(|selected| {
                self.draft
                    .kernels
                    .iter()
                    .find(|kernel| kernel.id == selected)
            })
            .or_else(|| {
                self.draft
                    .kernels
                    .iter()
                    .find(|kernel| kernel.target == self.selected_channel)
            })
    }

    pub fn select_next_kernel(&mut self) {
        let available = self
            .selected_rule_set
            .and_then(|rule_set| self.draft.rules.get(rule_set))
            .map(|rule| {
                rule.kernels
                    .iter()
                    .map(|kernel| kernel.id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| {
                self.draft
                    .kernels
                    .iter()
                    .filter(|kernel| kernel.target == self.selected_channel)
                    .map(|kernel| kernel.id)
                    .collect()
            });
        if available.is_empty() {
            self.selected_kernel = None;
            return;
        }
        let current = self
            .selected_kernel
            .and_then(|selected| available.iter().position(|kernel| *kernel == selected))
            .unwrap_or(available.len() - 1);
        self.selected_kernel = Some(available[(current + 1) % available.len()]);
        self.kernel_selection = None;
        self.periodic_kernel_selection = None;
    }

    pub fn detach_selected_ruleset(&mut self) -> Result<(), HistoryError> {
        let binding = BindingKey {
            basis: self.selected_basis,
            output: self.selected_channel,
        };
        self.execute(DraftCommand::DetachRuleSet { binding })?;
        self.refresh_rule_selection();
        Ok(())
    }

    pub fn reset_selected_ruleset_to_default(&mut self) -> Result<(), HistoryError> {
        let binding = BindingKey {
            basis: self.selected_basis,
            output: self.selected_channel,
        };
        self.execute(DraftCommand::ResetRuleSetToDefault { binding })?;
        self.refresh_rule_selection();
        Ok(())
    }

    pub fn set_selected_kernel_weight(
        &mut self,
        offset: [i16; 2],
        source_basis: BasisId,
        value: f32,
    ) -> Result<(), HistoryError> {
        let binding = BindingKey {
            basis: self.selected_basis,
            output: self.selected_channel,
        };
        let kernel = self
            .selected_kernel
            .ok_or_else(|| HistoryError::Edit("selected rule-set has no kernel".to_string()))?;
        let mut next = self.draft.clone();
        let rule_set = next
            .rules
            .detach(binding)
            .map_err(|error| HistoryError::Edit(error.to_string()))?;
        let target = next
            .rules
            .get_mut(rule_set)
            .and_then(|rule| rule.kernels.iter_mut().find(|entry| entry.id == kernel))
            .ok_or_else(|| HistoryError::Edit("selected rule kernel is missing".to_string()))?;
        let crate::sim::ruleset::KernelSpatialDefinition::Periodic(definition) =
            &mut target.spatial
        else {
            return Err(HistoryError::Edit(
                "selected kernel is not periodic".to_string(),
            ));
        };
        definition
            .set_weight(offset, source_basis, value)
            .map_err(|error| HistoryError::Edit(error.to_string()))?;
        self.execute(DraftCommand::ReplaceDraft(Box::new(next)))?;
        self.refresh_rule_selection();
        Ok(())
    }

    pub fn set_selected_kernel_active(
        &mut self,
        offset: [i16; 2],
        source_basis: BasisId,
        active: bool,
    ) -> Result<(), HistoryError> {
        let binding = BindingKey {
            basis: self.selected_basis,
            output: self.selected_channel,
        };
        let kernel = self
            .selected_kernel
            .ok_or_else(|| HistoryError::Edit("selected rule-set has no kernel".to_string()))?;
        let mut next = self.draft.clone();
        let rule_set = next
            .rules
            .detach(binding)
            .map_err(|error| HistoryError::Edit(error.to_string()))?;
        let target = next
            .rules
            .get_mut(rule_set)
            .and_then(|rule| rule.kernels.iter_mut().find(|entry| entry.id == kernel))
            .ok_or_else(|| HistoryError::Edit("selected rule kernel is missing".to_string()))?;
        let crate::sim::ruleset::KernelSpatialDefinition::Periodic(definition) =
            &mut target.spatial
        else {
            return Err(HistoryError::Edit(
                "selected kernel is not periodic".to_string(),
            ));
        };
        definition
            .set_active(offset, source_basis, active)
            .map_err(|error| HistoryError::Edit(error.to_string()))?;
        self.execute(DraftCommand::ReplaceDraft(Box::new(next)))?;
        self.refresh_rule_selection();
        Ok(())
    }

    pub fn resize_selected_periodic_kernel(
        &mut self,
        width: usize,
        height: usize,
        anchor_x: usize,
        anchor_y: usize,
    ) -> Result<crate::sim::basis_kernel::ResizeReport, HistoryError> {
        let binding = BindingKey {
            basis: self.selected_basis,
            output: self.selected_channel,
        };
        let kernel = self
            .selected_kernel
            .ok_or_else(|| HistoryError::Edit("selected rule-set has no kernel".to_string()))?;
        let mut next = self.draft.clone();
        let rule_set = next
            .rules
            .detach(binding)
            .map_err(|error| HistoryError::Edit(error.to_string()))?;
        let target = next
            .rules
            .get_mut(rule_set)
            .and_then(|rule| rule.kernels.iter_mut().find(|entry| entry.id == kernel))
            .ok_or_else(|| HistoryError::Edit("selected rule kernel is missing".to_string()))?;
        let crate::sim::ruleset::KernelSpatialDefinition::Periodic(definition) =
            &mut target.spatial
        else {
            return Err(HistoryError::Edit(
                "selected kernel is not periodic".to_string(),
            ));
        };
        let report = definition
            .resize(width, height, anchor_x, anchor_y)
            .map_err(|error| HistoryError::Edit(error.to_string()))?;
        self.execute(DraftCommand::ReplaceDraft(Box::new(next)))?;
        self.refresh_rule_selection();
        Ok(report)
    }

    pub fn generate_selected_periodic_kernel(
        &mut self,
        source_basis: BasisId,
        generation: crate::sim::kernel_sampling::KernelGenerationSpec,
    ) -> Result<(), HistoryError> {
        let binding = BindingKey {
            basis: self.selected_basis,
            output: self.selected_channel,
        };
        let kernel = self
            .selected_kernel
            .ok_or_else(|| HistoryError::Edit("selected rule-set has no kernel".to_string()))?;
        let mut next = self.draft.clone();
        let tiling = next
            .tiling
            .clone()
            .ok_or_else(|| HistoryError::Edit("periodic kernel needs a tiling".to_string()))?;
        let rule_set = next
            .rules
            .detach(binding)
            .map_err(|error| HistoryError::Edit(error.to_string()))?;
        let target = next
            .rules
            .get_mut(rule_set)
            .and_then(|rule| rule.kernels.iter_mut().find(|entry| entry.id == kernel))
            .ok_or_else(|| HistoryError::Edit("selected rule kernel is missing".to_string()))?;
        let crate::sim::ruleset::KernelSpatialDefinition::Periodic(definition) =
            &mut target.spatial
        else {
            return Err(HistoryError::Edit(
                "selected kernel is not periodic".to_string(),
            ));
        };
        let plane = crate::sim::kernel_sampling::generate_periodic_plane(
            &tiling,
            self.selected_basis,
            source_basis,
            definition,
            &generation,
        )
        .map_err(HistoryError::Edit)?;
        definition.planes.insert(source_basis, plane);
        self.execute(DraftCommand::ReplaceDraft(Box::new(next)))?;
        self.refresh_rule_selection();
        Ok(())
    }

    pub fn set_channel_view(&mut self, view: ChannelView) {
        self.channel_view = view;
    }

    fn replace_draft(&mut self, next: ExperimentSpec) -> Result<(), HistoryError> {
        self.execute(DraftCommand::ReplaceDraft(Box::new(next)))?;
        if self.draft.tiling.as_ref().is_some_and(|tiling| {
            self.selected_prototype
                .is_some_and(|id| !tiling.prototypes.iter().any(|prototype| prototype.id == id))
        }) {
            self.selected_prototype = self
                .draft
                .tiling
                .as_ref()
                .and_then(|tiling| tiling.prototypes.first().map(|prototype| prototype.id));
        } else if self.draft.tiling.is_some() && self.selected_prototype.is_none() {
            self.selected_prototype = self
                .draft
                .tiling
                .as_ref()
                .and_then(|tiling| tiling.prototypes.first().map(|prototype| prototype.id));
        }
        self.growth_editor =
            editor_for_basis(&self.draft, self.selected_basis, self.selected_channel);
        Ok(())
    }

    pub fn add_channel(&mut self) -> Result<(), HistoryError> {
        let added =
            crate::document::channels::add_channel(&self.draft).map_err(HistoryError::Edit)?;
        self.replace_draft(added.spec)?;
        self.selected_channel = added.channel;
        self.tiling_constraints.clear();
        self.refresh_rule_selection();
        self.selected_kernel = added.selected_kernel.or(self.selected_kernel);
        Ok(())
    }

    pub fn remove_selected_channel(&mut self) -> Result<(), String> {
        let (next, nearest) =
            crate::document::channels::remove_channel(&self.draft, self.selected_channel)?;
        self.replace_draft(next)
            .map_err(|error| error.to_string())?;
        self.selected_channel = nearest;
        self.refresh_rule_selection();
        Ok(())
    }

    pub fn select_next_channel(&mut self) {
        let index = self
            .draft
            .channels
            .iter()
            .position(|channel| channel.id == self.selected_channel)
            .unwrap_or(0);
        self.selected_channel = self.draft.channels[(index + 1) % self.draft.channels.len()].id;
        self.growth_editor =
            editor_for_basis(&self.draft, self.selected_basis, self.selected_channel);
        self.refresh_rule_selection();
    }

    pub fn cycle_channel_view(&mut self) {
        self.channel_view = match self.channel_view {
            ChannelView::Composite => ChannelView::Solo,
            ChannelView::Solo => ChannelView::Grid,
            ChannelView::Grid => ChannelView::Composite,
        };
    }

    pub fn cycle_selected_color(&mut self) -> Result<(), HistoryError> {
        let channel = self.selected_channel;
        let current = self
            .draft
            .channels
            .iter()
            .find(|entry| entry.id == channel)
            .map(|entry| entry.display.color.clone())
            .unwrap_or(DisplayColor::Auto);
        let color = match current {
            DisplayColor::Auto => DisplayColor::Custom(RgbColor {
                red: 255,
                green: 0,
                blue: 0,
            }),
            DisplayColor::Custom(RgbColor {
                red: 255,
                green: 0,
                blue: 0,
            }) => DisplayColor::Custom(RgbColor {
                red: 0,
                green: 255,
                blue: 0,
            }),
            DisplayColor::Custom(RgbColor {
                red: 0,
                green: 255,
                blue: 0,
            }) => DisplayColor::Custom(RgbColor {
                red: 0,
                green: 0,
                blue: 255,
            }),
            _ => DisplayColor::Auto,
        };
        self.execute(DraftCommand::SetChannelColor { channel, color })
    }

    pub fn toggle_selected_visibility(&mut self) -> Result<(), HistoryError> {
        let channel = self.selected_channel;
        let visible = !self
            .draft
            .channels
            .iter()
            .find(|entry| entry.id == channel)
            .is_none_or(|entry| entry.display.visible);
        self.execute(DraftCommand::SetChannelVisible { channel, visible })
    }

    pub fn toggle_selected_frozen(&mut self) -> Result<(), HistoryError> {
        let target = self.selected_channel;
        let Some(frozen) = self
            .draft
            .channels
            .iter()
            .find(|channel| channel.id == target)
            .map(|channel| !channel.frozen)
        else {
            return Ok(());
        };
        let next = crate::document::channels::set_channel_frozen(&self.draft, target, frozen)
            .map_err(HistoryError::Edit)?;
        self.replace_draft(next)
    }

    pub fn add_kernel_for_selected(&mut self) -> Result<(), HistoryError> {
        if !self.draft.rules.is_empty() {
            let binding = BindingKey {
                basis: self.selected_basis,
                output: self.selected_channel,
            };
            let mut next = self.draft.clone();
            let rule_set = next
                .rules
                .detach(binding)
                .map_err(|error| HistoryError::Edit(error.to_string()))?;
            let rule = next
                .rules
                .get_mut(rule_set)
                .ok_or_else(|| HistoryError::Edit("selected rule-set is missing".into()))?;
            let id = KernelId(
                rule.kernels
                    .iter()
                    .map(|kernel| kernel.id.0)
                    .max()
                    .unwrap_or(0)
                    .checked_add(1)
                    .ok_or_else(|| HistoryError::Edit("kernel id exhausted".into()))?,
            );
            let symbol = format!("k{}", id.0);
            let spatial = rule
                .kernels
                .first()
                .map(|kernel| kernel.spatial.clone())
                .unwrap_or_else(|| {
                    crate::sim::ruleset::KernelSpatialDefinition::Raster(
                        crate::sim::experiment_model::KernelSlot::identity(
                            id,
                            &symbol,
                            self.selected_channel,
                            self.selected_channel,
                        )
                        .definition,
                    )
                });
            rule.kernels.push(crate::sim::ruleset::RuleKernel {
                id,
                symbol: symbol.clone(),
                name: symbol,
                source_channel: self.selected_channel,
                spatial,
            });
            rule.growth.kernel_inputs.push(id);
            rule.validate()
                .map_err(|error| HistoryError::Edit(error.to_string()))?;
            self.execute(DraftCommand::ReplaceDraft(Box::new(next)))?;
            self.refresh_rule_selection();
            self.selected_kernel = Some(id);
            self.growth_editor =
                editor_for_basis(&self.draft, self.selected_basis, self.selected_channel);
            return Ok(());
        }
        let target = self.selected_channel;
        let mut next = self.draft.clone();
        let id = KernelId(
            next.kernels
                .iter()
                .map(|kernel| kernel.id.0.saturating_add(1))
                .max()
                .unwrap_or(0),
        );
        let symbol = format!("k{}", id.0);
        next.kernels
            .push(KernelSlot::identity(id, symbol, target, target));
        if let Some(growth) = next
            .growth
            .iter_mut()
            .find(|growth| growth.target == target)
        {
            growth.kernel_inputs.push(id);
            growth.kernel_inputs.sort_unstable();
        }
        self.replace_draft(next)?;
        self.selected_kernel = Some(id);
        self.growth_editor =
            editor_for_basis(&self.draft, self.selected_basis, self.selected_channel);
        Ok(())
    }

    pub fn cycle_selected_kernel_source(&mut self) -> Result<ChannelId, HistoryError> {
        if self.draft.channels.len() <= 1 {
            return Err(HistoryError::Edit(
                "add another channel before changing the kernel source".into(),
            ));
        }
        let selected = self
            .selected_kernel
            .ok_or_else(|| HistoryError::Edit("no kernel is selected".into()))?;
        let next_source = |current: ChannelId, channels: &[ChannelSpec]| {
            let index = channels
                .iter()
                .position(|channel| channel.id == current)
                .unwrap_or(0);
            channels[(index + 1) % channels.len()].id
        };
        let mut next = self.draft.clone();
        let source = if !next.rules.is_empty() {
            let binding = BindingKey {
                basis: self.selected_basis,
                output: self.selected_channel,
            };
            let rule_set = next
                .rules
                .detach(binding)
                .map_err(|error| HistoryError::Edit(error.to_string()))?;
            let kernel = next
                .rules
                .get_mut(rule_set)
                .and_then(|rule| rule.kernels.iter_mut().find(|kernel| kernel.id == selected))
                .ok_or_else(|| HistoryError::Edit("selected rule kernel is missing".into()))?;
            let source = next_source(kernel.source_channel, &next.channels);
            kernel.source_channel = source;
            next.rules
                .validate(&next.basis_ids(), &next.channels)
                .map_err(|errors| {
                    HistoryError::Edit(
                        errors
                            .into_iter()
                            .map(|error| error.to_string())
                            .collect::<Vec<_>>()
                            .join("; "),
                    )
                })?;
            source
        } else {
            let kernel = next
                .kernels
                .iter_mut()
                .find(|kernel| kernel.id == selected && kernel.target == self.selected_channel)
                .ok_or_else(|| HistoryError::Edit("selected kernel is missing".into()))?;
            let source = next_source(kernel.source, &next.channels);
            kernel.source = source;
            crate::sim::experiment_model::validate_structure(&next).map_err(|errors| {
                HistoryError::Edit(
                    errors
                        .into_iter()
                        .map(|error| error.to_string())
                        .collect::<Vec<_>>()
                        .join("; "),
                )
            })?;
            source
        };
        self.replace_draft(next)?;
        self.refresh_rule_selection();
        self.selected_kernel = Some(selected);
        Ok(source)
    }

    pub fn select_next_kernel_output(&mut self) -> Result<ChannelId, HistoryError> {
        let outputs = self
            .draft
            .channels
            .iter()
            .filter(|channel| !channel.frozen)
            .map(|channel| channel.id)
            .collect::<Vec<_>>();
        if outputs.is_empty() {
            return Err(HistoryError::Edit(
                "no active output channel is available".into(),
            ));
        }
        let index = outputs
            .iter()
            .position(|channel| *channel == self.selected_channel)
            .unwrap_or(0);
        self.selected_channel = outputs[(index + 1) % outputs.len()];
        self.growth_editor =
            editor_for_basis(&self.draft, self.selected_basis, self.selected_channel);
        self.refresh_rule_selection();
        Ok(self.selected_channel)
    }

    pub fn remove_last_kernel_for_selected(&mut self) -> Result<(), String> {
        if !self.draft.rules.is_empty() {
            let binding = BindingKey {
                basis: self.selected_basis,
                output: self.selected_channel,
            };
            let mut next = self.draft.clone();
            let rule_set = next
                .rules
                .detach(binding)
                .map_err(|error| error.to_string())?;
            let rule = next
                .rules
                .get_mut(rule_set)
                .ok_or("selected rule-set is missing")?;
            if rule.kernels.len() <= 1 {
                return Err("a rule-set must retain at least one kernel".into());
            }
            let position = self
                .selected_kernel
                .and_then(|id| rule.kernels.iter().position(|kernel| kernel.id == id))
                .unwrap_or(rule.kernels.len() - 1);
            let removed = rule.kernels.remove(position);
            rule.growth.kernel_inputs.retain(|id| *id != removed.id);
            validate_growth_after_kernel_removal(
                &rule.growth.source,
                rule.kernels
                    .iter()
                    .map(|kernel| kernel.symbol.clone())
                    .collect(),
                rule.growth.parameters.keys().cloned().collect(),
                &removed.symbol,
            )?;
            rule.validate().map_err(|error| error.to_string())?;
            self.execute(DraftCommand::ReplaceDraft(Box::new(next)))
                .map_err(|error| error.to_string())?;
            self.refresh_rule_selection();
            self.growth_editor =
                editor_for_basis(&self.draft, self.selected_basis, self.selected_channel);
            return Ok(());
        }
        let target = self.selected_channel;
        let mut next = self.draft.clone();
        let candidates = next
            .kernels
            .iter()
            .enumerate()
            .filter(|(_, kernel)| kernel.target == target)
            .map(|(position, kernel)| (position, kernel.id))
            .collect::<Vec<_>>();
        if candidates.len() <= 1 {
            return Err("a channel must retain at least one kernel".into());
        }
        let position = self
            .selected_kernel
            .and_then(|selected| {
                candidates
                    .iter()
                    .find(|(_, kernel)| *kernel == selected)
                    .map(|(position, _)| *position)
            })
            .unwrap_or_else(|| candidates.last().unwrap().0);
        let removed = next.kernels.remove(position);
        if let Some(growth) = next
            .growth
            .iter_mut()
            .find(|growth| growth.target == target)
        {
            growth.kernel_inputs.retain(|id| *id != removed.id);
            let symbols = growth
                .kernel_inputs
                .iter()
                .filter_map(|id| next.kernels.iter().find(|kernel| kernel.id == *id))
                .map(|kernel| kernel.symbol.clone())
                .collect();
            validate_growth_after_kernel_removal(
                &growth.source,
                symbols,
                growth.parameters.keys().cloned().collect(),
                &removed.symbol,
            )?;
        }
        self.replace_draft(next)
            .map_err(|error| error.to_string())?;
        self.refresh_rule_selection();
        self.growth_editor =
            editor_for_basis(&self.draft, self.selected_basis, self.selected_channel);
        Ok(())
    }

    pub fn cycle_tiling_preset(&mut self) -> Result<(), HistoryError> {
        let mut next = self.draft.clone();
        let previous_bases = next.basis_ids().len();
        let preset = match next
            .tiling
            .as_ref()
            .and_then(|tiling| tiling.prototypes.first())
            .map(|prototype| prototype.name.as_str())
        {
            None | Some("octagon") => TilingPreset::Square,
            Some("square") => TilingPreset::EquilateralTriangles,
            Some("up-triangle") => TilingPreset::RegularHexagon,
            Some("hexagon") => TilingPreset::OctagonSquare,
            Some(_) => TilingPreset::Square,
        };
        next.tiling = Some(build_preset(preset, 1.0));
        let bases = next.basis_ids();
        let tiles = next.geometry.tile_count().ok_or_else(|| {
            HistoryError::Edit("geometry is too large to resize its initial state".into())
        })?;
        for channel in &mut next.channels {
            if channel.initial.len() == tiles * previous_bases && previous_bases != bases.len() {
                let previous = std::mem::take(&mut channel.initial);
                channel.initial = (0..tiles)
                    .flat_map(|tile| {
                        (0..bases.len()).map({
                            let previous = &previous;
                            move |basis| {
                                previous[tile * previous_bases + basis.min(previous_bases - 1)]
                            }
                        })
                    })
                    .collect();
            }
        }
        if next.rules.is_empty() {
            next = next.normalize_rules().map_err(|errors| {
                HistoryError::Edit(
                    errors
                        .into_iter()
                        .map(|error| error.to_string())
                        .collect::<Vec<_>>()
                        .join("; "),
                )
            })?;
        }
        let active_channels = next
            .channels
            .iter()
            .filter(|channel| !channel.frozen)
            .map(|channel| channel.id)
            .collect::<Vec<_>>();
        next.rules.bindings.retain(|binding| {
            bases.contains(&binding.basis) && active_channels.contains(&binding.output)
        });
        for output in &active_channels {
            let default = *next.rules.defaults.get(output).ok_or_else(|| {
                HistoryError::Edit(format!("channel {:?} has no default rule-set", output))
            })?;
            for basis in &bases {
                if next.rules.binding(*basis, *output).is_none() {
                    next.rules.bindings.push(crate::sim::ruleset::RuleBinding {
                        basis: *basis,
                        output: *output,
                        rule_set: default,
                    });
                }
            }
        }
        for rule in &mut next.rules.sets {
            for kernel in &mut rule.kernels {
                let replacement = match &mut kernel.spatial {
                    crate::sim::ruleset::KernelSpatialDefinition::Raster(definition) => {
                        let built = definition
                            .build()
                            .map_err(|error| HistoryError::Edit(error.to_string()))?;
                        let planes = bases
                            .iter()
                            .map(|basis| {
                                (
                                    *basis,
                                    crate::sim::basis_kernel::BasisWeightPlane {
                                        values: built.values.clone(),
                                        mask: built.mask.clone(),
                                    },
                                )
                            })
                            .collect();
                        Some(crate::sim::ruleset::KernelSpatialDefinition::Periodic(
                            crate::sim::basis_kernel::PeriodicKernelDefinition {
                                width: built.width,
                                height: built.height,
                                anchor_x: built.anchor_x,
                                anchor_y: built.anchor_y,
                                planes,
                            },
                        ))
                    }
                    crate::sim::ruleset::KernelSpatialDefinition::Periodic(definition) => {
                        let plane_len = definition.width * definition.height;
                        let template = definition.planes.values().next().cloned().unwrap_or(
                            crate::sim::basis_kernel::BasisWeightPlane {
                                values: vec![0.0; plane_len],
                                mask: None,
                            },
                        );
                        let mut updated = std::collections::BTreeMap::new();
                        for basis in &bases {
                            updated.insert(
                                *basis,
                                definition
                                    .planes
                                    .get(basis)
                                    .cloned()
                                    .unwrap_or_else(|| template.clone()),
                            );
                        }
                        definition.planes = updated;
                        None
                    }
                };
                if let Some(replacement) = replacement {
                    kernel.spatial = replacement;
                }
            }
        }
        next.rules
            .validate(&bases, &next.channels)
            .map_err(|errors| {
                HistoryError::Edit(
                    errors
                        .into_iter()
                        .map(|error| error.to_string())
                        .collect::<Vec<_>>()
                        .join("; "),
                )
            })?;
        self.selected_basis = bases.first().copied().unwrap_or(BasisId(0));
        self.replace_draft(next)?;
        self.tiling_constraints.clear();
        self.refresh_rule_selection();
        self.growth_editor =
            editor_for_basis(&self.draft, self.selected_basis, self.selected_channel);
        Ok(())
    }

    /// Start an explicit new design instead of silently treating the loaded
    /// experiment's current tiling as a drawing preset. This is one undoable
    /// draft replacement and intentionally restores the documented defaults:
    /// one channel, one kernel, no polygon, and no periodic rule bindings.
    pub fn new_blank_design(&mut self) -> Result<(), HistoryError> {
        let (width, height) = match &self.draft.geometry {
            crate::sim::experiment_model::GeometrySpec::RasterGrid(grid) => {
                (grid.width, grid.height)
            }
        };
        let blank = ExperimentSpec::single_channel_lenia(width, height);
        self.replace_draft(blank)?;
        self.selected_channel = ChannelId(0);
        self.selected_basis = BasisId(0);
        self.selected_prototype = None;
        self.tiling_constraints.clear();
        self.tiling_construction.clear();
        self.tiling_pointer = None;
        self.tiling_tool = super::tiling_editor::TilingTool::Select;
        self.refresh_rule_selection();
        self.growth_editor =
            editor_for_basis(&self.draft, self.selected_basis, self.selected_channel);
        Ok(())
    }

    pub fn tiling_prototype(&self) -> Option<PrototypeId> {
        self.selected_prototype
    }

    pub fn select_next_prototype(&mut self) {
        let Some(tiling) = &self.draft.tiling else {
            return;
        };
        if tiling.instances.is_empty() {
            return;
        }
        let index = tiling
            .instances
            .iter()
            .position(|instance| instance.id == self.selected_basis)
            .unwrap_or(0);
        let instance = &tiling.instances[(index + 1) % tiling.instances.len()];
        self.selected_basis = instance.id;
        self.selected_prototype = Some(instance.prototype);
        self.tiling_selected_vertex = None;
        self.refresh_rule_selection();
        self.growth_editor =
            editor_for_basis(&self.draft, self.selected_basis, self.selected_channel);
    }

    pub fn adjust_prototype_sides(&mut self, delta: i16) -> Result<(), String> {
        let Some(selected) = self.selected_prototype else {
            return Err("choose a tiling prototype first".into());
        };
        let mut next = self.draft.clone();
        let Some(prototype) = next.tiling.as_mut().and_then(|tiling| {
            tiling
                .prototypes
                .iter_mut()
                .find(|prototype| prototype.id == selected)
        }) else {
            return Err("tiling prototype not found".into());
        };
        let PrototypeShape::RegularPolygon { sides, .. } = &mut prototype.shape else {
            return Err("custom polygon vertices are edited by loading a draft or preset".into());
        };
        *sides = (*sides as i16).saturating_add(delta).clamp(3, 64) as u16;
        self.replace_draft(next).map_err(|error| error.to_string())
    }

    pub fn import_draft(&mut self, draft: ExperimentSpec) -> Result<(), HistoryError> {
        self.replace_draft(draft)
    }

    pub fn import_tiling_drag_draft(&mut self, draft: ExperimentSpec) -> Result<(), HistoryError> {
        let command = DraftCommand::ReplaceDraft(Box::new(draft));
        if self.tiling_drag_active {
            self.history.coalesce_execute(&mut self.draft, command)?;
            self.selection_redo.clear();
        } else {
            self.execute(command)?;
            self.tiling_drag_active = true;
        }
        self.status = DraftStatus::Dirty;
        self.growth_editor =
            editor_for_basis(&self.draft, self.selected_basis, self.selected_channel);
        Ok(())
    }

    pub fn finish_tiling_drag(&mut self) {
        self.tiling_drag_active = false;
    }
}

/// A single translation-only triangle is necessarily half of a fundamental
/// parallelogram. When the user draws that first triangle, synthesize its
/// complementary triangle as a second semantic basis, then let the ordinary
/// strict full-edge solver establish and retain the seam constraints. This is
/// exact for every non-degenerate triangle; no equilateral assumption enters.
fn complete_single_triangle_cell(spec: &mut ExperimentSpec) -> Result<bool, String> {
    let previous_bases = spec.basis_ids();
    let Some(tiling) = spec.tiling.as_mut() else {
        return Ok(false);
    };
    if tiling.instances.len() != 1 || tiling.prototypes.len() != 1 {
        return Ok(false);
    }
    let original_basis = tiling.instances[0].id;
    let vertices = match &tiling.prototypes[0].shape {
        PrototypeShape::SimplePolygon { vertices } if vertices.len() == 3 => vertices.clone(),
        _ => return Ok(false),
    };
    let [p0, p1, p2] = [vertices[0], vertices[1], vertices[2]];
    let prototype = PrototypeId(
        tiling
            .prototypes
            .iter()
            .map(|entry| entry.id.0)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("prototype id exhausted")?,
    );
    let basis = BasisId(
        tiling
            .instances
            .iter()
            .map(|entry| entry.id.0)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("basis id exhausted")?,
    );
    tiling.translation_a = p1 - p0;
    tiling.translation_b = p2 - p0;
    tiling.prototypes.push(crate::sim::tiling::TilePrototype {
        id: prototype,
        name: format!("basis_{}_complement", basis.0),
        shape: PrototypeShape::SimplePolygon {
            vertices: vec![p1, p1 + p2 - p0, p2],
        },
    });
    tiling.instances.push(crate::sim::tiling::TileInstance {
        id: basis,
        prototype,
        transform: crate::sim::tiling::RigidTransform::default(),
    });

    let tile_count = spec
        .geometry
        .tile_count()
        .ok_or("geometry is too large to expand the triangle basis")?;
    if previous_bases.len() == 1 {
        for channel in &mut spec.channels {
            if channel.initial.len() == tile_count {
                channel.initial = channel
                    .initial
                    .iter()
                    .flat_map(|value| [*value, *value])
                    .collect();
            }
        }
    }
    for rule in &mut spec.rules.sets {
        for kernel in &mut rule.kernels {
            let crate::sim::ruleset::KernelSpatialDefinition::Periodic(definition) =
                &mut kernel.spatial
            else {
                continue;
            };
            let plane_len = definition.width * definition.height;
            let template = definition
                .planes
                .get(&original_basis)
                .cloned()
                .or_else(|| definition.planes.values().next().cloned())
                .unwrap_or(crate::sim::basis_kernel::BasisWeightPlane {
                    values: vec![0.0; plane_len],
                    mask: None,
                });
            definition.planes.insert(basis, template);
        }
    }
    for output in spec
        .channels
        .iter()
        .filter(|channel| !channel.frozen)
        .map(|channel| channel.id)
        .collect::<Vec<_>>()
    {
        let rule_set = spec
            .rules
            .binding(original_basis, output)
            .map(|binding| binding.rule_set)
            .or_else(|| spec.rules.defaults.get(&output).copied())
            .ok_or("active channel has no default rule-set")?;
        spec.rules.bindings.push(crate::sim::ruleset::RuleBinding {
            basis,
            output,
            rule_set,
        });
    }
    spec.rules
        .validate(&spec.basis_ids(), &spec.channels)
        .map_err(|errors| {
            errors
                .into_iter()
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        })?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::basis_kernel::PeriodicKernelDefinition;
    use crate::sim::ruleset::KernelSpatialDefinition;
    use crate::sim::tiling::BasisId;

    fn basis_fixture() -> ExperimentSpec {
        let mut spec = ExperimentSpec::single_channel_lenia(8, 8);
        spec.tiling = Some(build_preset(TilingPreset::OctagonSquare, 1.0));
        let mut spec = spec.normalize_rules().unwrap();
        let definition = PeriodicKernelDefinition {
            width: 1,
            height: 1,
            anchor_x: 0,
            anchor_y: 0,
            planes: [
                (
                    BasisId(0),
                    crate::sim::basis_kernel::BasisWeightPlane {
                        values: vec![1.0],
                        mask: None,
                    },
                ),
                (
                    BasisId(1),
                    crate::sim::basis_kernel::BasisWeightPlane {
                        values: vec![1.0],
                        mask: None,
                    },
                ),
            ]
            .into(),
        };
        spec.rules.sets[0].kernels[0].spatial = KernelSpatialDefinition::Periodic(definition);
        spec
    }

    #[test]
    fn changing_section_closes_editors_from_the_previous_section() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        state.select_section(WorkbenchSection::Growth);
        state.toggle_growth_editing();
        state.begin_numeric_editor(NumericEditor::begin("weight", 0.25, -1.0..=1.0));

        state.select_section(WorkbenchSection::Channels);

        assert!(!state.growth_editing());
        assert!(state.numeric_editor().is_none());
    }

    #[test]
    fn cycling_section_closes_editors_from_the_previous_section() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        state.select_section(WorkbenchSection::Growth);
        state.toggle_growth_editing();

        state.section_next();

        assert!(!state.growth_editing());
    }

    #[test]
    fn adding_kernel_updates_growth_arity_atomically() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        state.add_kernel_for_selected().unwrap();
        assert_eq!(state.draft().kernels.len(), 2);
        assert_eq!(state.draft().growth[0].kernel_inputs.len(), 2);
        assert_eq!(
            state.growth_editor().signature(),
            "fn growth(self: Scalar, potential: Scalar, k1: Scalar) -> Rate"
        );
        assert_eq!(state.selected_kernel(), Some(KernelId(1)));
        state.select_next_kernel();
        assert_eq!(state.selected_kernel(), Some(KernelId(0)));
        crate::sim::experiment_model::validate_structure(state.draft()).unwrap();
        state.undo().unwrap();
        assert_eq!(state.draft().kernels.len(), 1);
    }

    #[test]
    fn adding_a_legacy_channel_creates_one_editable_default_kernel() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));

        state.add_channel().unwrap();

        let output = state.selected_channel();
        let kernels = state
            .draft()
            .kernels
            .iter()
            .filter(|kernel| kernel.target == output)
            .collect::<Vec<_>>();
        assert_eq!(kernels.len(), 1);
        assert_eq!(kernels[0].source, output);
        assert_eq!(
            state
                .draft()
                .growth
                .iter()
                .find(|growth| growth.target == output)
                .unwrap()
                .kernel_inputs,
            vec![kernels[0].id],
        );
        assert_eq!(state.selected_kernel(), Some(kernels[0].id));
        crate::sim::experiment_model::validate_structure(state.draft()).unwrap();
    }

    #[test]
    fn deleting_a_selected_middle_channel_selects_nearest_and_restores_selection_on_undo() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        state.add_channel().unwrap();
        state.add_channel().unwrap();
        state.set_selected_channel(ChannelId(1)).unwrap();

        state.remove_selected_channel().unwrap();

        assert_eq!(state.selected_channel(), ChannelId(2));
        state.undo().unwrap();
        assert_eq!(state.selected_channel(), ChannelId(1));
        state.redo().unwrap();
        assert_eq!(state.selected_channel(), ChannelId(2));
    }

    #[test]
    fn deleting_then_adding_a_channel_keeps_ids_and_names_unique() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        state.add_channel().unwrap();
        state.add_channel().unwrap();
        state.set_selected_channel(ChannelId(1)).unwrap();
        state.remove_selected_channel().unwrap();

        state.add_channel().unwrap();

        assert_eq!(
            state
                .draft()
                .channels
                .iter()
                .map(|channel| channel.id)
                .collect::<Vec<_>>(),
            vec![ChannelId(0), ChannelId(2), ChannelId(3)]
        );
        let names = state
            .draft()
            .channels
            .iter()
            .map(|channel| channel.name.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(names.len(), 3);
        assert_eq!(
            state
                .draft()
                .channels
                .iter()
                .find(|channel| channel.id == ChannelId(3))
                .unwrap()
                .name,
            "channel_4"
        );
    }

    #[test]
    fn normalized_freeze_removes_bindings_and_unfreeze_restores_every_basis() {
        let mut state = WorkbenchState::new(basis_fixture());
        let bases = state.draft().basis_ids();

        state.toggle_selected_frozen().unwrap();

        assert!(state.draft().channels[0].frozen);
        assert!(
            bases
                .iter()
                .all(|basis| { state.draft().rules.binding(*basis, ChannelId(0)).is_none() })
        );

        state.toggle_selected_frozen().unwrap();

        assert!(!state.draft().channels[0].frozen);
        assert!(
            bases
                .iter()
                .all(|basis| { state.draft().rules.binding(*basis, ChannelId(0)).is_some() })
        );
        crate::sim::experiment_model::validate_structure(state.draft()).unwrap();
    }

    #[test]
    fn decision_stays_visible_across_section_changes_until_cancelled() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        state.present_decision(crate::workbench::decision::DecisionPanel::new(
            "Cannot delete",
            "Growth reads k1",
            vec![crate::workbench::decision::DecisionChoice::new(
                "cancel", "Cancel",
            )],
        ));

        state.select_section(WorkbenchSection::Growth);

        assert_eq!(state.decision().unwrap().detail, "Growth reads k1");
        state.cancel_decision();
        assert!(state.decision().is_none());
    }

    #[test]
    fn reverting_a_removed_selected_channel_restores_a_valid_editing_selection() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        state.add_channel().unwrap();
        state.accept(state.draft().clone());
        state.add_channel().unwrap();
        assert_eq!(state.draft().channels.len(), 3);

        state.revert();

        assert_eq!(state.draft().channels.len(), 2);
        assert!(
            state
                .draft()
                .channels
                .iter()
                .any(|channel| channel.id == state.selected_channel()),
            "revert left a removed channel selected"
        );
        let selected = state
            .selected_legacy_kernel()
            .expect("revert should select an editable kernel");
        assert_eq!(selected.target, state.selected_channel());
        assert!(state.selected_kernel().is_some());
    }

    #[test]
    fn legacy_kernel_source_cycles_across_channels_and_is_undoable() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        state.add_channel().unwrap();
        let kernel = state.selected_kernel().unwrap();
        let output = state.selected_channel();

        state.cycle_selected_kernel_source().unwrap();

        let changed = state
            .draft()
            .kernels
            .iter()
            .find(|entry| entry.id == kernel && entry.target == output)
            .unwrap();
        assert_eq!(changed.source, ChannelId(0));
        state.undo().unwrap();
        let restored = state
            .draft()
            .kernels
            .iter()
            .find(|entry| entry.id == kernel && entry.target == output)
            .unwrap();
        assert_eq!(restored.source, output);
    }

    #[test]
    fn tiling_pointer_drag_is_one_undo_unit() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        state.cycle_tiling_preset().unwrap();
        let before_drag = state.draft().clone();

        let mut first_motion = before_drag.clone();
        let PrototypeShape::SimplePolygon { vertices } =
            &mut first_motion.tiling.as_mut().unwrap().prototypes[0].shape
        else {
            panic!("square preset should be represented as an editable polygon");
        };
        vertices[0].x += 0.1;
        state.import_tiling_drag_draft(first_motion).unwrap();

        let mut final_motion = state.draft().clone();
        let PrototypeShape::SimplePolygon { vertices } =
            &mut final_motion.tiling.as_mut().unwrap().prototypes[0].shape
        else {
            panic!("square preset should be represented as an editable polygon");
        };
        vertices[0].x += 0.2;
        state
            .import_tiling_drag_draft(final_motion.clone())
            .unwrap();
        state.finish_tiling_drag();

        assert_eq!(state.draft(), &final_motion);
        state.undo().unwrap();
        assert_eq!(state.draft(), &before_drag);
        state.redo().unwrap();
        assert_eq!(state.draft(), &final_motion);
    }

    #[test]
    fn channel_and_tiling_actions_are_reversible() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        state.draft.channels[0].initial[3] = 0.375;
        state.add_channel().unwrap();
        assert_eq!(state.draft().channels.len(), 2);
        state.cycle_tiling_preset().unwrap();
        assert_eq!(state.draft().tiling.as_ref().unwrap().prototypes.len(), 1);
        state.cycle_tiling_preset().unwrap();
        assert_eq!(state.draft().tiling.as_ref().unwrap().prototypes.len(), 2);
        assert_eq!(state.draft().channels[0].initial.len(), 8 * 8 * 2);
        assert_eq!(state.draft().channels[0].initial[3 * 2], 0.375);
        assert_eq!(state.draft().channels[0].initial[3 * 2 + 1], 0.375);
    }

    #[test]
    fn normalized_raster_rule_kernel_is_exposed_and_editable() {
        let spec = ExperimentSpec::single_channel_lenia(8, 8)
            .normalize_rules()
            .unwrap();
        let mut state = WorkbenchState::new(spec);

        let definition = state
            .selected_raster_kernel_definition()
            .expect("normalized raster kernel should be visible to the editor")
            .clone();
        assert_eq!((definition.width, definition.height), (27, 27));

        let mut edited = definition;
        edited.values =
            crate::sim::kernel::KernelValues::Explicit(vec![0.25; edited.width * edited.height]);
        state
            .replace_selected_raster_kernel_definition(edited)
            .unwrap();

        let edited = state.selected_raster_kernel_definition().unwrap();
        let crate::sim::kernel::KernelValues::Explicit(values) = &edited.values else {
            panic!("edited raster should remain explicit");
        };
        assert!(values.iter().all(|value| *value == 0.25));
    }

    #[test]
    fn local_edit_detaches_whole_ruleset() {
        let mut state = WorkbenchState::new(basis_fixture());
        let sibling_before = state.rule_for(BasisId(1), ChannelId(0)).unwrap().clone();

        state.detach_selected_ruleset().unwrap();
        state
            .set_selected_kernel_weight([0, 0], BasisId(0), 0.25)
            .unwrap();

        assert_eq!(
            state.rule_for(BasisId(1), ChannelId(0)).unwrap(),
            &sibling_before
        );
        assert_ne!(
            state.rule_for(BasisId(0), ChannelId(0)).unwrap(),
            &sibling_before
        );
        state.undo().unwrap();
        state.undo().unwrap();
        assert_eq!(
            state.rule_for(BasisId(0), ChannelId(0)).unwrap(),
            &sibling_before
        );
    }

    #[test]
    fn periodic_weight_edit_automatically_detaches_one_basis_binding() {
        let mut state = WorkbenchState::new(basis_fixture());
        let sibling_before = state.rule_for(BasisId(1), ChannelId(0)).unwrap().clone();

        state
            .set_selected_kernel_weight([0, 0], BasisId(0), 0.25)
            .unwrap();

        assert_eq!(
            state.rule_for(BasisId(1), ChannelId(0)).unwrap(),
            &sibling_before,
            "editing one basis must not mutate its shared sibling",
        );
        assert_eq!(
            match &state.rule_for(BasisId(0), ChannelId(0)).unwrap().kernels[0].spatial {
                KernelSpatialDefinition::Periodic(definition) => {
                    definition.weight([0, 0], BasisId(0))
                }
                KernelSpatialDefinition::Raster(_) => None,
            },
            Some(0.25),
        );
        state.undo().unwrap();
        assert_eq!(
            state.rule_for(BasisId(0), ChannelId(0)).unwrap(),
            &sibling_before
        );
    }

    #[test]
    fn periodic_support_edit_is_reversible_and_clears_the_weight() {
        let mut state = WorkbenchState::new(basis_fixture());
        state
            .set_selected_kernel_weight([0, 0], BasisId(0), 0.25)
            .unwrap();

        state
            .set_selected_kernel_active([0, 0], BasisId(0), false)
            .unwrap();

        let KernelSpatialDefinition::Periodic(definition) =
            &state.selected_rule_kernel().unwrap().spatial
        else {
            panic!("basis fixture should use a periodic kernel");
        };
        assert_eq!(definition.is_active([0, 0], BasisId(0)), Some(false));
        assert_eq!(definition.raw_weight([0, 0], BasisId(0)), Some(0.0));

        state.undo().unwrap();
        let KernelSpatialDefinition::Periodic(definition) =
            &state.selected_rule_kernel().unwrap().spatial
        else {
            panic!("basis fixture should use a periodic kernel");
        };
        assert_eq!(definition.weight([0, 0], BasisId(0)), Some(0.25));
    }

    #[test]
    fn periodic_resize_is_one_reversible_draft_change() {
        let mut state = WorkbenchState::new(basis_fixture());

        state.resize_selected_periodic_kernel(3, 3, 1, 1).unwrap();

        let KernelSpatialDefinition::Periodic(definition) =
            &state.selected_rule_kernel().unwrap().spatial
        else {
            panic!("basis fixture should use a periodic kernel");
        };
        assert_eq!(
            (
                definition.width,
                definition.height,
                definition.anchor_x,
                definition.anchor_y,
            ),
            (3, 3, 1, 1),
        );
        assert_eq!(definition.weight([0, 0], BasisId(0)), Some(1.0));
        assert_eq!(definition.is_active([1, 0], BasisId(0)), Some(false));

        state.undo().unwrap();
        let KernelSpatialDefinition::Periodic(definition) =
            &state.selected_rule_kernel().unwrap().spatial
        else {
            panic!("basis fixture should use a periodic kernel");
        };
        assert_eq!((definition.width, definition.height), (1, 1));
    }

    #[test]
    fn kernel_tool_defaults_to_weights_and_cycles_to_support() {
        let mut state = WorkbenchState::new(basis_fixture());

        assert_eq!(
            state.kernel_tool(),
            crate::workbench::kernel_editor::KernelTool::Weights
        );
        state.cycle_kernel_tool();
        assert_eq!(
            state.kernel_tool(),
            crate::workbench::kernel_editor::KernelTool::Support
        );
        state.cycle_kernel_tool();
        assert_eq!(
            state.kernel_tool(),
            crate::workbench::kernel_editor::KernelTool::Weights
        );
    }

    #[test]
    fn selected_periodic_kernel_can_apply_a_world_space_gaussian_undoably() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        state.cycle_tiling_preset().unwrap();
        state.cycle_tiling_preset().unwrap();
        state.cycle_tiling_preset().unwrap();
        let before = state.selected_rule_kernel().unwrap().clone();

        state
            .generate_selected_periodic_kernel(
                BasisId(0),
                crate::sim::kernel_sampling::KernelGenerationSpec {
                    metric: crate::sim::kernel_sampling::KernelSamplingMetric::WorldEuclidean,
                    profile: crate::sim::kernel_sampling::KernelProfile::Gaussian { sigma: 1.0 },
                    amplitude: 1.0,
                    support_radius: None,
                },
            )
            .unwrap();

        let KernelSpatialDefinition::Periodic(definition) =
            &state.selected_rule_kernel().unwrap().spatial
        else {
            panic!("hexagonal preset must use periodic kernels");
        };
        let weights = [
            definition.weight([1, 0], BasisId(0)).unwrap(),
            definition.weight([0, 1], BasisId(0)).unwrap(),
            definition.weight([-1, 1], BasisId(0)).unwrap(),
            definition.weight([-1, 0], BasisId(0)).unwrap(),
            definition.weight([0, -1], BasisId(0)).unwrap(),
            definition.weight([1, -1], BasisId(0)).unwrap(),
        ];
        assert!(
            weights
                .windows(2)
                .all(|pair| (pair[0] - pair[1]).abs() < 1.0e-6)
        );

        state.undo().unwrap();
        assert_eq!(state.selected_rule_kernel(), Some(&before));
    }

    #[test]
    fn changing_basis_refreshes_the_basis_specific_growth_program() {
        let mut spec = basis_fixture();
        let second = spec
            .rules
            .detach(BindingKey {
                basis: BasisId(1),
                output: ChannelId(0),
            })
            .unwrap();
        spec.rules.get_mut(second).unwrap().growth.source = "self * 0.25".into();
        let mut state = WorkbenchState::new(spec);
        assert_ne!(state.growth_editor().buffer().as_str(), "self * 0.25");
        state.set_selected_basis(BasisId(1)).unwrap();
        assert_eq!(state.growth_editor().buffer().as_str(), "self * 0.25");
    }

    #[test]
    fn normalized_growth_edit_updates_only_the_selected_basis_ruleset() {
        let mut state = WorkbenchState::new(basis_fixture());
        let sibling_before = state.rule_for(BasisId(1), ChannelId(0)).unwrap().clone();
        state.growth_editor_mut().replace_source("self * 0.5");
        state.growth_editor_mut().refresh_now();
        state.sync_growth_source();

        assert_eq!(
            state
                .rule_for(BasisId(0), ChannelId(0))
                .unwrap()
                .growth
                .source,
            "self * 0.5",
        );
        assert_eq!(
            state.rule_for(BasisId(1), ChannelId(0)).unwrap(),
            &sibling_before
        );
    }

    #[test]
    fn growth_mode_is_per_ruleset_detaches_shared_defaults_and_undoes() {
        let mut state = WorkbenchState::new(basis_fixture());
        let sibling_before = state.rule_for(BasisId(1), ChannelId(0)).unwrap().clone();
        assert_eq!(
            state.selected_growth_mode(),
            Some(crate::sim::experiment_model::UpdateMode::GrowthRate),
        );

        state.toggle_selected_growth_mode().unwrap();

        assert_eq!(
            state.selected_growth_mode(),
            Some(crate::sim::experiment_model::UpdateMode::DirectUpdate),
        );
        assert_eq!(
            state.rule_for(BasisId(1), ChannelId(0)).unwrap(),
            &sibling_before,
            "changing one basis must not change a shared sibling",
        );
        assert!(state.growth_editor().signature().ends_with("-> Value"));

        state.undo().unwrap();
        assert_eq!(
            state.selected_growth_mode(),
            Some(crate::sim::experiment_model::UpdateMode::GrowthRate),
        );
    }

    #[test]
    fn simulation_dt_edit_is_validated_and_undoable() {
        let mut state = WorkbenchState::new(basis_fixture());
        assert!(state.set_simulation_dt(0.0).is_err());
        assert!(state.set_simulation_dt(f32::NAN).is_err());

        state.set_simulation_dt(0.025).unwrap();
        assert_eq!(state.draft().simulation_dt, 0.025);

        state.undo().unwrap();
        assert_eq!(state.draft().simulation_dt, 0.1);
    }

    #[test]
    fn normalized_kernel_add_updates_only_selected_ruleset_and_growth_arity() {
        let mut state = WorkbenchState::new(basis_fixture());
        let sibling_before = state.rule_for(BasisId(1), ChannelId(0)).unwrap().clone();
        state.add_kernel_for_selected().unwrap();
        let selected = state.rule_for(BasisId(0), ChannelId(0)).unwrap();
        assert_eq!(selected.kernels.len(), 2);
        assert_eq!(selected.growth.kernel_inputs.len(), 2);
        assert_eq!(
            state.rule_for(BasisId(1), ChannelId(0)).unwrap(),
            &sibling_before
        );
        selected.validate().unwrap();
    }

    #[test]
    fn kernel_removal_preserves_growth_signature_atomically() {
        let mut state = WorkbenchState::new(basis_fixture());
        state.add_kernel_for_selected().unwrap();
        let rule_set = state.selected_rule_set().unwrap();
        let removed = state.selected_kernel.unwrap();
        let removed_symbol = state
            .draft
            .rules
            .get(rule_set)
            .unwrap()
            .kernels
            .iter()
            .find(|kernel| kernel.id == removed)
            .unwrap()
            .symbol
            .clone();
        state.draft.rules.get_mut(rule_set).unwrap().growth.source =
            format!("self + {removed_symbol}");
        let before = state.draft.clone();
        let before_status = state.status();

        let error = state.remove_last_kernel_for_selected().unwrap_err();

        assert!(error.contains(&removed_symbol), "{error}");
        assert_eq!(state.draft(), &before);
        assert_eq!(state.status(), before_status);

        let rule_set = state.selected_rule_set().unwrap();
        let remaining_symbol = state
            .draft
            .rules
            .get(rule_set)
            .unwrap()
            .kernels
            .iter()
            .find(|kernel| kernel.id != removed)
            .unwrap()
            .symbol
            .clone();
        state.draft.rules.get_mut(rule_set).unwrap().growth.source =
            format!("self + {remaining_symbol}");
        let before_success = state.draft.clone();

        state.remove_last_kernel_for_selected().unwrap();
        assert_eq!(
            state
                .rule_for(state.selected_basis(), state.selected_channel())
                .unwrap()
                .kernels
                .len(),
            1,
        );
        state.undo().unwrap();
        assert_eq!(state.draft(), &before_success);
    }

    #[test]
    fn legacy_kernel_removal_also_rejects_a_referenced_symbol() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        state.add_kernel_for_selected().unwrap();
        let removed = state.selected_kernel.unwrap();
        let removed_symbol = state
            .draft
            .kernels
            .iter()
            .find(|kernel| kernel.id == removed)
            .unwrap()
            .symbol
            .clone();
        state
            .draft
            .growth
            .iter_mut()
            .find(|growth| growth.target == state.selected_channel)
            .unwrap()
            .source = format!("self + {removed_symbol}");
        let before = state.draft.clone();

        let error = state.remove_last_kernel_for_selected().unwrap_err();

        assert!(error.contains(&removed_symbol), "{error}");
        assert_eq!(state.draft(), &before);
    }

    #[test]
    fn normalized_channel_add_creates_one_kernel_default_for_every_basis() {
        let mut state = WorkbenchState::new(basis_fixture());
        state.add_channel().unwrap();
        let channel = state.selected_channel();
        let default = state.draft().rules.defaults[&channel];
        assert_eq!(state.draft().rules.get(default).unwrap().kernels.len(), 1);
        for basis in [BasisId(0), BasisId(1)] {
            assert!(state.draft().rules.binding(basis, channel).is_some());
        }
        state
            .draft()
            .rules
            .validate(&state.draft().basis_ids(), &state.draft().channels)
            .unwrap();
    }

    #[test]
    fn normalized_kernel_source_edit_detaches_only_the_selected_basis_output() {
        let mut state = WorkbenchState::new(basis_fixture());
        state.add_channel().unwrap();
        let output = state.selected_channel();
        let sibling_before = state.rule_for(BasisId(1), output).unwrap().clone();

        state.cycle_selected_kernel_source().unwrap();

        assert_eq!(
            state.rule_for(BasisId(0), output).unwrap().kernels[0].source_channel,
            ChannelId(0),
        );
        assert_eq!(state.rule_for(BasisId(1), output).unwrap(), &sibling_before);
        state
            .draft()
            .rules
            .validate(&state.draft().basis_ids(), &state.draft().channels)
            .unwrap();
    }

    #[test]
    fn preset_cycle_exposes_square_triangles_hexagon_and_octagon_square_with_periodic_rules() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        for (expected_name, expected_bases) in [
            ("square", 1),
            ("up-triangle", 2),
            ("hexagon", 1),
            ("octagon", 2),
        ] {
            state.cycle_tiling_preset().unwrap();
            let draft = state.draft();
            assert_eq!(
                draft.tiling.as_ref().unwrap().prototypes[0].name,
                expected_name
            );
            assert_eq!(draft.basis_ids().len(), expected_bases);
            let rule = state.rule_for(BasisId(0), ChannelId(0)).unwrap();
            let KernelSpatialDefinition::Periodic(definition) = &rule.kernels[0].spatial else {
                panic!("tiling preset must switch kernels to periodic basis planes");
            };
            assert_eq!(definition.planes.len(), expected_bases);
            draft
                .rules
                .validate(&draft.basis_ids(), &draft.channels)
                .unwrap();
        }
    }

    #[test]
    fn free_draw_can_add_a_second_semantic_basis_with_default_rules() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        state.cycle_tiling_preset().unwrap();
        state.begin_new_basis_polygon();
        for point in [
            crate::sim::tiling::Vec2::new(1.2, 0.1),
            crate::sim::tiling::Vec2::new(1.8, 0.1),
            crate::sim::tiling::Vec2::new(1.5, 0.6),
        ] {
            state.push_tiling_vertex(point).unwrap();
        }
        state.finish_tiling_construction().unwrap();
        assert_eq!(state.draft().basis_ids().len(), 2);
        let added = state.selected_basis();
        assert!(state.draft().rules.binding(added, ChannelId(0)).is_some());
        let rule = state.rule_for(added, ChannelId(0)).unwrap();
        let KernelSpatialDefinition::Periodic(definition) = &rule.kernels[0].spatial else {
            panic!("new basis needs a periodic kernel plane");
        };
        assert!(definition.planes.contains_key(&added));
    }

    #[test]
    fn free_draw_rejects_a_vertex_that_overlaps_an_existing_vertex_immediately() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        state.begin_new_basis_polygon();
        let point = crate::sim::tiling::Vec2::new(0.25, -0.5);

        state.push_tiling_vertex(point).unwrap();
        assert!(state.push_tiling_vertex(point).is_err());

        assert_eq!(
            state.tiling_construction(),
            &[point],
            "an invalid duplicate must never enter the in-progress path"
        );
    }

    #[test]
    fn free_draw_rejects_a_new_edge_that_crosses_the_open_path_immediately() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        state.begin_new_basis_polygon();
        for point in [
            crate::sim::tiling::Vec2::new(0.0, 0.0),
            crate::sim::tiling::Vec2::new(1.0, 1.0),
            crate::sim::tiling::Vec2::new(0.0, 1.0),
        ] {
            state.push_tiling_vertex(point).unwrap();
        }

        assert!(
            state
                .push_tiling_vertex(crate::sim::tiling::Vec2::new(1.0, 0.0))
                .is_err()
        );

        assert_eq!(
            state.tiling_construction().len(),
            3,
            "a crossing edge must be rejected before it enters the in-progress path"
        );
    }

    #[test]
    fn free_draw_can_create_the_first_verified_translation_tiling_without_a_preset() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        assert!(state.draft().tiling.is_none());
        state.begin_new_basis_polygon();
        for point in [
            crate::sim::tiling::Vec2::new(-1.0, -1.0),
            crate::sim::tiling::Vec2::new(1.0, -1.0),
            crate::sim::tiling::Vec2::new(1.0, 1.0),
            crate::sim::tiling::Vec2::new(-1.0, 1.0),
        ] {
            state.push_tiling_vertex(point).unwrap();
        }

        state.finish_tiling_construction().unwrap();

        let tiling = state.draft().tiling.as_ref().unwrap();
        assert_eq!(tiling.instances.len(), 1);
        crate::sim::tiling::validate_coverage(tiling).unwrap();
        let basis = state.selected_basis();
        let rule = state.rule_for(basis, ChannelId(0)).unwrap();
        let KernelSpatialDefinition::Periodic(definition) = &rule.kernels[0].spatial else {
            panic!("first free-drawn tiling must convert raster kernels to periodic basis planes");
        };
        assert!(definition.planes.contains_key(&basis));
        state
            .draft()
            .rules
            .validate(&state.draft().basis_ids(), &state.draft().channels)
            .unwrap();
    }

    #[test]
    fn closing_the_first_polygon_does_not_require_it_to_tile_by_itself() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        state.begin_new_basis_polygon();
        for point in [
            crate::sim::tiling::Vec2::new(0.0, 0.0),
            crate::sim::tiling::Vec2::new(2.0, 0.0),
            crate::sim::tiling::Vec2::new(0.0, 1.0),
        ] {
            state.push_tiling_vertex(point).unwrap();
        }

        state.finish_tiling_construction().unwrap();

        assert!(state.tiling_construction().is_empty());
        assert_ne!(
            state.tiling_tool(),
            crate::workbench::tiling_editor::TilingTool::DrawPolygon
        );
        let tiling = state
            .draft()
            .tiling
            .as_ref()
            .expect("the closed polygon must be visible in the draft");
        assert_eq!(tiling.prototypes.len(), 1);
        assert!(
            crate::sim::tiling::validate_coverage(tiling).is_err(),
            "a half-cell is allowed to remain visibly incomplete until its neighbor is drawn"
        );
    }

    #[test]
    fn free_draw_normalizes_the_opposite_screen_winding() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        state.begin_new_basis_polygon();
        for point in [
            crate::sim::tiling::Vec2::new(-1.0, -1.0),
            crate::sim::tiling::Vec2::new(-1.0, 1.0),
            crate::sim::tiling::Vec2::new(1.0, 1.0),
            crate::sim::tiling::Vec2::new(1.0, -1.0),
        ] {
            state.push_tiling_vertex(point).unwrap();
        }

        state.finish_tiling_construction().unwrap();

        let tiling = state.draft().tiling.as_ref().unwrap();
        let crate::sim::tiling::PrototypeShape::SimplePolygon { vertices } =
            &tiling.prototypes[0].shape
        else {
            panic!("free-drawn basis must remain a simple polygon");
        };
        assert!(crate::sim::tiling::polygon::signed_area(vertices) > 0.0);
        crate::sim::tiling::validate_coverage(tiling).unwrap();
    }

    #[test]
    fn next_basis_changes_the_ruleset_target_not_only_the_prototype_highlight() {
        let mut state = WorkbenchState::new(basis_fixture());
        assert_eq!(state.selected_basis(), BasisId(0));
        state.select_next_prototype();
        assert_eq!(state.selected_basis(), BasisId(1));
        assert_eq!(state.tiling_prototype(), Some(PrototypeId(1)));
    }

    #[test]
    fn seam_assist_solves_and_keeps_later_vertex_drags_edge_to_edge() {
        let mut state = WorkbenchState::new(basis_fixture());
        state.cycle_tiling_preset().unwrap();
        let mut rough = state.draft().clone();
        rough.tiling.as_mut().unwrap().translation_a.x += 0.02;
        state.replace_draft(rough).unwrap();

        let summary = state.solve_tiling_seams().unwrap();
        assert_eq!(summary.seams, 2);
        assert_eq!(state.tiling_constraint_count(), 2);
        crate::sim::tiling::validate_coverage(state.draft().tiling.as_ref().unwrap()).unwrap();

        let prototype = state.draft().tiling.as_ref().unwrap().prototypes[0].id;
        state
            .drag_constrained_tiling_vertex(prototype, 2, crate::sim::tiling::Vec2::new(1.1, 1.1))
            .unwrap();
        crate::sim::tiling::validate_coverage(state.draft().tiling.as_ref().unwrap()).unwrap();
    }

    #[test]
    fn blank_design_discards_the_loaded_tiling_and_restores_one_channel_and_kernel() {
        let mut state = WorkbenchState::new(basis_fixture());
        state.add_channel().unwrap();
        assert!(state.draft().tiling.is_some());

        state.new_blank_design().unwrap();

        assert!(state.draft().tiling.is_none());
        assert_eq!(state.draft().channels.len(), 1);
        assert_eq!(state.draft().kernels.len(), 1);
        assert!(state.draft().rules.is_empty());
        state.undo().unwrap();
        assert!(state.draft().tiling.is_some());
        assert_eq!(state.draft().channels.len(), 2);
    }

    #[test]
    fn seam_assist_completes_a_free_drawn_triangle_into_an_exact_periodic_cell() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        state.begin_new_basis_polygon();
        for point in [
            crate::sim::tiling::Vec2::new(-0.9, -0.7),
            crate::sim::tiling::Vec2::new(1.1, -0.6),
            crate::sim::tiling::Vec2::new(-0.2, 0.9),
        ] {
            state.push_tiling_vertex(point).unwrap();
        }
        state.finish_tiling_construction().unwrap();
        assert!(
            crate::sim::tiling::validate_coverage(state.draft().tiling.as_ref().unwrap()).is_err()
        );

        let summary = state.solve_tiling_seams().unwrap();

        assert_eq!(summary.seams, 3);
        assert_eq!(state.draft().basis_ids().len(), 2);
        crate::sim::tiling::validate_coverage(state.draft().tiling.as_ref().unwrap()).unwrap();
        state
            .draft()
            .rules
            .validate(&state.draft().basis_ids(), &state.draft().channels)
            .unwrap();
    }
}
