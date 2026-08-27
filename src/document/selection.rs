use crate::sim::experiment_model::{ChannelId, ExperimentSpec, KernelId};
use crate::sim::ruleset::RuleSetId;
use crate::sim::tiling::{BasisId, PrototypeId};

/// One input of a Growth program, addressed by stable identity rather than by
/// position, so a plot keeps its axes across kernel edits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlotSymbol {
    SelfValue,
    Kernel(KernelId),
}

/// Which Growth inputs the plot draws against.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlotAxes {
    Curve(PlotSymbol),
    Heatmap(PlotSymbol, PlotSymbol),
}

/// The editor selection carried through undo/redo. Every field is a stable
/// model identity, never an index into a rendered list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditorSelection {
    pub channel: ChannelId,
    pub basis: BasisId,
    pub rule_set: Option<RuleSetId>,
    pub kernel: Option<KernelId>,
    pub prototype: Option<PrototypeId>,
    pub plot_axes: Option<PlotAxes>,
}

impl EditorSelection {
    pub fn initial(spec: &ExperimentSpec) -> Self {
        let channel = spec
            .channels
            .first()
            .map(|entry| entry.id)
            .unwrap_or(ChannelId(0));
        let basis = spec
            .basis_ids()
            .first()
            .copied()
            .unwrap_or(BasisId::default());
        let mut selection = Self {
            channel,
            basis,
            rule_set: None,
            kernel: None,
            prototype: None,
            plot_axes: None,
        };
        selection.normalize(spec);
        selection
    }

    /// Re-anchor the selection on identities that still exist. Called only after
    /// a model transaction succeeds, so a rejected command never moves it.
    pub fn normalize(&mut self, spec: &ExperimentSpec) {
        if !spec.channels.iter().any(|entry| entry.id == self.channel) {
            self.channel = spec
                .channels
                .first()
                .map(|entry| entry.id)
                .unwrap_or(self.channel);
        }

        let bases = spec.basis_ids();
        if !bases.contains(&self.basis)
            && let Some(first) = bases.first()
        {
            self.basis = *first;
        }

        self.rule_set = spec
            .rules
            .binding(self.basis, self.channel)
            .map(|binding| binding.rule_set);

        let available = self.available_kernels(spec);
        if !self.kernel.is_some_and(|id| available.contains(&id)) {
            self.kernel = available.first().copied();
        }

        match &spec.tiling {
            Some(tiling) => {
                if !self
                    .prototype
                    .is_some_and(|id| tiling.prototypes.iter().any(|entry| entry.id == id))
                {
                    self.prototype = tiling.prototypes.first().map(|entry| entry.id);
                }
            }
            None => self.prototype = None,
        }

        if let Some(axes) = self.plot_axes
            && !axes_are_available(axes, &available)
        {
            self.plot_axes = None;
        }
    }

    /// Kernels of the selected binding, or the legacy per-channel kernels when
    /// the experiment has not crossed rule normalization yet.
    pub fn available_kernels(&self, spec: &ExperimentSpec) -> Vec<KernelId> {
        self.rule_set
            .and_then(|rule_set| spec.rules.get(rule_set))
            .map(|rule| rule.kernels.iter().map(|kernel| kernel.id).collect())
            .unwrap_or_else(|| {
                spec.kernels
                    .iter()
                    .filter(|kernel| kernel.target == self.channel)
                    .map(|kernel| kernel.id)
                    .collect()
            })
    }
}

fn axes_are_available(axes: PlotAxes, available: &[KernelId]) -> bool {
    let present = |symbol: PlotSymbol| match symbol {
        PlotSymbol::SelfValue => true,
        PlotSymbol::Kernel(id) => available.contains(&id),
    };
    match axes {
        PlotAxes::Curve(symbol) => present(symbol),
        PlotAxes::Heatmap(x, y) => present(x) && present(y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::channels;

    #[test]
    fn the_initial_selection_anchors_on_the_first_channel_and_basis() {
        let spec = ExperimentSpec::single_channel_lenia(8, 8);
        let selection = EditorSelection::initial(&spec);
        assert_eq!(selection.channel, ChannelId(0));
        assert_eq!(selection.prototype, None);
        assert!(selection.kernel.is_some());
    }

    #[test]
    fn normalizing_drops_a_channel_that_no_longer_exists() {
        let spec = ExperimentSpec::single_channel_lenia(8, 8);
        let addition = channels::add_channel(&spec).unwrap();
        let with_two = addition.spec;
        let mut selection = EditorSelection::initial(&with_two);
        selection.channel = addition.channel;

        let (without, _) = channels::remove_channel(&with_two, addition.channel).unwrap();
        selection.normalize(&without);
        assert_eq!(selection.channel, ChannelId(0));
    }

    #[test]
    fn normalizing_clears_plot_axes_that_reference_a_removed_kernel() {
        let spec = ExperimentSpec::single_channel_lenia(8, 8);
        let mut selection = EditorSelection::initial(&spec);
        selection.plot_axes = Some(PlotAxes::Curve(PlotSymbol::Kernel(KernelId(97))));
        selection.normalize(&spec);
        assert_eq!(selection.plot_axes, None);

        let existing = selection.kernel.unwrap();
        selection.plot_axes = Some(PlotAxes::Curve(PlotSymbol::Kernel(existing)));
        selection.normalize(&spec);
        assert!(selection.plot_axes.is_some());
    }

    #[test]
    fn a_self_only_curve_survives_normalization() {
        let spec = ExperimentSpec::single_channel_lenia(8, 8);
        let mut selection = EditorSelection::initial(&spec);
        selection.plot_axes = Some(PlotAxes::Curve(PlotSymbol::SelfValue));
        selection.normalize(&spec);
        assert_eq!(
            selection.plot_axes,
            Some(PlotAxes::Curve(PlotSymbol::SelfValue))
        );
    }
}
