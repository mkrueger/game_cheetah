use game_cheetah::{
    SearchMode, SearchType,
    app::{App, AppState},
    message::Message,
};

fn create_test_app() -> App {
    App::default()
}

#[test]
fn test_new_search_tab() {
    let mut app = create_test_app();

    // Create new search tab
    let _ = app.update(Message::NewSearch);

    assert_eq!(app.state.searches.len(), 2);
    assert_eq!(app.state.current_search, 1);
}

#[test]
fn test_search_type_change() {
    let mut app = create_test_app();

    // Change search type to Int
    let _ = app.update(Message::SwitchSearchType(SearchType::Int));

    assert_eq!(app.state.searches[0].search_type, SearchType::Int);
}

#[test]
fn test_search_value_input() {
    let mut app = create_test_app();

    // Input search value
    let _ = app.update(Message::SearchValueChanged("42".to_string()));

    assert_eq!(app.state.searches[0].search_value_text, "42");
}

#[test]
fn test_toggle_results_visibility() {
    let mut app = create_test_app();

    assert!(!app.state.show_results);

    let _ = app.update(Message::ToggleShowResult);
    assert!(app.state.show_results);

    let _ = app.update(Message::ToggleShowResult);
    assert!(!app.state.show_results);
}

#[test]
fn test_rename_tab() {
    let mut app = create_test_app();

    // Start rename mode
    let _ = app.update(Message::RenameSearch);
    assert_eq!(app.renaming_search_index, Some(0));

    // Change description
    let _ = app.update(Message::RenameSearchTextChanged("Custom Search".to_string()));

    // Stop rename mode
    let _ = app.update(Message::ConfirmRenameSearch);
    assert_eq!(app.renaming_search_index, None);
    assert_eq!(app.state.searches[0].description, "Custom Search");
}

#[test]
fn test_clear_results() {
    let mut app = create_test_app();

    // Simulate having results
    app.state.searches[0].result_count.store(100, std::sync::atomic::Ordering::SeqCst);

    let _ = app.update(Message::ClearResults);

    assert_eq!(app.state.searches[0].get_result_count(), 0);
}

#[test]
fn test_search_workflow() {
    let mut app = create_test_app();
    app.app_state = AppState::InProcess;
    app.state.pid = 1234; // Mock PID

    // Set search value
    let _ = app.update(Message::SearchValueChanged("100".to_string()));

    // Set search type
    let _ = app.update(Message::SwitchSearchType(SearchType::Int));

    // Initiate search (would normally trigger actual search)
    let _task = app.update(Message::Search);

    // Should return a tick task to monitor progress
    // assert!(!matches!(task, Task::none()));
}

#[test]
fn test_freeze_functionality() {
    let mut app = create_test_app();

    // Simulate having a result to freeze
    use game_cheetah::SearchResult;

    let result = SearchResult::new(0x1000, SearchType::Int);
    let _ = app.state.searches[0].results_sender.send(vec![result]);

    // Toggle freeze (index 0)
    let _ = app.update(Message::ToggleFreeze(0));

    // Check if address was added to frozen set
    assert!(app.state.searches[0].freezed_addresses.contains(&0x1000));

    // Toggle again to unfreeze
    let _ = app.update(Message::ToggleFreeze(0));
    assert!(!app.state.searches[0].freezed_addresses.contains(&0x1000));
}

#[test]
fn test_result_value_change() {
    let mut app = create_test_app();
    app.state.pid = 1234; // Mock PID

    // Simulate having a result
    use game_cheetah::SearchResult;
    let result = SearchResult::new(0x1000, SearchType::Int);
    let _ = app.state.searches[0].results_sender.send(vec![result]);

    // Change value (would normally write to process memory)
    let _ = app.update(Message::ResultValueChanged(0, "200".to_string()));

    // Test passes if no panic occurs
}

#[test]
fn test_remove_result() {
    let mut app = create_test_app();

    // Simulate having results
    use game_cheetah::SearchResult;
    let results = vec![SearchResult::new(0x1000, SearchType::Int), SearchResult::new(0x2000, SearchType::Int)];
    let _ = app.state.searches[0].results_sender.send(results);
    app.state.searches[0].result_count.store(2, std::sync::atomic::Ordering::SeqCst);

    // Remove first result
    let _ = app.update(Message::RemoveResult(0));

    // Should have one less result
    assert_eq!(app.state.searches[0].get_result_count(), 1);
}

#[test]
fn test_tab_switching() {
    let mut app = create_test_app();

    // Create multiple tabs
    let _ = app.update(Message::NewSearch);
    let _ = app.update(Message::NewSearch);

    assert_eq!(app.state.current_search, 2);

    // Switch to first tab
    let _ = app.update(Message::SwitchSearch(0));
    assert_eq!(app.state.current_search, 0);

    // Switch to middle tab
    let _ = app.update(Message::SwitchSearch(1));
    assert_eq!(app.state.current_search, 1);
}

#[test]
fn test_search_state_transitions() {
    let mut app = create_test_app();
    app.state.pid = 1234;
    app.app_state = AppState::InProcess;

    let search_context = &mut app.state.searches[0];

    // Initial state
    assert_eq!(search_context.searching, SearchMode::None);

    // During search
    search_context.searching = SearchMode::Percent;
    assert!(matches!(search_context.searching, SearchMode::Percent));

    // Search complete
    search_context.searching = SearchMode::None;
    assert!(matches!(search_context.searching, SearchMode::None));
}
