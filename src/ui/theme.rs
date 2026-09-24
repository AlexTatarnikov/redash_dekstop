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

use crate::export::ValueKind;
use crate::sql::Token;

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
    /// Results table header row.
    header: Color32,
    text: Color32,
    text_tertiary: Color32,
    danger: Color32,
    /// Selected toggles, the focused input's outline and selected text: a bluish fill
    /// with `selected_text`, readable on it and on `bg`.
    selected_fill: Color32,
    selected_text: Color32,
}

const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x2C, 0x2C, 0x2C),
    stripe: Color32::from_rgb(0x36, 0x36, 0x36),
    input: Color32::from_rgb(0x3A, 0x3A, 0x3A),
    hover: Color32::from_rgb(0x4A, 0x4A, 0x4A),
    pressed: Color32::from_rgb(0x55, 0x55, 0x55),
    border: Color32::from_rgb(0x52, 0x52, 0x52),
    header: Color32::from_rgb(0x40, 0x40, 0x40),
    text: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    text_tertiary: Color32::from_rgb(0xA8, 0xA8, 0xA8),
    danger: Color32::from_rgb(0xFF, 0x7B, 0x6B),
    selected_fill: Color32::from_rgb(0x1B, 0x4B, 0x75),
    selected_text: Color32::from_rgb(0x9C, 0xD4, 0xFF),
};

const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    stripe: Color32::from_rgb(0xF3, 0xF4, 0xF6),
    input: Color32::from_rgb(0xF3, 0xF3, 0xF3),
    hover: Color32::from_rgb(0xE3, 0xE3, 0xE3),
    pressed: Color32::from_rgb(0xD4, 0xD4, 0xD4),
    border: Color32::from_rgb(0xD2, 0xD2, 0xD2),
    header: Color32::from_rgb(0xE6, 0xE8, 0xEB),
    text: Color32::from_rgb(0x1E, 0x1E, 0x1E),
    text_tertiary: Color32::from_rgb(0x6B, 0x6B, 0x6B),
    danger: Color32::from_rgb(0xD1, 0x24, 0x2F),
    selected_fill: Color32::from_rgb(0xD6, 0xEC, 0xFF),
    selected_text: Color32::from_rgb(0x00, 0x5A, 0xB0),
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
    egui::Button::new(egui::RichText::new(text).color(Color32::WHITE))
        .fill(ACCENT)
        .stroke(Stroke::new(1.0, ACCENT))
}

fn palette(dark: bool) -> &'static Palette {
    if dark { &DARK } else { &LIGHT }
}

/// Background of the results table's header row, set apart from the striped rows.
pub fn table_header_fill(dark: bool) -> Color32 {
    palette(dark).header
}

/// Colour of a results cell by the kind of value it holds, so numbers, dates and
/// booleans stand apart from text at a glance. NULLs use the weak text colour.
pub fn value_color(kind: ValueKind, dark: bool) -> Color32 {
    match kind {
        ValueKind::Text => palette(dark).text,
        ValueKind::Null => palette(dark).text_tertiary,
        ValueKind::Number => syntax_color(Token::Constant, dark),
        ValueKind::Boolean => syntax_color(Token::Keyword, dark),
        ValueKind::Date => syntax_color(Token::Function, dark),
        ValueKind::Json => syntax_color(Token::Parameter, dark),
    }
}

/// SQL syntax colour, from GitHub's code view palette (Primer "prettylights").
pub fn syntax_color(token: Token, dark: bool) -> Color32 {
    let rgb = |hex: u32| Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
    let (light, dark_hex) = match token {
        Token::Plain => return palette(dark).text,
        Token::Keyword => (0xCF222E, 0xFF7B72),
        Token::Function => (0x8250DF, 0xD2A8FF),
        Token::Constant => (0x0550AE, 0x79C0FF),
        Token::String => (0x0A3069, 0xA5D6FF),
        Token::Comment => (0x6E7781, 0x8B949E),
        Token::Parameter => (0x953800, 0xFFA657),
    };
    rgb(if dark { dark_hex } else { light })
}

/// Square toggle with a painted "sidebar" icon (its left pane filled while `open`),
/// for showing and hiding a side panel. `label` names it for tooltips, screen
/// readers and tests.
pub fn sidebar_toggle(ui: &mut egui::Ui, open: bool, label: &str) -> egui::Response {
    let size = Vec2::splat(ui.spacing().interact_size.y);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, open, label));
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, open);
        let painter = ui.painter();
        painter.rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
        let icon = egui::Rect::from_center_size(rect.center(), Vec2::new(14.0, 12.0));
        let stroke = Stroke::new(1.2, visuals.fg_stroke.color);
        let pane = egui::Rect::from_min_max(icon.min, egui::pos2(icon.min.x + 5.0, icon.max.y));
        if open {
            painter.rect_filled(pane, CornerRadius { nw: 2, sw: 2, ne: 0, se: 0 }, visuals.fg_stroke.color);
        }
        painter.rect_stroke(icon, 2.0, stroke, egui::StrokeKind::Inside);
        painter.vline(pane.max.x, icon.y_range(), stroke);
    }
    response
}

/// Background and text colour of a search match in the results table: yellow, and
/// orange for the selected one, like Chrome's find in page. The same in both themes.
pub fn search_match(selected: bool) -> (Color32, Color32) {
    let background =
        if selected { Color32::from_rgb(0xFF, 0x96, 0x32) } else { Color32::from_rgb(0xF9, 0xE2, 0x7D) };
    (background, LIGHT.text)
}

/// Space between the SQL editor's box and its text.
pub const EDITOR_PADDING: Margin = Margin::symmetric(8, 6);

/// Horizontal space between a results table cell's edge and its text.
pub const CELL_PADDING: f32 = 8.0;

/// Frame for toolbars and status bars.
pub fn bar_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::side_top_panel(style).inner_margin(Margin::symmetric(12, 8))
}

/// Frame for popups anchored in the editor, like autocompletion.
pub fn popup_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::popup(style).inner_margin(Margin::same(4))
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
    v.selection.bg_fill = p.selected_fill;
    v.selection.stroke = Stroke::new(1.0, p.selected_text);

    let radius = CornerRadius::same(RADIUS);
    let w = &mut v.widgets;
    // Labels and separators.
    w.noninteractive.bg_fill = p.bg;
    w.noninteractive.weak_bg_fill = p.bg;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    // Buttons, combo boxes, checkboxes and text inputs: outlined so they stand out
    // from the panel, especially on white.
    for (state, fill, text, border) in [
        (&mut w.inactive, p.input, p.text, p.border),
        (&mut w.hovered, p.hover, p.text, p.pressed),
        (&mut w.active, p.pressed, p.text, p.pressed),
        (&mut w.open, p.hover, p.text, p.pressed),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, border);
        state.fg_stroke = Stroke::new(1.0, text);
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
    w.noninteractive.corner_radius = radius;
}
