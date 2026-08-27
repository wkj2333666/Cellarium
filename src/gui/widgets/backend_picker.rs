use eframe::egui::{self, RichText, Ui};

use crate::gui::theme;
use crate::sim::backend_selector::BackendPolicy;
use crate::sim::local_backend::{BackendKind, BackendProbe, GpuDeviceType};

/// What the user chose in the picker, if anything changed this frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendChoice {
    Select(BackendPolicy),
}

/// Everything the picker needs to explain the current backend.
pub struct BackendPickerModel<'a> {
    pub policy: &'a BackendPolicy,
    pub probes: &'a [BackendProbe],
    pub active: Option<&'a str>,
    pub notice: Option<&'a str>,
}

/// Show the Auto order, the device actually in use, and every probe's reason.
///
/// A backend that is unavailable is shown with why, rather than being hidden,
/// so the user can tell a missing driver from an unsupported plan.
pub fn backend_picker(ui: &mut Ui, model: BackendPickerModel<'_>) -> Option<BackendChoice> {
    let mut choice = None;

    ui.label(RichText::new("Compute backend").strong());
    if let Some(active) = model.active {
        ui.label(format!("Running on {active}"));
    }
    if let Some(notice) = model.notice {
        ui.label(RichText::new(notice).color(theme::state_color(theme::State::Stale)));
    }
    ui.separator();

    ui.label(RichText::new("Choose").strong());
    for (label, policy) in [
        ("Auto", BackendPolicy::Auto),
        ("CUDA", BackendPolicy::RequireCuda),
        ("GPU (wgpu)", BackendPolicy::RequireWgpu { adapter: None }),
        ("CPU", BackendPolicy::RequireCpu),
    ] {
        let selected = *model.policy == policy;
        let enabled = policy_is_offered(&policy, model.probes);
        let response = ui.add_enabled(
            enabled || selected,
            egui::Button::selectable(selected, label),
        );
        let response = match unavailable_reason(&policy, model.probes) {
            Some(reason) => response.on_disabled_hover_text(reason),
            None => response.on_hover_text(policy_hint(&policy)),
        };
        if response.clicked() && !selected {
            choice = Some(BackendChoice::Select(policy));
        }
    }

    ui.separator();
    ui.label(RichText::new("Auto order").strong());
    ui.label("CUDA, then a discrete GPU, then an integrated GPU, then CPU.");
    ui.separator();

    ui.label(RichText::new("Detected").strong());
    for probe in model.probes {
        let state = if probe.available {
            theme::State::Live
        } else {
            theme::State::Invalid
        };
        let name = probe.device_name.as_deref().unwrap_or("not found");
        ui.label(
            RichText::new(format!(
                "{} {} — {name}{}",
                theme::state_glyph(state),
                probe.kind.label(),
                device_suffix(probe.device_type),
            ))
            .color(theme::state_color(state)),
        );
        if let Some(reason) = &probe.reason {
            ui.label(RichText::new(format!("    {reason}")).weak());
        }
    }

    choice
}

fn device_suffix(device_type: Option<GpuDeviceType>) -> &'static str {
    match device_type {
        Some(GpuDeviceType::Discrete) => " (discrete)",
        Some(GpuDeviceType::Integrated) => " (integrated)",
        Some(GpuDeviceType::Virtual) => " (virtual)",
        Some(GpuDeviceType::Cpu) => "",
        Some(GpuDeviceType::Other) => " (other)",
        None => "",
    }
}

fn policy_hint(policy: &BackendPolicy) -> &'static str {
    match policy {
        BackendPolicy::Auto => "Pick the fastest available backend and fall back if it fails",
        BackendPolicy::RequireCuda => "Always use CUDA; pause instead of falling back",
        BackendPolicy::RequireWgpu { .. } => {
            "Always use a portable GPU; pause instead of falling back"
        }
        BackendPolicy::RequireCpu => "Always use the CPU reference",
    }
}

/// Whether a policy has any backend to run on right now.
pub fn policy_is_offered(policy: &BackendPolicy, probes: &[BackendProbe]) -> bool {
    let available = |kind: BackendKind| {
        probes
            .iter()
            .any(|probe| probe.kind == kind && probe.available)
    };
    match policy {
        BackendPolicy::Auto | BackendPolicy::RequireCpu => true,
        BackendPolicy::RequireCuda => available(BackendKind::Cuda),
        BackendPolicy::RequireWgpu { .. } => available(BackendKind::Wgpu),
    }
}

/// Why a policy cannot be chosen, taken from the probe that rejected it.
pub fn unavailable_reason(policy: &BackendPolicy, probes: &[BackendProbe]) -> Option<String> {
    if policy_is_offered(policy, probes) {
        return None;
    }
    let kind = match policy {
        BackendPolicy::RequireCuda => BackendKind::Cuda,
        BackendPolicy::RequireWgpu { .. } => BackendKind::Wgpu,
        _ => return None,
    };
    probes
        .iter()
        .find(|probe| probe.kind == kind)
        .and_then(|probe| probe.reason.clone())
        .or_else(|| Some(format!("{} is not available here", kind.label())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probes() -> Vec<BackendProbe> {
        vec![
            BackendProbe::unavailable(BackendKind::Cuda, "NVRTC missing"),
            BackendProbe::available(
                BackendKind::Wgpu,
                "Intel Iris Xe",
                GpuDeviceType::Integrated,
            ),
            BackendProbe::available(BackendKind::Cpu, "CPU reference", GpuDeviceType::Cpu),
        ]
    }

    #[test]
    fn a_policy_without_a_backend_is_not_offered() {
        let probes = probes();
        assert!(!policy_is_offered(&BackendPolicy::RequireCuda, &probes));
        assert!(policy_is_offered(
            &BackendPolicy::RequireWgpu { adapter: None },
            &probes
        ));
        assert!(policy_is_offered(&BackendPolicy::Auto, &probes));
        assert!(policy_is_offered(&BackendPolicy::RequireCpu, &probes));
    }

    #[test]
    fn an_unavailable_policy_explains_itself_with_the_probe_reason() {
        let probes = probes();
        assert_eq!(
            unavailable_reason(&BackendPolicy::RequireCuda, &probes).as_deref(),
            Some("NVRTC missing")
        );
        assert_eq!(unavailable_reason(&BackendPolicy::Auto, &probes), None);
    }

    #[test]
    fn a_device_type_is_named_rather_than_implied() {
        assert_eq!(device_suffix(Some(GpuDeviceType::Discrete)), " (discrete)");
        assert_eq!(
            device_suffix(Some(GpuDeviceType::Integrated)),
            " (integrated)"
        );
        assert_eq!(device_suffix(None), "");
    }
}
