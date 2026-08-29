//! How the workbench looks.
//!
//! [`theme`](super::theme) owns the colours of the *domain* — channels,
//! kernel weights, the tiling. This module owns the colours of the
//! *instrument*: panels, buttons, text, spacing. Until it existed the
//! workbench inherited egui's stock dark theme unchanged, which is why it read
//! as a debugger rather than a tool.
//!
//! Two rules hold the palette together:
//!
//! * The chrome is neutral. Every saturated colour on screen should belong to
//!   the experiment, not to the frame around it. A tool that competes with its
//!   own data for attention makes the data harder to read.
//! * Weight follows meaning. `Apply & Run` is the primary act of the
//!   application and `Save as` is not, so they must not be the same rectangle.

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, RichText, Stroke, TextStyle, Ui,
};

/// Behind every panel.
pub const BACKDROP: Color32 = Color32::from_rgb(16, 18, 23);
/// Panel interiors: navigation, inspector, toolbars, status.
pub const PANEL: Color32 = Color32::from_rgb(24, 27, 34);
/// A control at rest.
pub const SURFACE: Color32 = Color32::from_rgb(35, 39, 49);
/// A control under the pointer.
pub const SURFACE_HOVER: Color32 = Color32::from_rgb(48, 54, 67);
/// A control being pressed, or one that is switched on.
pub const SURFACE_ACTIVE: Color32 = Color32::from_rgb(62, 70, 86);
/// Hairlines and control outlines.
pub const OUTLINE: Color32 = Color32::from_rgb(52, 58, 71);
/// Outlines that need to be seen rather than felt.
pub const OUTLINE_STRONG: Color32 = Color32::from_rgb(86, 95, 113);
/// Body text.
pub const TEXT: Color32 = Color32::from_rgb(224, 229, 238);
/// Text that supports other text: units, hints, readouts.
pub const TEXT_DIM: Color32 = Color32::from_rgb(146, 155, 171);
/// Selection, focus, and the one primary action per surface.
pub const ACCENT: Color32 = Color32::from_rgb(118, 172, 246);
/// Legible on [`ACCENT`].
pub const ON_ACCENT: Color32 = Color32::from_rgb(10, 14, 22);
/// Actions that throw work away.
pub const DANGER: Color32 = Color32::from_rgb(226, 96, 92);

const RADIUS: u8 = 5;

/// Establish the workbench's visuals, type scale and spacing on a context.
///
/// Called once when the window is created. Everything here is a deliberate
/// departure from the egui default; anything left alone is left alone because
/// the default was already right.
pub fn install(ctx: &egui::Context) {
    // The palette is a dark one. Following the desktop's light preference
    // would leave the canvases — which are black by construction — sitting in
    // a white frame.
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.all_styles_mut(apply);
}

/// The whole of the workbench's departure from stock egui, in one place so it
/// can be applied to a context or inspected by a test.
pub fn apply(style: &mut egui::Style) {
    // A type scale, so a heading is a heading. Before this every string in the
    // application was one size and one weight, which left "Workspace" looking
    // exactly like the items underneath it.
    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::new(17.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(14.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Small,
            FontId::new(12.0, FontFamily::Proportional),
        ),
        // Readouts use this. A proportional digit changes width as the number
        // changes, so a live value visibly jitters in place.
        (
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        ),
    ]
    .into();

    let spacing = &mut style.spacing;
    spacing.item_spacing = egui::vec2(8.0, 6.0);
    spacing.button_padding = egui::vec2(10.0, 4.0);
    spacing.menu_margin = egui::Margin::same(6);
    spacing.indent = 18.0;
    // Stock egui aims for a 20px control. That is a small target for a
    // pointer and leaves labels crowded against their own borders.
    spacing.interact_size = egui::vec2(28.0, 26.0);
    spacing.slider_width = 120.0;
    spacing.combo_width = 120.0;

    let visuals = &mut style.visuals;
    visuals.dark_mode = true;
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = BACKDROP;
    visuals.faint_bg_color = Color32::from_rgb(30, 34, 42);
    visuals.window_stroke = Stroke::new(1.0, OUTLINE);
    visuals.window_corner_radius = CornerRadius::same(8);
    visuals.override_text_color = None;
    visuals.hyperlink_color = ACCENT;
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.45);
    visuals.selection.stroke = Stroke::new(1.0, TEXT);

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = PANEL;
    widgets.noninteractive.weak_bg_fill = PANEL;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, OUTLINE);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.noninteractive.corner_radius = CornerRadius::same(RADIUS);

    widgets.inactive.bg_fill = SURFACE;
    widgets.inactive.weak_bg_fill = SURFACE;
    widgets.inactive.bg_stroke = Stroke::new(1.0, OUTLINE);
    widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.inactive.corner_radius = CornerRadius::same(RADIUS);

    widgets.hovered.bg_fill = SURFACE_HOVER;
    widgets.hovered.weak_bg_fill = SURFACE_HOVER;
    widgets.hovered.bg_stroke = Stroke::new(1.0, OUTLINE_STRONG);
    widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    widgets.hovered.corner_radius = CornerRadius::same(RADIUS);

    widgets.active.bg_fill = SURFACE_ACTIVE;
    widgets.active.weak_bg_fill = SURFACE_ACTIVE;
    widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    widgets.active.corner_radius = CornerRadius::same(RADIUS);

    // A disabled control has to read as *present but unavailable*. Stock egui
    // dims it so far that it reads as absent, which is how a user concludes a
    // feature does not exist.
    widgets.open.bg_fill = SURFACE_ACTIVE;
    widgets.open.weak_bg_fill = SURFACE_ACTIVE;
    widgets.open.bg_stroke = Stroke::new(1.0, OUTLINE_STRONG);
    widgets.open.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.open.corner_radius = CornerRadius::same(RADIUS);
}

/// The one action on a surface that the user most likely came to press.
///
/// There should never be two of these visible at once; if there are, neither
/// is primary.
pub fn primary(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_owned()).color(ON_ACCENT).strong())
        .fill(ACCENT)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(RADIUS))
}

/// An action that discards work. Outlined rather than filled: it must be
/// findable without inviting a press.
pub fn danger(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_owned()).color(DANGER))
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, DANGER.gamma_multiply(0.55)))
        .corner_radius(CornerRadius::same(RADIUS))
}

/// Everything else.
pub fn secondary(text: &str) -> egui::Button<'static> {
    egui::Button::new(text.to_owned()).corner_radius(CornerRadius::same(RADIUS))
}

/// A heading over a group of controls or facts.
pub fn section_header(ui: &mut Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(RichText::new(text.to_owned()).heading().color(TEXT));
    ui.add_space(2.0);
}

/// The caption naming a cluster of controls inside a toolbar.
///
/// Deliberately quieter than the controls it introduces: a group caption drawn
/// at the same weight as its members reads as one of them, which is how
/// "Brush | Pencil Pen Brush" came to contain the word twice at equal weight.
pub fn group_caption(ui: &mut Ui, text: &str) -> egui::Response {
    ui.label(RichText::new(text.to_owned()).small().color(TEXT_DIM))
}

/// A number the user reads rather than presses. Monospaced so it stops
/// shifting sideways as its digits change.
pub fn readout(value: impl Into<String>) -> RichText {
    RichText::new(value.into()).monospace().color(TEXT)
}

/// A readout that is context rather than content.
pub fn dim_readout(value: impl Into<String>) -> RichText {
    RichText::new(value.into()).monospace().color(TEXT_DIM)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Contrast is not decoration. Text has to survive the surface it sits on,
    /// and the two colours that carry meaning have to be told apart.
    fn luminance(color: Color32) -> f32 {
        let channel = |value: u8| {
            let value = f32::from(value) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
    }

    fn contrast(foreground: Color32, background: Color32) -> f32 {
        let (high, low) = {
            let (a, b) = (luminance(foreground), luminance(background));
            if a >= b { (a, b) } else { (b, a) }
        };
        (high + 0.05) / (low + 0.05)
    }

    #[test]
    fn body_text_is_legible_on_every_surface_it_is_drawn_on() {
        for surface in [BACKDROP, PANEL, SURFACE, SURFACE_HOVER, SURFACE_ACTIVE] {
            let ratio = contrast(TEXT, surface);
            assert!(
                ratio >= 7.0,
                "body text on {surface:?} has contrast {ratio:.1}, below the 7:1 this palette \
                 claims"
            );
        }
    }

    #[test]
    fn supporting_text_stays_readable_rather_than_merely_dim() {
        for surface in [PANEL, SURFACE] {
            let ratio = contrast(TEXT_DIM, surface);
            assert!(
                ratio >= 4.5,
                "dim text on {surface:?} has contrast {ratio:.1}, below the 4.5:1 floor for text"
            );
        }
    }

    #[test]
    fn the_primary_action_is_legible_against_its_own_fill() {
        let ratio = contrast(ON_ACCENT, ACCENT);
        assert!(ratio >= 7.0, "primary button text contrast is {ratio:.1}");
    }

    /// The point of the surface ramp is that a hovered control looks different
    /// from a resting one. Steps too small to see are steps that do not exist.
    #[test]
    fn each_surface_step_is_visibly_lighter_than_the_last() {
        let ramp = [BACKDROP, PANEL, SURFACE, SURFACE_HOVER, SURFACE_ACTIVE];
        for pair in ramp.windows(2) {
            let step = luminance(pair[1]) - luminance(pair[0]);
            assert!(
                step > 0.004,
                "{:?} to {:?} is a step of {step:.4}, too small to see",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn the_accent_and_the_danger_colour_cannot_be_confused() {
        let difference = (i32::from(ACCENT.r()) - i32::from(DANGER.r())).abs()
            + (i32::from(ACCENT.g()) - i32::from(DANGER.g())).abs()
            + (i32::from(ACCENT.b()) - i32::from(DANGER.b())).abs();
        assert!(
            difference > 200,
            "the primary and destructive colours differ by only {difference}"
        );
    }

    /// Installing the theme must actually change the context, or every claim
    /// in this module is about a struct nobody reads.
    #[test]
    fn installing_the_theme_replaces_the_stock_style() {
        let ctx = egui::Context::default();
        let before = ctx.style_of(egui::Theme::Dark).text_styles[&TextStyle::Heading].size;
        install(&ctx);
        let after = ctx.style_of(egui::Theme::Dark);
        assert_ne!(
            before,
            after.text_styles[&TextStyle::Heading].size,
            "the heading size must not be left at egui's default"
        );
        assert_eq!(after.visuals.panel_fill, PANEL);
        assert!(
            after.text_styles[&TextStyle::Heading].size > after.text_styles[&TextStyle::Body].size,
            "a heading has to be larger than body text"
        );
        assert!(
            after.text_styles[&TextStyle::Body].size > after.text_styles[&TextStyle::Small].size,
            "small text has to be smaller than body text"
        );
    }

    #[test]
    fn readouts_are_monospaced_so_changing_digits_do_not_shift() {
        let text = readout("1.000");
        assert!(
            format!("{text:?}").contains("Monospace"),
            "a readout must use the monospace family"
        );
    }
}
