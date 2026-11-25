//! Softcaller's colours and their mapping onto egui's style.
//!
//! The palette uses fastframe-theme's sixteen base colour names, so palette
//! files and fastframe's shared palettes can be offered later (Phase 8),
//! plus `success` for answer/call buttons.

use std::collections::BTreeSet;

use egui::{Color32, CornerRadius, FontFamily, FontId, Shadow, Stroke, TextStyle, Visuals, vec2};
use fastframe_fonts::Weight;
use fastframe_theme::Base;
use softcaller_core::settings::ThemePreference;

/// Corner radius for buttons, fields and list rows.
pub const RADIUS: u8 = 10;
/// Corner radius for cards and windows.
pub const CARD_RADIUS: u8 = 14;

/// The three source colours of the Green Enchanted Forest identity.
/// Interactive widgets use the derived palette; these are for artwork and
/// brand accents that need the exact source values.
pub const IVORY: Color32 = Color32::from_rgb(0xf1, 0xf7, 0xed);
pub const DARK_SLATE: Color32 = Color32::from_rgb(0x24, 0x3e, 0x36);
pub const MUTED_TEAL: Color32 = Color32::from_rgb(0x7c, 0xa9, 0x82);

#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub window: Color32,
    pub panel: Color32,
    pub surface: Color32,
    pub surface_hover: Color32,
    pub surface_active: Color32,
    pub outline: Color32,
    pub text: Color32,
    pub secondary: Color32,
    pub dim: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub on_accent: Color32,
    pub danger: Color32,
    pub warning: Color32,
    pub success: Color32,
    pub overlay: Color32,
    pub shadow: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const fn rgba(hex: u32, alpha: u8) -> Color32 {
    Color32::from_rgba_premultiplied(
        ((hex >> 16 & 0xff) * alpha as u32 / 255) as u8,
        ((hex >> 8 & 0xff) * alpha as u32 / 255) as u8,
        ((hex & 0xff) * alpha as u32 / 255) as u8,
        alpha,
    )
}

impl Palette {
    /// Dark: the Green Enchanted Forest at dusk — deep pine, soft ivory text,
    /// muted teal for the one action that matters.
    pub const fn dark() -> Self {
        Self {
            dark: true,
            window: DARK_SLATE,
            panel: rgb(0x1e342d),
            surface: rgb(0x2c4a41),
            surface_hover: rgb(0x35574c),
            surface_active: rgb(0x3f6457),
            outline: rgb(0x45685b),
            text: IVORY,
            secondary: rgb(0xc9d8cb),
            dim: rgb(0x9fb3a5),
            accent: MUTED_TEAL,
            accent_hover: rgb(0x95bd99),
            on_accent: rgb(0x16261f),
            danger: rgb(0xe88a7d),
            warning: rgb(0xe5c07b),
            success: rgb(0x8fd19e),
            overlay: rgba(0x000000, 0x99),
            shadow: rgba(0x000000, 0x73),
        }
    }

    /// Light: ivory paper, dark slate text, and a darker teal for actions so
    /// small text keeps WCAG AA contrast.
    pub const fn light() -> Self {
        Self {
            dark: false,
            window: IVORY,
            panel: rgb(0xe7f0e2),
            surface: rgb(0xdce8d7),
            surface_hover: rgb(0xd0e0cb),
            surface_active: rgb(0xc3d7bd),
            outline: rgb(0xb8cbb2),
            text: DARK_SLATE,
            secondary: rgb(0x3d5a4e),
            dim: rgb(0x5f7a6d),
            accent: rgb(0x4a7856),
            accent_hover: rgb(0x406b4b),
            on_accent: rgb(0xf1f7ed),
            danger: rgb(0xa83f32),
            warning: rgb(0x8a5a00),
            success: rgb(0x2f6b45),
            overlay: rgba(0x000000, 0x59),
            shadow: rgba(0x000000, 0x2e),
        }
    }

    pub fn for_base(base: Base) -> Self {
        match base {
            Base::Dark => Self::dark(),
            Base::Light => Self::light(),
        }
    }
}

impl fastframe_theme::Palette for Palette {
    fn base(base: Base) -> Self {
        Self::for_base(base)
    }

    fn set(&mut self, name: &str, color: Color32) -> bool {
        let slot = match name {
            "window" => &mut self.window,
            "panel" => &mut self.panel,
            "surface" => &mut self.surface,
            "surface_hover" => &mut self.surface_hover,
            "surface_active" => &mut self.surface_active,
            "outline" => &mut self.outline,
            "text" => &mut self.text,
            "secondary" => &mut self.secondary,
            "dim" => &mut self.dim,
            "accent" => &mut self.accent,
            "accent_hover" => &mut self.accent_hover,
            "on_accent" => &mut self.on_accent,
            "danger" => &mut self.danger,
            "warning" => &mut self.warning,
            "success" => &mut self.success,
            "overlay" => &mut self.overlay,
            "shadow" => &mut self.shadow,
            _ => return false,
        };
        *slot = color;
        true
    }

    fn derive(&mut self, given: &BTreeSet<&str>) {
        // Files written for other fastframe apps don't know `success`.
        if !given.contains("success") && given.contains("accent") {
            self.success = if self.dark { rgb(0x8fd19e) } else { rgb(0x2f6b45) };
        }
    }
}

impl fastframe_theme::PaletteBase for Palette {
    fn palette_base(&self) -> Base {
        Base::from_dark(self.dark)
    }
}

/// Which base the preference asks for, given the system's appearance.
pub fn wanted_base(pref: ThemePreference, system: Option<egui::Theme>) -> Base {
    match pref {
        ThemePreference::Light => Base::Light,
        ThemePreference::Dark => Base::Dark,
        ThemePreference::System => match system {
            Some(egui::Theme::Light) => Base::Light,
            _ => Base::Dark,
        },
    }
}

/// Applies `p` to every widget: visuals, spacing and text styles.
pub fn apply(ctx: &egui::Context, p: &Palette, rendering: &fastframe_text::TextRendering) {
    let theme = if p.dark { egui::Theme::Dark } else { egui::Theme::Light };
    let mut visuals = visuals(p);
    rendering.apply_to_visuals(&mut visuals);
    ctx.set_theme(theme);
    // Both of egui's theme slots get Softcaller's visuals: eframe may switch the
    // context's theme on its own (e.g. when the system frame reports a dark
    // window), and must never fall back to egui's default style.
    ctx.all_styles_mut(|style| {
        style.visuals = visuals.clone();
        style.spacing.item_spacing = vec2(8.0, 8.0);
        style.spacing.button_padding = vec2(12.0, 6.0);
        style.spacing.interact_size.y = 30.0;
        style.spacing.window_margin = egui::Margin::same(16);
        style.text_styles = [
            (TextStyle::Small, Weight::Regular.font_id(12.0)),
            (TextStyle::Body, Weight::Regular.font_id(14.0)),
            (TextStyle::Button, Weight::Medium.font_id(14.0)),
            (TextStyle::Heading, Weight::SemiBold.font_id(20.0)),
            (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
        ]
        .into();
    });
}

/// Re-applies the palette to a UI's own style, so standard widgets (combo
/// boxes, buttons, text edits, popups) stay on the app's colours even if the
/// context's style was replaced by a system-theme change or a new viewport.
pub fn apply_to_ui(ui: &mut egui::Ui, p: &Palette) {
    let mut themed = visuals(p);
    let text_options = ui.visuals().text_options;
    themed.text_options = text_options;
    *ui.visuals_mut() = themed;
}

fn visuals(p: &Palette) -> Visuals {
    let mut v = if p.dark { Visuals::dark() } else { Visuals::light() };
    let radius = CornerRadius::same(RADIUS);

    v.override_text_color = Some(p.text);
    v.hyperlink_color = p.accent;
    v.window_fill = p.window;
    v.panel_fill = p.window;
    v.faint_bg_color = p.panel;
    v.extreme_bg_color = p.surface;
    v.code_bg_color = p.surface;
    v.warn_fg_color = p.warning;
    v.error_fg_color = p.danger;
    v.window_stroke = Stroke::new(1.0, p.outline);
    v.window_corner_radius = CornerRadius::same(CARD_RADIUS);
    v.menu_corner_radius = radius;
    v.window_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: p.shadow };
    v.popup_shadow = Shadow { offset: [0, 4], blur: 12, spread: 0, color: p.shadow };
    v.selection.bg_fill = p.accent.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.text_cursor.stroke = Stroke::new(2.0, p.accent);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.window;
    w.noninteractive.weak_bg_fill = p.panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.outline);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.secondary);
    for (state, bg, stroke) in [
        (&mut w.inactive, p.surface, Stroke::NONE),
        (&mut w.hovered, p.surface_hover, Stroke::new(1.0, p.outline)),
        (&mut w.active, p.surface_active, Stroke::new(1.0, p.accent)),
        (&mut w.open, p.surface_active, Stroke::NONE),
    ] {
        state.bg_fill = bg;
        state.weak_bg_fill = bg;
        state.bg_stroke = stroke;
        state.fg_stroke = Stroke::new(1.5, p.text);
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
    w.noninteractive.corner_radius = radius;
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Color32) -> f64 {
        let lin = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    /// WCAG AA: 4.5:1 for body text, 3:1 for large text and UI glyphs.
    #[test]
    fn both_themes_are_readable() {
        for p in [Palette::dark(), Palette::light()] {
            for bg in [p.window, p.panel, p.surface] {
                assert!(contrast(p.text, bg) >= 7.0, "text on {bg:?}: {}", contrast(p.text, bg));
                assert!(contrast(p.secondary, bg) >= 4.5, "secondary on {bg:?}");
                assert!(contrast(p.dim, bg) >= 3.0, "dim on {bg:?}: {}", contrast(p.dim, bg));
            }
            assert!(contrast(p.on_accent, p.accent) >= 4.5, "on_accent");
            for c in [p.accent, p.success, p.danger] {
                assert!(contrast(c, p.window) >= 3.0, "{c:?} on window");
            }
        }
    }

    #[test]
    fn standard_widgets_use_the_palette() {
        let ctx = egui::Context::default();
        for p in [Palette::dark(), Palette::light()] {
            apply(&ctx, &p, &fastframe_text::TextRendering::default());
            let theme = if p.dark { egui::Theme::Dark } else { egui::Theme::Light };
            let v = &ctx.style_of(theme).visuals;
            assert_eq!(v.panel_fill, p.window);
            assert_eq!(v.widgets.inactive.weak_bg_fill, p.surface, "buttons and combo boxes");
            assert_eq!(v.widgets.hovered.weak_bg_fill, p.surface_hover);
            assert_eq!(v.extreme_bg_color, p.surface, "text edits");
        }
    }

    #[test]
    fn the_source_palette_is_in_use() {
        assert_eq!(DARK_SLATE, Palette::dark().window);
        assert_eq!(IVORY, Palette::dark().text);
        assert_eq!(MUTED_TEAL, Palette::dark().accent);
        assert_eq!(IVORY, Palette::light().window);
        assert_eq!(DARK_SLATE, Palette::light().text);
        // The light interactive accent is darker than the brand teal for AA.
        assert_eq!(Palette::light().accent, rgb(0x4a7856));
    }

    #[test]
    fn system_preference_follows_the_os() {
        assert_eq!(wanted_base(ThemePreference::System, Some(egui::Theme::Light)), Base::Light);
        assert_eq!(wanted_base(ThemePreference::System, Some(egui::Theme::Dark)), Base::Dark);
        assert_eq!(wanted_base(ThemePreference::System, None), Base::Dark);
        assert_eq!(wanted_base(ThemePreference::Light, Some(egui::Theme::Dark)), Base::Light);
    }
}
