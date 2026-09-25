//! Visual theme modelled on Keel's workspace UI: Geist, zinc greys, 28px controls
//! with 8px corners and an orange #FF6A2B accent. The sidebar sits on a dark canvas;
//! the work area is a raised surface with 12px corners inset from the window edge.
//! Light and dark follow the OS.
//!
//! All colours, sizes and fonts live here. Screens use the helpers below
//! (`primary_button`, `bar_frame`, `workspace_frame`, `semibold`) instead of
//! hard-coding styling.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke,
    TextStyle, Theme, Vec2,
};

use crate::export::ValueKind;
use crate::sql::Token;

pub const ACCENT: Color32 = Color32::from_rgb(0xFF, 0x6A, 0x2B);
/// Text on the accent: near-black, since white on orange is too faint.
const ON_ACCENT: Color32 = Color32::from_rgb(0x1A, 0x0B, 0x03);
const RADIUS: u8 = 8;
/// Corners of the work area and the setup card.
const PANEL_RADIUS: u8 = 12;
/// Space between the window edge and the work area.
pub const SHELL_GAP: i8 = 8;
const SEMIBOLD: &str = "Geist-SemiBold";

struct Palette {
    /// Behind everything: the window, the sidebar.
    canvas: Color32,
    /// The work area (toolbar, editor, results) and the setup card.
    surface: Color32,
    /// Buttons and text inputs.
    raised: Color32,
    /// Popups and menus.
    overlay: Color32,
    /// Alternate table rows.
    stripe: Color32,
    /// The SQL editor's line holding the cursor.
    current_line: Color32,
    /// A hovered list item (menu option, tab, sidebar row): stands out on both the
    /// canvas and the overlay of popups, unlike `hover`, made for raised buttons.
    item_hover: Color32,
    /// A hovered icon button on a hovered list item.
    item_active: Color32,
    hover: Color32,
    pressed: Color32,
    /// Hairlines between panels, around cards.
    line: Color32,
    /// Control outlines.
    line_strong: Color32,
    /// Results table header row.
    header: Color32,
    text: Color32,
    text_secondary: Color32,
    danger: Color32,
    /// Selected toggles, the focused input's outline and selected text: an orange tint
    /// with `selected_text`, readable on it and on `surface`.
    selected_fill: Color32,
    selected_text: Color32,
    shadow: Color32,
}

const DARK: Palette = Palette {
    canvas: Color32::from_rgb(0x0A, 0x0A, 0x0B),
    surface: Color32::from_rgb(0x11, 0x11, 0x13),
    raised: Color32::from_rgb(0x17, 0x17, 0x1A),
    overlay: Color32::from_rgb(0x1D, 0x1D, 0x21),
    stripe: Color32::from_rgb(0x15, 0x15, 0x17),
    current_line: Color32::from_rgb(0x1B, 0x1B, 0x1F),
    item_hover: Color32::from_rgb(0x2E, 0x2E, 0x34),
    item_active: Color32::from_rgb(0x3E, 0x3E, 0x46),
    hover: Color32::from_rgb(0x21, 0x21, 0x25),
    pressed: Color32::from_rgb(0x29, 0x29, 0x2D),
    line: Color32::from_rgb(0x25, 0x25, 0x28),
    line_strong: Color32::from_rgb(0x31, 0x31, 0x35),
    header: Color32::from_rgb(0x19, 0x19, 0x1C),
    text: Color32::from_rgb(0xED, 0xED, 0xEF),
    text_secondary: Color32::from_rgb(0xA1, 0xA1, 0xAA),
    danger: Color32::from_rgb(0xFF, 0x5C, 0x8A),
    selected_fill: Color32::from_rgb(0x4A, 0x26, 0x17),
    selected_text: Color32::from_rgb(0xFF, 0x7A, 0x40),
    shadow: Color32::from_black_alpha(140),
};

const LIGHT: Palette = Palette {
    canvas: Color32::from_rgb(0xF3, 0xF3, 0xF4),
    surface: Color32::from_rgb(0xFA, 0xFA, 0xFA),
    raised: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    overlay: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    stripe: Color32::from_rgb(0xF3, 0xF3, 0xF4),
    current_line: Color32::from_rgb(0xEF, 0xEF, 0xF1),
    item_hover: Color32::from_rgb(0xE6, 0xE6, 0xE9),
    item_active: Color32::from_rgb(0xD4, 0xD4, 0xD9),
    hover: Color32::from_rgb(0xEE, 0xEE, 0xEF),
    pressed: Color32::from_rgb(0xE4, 0xE4, 0xE6),
    line: Color32::from_rgb(0xE4, 0xE4, 0xE6),
    line_strong: Color32::from_rgb(0xD6, 0xD6, 0xD9),
    header: Color32::from_rgb(0xF0, 0xF0, 0xF1),
    text: Color32::from_rgb(0x10, 0x10, 0x12),
    text_secondary: Color32::from_rgb(0x5E, 0x5E, 0x66),
    danger: Color32::from_rgb(0xC8, 0x24, 0x55),
    selected_fill: Color32::from_rgb(0xFD, 0xE3, 0xD8),
    selected_text: Color32::from_rgb(0xC4, 0x47, 0x0F),
    shadow: Color32::from_black_alpha(36),
};

/// Installs fonts and both light and dark styles. Takes effect from the next frame.
pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.style_mut_of(Theme::Dark, |s| style(s, &DARK, true));
    ctx.style_mut_of(Theme::Light, |s| style(s, &LIGHT, false));
}

/// Semibold Geist, for headings and table headers.
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

/// Filled orange button for the main action on a screen.
pub fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text).color(ON_ACCENT))
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
        ValueKind::Null => palette(dark).text_secondary,
        ValueKind::Number => syntax_color(Token::Constant, dark),
        ValueKind::Boolean => syntax_color(Token::Keyword, dark),
        ValueKind::Date => syntax_color(Token::Function, dark),
        ValueKind::Json => syntax_color(Token::Parameter, dark),
    }
}

/// SQL syntax colour, from Keel's data palette (violet, sky, amber, teal), darkened
/// in light mode to keep AA contrast on the surface.
pub fn syntax_color(token: Token, dark: bool) -> Color32 {
    let rgb = |hex: u32| Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
    let (light, dark_hex) = match token {
        Token::Plain => return palette(dark).text,
        Token::Keyword => (0x5B48E0, 0x9C8CFF),
        Token::Function => (0x1569B8, 0x56B4FF),
        Token::Constant => (0x8F5A00, 0xF5B53D),
        Token::String => (0x08735F, 0x2EC4A6),
        Token::Comment => (0x74747C, 0x7E7E87),
        Token::Parameter => (0xC4470F, 0xFF7A40),
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
    pointer(&response);
    response
}

/// Background of a hovered list item: menu options, tabs, sidebar rows, suggestions.
pub fn item_hover_fill(dark: bool) -> Color32 {
    palette(dark).item_hover
}

/// Background of the SQL editor's line holding the cursor: a faint band on the surface.
pub fn current_line_fill(dark: bool) -> Color32 {
    palette(dark).current_line
}

/// A painted icon: egui's default fonts lack most symbols.
#[derive(Clone, Copy)]
pub enum Icon {
    /// A circular arrow, for reloading.
    Refresh,
    /// A cross, for removing an item.
    Remove,
    /// A double chevron pointing right, for putting something into the query.
    Insert,
    /// A small grid with a header row, for a database table.
    Table,
    /// A floppy disk, for saving.
    Save,
    /// Curly braces, for `{{ variables }}`.
    Variables,
}

/// Paints `icon` centred in `rect`, about 12px across.
pub fn paint_icon(painter: &egui::Painter, rect: egui::Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let stroke = Stroke::new(1.3, color);
    match icon {
        Icon::Refresh => {
            // A circle open at the top right, ending in a filled arrowhead that points
            // along it (clockwise, like a reload).
            let r = 4.5;
            let (start, sweep) = (80f32.to_radians(), -290f32.to_radians());
            let point = |a: f32| c + r * Vec2::new(a.cos(), -a.sin());
            let arc: Vec<_> = (0..=24).map(|i| point(start + sweep * i as f32 / 24.0)).collect();
            painter.add(egui::Shape::line(arc, stroke));
            let end = start + sweep;
            let tangent = Vec2::new(end.sin(), end.cos()); // clockwise on screen
            let radial = Vec2::new(end.cos(), -end.sin());
            let tip = point(end) + 3.0 * tangent;
            let base = [point(end) + 2.6 * radial, point(end) - 2.6 * radial];
            painter.add(egui::Shape::convex_polygon(vec![tip, base[0], base[1]], color, Stroke::NONE));
        }
        Icon::Remove => {
            let d = 3.5;
            painter.line_segment([c + Vec2::new(-d, -d), c + Vec2::new(d, d)], stroke);
            painter.line_segment([c + Vec2::new(-d, d), c + Vec2::new(d, -d)], stroke);
        }
        Icon::Insert => {
            for dx in [-2.5, 2.0] {
                let tip = c + Vec2::new(dx + 1.75, 0.0);
                let chevron = vec![tip + Vec2::new(-3.5, -4.0), tip, tip + Vec2::new(-3.5, 4.0)];
                painter.add(egui::Shape::line(chevron, stroke));
            }
        }
        Icon::Save => {
            let disk = egui::Rect::from_center_size(c, Vec2::splat(12.0));
            painter.rect_stroke(disk, 2.0, stroke, egui::StrokeKind::Inside);
            // The shutter at the top and the label at the bottom.
            let shutter = egui::Rect::from_min_max(
                disk.min + Vec2::new(3.0, 0.0),
                disk.right_top() + Vec2::new(-3.5, 4.0),
            );
            painter.rect_stroke(shutter, 0.0, stroke, egui::StrokeKind::Inside);
            let label = egui::Rect::from_min_max(
                disk.left_bottom() + Vec2::new(2.5, -5.0),
                disk.max - Vec2::new(2.5, 0.0),
            );
            painter.rect_stroke(label, 0.0, stroke, egui::StrokeKind::Inside);
        }
        Icon::Variables => {
            // `{` left of the centre and `}` right of it, 12px tall. `side` is -1 for
            // `{`: its tips point right (towards the centre), its middle bulges left.
            for side in [-1.0f32, 1.0] {
                let x = c.x + side * 3.5;
                let (top, bottom) = (c.y - 6.0, c.y + 6.0);
                let (tips, spine, bulge) = (x - side * 1.5, x, x + side * 1.5);
                let brace = vec![
                    egui::pos2(tips, top),
                    egui::pos2(spine, top + 1.5),
                    egui::pos2(spine, c.y - 1.5),
                    egui::pos2(bulge, c.y),
                    egui::pos2(spine, c.y + 1.5),
                    egui::pos2(spine, bottom - 1.5),
                    egui::pos2(tips, bottom),
                ];
                painter.add(egui::Shape::line(brace, stroke));
            }
        }
        Icon::Table => {
            let grid = egui::Rect::from_center_size(c, Vec2::new(12.0, 10.0));
            let stroke = Stroke::new(1.1, color);
            painter.rect_stroke(grid, 1.5, stroke, egui::StrokeKind::Inside);
            let header = grid.top() + 3.5;
            painter.hline(grid.x_range(), header, stroke);
            painter.vline(grid.center().x, egui::Rangef::new(header, grid.bottom()), stroke);
        }
    }
}

/// A square, frameless button showing `icon`, filled when hovered. `label` names it
/// for tooltips, screen readers and tests.
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, size: f32, label: &str) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    icon_button_at(ui, rect, icon, label)
}

/// An outlined, square button showing `icon`, the size and look of the text buttons
/// beside it (filled like a toggle when `selected`). `label` names it for tooltips,
/// screen readers and tests.
pub fn outlined_icon_button(ui: &mut egui::Ui, icon: Icon, label: &str, selected: bool) -> egui::Response {
    let size = Vec2::splat(ui.spacing().interact_size.y);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, label)
    });
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, selected);
        let painter = ui.painter();
        painter.rect(
            rect,
            visuals.corner_radius,
            visuals.weak_bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
        paint_icon(painter, rect, icon, visuals.text_color());
    }
    pointer(&response);
    response
}

/// Like `icon_button`, over `rect` without taking space in the layout: for a button
/// laid over a row, which would otherwise move the rows after it.
pub fn icon_button_at(ui: &mut egui::Ui, rect: egui::Rect, icon: Icon, label: &str) -> egui::Response {
    let response = ui.interact(rect, ui.id().with(("icon_button", label)), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
    if ui.is_rect_visible(rect) {
        let visuals = ui.visuals();
        if response.hovered() {
            let fill = palette(visuals.dark_mode).item_active;
            ui.painter().rect_filled(rect, visuals.widgets.hovered.corner_radius, fill);
        }
        let color = if !ui.is_enabled() {
            visuals.weak_text_color().gamma_multiply(0.5)
        } else if response.hovered() {
            visuals.strong_text_color()
        } else {
            visuals.weak_text_color()
        };
        paint_icon(ui.painter(), rect, icon, color);
    }
    pointer(&response);
    response
}

/// The hover highlight of a sidebar list row, across the whole sidebar (through any
/// indent: the list's scroll area already clips at the sidebar's edges), not just the row. Reserve it before drawing the row, so it
/// goes under the row's text, then `fill` it once the row's rect is known.
pub struct RowHighlight {
    painter: egui::Painter,
    slot: egui::layers::ShapeIdx,
}

impl RowHighlight {
    pub fn reserve(ui: &egui::Ui) -> Self {
        let painter = ui.painter().clone();
        let slot = painter.add(egui::Shape::Noop);
        Self { painter, slot }
    }

    /// Fills the highlight for the row at `row` if the pointer is over it.
    pub fn fill(self, ui: &egui::Ui, row: egui::Rect) {
        if ui.rect_contains_pointer(row) && ui.is_enabled() {
            let rect = egui::Rect::from_x_y_ranges(self.painter.clip_rect().x_range(), row.y_range());
            let fill = item_hover_fill(ui.visuals().dark_mode);
            self.painter.set(self.slot, egui::Shape::rect_filled(rect, 0.0, fill));
        }
    }
}

/// Shows the pointing hand while `response` is hovered, for clickable widgets egui
/// doesn't give one itself: anything but a `Button` (which follows
/// `interact_cursor`), like combo boxes, collapsing headers and painted widgets.
pub fn pointer(response: &egui::Response) {
    if response.hovered() {
        response.ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}

/// A tab, like the sidebar's History / Saved / Schema: plain text that gets a fill
/// when hovered or selected.
pub fn tab(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    selectable(ui, text, selected, false)
}

/// An item of a combo box's list, as wide as the list, text on the left; filled when
/// hovered or selected. Use instead of `selectable_label` / `selectable_value`.
pub fn option(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    selectable(ui, text, selected, true)
}

/// Text that gets a fill when hovered or selected, the same size in every state.
/// Painted rather than a frameless `Button` (what `selectable_label` is), which adds
/// its border on hover and so grows, shifting whatever comes after it.
fn selectable(ui: &mut egui::Ui, text: &str, selected: bool, full_width: bool) -> egui::Response {
    let padding = ui.spacing().button_padding;
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        TextStyle::Button,
    );
    let mut width = galley.size().x + 2.0 * padding.x;
    if full_width {
        width = width.max(ui.available_width());
    }
    let size = Vec2::new(width, ui.spacing().interact_size.y);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let kind = if full_width { egui::WidgetType::SelectableLabel } else { egui::WidgetType::Button };
    response.widget_info(|| egui::WidgetInfo::selected(kind, ui.is_enabled(), selected, text));
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, selected);
        if selected {
            ui.painter().rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
        } else if response.hovered() {
            let fill = item_hover_fill(ui.visuals().dark_mode);
            ui.painter().rect_filled(rect, visuals.corner_radius, fill);
        }
        let pos = if full_width {
            egui::pos2(rect.left() + padding.x, rect.center().y - 0.5 * galley.size().y)
        } else {
            rect.center() - 0.5 * galley.size()
        };
        ui.painter().galley(pos, galley, visuals.text_color());
    }
    pointer(&response);
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

/// Frame for toolbars, status bars and the panels inside the work area: no fill of
/// its own, as the work area's surface shows through (a fill would square its corners).
pub fn bar_frame(_style: &egui::Style) -> egui::Frame {
    egui::Frame::new().inner_margin(Margin::symmetric(12, 8))
}

/// Frame for the sidebar, on the canvas beside the work area.
pub fn sidebar_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::side_top_panel(style).inner_margin(Margin { left: 8, right: 8, top: 10, bottom: 8 })
}

/// The canvas around the work area: `SHELL_GAP` on each side, except the left one
/// when the sidebar is there (its own margin does it).
pub fn shell_frame(style: &egui::Style, sidebar: bool) -> egui::Frame {
    let left = if sidebar { 0 } else { SHELL_GAP };
    egui::Frame::central_panel(style).inner_margin(Margin {
        left,
        right: SHELL_GAP,
        top: SHELL_GAP,
        bottom: SHELL_GAP,
    })
}

/// The raised surface holding the toolbar, editor and results; also the setup card.
pub fn workspace_frame(style: &egui::Style) -> egui::Frame {
    let dark = style.visuals.dark_mode;
    egui::Frame::new()
        .fill(palette(dark).surface)
        .stroke(style.visuals.widgets.noninteractive.bg_stroke)
        .corner_radius(PANEL_RADIUS)
}

/// Frame for popups anchored in the editor, like autocompletion.
pub fn popup_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::popup(style).inner_margin(Margin::same(4))
}

fn fonts() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    defs.font_data.insert(
        "Geist".into(),
        Arc::new(FontData::from_static(include_bytes!("../../assets/fonts/Geist-Regular.ttf"))),
    );
    defs.font_data.insert(
        SEMIBOLD.into(),
        Arc::new(FontData::from_static(include_bytes!("../../assets/fonts/Geist-SemiBold.ttf"))),
    );
    defs.font_data.insert(
        "GeistMono".into(),
        Arc::new(FontData::from_static(include_bytes!("../../assets/fonts/GeistMono-Regular.ttf"))),
    );
    // Keep egui's default fonts as fallbacks for symbols Geist lacks (▶, emoji).
    defs.families.entry(FontFamily::Monospace).or_default().insert(0, "GeistMono".into());
    let proportional = defs.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, "Geist".into());
    let mut semibold = vec![SEMIBOLD.to_string()];
    semibold.extend(proportional.iter().cloned());
    defs.families.insert(FontFamily::Name(SEMIBOLD.into()), semibold);
    defs
}

fn style(s: &mut egui::Style, p: &Palette, dark: bool) {
    use TextStyle::{Body, Button, Heading, Monospace, Small};
    s.text_styles = [
        (Small, FontId::proportional(11.0)),
        (Body, FontId::proportional(13.0)),
        (Button, FontId::proportional(13.0)),
        (Monospace, FontId::monospace(13.0)),
        (Heading, semibold(20.0)),
    ]
    .into();

    let sp = &mut s.spacing;
    sp.item_spacing = Vec2::new(8.0, 6.0);
    sp.button_padding = Vec2::new(10.0, 5.0);
    sp.interact_size.y = 28.0;
    sp.window_margin = Margin::same(12);
    sp.menu_margin = Margin::same(6);

    let v = &mut s.visuals;
    *v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = p.canvas;
    v.window_fill = p.overlay;
    v.window_stroke = Stroke::new(1.0, p.line_strong);
    v.window_corner_radius = CornerRadius::same(PANEL_RADIUS);
    v.menu_corner_radius = CornerRadius::same(RADIUS);
    v.window_shadow = Shadow { offset: [0, 12], blur: 32, spread: 0, color: p.shadow };
    v.popup_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: p.shadow };
    v.faint_bg_color = p.stripe;
    v.extreme_bg_color = p.raised;
    v.text_edit_bg_color = Some(p.raised);
    v.code_bg_color = p.raised;
    v.hyperlink_color = p.selected_text;
    // Buttons show the pointing hand; other clickable widgets use `pointer`.
    v.interact_cursor = Some(egui::CursorIcon::PointingHand);
    v.error_fg_color = p.danger;
    v.weak_text_color = Some(p.text_secondary);
    v.selection.bg_fill = p.selected_fill;
    v.selection.stroke = Stroke::new(1.0, p.selected_text);

    let radius = CornerRadius::same(RADIUS);
    let w = &mut v.widgets;
    // Labels, separators and the hairlines between panels.
    w.noninteractive.bg_fill = p.surface;
    w.noninteractive.weak_bg_fill = p.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.line);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    // Buttons, combo boxes, checkboxes and text inputs: outlined so they stand out
    // from the surface, especially in light mode.
    for (state, fill, border) in [
        (&mut w.inactive, p.raised, p.line_strong),
        (&mut w.hovered, p.hover, p.line_strong),
        (&mut w.active, p.pressed, p.line_strong),
        (&mut w.open, p.hover, p.line_strong),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, border);
        state.fg_stroke = Stroke::new(1.0, p.text);
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
    w.noninteractive.corner_radius = radius;
}
