//! UI behavior tests. These were originally written against the
//! `Message`-based update loop; with the egui port they target the
//! direct-call methods on [`App`] instead. The semantics being asserted
//! are unchanged.

use game_cheetah::{App, AppState, SearchMode, SearchResult, SearchType};

fn create_test_app() -> App {
    App::default()
}

fn result_identity(result: Option<SearchResult>) -> Option<(usize, SearchType)> {
    result.map(|result| (result.addr, result.search_type))
}

fn seed_result_interaction(app: &mut App) {
    let result = SearchResult::new(0x2000, SearchType::Int);
    app.state.searches[app.state.current_search].set_cached_results(vec![result]);
    app.selected_result = Some(result);
    app.editing_result = Some((0, "42".to_owned()));
    app.result_selection_request_scroll = true;
    app.result_edit_request_focus = true;
    app.hovered_result_row = Some(0);
}

fn assert_result_interaction_cleared(app: &App) {
    assert!(app.selected_result.is_none());
    assert!(app.editing_result.is_none());
    assert!(!app.result_selection_request_scroll);
    assert!(!app.result_edit_request_focus);
    assert!(app.hovered_result_row.is_none());
}

fn assert_seeded_result_interaction(app: &App) {
    assert_eq!(result_identity(app.selected_result), Some((0x2000, SearchType::Int)));
    assert_eq!(app.editing_result, Some((0, "42".to_owned())));
    assert!(app.result_selection_request_scroll);
    assert!(app.result_edit_request_focus);
    assert_eq!(app.hovered_result_row, Some(0));
}

#[test]
fn test_result_interaction_defaults_and_explicit_reset() {
    let mut app = create_test_app();
    assert_result_interaction_cleared(&app);
    seed_result_interaction(&mut app);

    app.clear_result_interaction();

    assert_result_interaction_cleared(&app);
    assert_eq!(app.state.searches[0].get_result_count(), 1);
}

#[test]
fn test_tab_and_reset_actions_clear_result_interaction() {
    for action in [
        "switch",
        "switch_same",
        "new",
        "close_current",
        "close_other",
        "close_others",
        "clear",
        "undo",
        "main_menu",
    ] {
        let mut app = create_test_app();
        app.new_search();
        app.new_search();
        seed_result_interaction(&mut app);

        match action {
            "switch" => app.switch_search(0),
            "switch_same" => app.switch_search(2),
            "new" => app.new_search(),
            "close_current" => app.close_search(2),
            "close_other" => app.close_search(0),
            "close_others" => app.close_other_searches(1),
            "clear" => app.clear_results(),
            "undo" => app.undo_search(),
            "main_menu" => app.back_to_main_menu(),
            _ => unreachable!(),
        }

        assert_result_interaction_cleared(&app);
    }
}

#[test]
fn test_closing_last_tab_clears_result_interaction() {
    let mut app = create_test_app();
    seed_result_interaction(&mut app);

    app.close_search(0);

    assert_result_interaction_cleared(&app);
    assert_eq!(app.state.searches.len(), 1);
    assert_eq!(app.state.current_search, 0);
    assert_eq!(app.state.searches[0].get_result_count(), 0);
}

#[test]
fn test_invalid_tab_actions_preserve_result_interaction() {
    let mut app = create_test_app();
    seed_result_interaction(&mut app);

    app.switch_search(1);
    assert_seeded_result_interaction(&app);
    app.close_search(1);
    assert_seeded_result_interaction(&app);
    app.close_other_searches(1);
    assert_seeded_result_interaction(&app);
    assert_eq!(app.state.searches.len(), 1);
    assert_eq!(app.state.current_search, 0);
}

#[test]
fn test_clear_change_tracker_preserves_result_interaction() {
    let mut app = create_test_app();
    seed_result_interaction(&mut app);
    app.value_change_tracker.put(0x2000, vec![42]);
    app.changed_addresses.insert(0x2000, std::time::Instant::now());

    app.clear_change_tracker();

    assert_seeded_result_interaction(&app);
    assert!(app.value_change_tracker.is_empty());
    assert!(app.changed_addresses.is_empty());
}

#[test]
fn test_rejected_search_preserves_result_interaction() {
    // These calls return before dispatch: no process access or worker threads.
    for text in ["", "not-an-integer", "999999999999999999999999999999"] {
        let mut app = create_test_app();
        seed_result_interaction(&mut app);
        app.state.searches[0].search_type = SearchType::Int;
        app.state.searches[0].search_value_text = text.to_owned();

        app.start_search();

        assert_seeded_result_interaction(&app);
        assert_eq!(app.state.searches[0].searching, SearchMode::None);
        assert_eq!(app.state.searches[0].get_result_count(), 1);
    }
}

#[test]
fn test_start_search_with_invalid_tab_preserves_result_interaction() {
    let mut app = create_test_app();
    seed_result_interaction(&mut app);
    app.state.current_search = app.state.searches.len();

    app.start_search();

    assert_seeded_result_interaction(&app);
}

#[test]
fn test_already_running_search_preserves_result_interaction() {
    for search_type in [SearchType::Int, SearchType::String, SearchType::Unknown] {
        for mode in [SearchMode::Memory, SearchMode::Percent] {
            let mut app = create_test_app();
            seed_result_interaction(&mut app);
            app.state.searches[0].search_type = search_type;
            app.state.searches[0].search_value_text = "42".to_owned();
            app.state.searches[0].searching = mode;

            app.start_search();

            assert_seeded_result_interaction(&app);
            assert_eq!(app.state.searches[0].searching, mode);
        }
    }
}

#[test]
fn test_removing_other_result_preserves_selected_identity_after_sorting() {
    let mut app = create_test_app();
    seed_result_interaction(&mut app);
    app.result_selection_request_scroll = false;
    // A streamed batch inserts rows before the selected identity, including
    // a different type at the same address. The old selected row index is stale.
    app.state.searches[0]
        .results_sender
        .send(vec![SearchResult::new(0x3000, SearchType::Int), SearchResult::new(0x2000, SearchType::Byte)])
        .unwrap();

    app.remove_result(0);

    assert_eq!(result_identity(app.selected_result), Some((0x2000, SearchType::Int)));
    assert!(!app.result_selection_request_scroll);
    assert!(app.editing_result.is_none());
    assert!(!app.result_edit_request_focus);
    let results = app.state.searches[0].collect_results();
    assert_eq!(results.len(), 2);
    assert_eq!(result_identity(results.first().copied()), result_identity(app.selected_result));
}

#[test]
fn test_removing_selected_result_selects_successor_or_last_result() {
    for (index, expected) in [(0, 0x2000), (1, 0x3000), (2, 0x2000)] {
        let mut app = create_test_app();
        seed_result_interaction(&mut app);
        app.state.searches[0].set_cached_results(vec![
            SearchResult::new(0x3000, SearchType::Int),
            SearchResult::new(0x1000, SearchType::Int),
            SearchResult::new(0x2000, SearchType::Int),
        ]);
        app.selected_result = Some(app.state.searches[0].collect_results()[index]);
        app.editing_result = Some((index, "42".to_owned()));
        app.result_selection_request_scroll = false;

        app.remove_result(index);

        assert_eq!(result_identity(app.selected_result), Some((expected, SearchType::Int)));
        assert!(app.result_selection_request_scroll);
        assert!(app.editing_result.is_none());
        assert!(!app.result_edit_request_focus);
        assert!(!app.search_value_request_focus);
    }
}

#[test]
fn test_removing_last_selected_result_clears_selection() {
    let mut app = create_test_app();
    seed_result_interaction(&mut app);
    app.result_selection_request_scroll = false;

    app.remove_result(0);

    assert!(app.selected_result.is_none());
    assert!(app.editing_result.is_none());
    assert!(!app.result_edit_request_focus);
    assert!(app.result_selection_request_scroll);
    assert!(app.search_value_request_focus);
    assert_eq!(app.state.searches[0].get_result_count(), 0);
}

#[test]
fn test_removing_result_without_selection_does_not_select_a_row() {
    let mut app = create_test_app();
    app.state.searches[0].set_cached_results(vec![SearchResult::new(0x1000, SearchType::Int), SearchResult::new(0x2000, SearchType::Int)]);

    app.remove_result(0);

    assert_result_interaction_cleared(&app);
    assert_eq!(app.state.searches[0].get_result_count(), 1);
}

#[test]
fn test_undo_restores_results_but_clears_result_interaction() {
    let mut app = create_test_app();
    seed_result_interaction(&mut app);
    app.state.searches[0].set_cached_results(vec![SearchResult::new(0x1000, SearchType::Int), SearchResult::new(0x2000, SearchType::Int)]);
    app.remove_result(1);
    assert_eq!(result_identity(app.selected_result), Some((0x1000, SearchType::Int)));
    app.editing_result = Some((0, "99".to_owned()));
    app.result_edit_request_focus = true;

    app.undo_search();

    assert_result_interaction_cleared(&app);
    assert_eq!(app.state.searches[0].get_result_count(), 2);
    assert_eq!(
        result_identity(app.state.searches[0].collect_results().get(1).copied()),
        Some((0x2000, SearchType::Int))
    );
}

#[test]
fn test_change_result_type_transfers_selection_after_sorting_and_deduplication() {
    // Double moves the row after Float; Float merges it with an existing hit.
    for new_type in [SearchType::Double, SearchType::Float] {
        let mut app = create_test_app();
        seed_result_interaction(&mut app);
        app.state.searches[0].set_cached_results(vec![SearchResult::new(0x2000, SearchType::Float), SearchResult::new(0x2000, SearchType::Int)]);
        // Set the index directly: opening the memory editor would read a process.
        app.memory_editor_result_index = Some(0);

        app.change_result_type(new_type);

        assert_eq!(result_identity(app.selected_result), Some((0x2000, new_type)));
        assert!(app.editing_result.is_none());
        assert!(!app.result_edit_request_focus);
        let results = app.state.searches[0].collect_results();
        let new_index = app.memory_editor_result_index.unwrap();
        assert_eq!(result_identity(results.get(new_index).copied()), result_identity(app.selected_result));
        assert_eq!(new_index, usize::from(new_type == SearchType::Double));
        assert_eq!(results.len(), if new_type == SearchType::Double { 2 } else { 1 });
    }
}

#[test]
fn test_change_other_result_type_preserves_selected_identity() {
    let mut app = create_test_app();
    seed_result_interaction(&mut app);
    app.state.searches[0].set_cached_results(vec![SearchResult::new(0x2000, SearchType::Byte), SearchResult::new(0x2000, SearchType::Int)]);
    app.memory_editor_result_index = Some(0);
    app.result_selection_request_scroll = false;

    app.change_result_type(SearchType::Double);

    assert_eq!(result_identity(app.selected_result), Some((0x2000, SearchType::Int)));
    assert!(!app.result_selection_request_scroll);
    assert!(app.editing_result.is_none());
    assert!(!app.result_edit_request_focus);
    assert_eq!(app.memory_editor_result_index, Some(1));
}

#[test]
fn test_new_search_tab() {
    let mut app = create_test_app();
    app.new_search();

    assert_eq!(app.state.searches.len(), 2);
    assert_eq!(app.state.current_search, 1);
}

#[test]
fn test_search_type_change() {
    let mut app = create_test_app();
    let idx = app.state.current_search;
    app.state.searches[idx].search_type = SearchType::Int;

    assert_eq!(app.state.searches[0].search_type, SearchType::Int);
}

#[test]
fn test_search_value_input() {
    let mut app = create_test_app();
    let idx = app.state.current_search;
    app.state.searches[idx].search_value_text = "42".to_string();

    assert_eq!(app.state.searches[0].search_value_text, "42");
}

#[test]
fn test_toggle_results_visibility() {
    let mut app = create_test_app();

    assert!(!app.state.show_results);

    app.state.show_results = !app.state.show_results;
    assert!(app.state.show_results);

    app.state.show_results = !app.state.show_results;
    assert!(!app.state.show_results);
}

#[test]
fn test_clear_results_requests_search_value_focus() {
    let mut app = create_test_app();

    app.clear_results();

    assert!(app.search_value_request_focus);
}

#[test]
fn test_settings_toggle_auto_reconnect() {
    let mut app = create_test_app();

    assert_eq!(app.app_state, AppState::MainWindow);
    assert!(!app.auto_reconnect);

    app.app_state = AppState::Settings;
    assert_eq!(app.app_state, AppState::Settings);

    app.auto_reconnect = !app.auto_reconnect;
    assert!(app.auto_reconnect);

    app.back_to_main_menu();
    assert_eq!(app.app_state, AppState::MainWindow);
    // back_to_main_menu resets engine state but preserves user settings.
    assert!(app.auto_reconnect);
}

#[test]
fn test_back_to_main_menu_clears_cheat_table_toast() {
    let mut app = create_test_app();
    app.cheat_table_status = "Saved".to_owned();
    app.cheat_table_status_at = Some(std::time::Instant::now());

    app.back_to_main_menu();

    assert!(app.cheat_table_status.is_empty());
    assert!(app.cheat_table_status_at.is_none());
}

#[test]
fn test_rename_tab() {
    let mut app = create_test_app();

    app.begin_rename_search(app.state.current_search);
    assert_eq!(app.renaming_search_index, Some(0));

    app.rename_search_text = "Custom Search".to_string();

    app.commit_rename_search();
    assert_eq!(app.renaming_search_index, None);
    assert_eq!(app.state.searches[0].description, "Custom Search");
}

#[test]
fn test_search_workflow() {
    let mut app = create_test_app();
    app.app_state = AppState::InProcess;
    app.state.pid = 1234; // Mock PID.

    let idx = app.state.current_search;
    app.state.searches[idx].search_value_text = "100".to_string();
    app.state.searches[idx].search_type = SearchType::Int;

    // Should not panic with a bogus PID — the work spawns in a worker
    // thread and the failure surfaces asynchronously.
    app.start_search();
}

#[test]
fn test_freeze_functionality() {
    let mut app = create_test_app();

    let result = SearchResult::new(0x1000, SearchType::Int);
    let _ = app.state.searches[0].results_sender.send(vec![result]);

    app.toggle_freeze(0);
    assert!(app.state.searches[0].freezed_addresses.contains(&0x1000));

    app.toggle_freeze(0);
    assert!(!app.state.searches[0].freezed_addresses.contains(&0x1000));
}

#[test]
fn test_removing_last_result_requests_search_value_focus() {
    let mut app = create_test_app();
    let _ = app.state.searches[0].results_sender.send(vec![SearchResult::new(0x1000, SearchType::Int)]);

    app.remove_result(0);

    assert_eq!(app.state.searches[0].get_result_count(), 0);
    assert!(app.search_value_request_focus);
}

#[test]
fn test_result_value_change() {
    let mut app = create_test_app();
    app.state.pid = 1234; // Mock PID.

    let result = SearchResult::new(0x1000, SearchType::Int);
    let _ = app.state.searches[0].results_sender.send(vec![result]);

    // Best-effort: with a fake PID the write will fail and an error gets
    // pushed onto the error stack. Test just ensures the path doesn't panic.
    let _ = app.commit_result_value(0, "200");
}

#[test]
fn test_tab_switching() {
    let mut app = create_test_app();

    app.new_search();
    app.new_search();
    assert_eq!(app.state.current_search, 2);

    app.switch_search(0);
    assert_eq!(app.state.current_search, 0);

    app.switch_search(1);
    assert_eq!(app.state.current_search, 1);
}

#[test]
fn test_search_state_transitions() {
    let mut app = create_test_app();
    app.state.pid = 1234;
    app.app_state = AppState::InProcess;

    let search_context = &mut app.state.searches[0];

    assert_eq!(search_context.searching, SearchMode::None);

    search_context.searching = SearchMode::Percent;
    assert!(matches!(search_context.searching, SearchMode::Percent));

    search_context.searching = SearchMode::None;
    assert!(matches!(search_context.searching, SearchMode::None));
}
