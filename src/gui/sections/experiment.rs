//! The Experiment workspace: what this experiment is, and what is wrong with it.

use eframe::egui::{self, RichText, Ui};

use crate::document::DocumentCommand;
use crate::gui::app::{CellariumGui, Section};
use crate::gui::theme;
use crate::sim::experiment_model::GeometrySpec;

/// A summary card. Each one names a part of the experiment and, where it can,
/// offers the way to the workspace that edits it.
struct Card {
    title: &'static str,
    section: Option<Section>,
    lines: Vec<String>,
}

pub fn draw(app: &mut CellariumGui, ui: &mut Ui) {
    toolbar(app, ui);
    ui.separator();
    diagnostics(app, ui);
    egui::ScrollArea::vertical().show(ui, |ui| {
        for card in cards(app) {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(card.title).strong());
                    if let Some(section) = card.section
                        && ui
                            .button("Open")
                            .on_hover_text(format!("Go to the {} workspace", section.label()))
                            .clicked()
                    {
                        app.navigation_mut().select(section);
                    }
                });
                for line in &card.lines {
                    ui.label(line);
                }
            });
        }
    });
}

fn toolbar(app: &mut CellariumGui, ui: &mut Ui) {
    ui.horizontal_wrapped(|ui| {
        let mut dt = app.spec().simulation_dt;
        if ui
            .add(
                egui::DragValue::new(&mut dt)
                    .speed(0.001)
                    .range(0.001..=10.0)
                    .prefix("dt "),
            )
            .on_hover_text("Simulation time step")
            .changed()
        {
            app.dispatch_document(DocumentCommand::SetSimulationDt(dt));
        }
        ui.separator();
        let path = app
            .experiment_path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "not saved yet".to_string());
        ui.label(RichText::new(path).weak());
    });
}

/// Diagnostics are the reason this workspace exists: they say what stops the
/// draft being applied, and each one leads to the place it can be fixed.
fn diagnostics(app: &mut CellariumGui, ui: &mut Ui) {
    let problems = app.draft_problems();
    if problems.is_empty() {
        ui.label(
            RichText::new(format!(
                "{} this draft is ready to run",
                theme::state_glyph(theme::State::Live)
            ))
            .color(theme::state_color(theme::State::Live)),
        );
        return;
    }
    for problem in &problems {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} {problem}",
                    theme::state_glyph(theme::State::Invalid)
                ))
                .color(theme::state_color(theme::State::Invalid)),
            );
            // A diagnostic that cannot be acted on is only a complaint.
            if let Some(section) = section_for(problem)
                && ui
                    .button(format!("Fix in {}", section.label()))
                    .on_hover_text("Go to the workspace that owns this")
                    .clicked()
            {
                app.navigation_mut().select(section);
            }
        });
    }
    ui.separator();
    // Apply & Run lives in the toolbar and is always in reach. Repeating it
    // here would put two controls with one name on the same screen, so this
    // says what the state of the draft means for it instead.
    ui.label(
        RichText::new("Apply & Run is refused until these are fixed")
            .color(theme::state_color(theme::State::Stale)),
    );
}

/// Route a diagnostic to the workspace that owns the thing it names.
pub fn section_for(problem: &str) -> Option<Section> {
    let lowered = problem.to_lowercase();
    // Ordered from the most specific term to the least, so "kernel input" lands
    // on Kernels rather than on Growth.
    for (needle, section) in [
        ("kernel", Section::Kernels),
        ("tiling", Section::Tiling),
        ("basis", Section::Tiling),
        ("prototype", Section::Tiling),
        ("growth", Section::Growth),
        ("rule", Section::Growth),
        ("channel", Section::Channels),
        ("dt", Section::Experiment),
    ] {
        if lowered.contains(needle) {
            return Some(section);
        }
    }
    None
}

fn cards(app: &CellariumGui) -> Vec<Card> {
    let spec = app.spec();
    let GeometrySpec::RasterGrid(grid) = &spec.geometry;
    let active = spec.channels.iter().filter(|entry| !entry.frozen).count();
    let bases = spec.basis_ids().len();

    let mut cards = vec![
        Card {
            title: "World and lattice",
            section: Some(Section::Tiling),
            lines: vec![
                format!("{} x {} cells", grid.width, grid.height),
                format!("{bases} bases in the unit cell"),
                format!("dt {}", spec.simulation_dt),
                format!("seed {}", spec.seed),
            ],
        },
        Card {
            title: "Channel summary",
            section: Some(Section::Channels),
            lines: vec![
                format!(
                    "{} total, {active} updating, {} frozen",
                    spec.channels.len(),
                    spec.channels.len() - active
                ),
                format!(
                    "{} visible",
                    spec.channels
                        .iter()
                        .filter(|entry| entry.display.visible)
                        .count()
                ),
            ],
        },
    ];

    let binding = app.selected_binding();
    let kernels = app.kernel_cards();
    cards.push(Card {
        title: "Selected binding",
        section: Some(Section::Kernels),
        lines: vec![
            format!("basis {} to channel {}", binding.basis.0, binding.output.0),
            format!("{} kernels", kernels.len()),
            format!(
                "{} cells of support in total",
                kernels.iter().map(|card| card.support_cells).sum::<usize>()
            ),
        ],
    });

    let signature = app.growth_signature();
    let referenced = app.growth_referenced();
    cards.push(Card {
        title: "Growth program",
        section: Some(Section::Growth),
        lines: vec![
            signature.rendered(),
            format!(
                "{} of {} inputs are read",
                referenced.len(),
                signature.kernel_inputs.len()
            ),
            match app.growth_diagnostics().is_empty() {
                true => "compiles".to_string(),
                false => format!("{} problems", app.growth_diagnostics().len()),
            },
        ],
    });

    cards.push(Card {
        title: "Compute backends",
        section: None,
        lines: app
            .probes()
            .iter()
            .map(|probe| {
                format!(
                    "{:?} {}{}",
                    probe.kind,
                    probe.device_name.as_deref().unwrap_or("no device named"),
                    if probe.available {
                        String::new()
                    } else {
                        format!(
                            " — unavailable: {}",
                            probe.reason.as_deref().unwrap_or("no reason given")
                        )
                    }
                )
            })
            .collect(),
    });

    cards
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diagnostic_routes_to_the_workspace_that_owns_it() {
        assert_eq!(
            section_for("kernel KernelId(1) is invalid"),
            Some(Section::Kernels)
        );
        assert_eq!(
            section_for("growth would become invalid"),
            Some(Section::Growth)
        );
        assert_eq!(
            section_for("kernel input is missing from the growth signature"),
            Some(Section::Kernels),
            "a sentence naming both lands on the more specific one"
        );
        assert_eq!(
            section_for("active channel has no default rule-set"),
            Some(Section::Growth),
            "a rule-set problem belongs where rule-sets are edited"
        );
        assert_eq!(
            section_for("basis 2 references a missing prototype"),
            Some(Section::Tiling)
        );
        assert_eq!(section_for("something else entirely"), None);
    }
}
