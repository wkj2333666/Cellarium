use super::growth_editor::editor_for_basis;
use super::kernel_editor::{KernelPoint, KernelSelection};
use super::numeric_editor::NumericEditor;
use super::{ChannelView, DraftCommand, GrowthEditorState, History, HistoryError};
use crate::sim::experiment_model::{
    ChannelId, DisplayColor, ExperimentSpec, KernelId, KernelSlot, RgbColor,
};
use crate::sim::ruleset::{BindingKey, RuleKernel, RuleSet, RuleSetId};
use crate::sim::tiling::{BasisId, PrototypeId, PrototypeShape, TilingPreset, build_preset};

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

#[derive(Clone, Debug)]
pub struct WorkbenchState {
    authoritative: ExperimentSpec,
    draft: ExperimentSpec,
    history: History,
    section: WorkbenchSection,
    focus: WorkbenchFocus,
    status: DraftStatus,
    selected_channel: ChannelId,
    selected_basis: BasisId,
    selected_rule_set: Option<RuleSetId>,
    selected_kernel: Option<KernelId>,
    channel_view: ChannelView,
    growth_editor: GrowthEditorState,
    growth_editing: bool,
    selected_prototype: Option<PrototypeId>,
    kernel_view: super::kernel_editor::KernelView,
    kernel_selection: Option<KernelPoint>,
    periodic_kernel_selection: Option<KernelSelection>,
    kernel_paint_value: f32,
    numeric_editor: Option<NumericEditor>,
    tiling_tool: super::tiling_editor::TilingTool,
    tiling_construction: Vec<crate::sim::tiling::Vec2>,
    tiling_new_basis: bool,
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
            .map(|kernel| kernel.id);
        let growth_editor = editor_for_basis(&spec, selected_basis, selected_channel);
        let selected_prototype = spec
            .tiling
            .as_ref()
            .and_then(|tiling| tiling.prototypes.first().map(|prototype| prototype.id));
        Self {
            authoritative: spec.clone(),
            draft: spec,
            history: History::default(),
            section: WorkbenchSection::World,
            focus: WorkbenchFocus::Outline,
            status: DraftStatus::Clean,
            selected_channel,
            selected_basis,
            selected_rule_set,
            selected_kernel,
            channel_view: ChannelView::Composite,
            growth_editor,
            growth_editing: false,
            selected_prototype,
            kernel_view: super::kernel_editor::KernelView::default(),
            kernel_selection: None,
            periodic_kernel_selection: None,
            kernel_paint_value: 0.05,
            numeric_editor: None,
            tiling_tool: super::tiling_editor::TilingTool::Select,
            tiling_construction: Vec::new(),
            tiling_new_basis: false,
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
    }
    pub fn take_numeric_editor(&mut self) -> Option<NumericEditor> {
        self.numeric_editor.take()
    }
    pub fn tiling_tool(&self) -> super::tiling_editor::TilingTool {
        self.tiling_tool
    }
    pub fn set_tiling_tool(&mut self, tool: super::tiling_editor::TilingTool) {
        self.tiling_tool = tool;
        if tool != super::tiling_editor::TilingTool::DrawPolygon {
            self.tiling_construction.clear();
            self.tiling_new_basis = false;
        }
    }
    pub fn begin_new_basis_polygon(&mut self) {
        self.tiling_tool = super::tiling_editor::TilingTool::DrawPolygon;
        self.tiling_construction.clear();
        self.tiling_new_basis = true;
    }
    pub fn is_drawing_new_basis(&self) -> bool {
        self.tiling_new_basis
    }
    pub fn tiling_construction(&self) -> &[crate::sim::tiling::Vec2] {
        &self.tiling_construction
    }
    pub fn push_tiling_vertex(&mut self, point: crate::sim::tiling::Vec2) {
        self.tiling_construction.push(point);
    }
    pub fn cancel_tiling_construction(&mut self) {
        self.tiling_construction.clear();
        self.tiling_tool = super::tiling_editor::TilingTool::Select;
        self.tiling_new_basis = false;
    }
    pub fn finish_tiling_construction(&mut self) -> Result<(), String> {
        if self.tiling_construction.len() < 3 {
            return Err("place at least three vertices before closing the polygon".into());
        }
        let issues = crate::sim::tiling::polygon::validate_polygon(&self.tiling_construction);
        if let Some(issue) = issues.first() {
            return Err(issue.message.clone());
        }
        let mut next = self.draft.clone();
        if self.tiling_new_basis {
            let tiling = next
                .tiling
                .as_mut()
                .ok_or("create or select a tiling first")?;
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
            tiling.prototypes.push(crate::sim::tiling::TilePrototype {
                id: prototype,
                name: format!("basis_{}", basis.0),
                shape: PrototypeShape::SimplePolygon {
                    vertices: self.tiling_construction.clone(),
                },
            });
            tiling.instances.push(crate::sim::tiling::TileInstance {
                id: basis,
                prototype,
                transform: crate::sim::tiling::RigidTransform::default(),
            });
            if next.rules.is_empty() {
                next = next.normalize_rules().map_err(|errors| {
                    errors
                        .into_iter()
                        .map(|error| error.to_string())
                        .collect::<Vec<_>>()
                        .join("; ")
                })?;
            } else {
                for rule in &mut next.rules.sets {
                    for kernel in &mut rule.kernels {
                        if let crate::sim::ruleset::KernelSpatialDefinition::Periodic(definition) =
                            &mut kernel.spatial
                        {
                            let plane_len = definition.width * definition.height;
                            let template = definition.planes.values().next().cloned().unwrap_or(
                                crate::sim::basis_kernel::BasisWeightPlane {
                                    values: vec![0.0; plane_len],
                                    mask: None,
                                },
                            );
                            definition.planes.insert(basis, template);
                        }
                    }
                }
                for output in next
                    .channels
                    .iter()
                    .filter(|channel| !channel.frozen)
                    .map(|channel| channel.id)
                    .collect::<Vec<_>>()
                {
                    let default = *next
                        .rules
                        .defaults
                        .get(&output)
                        .ok_or("active channel has no default rule-set")?;
                    next.rules.bindings.push(crate::sim::ruleset::RuleBinding {
                        basis,
                        output,
                        rule_set: default,
                    });
                }
            }
            self.selected_prototype = Some(prototype);
            self.selected_basis = basis;
        } else {
            let selected = self
                .selected_prototype
                .ok_or("select a basis polygon first")?;
            let prototype = next
                .tiling
                .as_mut()
                .and_then(|tiling| {
                    tiling
                        .prototypes
                        .iter_mut()
                        .find(|entry| entry.id == selected)
                })
                .ok_or("selected basis polygon is missing")?;
            prototype.shape = PrototypeShape::SimplePolygon {
                vertices: self.tiling_construction.clone(),
            };
        }
        self.replace_draft(next)
            .map_err(|error| error.to_string())?;
        self.tiling_construction.clear();
        self.tiling_tool = super::tiling_editor::TilingTool::Select;
        self.tiling_new_basis = false;
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
        self.selected_prototype = self
            .draft
            .tiling
            .as_ref()
            .and_then(|tiling| tiling.prototypes.first().map(|prototype| prototype.id));
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
    pub fn execute(&mut self, command: DraftCommand) -> Result<(), HistoryError> {
        self.history.execute(&mut self.draft, command)?;
        self.status = DraftStatus::Dirty;
        Ok(())
    }
    pub fn undo(&mut self) -> Result<(), HistoryError> {
        self.history.undo(&mut self.draft)?;
        self.status = if self.draft == self.authoritative {
            DraftStatus::Clean
        } else {
            DraftStatus::Dirty
        };
        Ok(())
    }
    pub fn redo(&mut self) -> Result<(), HistoryError> {
        self.history.redo(&mut self.draft)?;
        self.status = DraftStatus::Dirty;
        Ok(())
    }
    pub fn revert(&mut self) {
        self.draft = self.authoritative.clone();
        self.growth_editor =
            editor_for_basis(&self.draft, self.selected_basis, self.selected_channel);
        self.growth_editing = false;
        self.selected_prototype = self
            .draft
            .tiling
            .as_ref()
            .and_then(|tiling| tiling.prototypes.first().map(|prototype| prototype.id));
        self.history.clear();
        self.status = DraftStatus::Clean;
        self.selected_basis = self
            .draft
            .basis_ids()
            .first()
            .copied()
            .unwrap_or(BasisId(0));
        self.refresh_rule_selection();
    }
    pub fn accept(&mut self, normalized: ExperimentSpec) {
        self.authoritative = normalized.clone();
        self.draft = normalized;
        self.selected_channel = self.draft.channels.first().map_or(ChannelId(0), |c| c.id);
        self.growth_editor =
            editor_for_basis(&self.draft, self.selected_basis, self.selected_channel);
        self.growth_editing = false;
        self.history.clear();
        self.status = DraftStatus::Clean;
        self.selected_basis = self
            .draft
            .basis_ids()
            .first()
            .copied()
            .unwrap_or(BasisId(0));
        self.refresh_rule_selection();
    }
    pub fn select_section(&mut self, section: WorkbenchSection) {
        self.section = section;
    }
    pub fn section_next(&mut self) {
        let index = WorkbenchSection::ALL
            .iter()
            .position(|value| *value == self.section)
            .unwrap_or(0);
        self.section = WorkbenchSection::ALL[(index + 1) % WorkbenchSection::ALL.len()];
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
        self.selected_kernel = self
            .selected_rule_set
            .and_then(|rule_set| self.draft.rules.get(rule_set))
            .and_then(|rule| rule.kernels.first())
            .map(|kernel| kernel.id);
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
        let mut next = self.draft.clone();
        let name = format!("channel_{}", next.channels.len() + 1);
        let id = next.add_channel(name, false);
        if !next.rules.is_empty() {
            next.growth.retain(|growth| growth.target != id);
            let rule_set_id = RuleSetId(
                next.rules
                    .sets
                    .iter()
                    .map(|rule| rule.id.0)
                    .max()
                    .unwrap_or(0)
                    .checked_add(1)
                    .ok_or_else(|| HistoryError::Edit("rule-set id exhausted".into()))?,
            );
            let mut rule_set = RuleSet::identity(rule_set_id, id);
            if next.tiling.is_some() {
                let bases = next.basis_ids();
                let planes = bases
                    .iter()
                    .map(|basis| {
                        (
                            *basis,
                            crate::sim::basis_kernel::BasisWeightPlane {
                                values: vec![1.0],
                                mask: None,
                            },
                        )
                    })
                    .collect();
                rule_set.kernels[0].spatial =
                    crate::sim::ruleset::KernelSpatialDefinition::Periodic(
                        crate::sim::basis_kernel::PeriodicKernelDefinition {
                            width: 1,
                            height: 1,
                            anchor_x: 0,
                            anchor_y: 0,
                            planes,
                        },
                    );
            }
            next.rules.defaults.insert(id, rule_set_id);
            next.rules.sets.push(rule_set);
            next.rules
                .bindings
                .extend(next.basis_ids().into_iter().map(|basis| {
                    crate::sim::ruleset::RuleBinding {
                        basis,
                        output: id,
                        rule_set: rule_set_id,
                    }
                }));
        }
        self.selected_channel = id;
        self.replace_draft(next)?;
        self.refresh_rule_selection();
        Ok(())
    }

    pub fn remove_selected_channel(&mut self) -> Result<(), String> {
        if self.draft.channels.len() <= 1 {
            return Err("an experiment must retain at least one channel".into());
        }
        let removed = self.selected_channel;
        let mut next = self.draft.clone();
        if !next.rules.is_empty()
            && next.rules.sets.iter().any(|rule| {
                rule.kernels
                    .iter()
                    .any(|kernel| kernel.source_channel == removed && rule.growth.target != removed)
            })
        {
            return Err(
                "channel is still used as a kernel source; reroute those kernels first".into(),
            );
        }
        next.channels.retain(|channel| channel.id != removed);
        next.kernels
            .retain(|kernel| kernel.source != removed && kernel.target != removed);
        next.growth.retain(|growth| growth.target != removed);
        for growth in &mut next.growth {
            growth.kernel_inputs.retain(|id| {
                next.kernels
                    .iter()
                    .any(|kernel| kernel.id == *id && kernel.target == growth.target)
            });
        }
        if !next.rules.is_empty() {
            next.rules
                .bindings
                .retain(|binding| binding.output != removed);
            next.rules.defaults.remove(&removed);
            let referenced = next
                .rules
                .bindings
                .iter()
                .map(|binding| binding.rule_set)
                .chain(next.rules.defaults.values().copied())
                .collect::<std::collections::BTreeSet<_>>();
            next.rules.sets.retain(|rule| referenced.contains(&rule.id));
        }
        self.selected_channel = next.channels[0].id;
        self.replace_draft(next)
            .map_err(|error| error.to_string())?;
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
        let mut next = self.draft.clone();
        let Some(channel) = next
            .channels
            .iter_mut()
            .find(|channel| channel.id == target)
        else {
            return Ok(());
        };
        channel.frozen = !channel.frozen;
        if channel.frozen {
            next.kernels.retain(|kernel| kernel.target != target);
            next.growth.retain(|growth| growth.target != target);
        } else if !next.growth.iter().any(|growth| growth.target == target) {
            next.growth
                .push(crate::sim::experiment_model::GrowthSource {
                    target,
                    kernel_inputs: Vec::new(),
                    parameters: Default::default(),
                    source: "self".into(),
                    mode: crate::sim::experiment_model::UpdateMode::DirectUpdate,
                });
        }
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
        self.replace_draft(next)
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
            let removed = rule.kernels.remove(position).id;
            rule.growth.kernel_inputs.retain(|id| *id != removed);
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
        let Some(position) = next
            .kernels
            .iter()
            .rposition(|kernel| kernel.target == target)
        else {
            return Err("selected channel has no kernel".into());
        };
        let removed = next.kernels.remove(position).id;
        if let Some(growth) = next
            .growth
            .iter_mut()
            .find(|growth| growth.target == target)
        {
            growth.kernel_inputs.retain(|id| *id != removed);
        }
        self.replace_draft(next).map_err(|error| error.to_string())
    }

    pub fn cycle_tiling_preset(&mut self) -> Result<(), HistoryError> {
        let mut next = self.draft.clone();
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
        let bases = next.basis_ids();
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
    fn adding_kernel_updates_growth_arity_atomically() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        state.add_kernel_for_selected().unwrap();
        assert_eq!(state.draft().kernels.len(), 2);
        assert_eq!(state.draft().growth[0].kernel_inputs.len(), 2);
        crate::sim::experiment_model::validate_structure(state.draft()).unwrap();
        state.undo().unwrap();
        assert_eq!(state.draft().kernels.len(), 1);
    }

    #[test]
    fn channel_and_tiling_actions_are_reversible() {
        let mut state = WorkbenchState::new(ExperimentSpec::single_channel_lenia(8, 8));
        state.add_channel().unwrap();
        assert_eq!(state.draft().channels.len(), 2);
        state.cycle_tiling_preset().unwrap();
        assert_eq!(state.draft().tiling.as_ref().unwrap().prototypes.len(), 1);
        state.cycle_tiling_preset().unwrap();
        assert_eq!(state.draft().tiling.as_ref().unwrap().prototypes.len(), 2);
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
            state.push_tiling_vertex(point);
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
    fn next_basis_changes_the_ruleset_target_not_only_the_prototype_highlight() {
        let mut state = WorkbenchState::new(basis_fixture());
        assert_eq!(state.selected_basis(), BasisId(0));
        state.select_next_prototype();
        assert_eq!(state.selected_basis(), BasisId(1));
        assert_eq!(state.tiling_prototype(), Some(PrototypeId(1)));
    }
}
