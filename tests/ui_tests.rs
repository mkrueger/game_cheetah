//! UI behavior tests. These were originally written against the
//! `Message`-based update loop; with the egui port they target the
//! direct-call methods on [`App`] instead. The semantics being asserted
//! are unchanged.

use game_cheetah::{App, AppState, SearchMode, SearchResult, SearchType};

fn create_test_app() -> App {
    App::default()
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
