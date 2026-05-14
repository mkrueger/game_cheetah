/// Apply the application-wide egui theme.
pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.global_style()).clone();

    style
        .text_styles
        .insert(egui::TextStyle::Small, egui::FontId::new(12.0, egui::FontFamily::Proportional));
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::new(16.0, egui::FontFamily::Proportional));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::new(16.0, egui::FontFamily::Proportional));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, egui::FontId::new(26.0, egui::FontFamily::Proportional));
    style
        .text_styles
        .insert(egui::TextStyle::Monospace, egui::FontId::new(15.0, egui::FontFamily::Monospace));

    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(16.0, 8.0);
    style.spacing.interact_size = egui::vec2(44.0, 34.0);
    style.spacing.icon_width = 18.0;
    style.spacing.icon_width_inner = 10.0;
    style.spacing.icon_spacing = 8.0;
    style.spacing.scroll = egui::style::ScrollStyle::thin();
    style.visuals = dark_visuals();

    // Make sure none of egui's debug paint overlays are enabled. Some
    // (e.g. `show_interactive_widgets`) sneak red/blue rectangles around
    // every interactive widget and can be flipped on through the
    // built-in style editor; force them off on every theme apply so we
    // never end up shipping a UI with debug rectangles visible.
    // `Style::debug` only exists when `debug_assertions` is on (egui
    // strips the field in release builds).
    #[cfg(debug_assertions)]
    {
        style.debug = egui::style::DebugOptions::default();
    }

    ctx.set_global_style(style);
}

fn dark_visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    let accent = egui::Color32::from_rgb(0, 132, 176);
    let accent_hover = egui::Color32::from_rgb(0, 156, 207);
    let accent_active = egui::Color32::from_rgb(0, 111, 150);
    let surface = egui::Color32::from_rgb(30, 33, 38);
    let surface_hover = egui::Color32::from_rgb(42, 47, 54);
    let stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(66, 72, 82));

    visuals.panel_fill = egui::Color32::from_rgb(18, 20, 23);
    visuals.window_fill = egui::Color32::from_rgb(24, 27, 31);
    visuals.window_stroke = stroke;
    visuals.window_corner_radius = egui::CornerRadius::same(14);
    visuals.menu_corner_radius = egui::CornerRadius::same(10);
    visuals.faint_bg_color = egui::Color32::from_rgb(27, 30, 35);
    visuals.extreme_bg_color = egui::Color32::from_rgb(13, 15, 18);
    visuals.text_edit_bg_color = Some(egui::Color32::from_rgb(22, 25, 29));
    visuals.code_bg_color = egui::Color32::from_rgb(20, 23, 27);
    visuals.hyperlink_color = egui::Color32::from_rgb(96, 175, 255);
    visuals.warn_fg_color = egui::Color32::from_rgb(255, 191, 102);
    visuals.error_fg_color = egui::Color32::from_rgb(255, 123, 123);
    visuals.weak_text_color = Some(egui::Color32::from_rgb(154, 160, 170));
    visuals.selection.bg_fill = accent;
    visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    visuals.striped = true;
    visuals.button_frame = true;
    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    visuals.text_cursor.stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(180, 226, 255));

    visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(24, 27, 31);
    visuals.widgets.noninteractive.weak_bg_fill = egui::Color32::from_rgb(27, 30, 35);
    visuals.widgets.noninteractive.bg_stroke = stroke;
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(226, 229, 234));
    visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(10);

    visuals.widgets.inactive.bg_fill = surface;
    visuals.widgets.inactive.weak_bg_fill = surface;
    visuals.widgets.inactive.bg_stroke = stroke;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(228, 232, 238));
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(10);

    visuals.widgets.hovered.bg_fill = surface_hover;
    visuals.widgets.hovered.weak_bg_fill = surface_hover;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, accent_hover);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(10);
    visuals.widgets.hovered.expansion = 1.0;

    visuals.widgets.active.bg_fill = accent_active;
    visuals.widgets.active.weak_bg_fill = accent_active;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, accent_hover);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(10);

    visuals.widgets.open.bg_fill = surface_hover;
    visuals.widgets.open.weak_bg_fill = surface_hover;
    visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0, accent_hover);
    visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    visuals.widgets.open.corner_radius = egui::CornerRadius::same(10);

    visuals
}
