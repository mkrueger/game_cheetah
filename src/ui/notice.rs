//! Compact status messages with structured, opt-in technical details.
use std::path::Path;

use i18n_embed_fl::fl;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Notice {
    pub summary: String,
    pub details: String,
}

impl Notice {
    pub fn new(summary: String, details: String) -> Self {
        Self { summary, details }
    }

    pub fn save_failed(path: &Path, error: String) -> Self {
        Self::new(fl!(crate::LANGUAGE_LOADER, "notice-save-failed"), format!("{}\n{error}", file_details(path)))
    }

    pub fn load_failed(path: &Path, error: String) -> Self {
        Self::new(fl!(crate::LANGUAGE_LOADER, "notice-load-failed"), format!("{}\n{error}", file_details(path)))
    }
}

pub(crate) fn file_details(path: &Path) -> String {
    fl!(crate::LANGUAGE_LOADER, "notice-file", path = path.display().to_string())
}

pub(crate) fn unverified_summary(summary: &mut String) {
    summary.push_str(" · ");
    summary.push_str(&fl!(crate::LANGUAGE_LOADER, "notice-unverified"));
}

pub(crate) struct NoticeResponse {
    pub dismissed: bool,
    pub hovered: bool,
    pub expanded: bool,
}

/// The caller owns lifetime/dismissal. Content is part of the ID so changed
/// content starts collapsed, even if the previous notice's details were open.
pub(crate) fn show(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, summary: &str, details: &str) -> NoticeResponse {
    let mut dismissed = false;
    let mut expanded = false;
    let response = ui.scope(|ui| {
        // Small buttons otherwise change layout height on hover in egui 0.36.
        // Scope this to the notice, preserving the application's theme.
        let widgets = &mut ui.style_mut().visuals.widgets;
        widgets.inactive.expansion = 0.0;
        widgets.hovered.expansion = 0.0;
        widgets.active.expansion = 0.0;
        ui.horizontal(|ui| {
            // Reserve dismiss first so even a wrapped summary cannot hide it.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                dismissed = ui.small_button(fl!(crate::LANGUAGE_LOADER, "auto-save-dismiss")).clicked();
                ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                    ui.add(egui::Label::new(summary).wrap());
                });
            });
        });
        if !details.is_empty() {
            let response = egui::CollapsingHeader::new(fl!(crate::LANGUAGE_LOADER, "notice-details"))
                .id_salt((id, summary, details))
                .default_open(false)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                        ui.add(egui::Label::new(details).wrap());
                    });
                });
            expanded = response.body_response.is_some();
        }
    });
    NoticeResponse {
        dismissed,
        hovered: response.response.contains_pointer(),
        expanded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_shape(output: &egui::FullOutput, label: &str) -> Option<(egui::Rect, egui::Rect)> {
        fn find(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Rect> {
            match shape {
                egui::epaint::Shape::Text(text) if text.galley.job.text == label => Some(text.galley.rect.translate(text.pos.to_vec2())),
                egui::epaint::Shape::Vec(children) => children.iter().find_map(|child| find(child, label)),
                _ => None,
            }
        }
        output
            .shapes
            .iter()
            .find_map(|shape| find(&shape.shape, label).map(|rect| (rect, shape.clip_rect)))
    }

    fn click(position: egui::Pos2) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            },
        ]
    }

    #[test]
    fn details_start_collapsed_and_expand_in_bounded_scroll_without_hover_jumps() {
        for width in [320.0, 640.0, 1100.0] {
            let ctx = egui::Context::default();
            crate::ui::theme::apply(&ctx);
            let mut style = (*ctx.global_style()).clone();
            style.animation_time = 0.0;
            ctx.set_global_style(style);
            let details = "Long technical path and pointer risk\n".repeat(80);
            let summary = "Saved 2 entries · Pointers unverified after restart";
            let toggle = fl!(crate::LANGUAGE_LOADER, "notice-details");
            let dismiss = fl!(crate::LANGUAGE_LOADER, "auto-save-dismiss");
            let render = |events| {
                let mut response = None;
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 800.0))),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        let expansion = ui.visuals().widgets.hovered.expansion;
                        response = Some(show(ui, "test_notice", summary, &details));
                        assert_eq!(ui.visuals().widgets.hovered.expansion, expansion, "notice styling must stay local");
                        ui.label("Content below notice");
                    },
                );
                output.textures_delta.clear();
                (output, response.unwrap())
            };
            for _ in 0..4 {
                render(vec![]);
            }
            let (initial, response) = render(vec![]);
            assert!(!response.expanded);
            assert!(text_shape(&initial, &details).is_none());
            let compact_bottom = text_shape(&initial, "Content below notice").unwrap().0.top();
            // At narrow widths the warning wraps rather than being truncated.
            assert!(compact_bottom < 160.0, "compact height at width {width}: {compact_bottom}");
            let toggle_position = text_shape(&initial, &toggle).unwrap().0.center();
            render(click(toggle_position));
            let (expanded, response) = render(vec![]);
            assert!(response.expanded);
            let (_, clip) = text_shape(&expanded, &details).expect("expanded details must be rendered");
            assert!(clip.height() <= 150.0, "scroll viewport including clip margin must be bounded: {clip:?}");
            let below = text_shape(&expanded, "Content below notice").unwrap().0;
            assert!(below.top() - compact_bottom < 180.0, "details must not consume the window: {below:?}");
            for label in [&toggle, &dismiss] {
                let button = text_shape(&expanded, label).unwrap().0;
                for position in [button.center(), egui::pos2(width - 5.0, 790.0), button.center()] {
                    let (output, response) = render(vec![egui::Event::PointerMoved(position)]);
                    assert!(response.expanded);
                    assert_eq!(text_shape(&output, label).unwrap().0, button);
                    assert_eq!(text_shape(&output, "Content below notice").unwrap().0, below);
                }
            }
            render(click(toggle_position));
            assert!(!render(vec![]).1.expanded);
        }
    }

    #[test]
    fn load_toast_does_not_expire_while_hovered_or_details_are_open() {
        let ctx = egui::Context::default();
        crate::ui::theme::apply(&ctx);
        let mut style = (*ctx.global_style()).clone();
        style.animation_time = 0.0;
        ctx.set_global_style(style);
        let mut app = crate::App::default();
        app.set_persistence_enabled(true);
        app.cheat_table_status = "Loaded 2 entries".into();
        app.cheat_table_status_details = "File: example table\nTechnical address details".into();
        app.cheat_table_status_at = Some(std::time::Instant::now());
        let render = |app: &mut crate::App, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 800.0))),
                    events,
                    ..Default::default()
                },
                |ui| crate::ui::in_process_view::view_in_process(app, ui),
            );
            output.textures_delta.clear();
            output
        };
        for _ in 0..5 {
            render(&mut app, vec![]);
        }
        let initial = render(&mut app, vec![]);
        let summary = text_shape(&initial, &app.cheat_table_status).unwrap().0;
        let toggle = fl!(crate::LANGUAGE_LOADER, "notice-details");
        let toggle_position = text_shape(&initial, &toggle).unwrap().0.center();
        let overdue = || Some(std::time::Instant::now() - std::time::Duration::from_secs(5));
        app.cheat_table_status_at = overdue();
        render(&mut app, vec![egui::Event::PointerMoved(summary.center())]);
        assert!(!app.cheat_table_status.is_empty(), "hover must protect even an overdue toast");
        render(&mut app, click(toggle_position));
        app.cheat_table_status_at = overdue();
        let expanded = render(&mut app, vec![egui::Event::PointerMoved(egui::pos2(10.0, 790.0))]);
        assert!(!app.cheat_table_status.is_empty(), "open details must protect the toast away from hover");
        assert!(text_shape(&expanded, &app.cheat_table_status_details).is_some());
        render(&mut app, click(toggle_position));
        app.cheat_table_status_at = overdue();
        render(&mut app, vec![egui::Event::PointerMoved(egui::pos2(10.0, 790.0))]);
        assert!(app.cheat_table_status.is_empty());
        assert!(app.cheat_table_status_details.is_empty());
        assert!(app.cheat_table_status_at.is_none());
    }

    #[test]
    fn failures_keep_original_error_and_path_out_of_summary() {
        let path = Path::new("/very/long/technical/table.toml");
        let error = "original diagnostic: Permission denied (os error 13)\nmore context";
        for notice in [Notice::save_failed(path, error.into()), Notice::load_failed(path, error.into())] {
            assert!(!notice.summary.contains(error));
            assert!(!notice.summary.contains(&path.display().to_string()));
            assert!(notice.details.contains(error));
            assert!(notice.details.contains(&path.display().to_string()));
        }
    }
}
