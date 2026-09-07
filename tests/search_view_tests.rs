//! Headless integration regressions for the real in-process view.
//!
//! No locale/environment mutation, attachment or search dispatch. Synthetic
//! rows use PID 0; Linux-only editing tests touch exclusively owned test data.
//! Only the view is run, not App::logic (process polling/background work).

use std::sync::atomic::Ordering;

use egui::{Context, Event, FullOutput, Key, Modifiers, Pos2, Rect, Vec2, epaint::Shape};
use game_cheetah::{App, AppState, LANGUAGE_LOADER, SearchMode, SearchResult, SearchType};
use i18n_embed_fl::fl;

const LARGE: Vec2 = egui::vec2(1100.0, 720.0);
const SIZES: [Vec2; 2] = [egui::vec2(640.0, 420.0), LARGE];

fn app_with_results(count: usize) -> App {
    let mut app = App::default();
    app.app_state = AppState::InProcess;
    app.state.pid = 0;
    app.state.process_name = "Headless test".to_owned();
    let search = &mut app.state.searches[0];
    search.search_type = SearchType::Int;
    search.search_value_text = if count == 0 { String::new() } else { "12345".to_owned() };
    search.set_cached_results((1..=count).map(|i| SearchResult::new(i * 0x1000, SearchType::Int)).collect());
    search.search_complete.store(count > 0, Ordering::Release);
    app
}

fn context() -> Context {
    let ctx = Context::default();
    game_cheetah::ui::theme::apply(&ctx);
    ctx
}

fn frame(app: &mut App, ctx: &Context, size: Vec2, events: Vec<Event>) -> FullOutput {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ui| game_cheetah::ui::in_process_view::view_in_process(app, ui),
    );
    // Must happen before every FullOutput is dropped, including warm-up frames.
    output.textures_delta.clear();
    output
}

fn settle(app: &mut App, ctx: &Context, size: Vec2) -> FullOutput {
    frame(app, ctx, size, vec![]);
    frame(app, ctx, size, vec![])
}

fn shapes(output: &FullOutput) -> Vec<(Rect, &Shape)> {
    fn collect<'a>(clip: Rect, shape: &'a Shape, out: &mut Vec<(Rect, &'a Shape)>) {
        if let Shape::Vec(children) = shape {
            for child in children {
                collect(clip, child, out);
            }
        } else {
            out.push((clip, shape));
        }
    }
    let mut out = Vec::new();
    for shape in &output.shapes {
        collect(shape.clip_rect, &shape.shape, &mut out);
    }
    out
}

fn text_rect(output: &FullOutput, label: &str) -> Rect {
    shapes(output)
        .into_iter()
        .find_map(|(clip, shape)| match shape {
            Shape::Text(text) if text.galley.job.text == label => {
                let rect = text.galley.rect.translate(text.pos.to_vec2());
                clip.contains(rect.center()).then_some(rect)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Missing visible text: {label:?}"))
}

fn assert_single_line_visible(output: &FullOutput, label: &str, size: Vec2) {
    let matches: Vec<_> = shapes(output)
        .into_iter()
        .filter_map(|(clip, shape)| match shape {
            Shape::Text(text) if text.galley.job.text == label => Some((clip, text)),
            _ => None,
        })
        .collect();
    assert_eq!(matches.len(), 1, "Expected ONE galley for {label:?} at {size:?}");
    let (clip, text) = matches[0];
    assert_eq!(text.galley.rows.len(), 1, "Wrapped text: {label:?} at {size:?}");
    let rect = text.galley.rect.translate(text.pos.to_vec2());
    assert!(Rect::from_min_size(Pos2::ZERO, size).contains_rect(rect), "Offscreen {label:?}: {rect:?}");
    assert!(clip.expand(1.0).contains_rect(rect), "Clipped {label:?}: {rect:?}, clip={clip:?}");
}

// Check the painted button frame, not just its label: a label fitting into the
// viewport must not hide an overflowing hit target. The smallest enclosing
// filled rectangle distinguishes the button from its surrounding card/panel.
fn button_frame(output: &FullOutput, label: &str) -> (Rect, egui::Color32, Rect) {
    let label_rect = text_rect(output, label);
    shapes(output)
        .into_iter()
        .filter_map(|(clip, shape)| match shape {
            Shape::Rect(rect) if rect.rect.contains_rect(label_rect) && rect.rect.height() >= 28.0 && rect.fill != egui::Color32::TRANSPARENT => {
                Some((rect.rect, rect.fill, clip))
            }
            _ => None,
        })
        .min_by(|a, b| a.0.area().total_cmp(&b.0.area()))
        .unwrap_or_else(|| panic!("Missing button frame for {label:?}"))
}

fn assert_button_visible(output: &FullOutput, label: &str, size: Vec2) {
    assert_single_line_visible(output, label, size);
    let (rect, _, clip) = button_frame(output, label);
    assert!(rect.height() <= 40.0, "Matched a panel instead of a button: {label:?}, {rect:?}");
    assert!(
        Rect::from_min_size(Pos2::ZERO, size).contains_rect(rect),
        "Offscreen button {label:?}: {rect:?}"
    );
    assert!(clip.expand(1.0).contains_rect(rect), "Clipped button {label:?}: {rect:?}");
}

fn click(app: &mut App, ctx: &Context, size: Vec2, pos: Pos2) {
    // Separate hover, press and release frames exercise normal egui hit testing.
    frame(app, ctx, size, vec![Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            size,
            vec![Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            }],
        );
    }
}

fn press_key(app: &mut App, ctx: &Context, key: Key) {
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            LARGE,
            vec![Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Modifiers::NONE,
            }],
        );
    }
}

fn identity(result: Option<SearchResult>) -> Option<(usize, SearchType)> {
    result.map(|result| (result.addr, result.search_type))
}

fn result_identities(app: &App) -> Vec<(usize, SearchType)> {
    app.state.searches[0]
        .collect_results()
        .iter()
        .map(|result| (result.addr, result.search_type))
        .collect()
}

fn click_address(app: &mut App, ctx: &Context, address: usize) {
    let output = settle(app, ctx, LARGE);
    click(app, ctx, LARGE, text_rect(&output, &format!("0x{address:X}")).center());
}

#[test]
fn result_count_is_one_unwrapped_galley_and_actions_fit_both_viewports() {
    for size in SIZES {
        let mut app = app_with_results(23);
        let search = &mut app.state.searches[0];
        search.push_undo_state(search.collect_results());
        let ctx = context();
        let output = settle(&mut app, &ctx, size);

        assert_single_line_visible(&output, &format!("23 {}", fl!(LANGUAGE_LOADER, "result-unit-plural")), size);
        for label in [
            fl!(LANGUAGE_LOADER, "update-button"),
            fl!(LANGUAGE_LOADER, "undo-button"),
            fl!(LANGUAGE_LOADER, "clear-button"),
        ] {
            assert_button_visible(&output, &label, size);
        }
    }
}

#[test]
fn numeric_filter_can_expand_with_large_hidden_results_and_range_actions_fit() {
    for size in SIZES {
        let mut app = app_with_results(20_000);
        let ctx = context();
        let output = settle(&mut app, &ctx, size);
        let toggle = fl!(LANGUAGE_LOADER, "numeric-filter-title");
        click(&mut app, &ctx, size, text_rect(&output, &toggle).center());
        assert!(app.state.searches[0].show_numeric_filter);
        app.state.searches[0].numeric_comparison = game_cheetah::NumericComparison::Between;
        app.state.searches[0].numeric_filter_upper = "1000".to_owned();
        let output = settle(&mut app, &ctx, size);
        assert_button_visible(&output, &fl!(LANGUAGE_LOADER, "numeric-filter-apply"), size);
        assert_single_line_visible(&output, &fl!(LANGUAGE_LOADER, "numeric-filter-and"), size);
        assert_eq!(app.state.searches[0].get_result_count(), 20_000);
    }
}

#[test]
fn invalid_numeric_range_cannot_dispatch_and_filter_inputs_stay_per_tab() {
    let mut app = app_with_results(23);
    app.state.searches[0].show_numeric_filter = true;
    app.state.searches[0].numeric_comparison = game_cheetah::NumericComparison::Between;
    app.state.searches[0].numeric_filter_lower = "10".to_owned();
    app.state.searches[0].numeric_filter_upper = "5".to_owned();
    let ctx = context();
    let output = settle(&mut app, &ctx, LARGE);
    text_rect(&output, &fl!(LANGUAGE_LOADER, "numeric-filter-reversed"));
    click(
        &mut app,
        &ctx,
        LARGE,
        text_rect(&output, &fl!(LANGUAGE_LOADER, "numeric-filter-apply")).center(),
    );
    assert_eq!(app.state.searches[0].get_result_count(), 23);
    assert!(app.state.searches[0].old_results.is_empty());
    assert_eq!(app.state.searches[0].searching, SearchMode::None);
    app.new_search();
    assert!(!app.state.searches[1].show_numeric_filter);
    assert_eq!(app.state.searches[1].numeric_filter_lower, "0");
    app.switch_search(0);
    assert_eq!(app.state.searches[0].numeric_filter_lower, "10");
}

#[test]
fn all_filter_controls_fit_small_and_large_windows() {
    for size in SIZES {
        let mut app = app_with_results(20_000);
        let search = &mut app.state.searches[0];
        search.show_numeric_filter = true;
        search.type_filter_enabled = true;
        search.stable_filter_enabled = true;
        search.numeric_comparison = game_cheetah::NumericComparison::Between;
        search.numeric_filter_upper = "1000".to_owned();
        let ctx = context();
        let output = settle(&mut app, &ctx, size);
        assert_button_visible(&output, &fl!(LANGUAGE_LOADER, "numeric-filter-apply"), size);
        for label in ["UInt8", "Int16", "Int64", "Float32", "Float64"] {
            assert_single_line_visible(&output, label, size);
        }
        assert_single_line_visible(&output, &fl!(LANGUAGE_LOADER, "result-filter-seconds"), size);
    }
}

#[test]
fn type_chips_and_filter_toggles_are_independent_and_empty_selection_cannot_apply() {
    let mut app = app_with_results(20_000);
    app.state.searches[0].show_numeric_filter = true;
    let ctx = context();
    let output = settle(&mut app, &ctx, LARGE);
    click(&mut app, &ctx, LARGE, text_rect(&output, &fl!(LANGUAGE_LOADER, "result-filter-types")).center());
    assert!(app.state.searches[0].type_filter_enabled);
    let output = settle(&mut app, &ctx, LARGE);
    click(
        &mut app,
        &ctx,
        LARGE,
        text_rect(&output, &fl!(LANGUAGE_LOADER, "result-filter-types-none")).center(),
    );
    assert_eq!(app.state.searches[0].filter_types, [false; 6]);
    let output = settle(&mut app, &ctx, LARGE);
    text_rect(&output, &fl!(LANGUAGE_LOADER, "result-filter-no-types"));
    click(&mut app, &ctx, LARGE, text_rect(&output, "Int64").center());
    assert_eq!(app.state.searches[0].filter_types, [false, false, false, true, false, false]);
    app.state.searches[0].numeric_filter_enabled = false;
    app.state.searches[0].numeric_filter_lower = "invalid".to_owned();
    assert!(app.state.searches[0].result_filter().is_ok());
    let output = settle(&mut app, &ctx, LARGE);
    click(
        &mut app,
        &ctx,
        LARGE,
        text_rect(&output, &fl!(LANGUAGE_LOADER, "result-filter-stable")).center(),
    );
    assert!(app.state.searches[0].stable_filter_enabled);
    app.new_search();
    assert!(!app.state.searches[1].stable_filter_enabled);
    assert!(!app.state.searches[1].type_filter_enabled);
    app.switch_search(0);
    assert!(app.state.searches[0].stable_filter_enabled);
    assert_eq!(app.state.searches[0].filter_types, [false, false, false, true, false, false]);
}

#[cfg(target_os = "linux")]
#[test]
fn stability_progress_has_a_working_cancel_action() {
    let mut value = Box::new(24680_i32);
    let mut app = own_value_app(&mut value);
    app.state.searches[0].stable_filter_enabled = true;
    app.state.searches[0].stable_filter_seconds = 30;
    app.apply_numeric_filter();
    let ctx = context();
    let output = settle(&mut app, &ctx, LARGE);
    let cancel = fl!(LANGUAGE_LOADER, "result-filter-cancel");
    assert_button_visible(&output, &cancel, LARGE);
    click(&mut app, &ctx, LARGE, text_rect(&output, &cancel).center());
    assert_eq!(app.state.searches[0].searching, SearchMode::None);
    assert_eq!(app.state.searches[0].get_result_count(), 1);
    assert!(app.state.searches[0].old_results.is_empty());
    assert_eq!(*value, 24680);
}

#[cfg(target_os = "linux")]
#[test]
fn every_progress_mode_offers_cancel_in_both_window_sizes() {
    for size in SIZES {
        for mode in [SearchMode::Memory, SearchMode::Percent, SearchMode::Stability] {
            let mut value = Box::new(24680_i32);
            let mut app = own_value_app(&mut value);
            app.state.searches[0].stable_filter_enabled = true;
            app.state.searches[0].stable_filter_seconds = 30;
            app.apply_numeric_filter();
            // Reuse a real cancellable task to exercise each progress presentation.
            app.state.searches[0].searching = mode;
            let ctx = context();
            let output = settle(&mut app, &ctx, size);
            let cancel = fl!(LANGUAGE_LOADER, "result-filter-cancel");
            assert_button_visible(&output, &cancel, size);
            click(&mut app, &ctx, size, text_rect(&output, &cancel).center());
            assert_eq!(app.state.searches[0].searching, SearchMode::None);
            assert_eq!(app.state.searches[0].get_result_count(), 1);
            assert!(app.state.searches[0].old_results.is_empty());
            assert_eq!(*value, 24680);
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn switching_tabs_keeps_search_running_and_closing_it_is_safe() {
    let mut value = Box::new(24680_i32);
    let mut app = own_value_app(&mut value);
    app.state.searches[0].stable_filter_enabled = true;
    app.state.searches[0].stable_filter_seconds = 30;
    app.apply_numeric_filter();
    app.new_search();
    assert_eq!(app.state.searches[0].searching, SearchMode::Stability);
    app.close_search(0);
    assert_eq!(app.state.searches.len(), 1);
    assert_eq!(app.state.current_search, 0);
    assert_eq!(app.state.searches[0].searching, SearchMode::None);
    assert!(app.state.searches[0].old_results.is_empty());
    assert_eq!(*value, 24680);
}

#[cfg(target_os = "linux")]
#[test]
fn applying_numeric_filter_through_ui_is_read_only_and_undoable() {
    let mut value = Box::new(24680_i32);
    let mut app = own_value_app(&mut value);
    app.state.searches[0].show_numeric_filter = true;
    app.state.searches[0].numeric_filter_lower = "30000".to_owned();
    let ctx = context();
    let output = settle(&mut app, &ctx, LARGE);
    click(
        &mut app,
        &ctx,
        LARGE,
        text_rect(&output, &fl!(LANGUAGE_LOADER, "numeric-filter-apply")).center(),
    );
    assert_eq!(app.state.searches[0].get_result_count(), 0);
    assert_eq!(*value, 24680);
    let output = settle(&mut app, &ctx, LARGE);
    click(&mut app, &ctx, LARGE, text_rect(&output, &fl!(LANGUAGE_LOADER, "undo-button")).center());
    assert_eq!(app.state.searches[0].get_result_count(), 1);
    assert_eq!(*value, 24680);
}

#[cfg(target_os = "linux")]
#[test]
fn overflow_offers_manual_type_inspection_without_widening_or_writing() {
    let mut value = Box::new(24680_i32);
    let mut app = own_value_app(&mut value);
    let address = &*value as *const i32 as usize;
    app.selected_result = Some(SearchResult::new(address, SearchType::Int));
    app.editing_result = Some((0, "3000000000".to_owned()));
    let ctx = context();
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("result-value", address, SearchType::Int))));
    let output = settle(&mut app, &ctx, LARGE);
    let inspect = text_rect(&output, &fl!(LANGUAGE_LOADER, "check-result-type"));
    assert_eq!(*value, 24680);
    click(&mut app, &ctx, LARGE, inspect.center());
    assert_eq!(app.app_state, AppState::MemoryEditor);
    assert_eq!(app.state.searches[0].collect_results()[0].search_type, SearchType::Int);
    assert_eq!(*value, 24680);
}

#[test]
fn unknown_capture_and_snapshot_comparisons_fit_both_viewports() {
    for size in SIZES {
        let mut app = app_with_results(0);
        app.state.searches[0].search_type = SearchType::Unknown;
        let ctx = context();
        let output = settle(&mut app, &ctx, size);
        assert_button_visible(&output, &fl!(LANGUAGE_LOADER, "capture-snapshot-button"), size);

        // A synthetic snapshot enables comparisons without a real capture.
        let search = &mut app.state.searches[0];
        search.store_memory_snapshot(0x1000, vec![0; 8]);
        search.search_complete.store(true, Ordering::Release);
        search.push_undo_state(search.collect_results());
        let output = settle(&mut app, &ctx, size);
        assert_single_line_visible(&output, &fl!(LANGUAGE_LOADER, "snapshot-ready-label"), size);
        for label in [
            fl!(LANGUAGE_LOADER, "decreased-button"),
            fl!(LANGUAGE_LOADER, "increased-button"),
            fl!(LANGUAGE_LOADER, "changed-button"),
            fl!(LANGUAGE_LOADER, "unchanged-button"),
            fl!(LANGUAGE_LOADER, "undo-button"),
            fl!(LANGUAGE_LOADER, "clear-button"),
        ] {
            assert_button_visible(&output, &label, size);
        }
        assert_eq!(app.state.searches[0].searching, SearchMode::None);
    }
}

#[test]
fn empty_search_button_is_disabled_and_click_does_not_dispatch() {
    let mut app = app_with_results(0);
    let ctx = context();
    let label = fl!(LANGUAGE_LOADER, "initial-search-button");
    let output = settle(&mut app, &ctx, LARGE);
    let (_, disabled_fill, _) = button_frame(&output, &label);
    click(&mut app, &ctx, LARGE, text_rect(&output, &label).center());

    let search = &app.state.searches[0];
    assert_eq!(search.searching, SearchMode::None);
    assert!(!search.search_complete.load(Ordering::Acquire));
    assert_eq!(search.get_result_count(), 0);
    assert!(search.old_results.is_empty());
    assert!(search.results_receiver.is_empty());
    assert_eq!(app.state.pid, 0);

    // No Response is exposed by the view: verify disabled painting against
    // the same button with valid input, but NEVER click the enabled button.
    app.state.searches[0].search_value_text = "42".to_owned();
    frame(&mut app, &ctx, LARGE, vec![Event::PointerGone]);
    let output = settle(&mut app, &ctx, LARGE);
    let (_, enabled_fill, _) = button_frame(&output, &label);
    assert_ne!(disabled_fill, enabled_fill, "Empty input must visibly disable the primary button");
    assert_eq!(app.state.searches[0].searching, SearchMode::None);
}

#[test]
fn byte_can_be_selected_from_the_actual_type_dropdown_without_searching() {
    let mut app = app_with_results(0);
    let ctx = context();
    let output = settle(&mut app, &ctx, LARGE);
    click(&mut app, &ctx, LARGE, text_rect(&output, "Int32").center());
    assert!(egui::Popup::is_any_open(&ctx));
    let output = settle(&mut app, &ctx, LARGE);
    click(&mut app, &ctx, LARGE, text_rect(&output, &SearchType::Byte.get_description_text()).center());
    let output = settle(&mut app, &ctx, LARGE);

    assert_eq!(app.state.searches[0].search_type, SearchType::Byte);
    assert_single_line_visible(&output, "UInt8", LARGE);
    assert!(!egui::Popup::is_any_open(&ctx));
    assert_eq!(app.state.searches[0].searching, SearchMode::None);
    assert_eq!(app.state.searches[0].get_result_count(), 0);
    assert!(!app.state.searches[0].search_complete.load(Ordering::Acquire));
    assert_eq!(app.state.pid, 0);
}

#[test]
fn address_click_then_down_then_delete_targets_selection_not_hover() {
    let mut app = app_with_results(3);
    let ctx = context();
    click_address(&mut app, &ctx, 0x1000);
    assert_eq!(identity(app.selected_result), Some((0x1000, SearchType::Int)));
    assert_eq!(app.app_state, AppState::InProcess);
    assert!(app.memory_editor_result_index.is_none());

    press_key(&mut app, &ctx, Key::ArrowDown);
    assert_eq!(identity(app.selected_result), Some((0x2000, SearchType::Int)));
    let output = settle(&mut app, &ctx, LARGE);
    frame(&mut app, &ctx, LARGE, vec![Event::PointerMoved(text_rect(&output, "0x3000").center())]);
    assert_eq!(app.hovered_result_row, Some(2));
    press_key(&mut app, &ctx, Key::Delete);

    assert_eq!(result_identities(&app), vec![(0x1000, SearchType::Int), (0x3000, SearchType::Int)]);
    assert_eq!(identity(app.selected_result), Some((0x3000, SearchType::Int)));
    assert!(app.editing_result.is_none());
    assert_eq!(app.state.pid, 0);
}

#[test]
fn delete_without_selection_does_not_remove_the_hovered_row() {
    let mut app = app_with_results(3);
    let ctx = context();
    let output = settle(&mut app, &ctx, LARGE);
    let before = result_identities(&app);
    frame(&mut app, &ctx, LARGE, vec![Event::PointerMoved(text_rect(&output, "0x2000").center())]);
    assert_eq!(app.hovered_result_row, Some(1));
    assert!(app.selected_result.is_none());

    press_key(&mut app, &ctx, Key::Delete);

    assert_eq!(result_identities(&app), before);
    assert!(app.selected_result.is_none());
    assert!(app.state.searches[0].old_results.is_empty());
}

#[test]
fn focused_search_text_edit_keeps_down_and_delete_away_from_results() {
    let mut app = app_with_results(3);
    let ctx = context();
    click_address(&mut app, &ctx, 0x1000);
    let table_focus = ctx.memory(|memory| memory.focused());
    let output = settle(&mut app, &ctx, LARGE);
    click(&mut app, &ctx, LARGE, text_rect(&output, "12345").center());
    let input_focus = ctx.memory(|memory| memory.focused());
    assert!(input_focus.is_some());
    assert_ne!(input_focus, table_focus, "The click must focus the search TextEdit");
    let before = result_identities(&app);
    frame(&mut app, &ctx, LARGE, vec![Event::PointerMoved(text_rect(&output, "0x3000").center())]);

    press_key(&mut app, &ctx, Key::ArrowDown);
    assert_eq!(identity(app.selected_result), Some((0x1000, SearchType::Int)));
    assert_eq!(ctx.memory(|memory| memory.focused()), input_focus);
    press_key(&mut app, &ctx, Key::Home);
    press_key(&mut app, &ctx, Key::Delete);

    assert_eq!(app.state.searches[0].search_value_text, "2345", "Delete belongs to the text cursor");
    assert_eq!(result_identities(&app), before);
    assert_eq!(identity(app.selected_result), Some((0x1000, SearchType::Int)));
    assert_eq!(ctx.memory(|memory| memory.focused()), input_focus);
    assert!(app.state.searches[0].old_results.is_empty());
    assert_eq!(app.state.searches[0].searching, SearchMode::None);
}

#[test]
fn streamed_results_preserve_address_and_type_selection_and_navigation() {
    let mut app = app_with_results(3);
    app.state.searches[0].search_type = SearchType::Guess;
    let ctx = context();
    click_address(&mut app, &ctx, 0x2000);
    assert_eq!(identity(app.selected_result), Some((0x2000, SearchType::Int)));
    // Reverse arrival order and a second type at the selected address move
    // the selected row's index without changing its identity.
    app.state.searches[0]
        .results_sender
        .send(vec![SearchResult::new(0x2000, SearchType::Byte), SearchResult::new(0x0800, SearchType::Int)])
        .unwrap();
    settle(&mut app, &ctx, LARGE);

    assert_eq!(identity(app.selected_result), Some((0x2000, SearchType::Int)));
    assert_eq!(result_identities(&app)[3], (0x2000, SearchType::Int));
    press_key(&mut app, &ctx, Key::ArrowDown);
    assert_eq!(
        identity(app.selected_result),
        Some((0x3000, SearchType::Int)),
        "Down must start at the new index, not the old one"
    );
}

#[test]
fn vanished_selection_is_cleared_even_if_the_address_survives_with_another_type() {
    for empty in [false, true] {
        let mut app = app_with_results(3);
        let ctx = context();
        click_address(&mut app, &ctx, 0x2000);
        frame(&mut app, &ctx, LARGE, vec![Event::PointerGone]);
        app.result_selection_request_scroll = true;
        app.result_edit_request_focus = true;
        app.state.searches[0].set_cached_results(if empty { vec![] } else { vec![SearchResult::new(0x2000, SearchType::Byte)] });

        settle(&mut app, &ctx, LARGE);

        assert!(app.selected_result.is_none(), "Stale address+type selection must be cleared (empty={empty})");
        assert!(app.editing_result.is_none());
        assert!(!app.result_selection_request_scroll);
        assert!(!app.result_edit_request_focus);
        assert!(app.hovered_result_row.is_none());
    }
}

#[test]
fn unreadable_rows_show_compact_type_live_editing_and_only_matching_frozen_count() {
    let mut app = app_with_results(3);
    // Metadata only: do not call toggle_freeze or send freeze-worker messages.
    app.state.searches[0].freezed_addresses.extend([0x1000, 0x3000, 0xDEAD]);
    let ctx = context();
    let output = settle(&mut app, &ctx, LARGE);

    assert_single_line_visible(&output, "Int32", LARGE);
    assert_single_line_visible(&output, &fl!(LANGUAGE_LOADER, "result-live-edit-label"), LARGE);
    assert_single_line_visible(&output, &format!("2 {}", fl!(LANGUAGE_LOADER, "result-frozen-count")), LARGE);
    let unreadable = fl!(LANGUAGE_LOADER, "result-unreadable");
    for address in [0x1000, 0x2000, 0x3000] {
        let address_rect = text_rect(&output, &format!("0x{address:X}"));
        assert!(
            shapes(&output).into_iter().any(|(clip, shape)| match shape {
                Shape::Text(text) if text.galley.job.text == unreadable => {
                    let rect = text.galley.rect.translate(text.pos.to_vec2());
                    (rect.center().y - address_rect.center().y).abs() < 8.0 && clip.contains_rect(rect)
                }
                _ => false,
            }),
            "Missing visible unreadable value for address {address:X}"
        );
    }
    assert!(app.editing_result.is_none());
    assert_eq!(app.state.pid, 0);
}

#[test]
fn f2_on_an_unreadable_selected_row_does_not_start_editing_or_rename_the_tab() {
    let mut app = app_with_results(3);
    let ctx = context();
    click_address(&mut app, &ctx, 0x1000);
    let before = result_identities(&app);

    press_key(&mut app, &ctx, Key::F2);

    assert_eq!(identity(app.selected_result), Some((0x1000, SearchType::Int)));
    assert!(app.editing_result.is_none());
    assert!(!app.result_edit_request_focus);
    assert!(app.renaming_search_index.is_none());
    assert_eq!(result_identities(&app), before);
    assert_eq!(app.app_state, AppState::InProcess);
}

#[test]
fn freeze_buttons_are_frameless_for_both_frozen_and_unfrozen_rows() {
    let mut app = app_with_results(2);
    app.state.searches[0].freezed_addresses.insert(0x1000);
    let ctx = context();
    let output = settle(&mut app, &ctx, LARGE);
    for address in [0x1000, 0x2000] {
        let row_y = text_rect(&output, &format!("0x{address:X}")).center().y;
        let glyph_rect = shapes(&output)
            .into_iter()
            .find_map(|(_, shape)| match shape {
                Shape::Text(text) if text.galley.job.text == "❄" => {
                    let rect = text.galley.rect.translate(text.pos.to_vec2());
                    ((rect.center().y - row_y).abs() < 8.0).then_some(rect)
                }
                _ => None,
            })
            .expect("Freeze icon must be visible in each row");
        let frames: Vec<_> = shapes(&output)
            .into_iter()
            .filter_map(|(_, shape)| match shape {
                Shape::Rect(rect) if rect.rect.contains_rect(glyph_rect) && rect.stroke.width > 0.0 && rect.rect.height() < 32.0 => Some(rect.rect),
                _ => None,
            })
            .collect();
        assert!(frames.is_empty(), "Freeze buttons must never have a frame: {address:X}");
    }
}

#[test]
fn result_actions_only_appear_on_cell_hover_even_when_selected() {
    for selected in [false, true] {
        let mut app = app_with_results(1);
        if selected {
            app.selected_result = Some(SearchResult::new(0x1000, SearchType::Int));
        }
        let ctx = context();
        let output = settle(&mut app, &ctx, LARGE);
        let address_rect = text_rect(&output, "0x1000");
        let value_rect = text_rect(&output, &fl!(LANGUAGE_LOADER, "result-unreadable"));
        let icons = |output: &FullOutput| -> Vec<String> {
            shapes(output)
                .into_iter()
                .filter_map(|(_, shape)| match shape {
                    Shape::Text(text)
                        if matches!(text.galley.job.text.as_str(), "×" | "…")
                            && (text.galley.rect.translate(text.pos.to_vec2()).center().y - address_rect.center().y).abs() < 16.0 =>
                    {
                        Some(text.galley.job.text.clone())
                    }
                    _ => None,
                })
                .collect()
        };
        assert!(icons(&output).is_empty(), "Selection alone must not reveal actions");
        let output = frame(&mut app, &ctx, LARGE, vec![Event::PointerMoved(address_rect.center())]);
        assert_eq!(icons(&output), vec!["×"], "Address hover reveals only Remove");
        let output = frame(&mut app, &ctx, LARGE, vec![Event::PointerMoved(value_rect.center())]);
        assert_eq!(icons(&output), vec!["…"], "Value hover reveals only the memory editor action");
        let output = frame(&mut app, &ctx, LARGE, vec![Event::PointerGone]);
        assert!(icons(&output).is_empty(), "Leaving the row must hide actions again");
        assert_eq!(app.selected_result.is_some(), selected);
    }
}

#[cfg(target_os = "linux")]
fn own_value_app(value: &mut i32) -> App {
    let mut app = app_with_results(1);
    app.state.pid = std::process::id() as _;
    app.state.searches[0].set_cached_results(vec![SearchResult::new(value as *mut i32 as usize, SearchType::Int)]);
    app
}

#[cfg(target_os = "linux")]
#[test]
fn opt_in_confirmation_buffers_edits_until_enter_for_click_and_f2() {
    for via_f2 in [false, true] {
        let mut value = Box::new(24680_i32);
        let mut app = own_value_app(&mut value);
        assert!(!app.confirm_value_writes);
        app.confirm_value_writes = true;
        let ctx = context();
        let output = settle(&mut app, &ctx, LARGE);
        text_rect(&output, &fl!(LANGUAGE_LOADER, "result-confirm-edit-label"));
        if via_f2 {
            click_address(&mut app, &ctx, &*value as *const i32 as usize);
            press_key(&mut app, &ctx, Key::F2);
        } else {
            click(&mut app, &ctx, LARGE, text_rect(&output, "24680").center());
        }
        press_key(&mut app, &ctx, Key::Home);
        press_key(&mut app, &ctx, Key::Delete);
        frame(&mut app, &ctx, LARGE, vec![Event::Text("3".to_owned())]);
        assert_eq!(app.editing_result.as_ref().unwrap().1, "34680");
        assert_eq!(*value, 24680);
        press_key(&mut app, &ctx, Key::Enter);
        assert_eq!(*value, 34680);
        assert!(app.editing_result.is_none());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn opt_in_confirmation_escape_and_focus_loss_discard_without_writing() {
    for escape in [true, false] {
        let mut value = Box::new(24680_i32);
        let mut app = own_value_app(&mut value);
        app.confirm_value_writes = true;
        let ctx = context();
        click_address(&mut app, &ctx, &*value as *const i32 as usize);
        press_key(&mut app, &ctx, Key::F2);
        press_key(&mut app, &ctx, Key::Home);
        press_key(&mut app, &ctx, Key::Delete);
        assert_eq!(*value, 24680);
        if escape {
            press_key(&mut app, &ctx, Key::Escape);
        } else {
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("search_value_input")));
            settle(&mut app, &ctx, LARGE);
        }
        assert!(app.editing_result.is_none());
        assert_eq!(*value, 24680);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn opt_in_invalid_enter_preserves_input_and_focus_for_correction() {
    let mut value = Box::new(24680_i32);
    let mut app = own_value_app(&mut value);
    app.confirm_value_writes = true;
    let ctx = context();
    let address = &*value as *const i32 as usize;
    click_address(&mut app, &ctx, address);
    press_key(&mut app, &ctx, Key::F2);
    press_key(&mut app, &ctx, Key::End);
    frame(&mut app, &ctx, LARGE, vec![Event::Text("x".to_owned())]);
    press_key(&mut app, &ctx, Key::Enter);
    settle(&mut app, &ctx, LARGE);
    assert_eq!(*value, 24680);
    assert_eq!(app.editing_result.as_ref().unwrap().1, "24680x");
    assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(("result-value", address, SearchType::Int)))));
    assert!(app.state.current_error().is_some());
    press_key(&mut app, &ctx, Key::Backspace);
    press_key(&mut app, &ctx, Key::Backspace);
    assert_eq!(*value, 24680);
    press_key(&mut app, &ctx, Key::Enter);
    assert_eq!(*value, 2468);
    assert!(app.editing_result.is_none());
}

#[test]
fn settings_show_the_opt_in_confirmation_option() {
    let mut app = App::default();
    assert!(!app.confirm_value_writes);
    let ctx = context();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, LARGE)),
            ..Default::default()
        },
        |ui| game_cheetah::ui::main_window::view_settings(&mut app, ui),
    );
    output.textures_delta.clear();
    text_rect(&output, &fl!(LANGUAGE_LOADER, "confirm-value-writes-label"));
    assert!(!app.confirm_value_writes);
}

#[cfg(target_os = "linux")]
#[test]
fn f2_edits_only_the_selected_value_and_escape_keeps_documented_live_writes() {
    let mut value = Box::new(24680_i32);
    let mut app = own_value_app(&mut value);
    let ctx = context();
    let address = (&*value) as *const i32 as usize;
    click_address(&mut app, &ctx, address);
    press_key(&mut app, &ctx, Key::F2);
    assert!(app.editing_result.is_some());
    assert!(app.renaming_search_index.is_none());
    assert!(!app.result_edit_request_focus);

    press_key(&mut app, &ctx, Key::Home);
    press_key(&mut app, &ctx, Key::Delete);
    assert_eq!(*value, 4680, "Delete must edit the value, not remove its row");
    assert_eq!(app.state.searches[0].get_result_count(), 1);
    frame(&mut app, &ctx, LARGE, vec![Event::Text("3".to_owned())]);
    assert_eq!(*value, 34680);
    press_key(&mut app, &ctx, Key::Escape);
    assert!(app.editing_result.is_none());
    assert_eq!(*value, 34680, "Escape ends live editing; it does not roll back previous writes");
    assert_eq!(identity(app.selected_result), Some((address, SearchType::Int)));
}

#[cfg(target_os = "linux")]
#[test]
fn clicking_a_borderless_value_keeps_text_focus_and_enter_finishes_editing() {
    let mut value = Box::new(24680_i32);
    let mut app = own_value_app(&mut value);
    let ctx = context();
    let output = settle(&mut app, &ctx, LARGE);
    click(&mut app, &ctx, LARGE, text_rect(&output, "24680").center());
    assert!(app.editing_result.is_some());
    assert_eq!(identity(app.selected_result), Some(((&*value) as *const i32 as usize, SearchType::Int)));
    press_key(&mut app, &ctx, Key::Home);
    press_key(&mut app, &ctx, Key::Delete);
    assert_eq!(*value, 4680);
    press_key(&mut app, &ctx, Key::Enter);
    assert!(app.editing_result.is_none());
    assert_eq!(*value, 4680);
    assert_eq!(app.state.searches[0].searching, SearchMode::None);
}

#[cfg(target_os = "linux")]
#[test]
fn selected_changed_values_keep_selection_foreground_and_marker_outside_text() {
    let mut value = Box::new(24680_i32);
    let mut app = own_value_app(&mut value);
    let address = (&*value) as *const i32 as usize;
    app.state.searches[0].search_type = SearchType::Guess;
    app.selected_result = Some(SearchResult::new(address, SearchType::Int));
    // Metadata only: no writes from the freeze worker.
    app.state.searches[0].freezed_addresses.insert(address);
    app.changed_addresses.insert(address, std::time::Instant::now());
    let ctx = context();
    let selected_color = egui::Color32::from_rgb(245, 230, 150);
    ctx.style_mut_of(egui::Theme::Dark, |style| style.visuals.selection.stroke.color = selected_color);
    let output = settle(&mut app, &ctx, LARGE);
    let value_rect = text_rect(&output, "24680");
    for label in [format!("0x{address:X}"), "24680".to_owned(), "Int32".to_owned(), "❄".to_owned()] {
        let text = shapes(&output)
            .into_iter()
            .find_map(|(_, shape)| match shape {
                Shape::Text(text)
                    if text.galley.job.text == label && (text.galley.rect.translate(text.pos.to_vec2()).center().y - value_rect.center().y).abs() < 16.0 =>
                {
                    Some(text)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("Missing selected row label {label}"));
        for section in &text.galley.job.sections {
            let color = text.override_text_color.unwrap_or(if section.format.color == egui::Color32::PLACEHOLDER {
                text.fallback_color
            } else {
                section.format.color
            });
            assert_eq!(color, selected_color, "Selected row foreground: {label}");
        }
    }
    let markers: Vec<_> = shapes(&output)
        .into_iter()
        .filter_map(|(_, shape)| match shape {
            Shape::Rect(rect) if rect.rect.width() <= 2.1 && rect.fill.a() > 0 && (rect.rect.center().y - value_rect.center().y).abs() < 3.0 => Some(rect.rect),
            _ => None,
        })
        .collect();
    assert!(!markers.is_empty(), "A changed value must have a visible marker");
    assert!(markers.iter().all(|rect| !rect.intersects(value_rect)), "Change markers must not cover text");
}
