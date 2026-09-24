//! Visual theme modelled on Figma's desktop UI ("UI3"): Inter, compact 24px controls,
//! 5px corners, blue #0D99FF accent, grey panels. Light and dark follow the OS.
//!
//! All colours, sizes and fonts live here. Screens use the helpers below
//! (`primary_button`, `bar_frame`, `semibold`) instead of hard-coding styling.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke,
    TextStyle, Theme, Vec2,
};

pub const ACCENT: Color32 = Color32::from_rgb(0x0D, 0x99, 0xFF);
const RADIUS: u8 = 5;
const SEMIBOLD: &str = "Inter-SemiBold";

struct Palette {
    /// Panels, toolbars.
    bg: Color32,
    /// Alternate table rows.
    stripe: Color32,
    /// Text inputs and secondary buttons.
    input: Color32,
    hover: Color32,
    pressed: Color32,
    border: Color32,
    text: Color32,
    text_tertiary: Color32,
    danger: Color32,
}

const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x2C, 0x2C, 0x2C),
    stripe: Color32::from_rgb(0x33, 0x33, 0x33),
    input: Color32::from_rgb(0x38, 0x38, 0x38),
    hover: Color32::from_rgb(0x44, 0x44, 0x44),
    pressed: Color32::from_rgb(0x4D, 0x4D, 0x4D),
    border: Color32::from_rgb(0x44, 0x44, 0x44),
    text: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    text_tertiary: Color32::from_rgb(0x8C, 0x8C, 0x8C),
    danger: Color32::from_rgb(0xFF, 0x72, 0x62),
};

const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    stripe: Color32::from_rgb(0xF9, 0xF9, 0xF9),
    input: Color32::from_rgb(0xF5, 0xF5, 0xF5),
    hover: Color32::from_rgb(0xE6, 0xE6, 0xE6),
    pressed: Color32::from_rgb(0xD9, 0xD9, 0xD9),
    border: Color32::from_rgb(0xE6, 0xE6, 0xE6),
    text: Color32::from_rgb(0x1E, 0x1E, 0x1E),
    text_tertiary: Color32::from_rgb(0x8F, 0x8F, 0x8F),
    danger: Color32::from_rgb(0xF2, 0x48, 0x22),
};

/// Installs fonts and both light and dark styles. Takes effect from the next frame.
pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.style_mut_of(Theme::Dark, |s| style(s, &DARK, true));
    ctx.style_mut_of(Theme::Light, |s| style(s, &LIGHT, false));
}

/// Semibold Inter, for headings and table headers.
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

/// Filled blue button for the main action on a screen.
pub fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text).color(Color32::WHITE)).fill(ACCENT)
}

/// Frame for toolbars and status bars.
pub fn bar_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::side_top_panel(style).inner_margin(Margin::symmetric(12, 8))
}

fn fonts() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    defs.font_data.insert(
        "Inter".into(),
        Arc::new(FontData::from_static(include_bytes!("../../assets/fonts/Inter-Regular.ttf"))),
    );
    defs.font_data.insert(
        SEMIBOLD.into(),
        Arc::new(FontData::from_static(include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"))),
    );
    // Keep egui's default fonts as fallbacks for symbols Inter lacks (▶, emoji).
    let proportional = defs.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, "Inter".into());
    let mut semibold = vec![SEMIBOLD.to_string()];
    semibold.extend(proportional.iter().cloned());
    defs.families.insert(FontFamily::Name(SEMIBOLD.into()), semibold);
    defs
}

fn style(s: &mut egui::Style, p: &Palette, dark: bool) {
    use TextStyle::{Body, Button, Heading, Monospace, Small};
    s.text_styles = [
        (Small, FontId::proportional(10.0)),
        (Body, FontId::proportional(12.0)),
        (Button, FontId::proportional(12.0)),
        (Monospace, FontId::monospace(12.0)),
        (Heading, semibold(16.0)),
    ]
    .into();

    let sp = &mut s.spacing;
    sp.item_spacing = Vec2::new(8.0, 6.0);
    sp.button_padding = Vec2::new(10.0, 4.0);
    sp.interact_size.y = 24.0;
    sp.window_margin = Margin::same(12);
    sp.menu_margin = Margin::same(6);

    let v = &mut s.visuals;
    *v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = p.bg;
    v.window_fill = p.bg;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(6);
    v.window_shadow = Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(60) };
    v.popup_shadow = Shadow { offset: [0, 2], blur: 8, spread: 0, color: Color32::from_black_alpha(40) };
    v.faint_bg_color = p.stripe;
    v.extreme_bg_color = p.input;
    v.text_edit_bg_color = Some(p.input);
    v.code_bg_color = p.input;
    v.hyperlink_color = ACCENT;
    v.error_fg_color = p.danger;
    v.weak_text_color = Some(p.text_tertiary);
    v.selection.bg_fill = ACCENT.gamma_multiply(0.4);
    v.selection.stroke = Stroke::new(1.0, ACCENT);

    let radius = CornerRadius::same(RADIUS);
    let w = &mut v.widgets;
    // Labels and separators.
    w.noninteractive.bg_fill = p.bg;
    w.noninteractive.weak_bg_fill = p.bg;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    // Buttons, combo boxes, checkboxes.
    for (state, fill, text) in [
        (&mut w.inactive, p.input, p.text),
        (&mut w.hovered, p.hover, p.text),
        (&mut w.active, p.pressed, p.text),
        (&mut w.open, p.hover, p.text),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::NONE;
        state.fg_stroke = Stroke::new(1.0, text);
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
    w.noninteractive.corner_radius = radius;
}
