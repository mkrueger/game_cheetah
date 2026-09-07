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
fn test_persistence_defaults_off_and_ui_actions_are_noops() {
    let mut app = create_test_app();
    assert!(!app.enable_persistence);
    seed_result_interaction(&mut app);
    let spec = game_cheetah::AddressSpec::Module {
        module: "__missing_test_game__.so".into(),
        offset: "0x20".into(),
    };
    app.state.searches[0].address_overrides.insert((0x2000, SearchType::Int), spec.clone());
    app.state.searches[0].unresolved_addresses.push(game_cheetah::PendingAddress {
        address: spec.clone(),
        search_type: SearchType::Int,
        reason: "Unchanged pending entry".into(),
    });
    app.cheat_table_status = "Unchanged status".into();
    let status_at = std::time::Instant::now();
    app.cheat_table_status_at = Some(status_at);
    app.value_change_tracker.put(0x2000, vec![42]);
    let disabled = i18n_embed_fl::fl!(game_cheetah::LANGUAGE_LOADER, "persistence-disabled");

    // Default-off guards must return before process access or any file/settings I/O.
    app.save_cheat_table();
    app.load_cheat_table();
    app.begin_address_edit(0);
    app.begin_pending_address_edit(0);
    assert_eq!(app.apply_address_definition(game_cheetah::AddressSpec::absolute(0x3000)), Err(disabled.clone()));
    app.retry_module_addresses();
    app.begin_pointer_scan(0);
    for filter in [false, true] {
        assert_eq!(app.start_pointer_scan(filter), Err(disabled.clone()));
    }
    assert_eq!(app.adopt_pointer_candidate(0), Err(disabled));
    app.poll_pointer_scan();
    app.poll_automatic_save();

    assert_seeded_result_interaction(&app);
    assert!(app.address_editor.is_none());
    assert!(!app.pointer_scanner.open);
    assert!(app.pointer_scanner.target.is_none());
    assert!(app.pointer_scanner.job.is_none());
    assert!(app.pointer_scanner.candidates.is_none());
    assert!(app.pointer_scanner.error.is_none());
    assert!(!app.is_saving_cheat_table());
    assert_eq!(app.cheat_table_status, "Unchanged status");
    assert_eq!(app.cheat_table_status_at, Some(status_at));
    assert_eq!(app.value_change_tracker.put(0x2000, vec![42]), Some(vec![42]));
    assert!(app.state.current_error().is_none());
    let search = &app.state.searches[0];
    assert_eq!(search.get_result_count(), 1);
    assert_eq!(search.address_overrides[&(0x2000, SearchType::Int)], spec);
    assert_eq!(search.unresolved_addresses.len(), 1);
    assert_eq!(search.unresolved_addresses[0].address, spec);
    assert_eq!(search.unresolved_addresses[0].reason, "Unchanged pending entry");
    assert!(search.old_results.is_empty());
}

#[test]
fn test_disabling_persistence_clears_affected_tabs_and_dialogs_but_preserves_normal_tab_and_values() {
    use std::sync::atomic::Ordering;

    use game_cheetah::{
        AddressSpec, PendingAddress, PointerWidth,
        pointer_scan::{CandidateSet, ProcessIdentity},
        ui::pointer_scanner::ScanTarget,
    };

    for screen in [AppState::InProcess, AppState::MemoryEditor] {
        let values = Box::new([17_i32, 42, 99, 123]);
        let results: Vec<_> = values
            .iter()
            .map(|value| SearchResult::new(value as *const i32 as usize, SearchType::Int))
            .collect();
        let mut app = create_test_app();
        // This setter deliberately does not persist the user's Settings.
        app.set_persistence_enabled(true);
        assert!(app.enable_persistence);
        // Intercept release messages; no freeze worker ever writes test memory.
        let (tx, rx) = crossbeam_channel::unbounded();
        app.state.freeze_sender = tx;
        let normal = &mut app.state.searches[0];
        normal.description = "Normal search".into();
        normal.search_type = SearchType::Int;
        normal.search_value_text = "17".into();
        normal.numeric_filter_lower = "10".into();
        normal.set_cached_results(vec![results[0]]);
        normal.freezed_addresses.insert(results[0].addr);
        normal.search_complete.store(true, Ordering::Release);
        normal.store_memory_snapshot(results[0].addr, vec![17, 0, 0, 0]);
        normal.push_undo_state(normal.collect_results());

        let module = AddressSpec::Module {
            module: "__missing_test_game__.so".into(),
            offset: "0x20".into(),
        };
        let pointer = AddressSpec::Pointer {
            module: "__missing_test_game__.so".into(),
            offset: "0x40".into(),
            offsets: vec!["0x10".into()],
            pointer_width: PointerWidth::Bits64,
        };
        for (index, spec) in [module.clone(), pointer.clone(), module].into_iter().enumerate() {
            app.state.new_search();
            let result = results[index + 1];
            let search = &mut app.state.searches[index + 1];
            search.set_cached_results(vec![result]);
            if index < 2 {
                search.address_overrides.insert((result.addr, result.search_type), spec);
            } else {
                // A pending-only definition must also mark the entire tab affected.
                search.unresolved_addresses.push(PendingAddress {
                    address: spec,
                    search_type: SearchType::Int,
                    reason: "Missing module".into(),
                });
            }
            search.freezed_addresses.insert(result.addr);
            search.freezed_addresses.insert(results[0].addr);
            search.search_complete.store(true, Ordering::Release);
            search.store_memory_snapshot(result.addr, vec![42, 0, 0, 0]);
            search.push_undo_state(search.collect_results());
            search.results_sender.send(vec![SearchResult::new(0x9000 + index, SearchType::Int)]).unwrap();
        }
        app.state.current_search = 0;
        app.begin_address_edit(0);
        assert!(app.address_editor.is_some());
        app.pointer_scanner.open = true;
        app.pointer_scanner.target = Some(ScanTarget {
            identity: ProcessIdentity {
                pid: 0,
                start_time: 0,
                executable: "/test/game".into(),
            },
            result: results[0],
        });
        app.pointer_scanner.candidates = Some(CandidateSet {
            version: 1,
            executable: "/test/game".into(),
            value_type: SearchType::Int,
            options: Default::default(),
            candidates: vec![pointer],
            limited: true,
        });
        app.pointer_scanner.error = Some("Old scanner error".into());
        app.pointer_scanner.status = "Old scanner status".into();
        app.cheat_table_status = "Old save notice".into();
        app.cheat_table_status_at = Some(std::time::Instant::now());
        app.selected_result = Some(results[0]);
        app.editing_result = Some((0, "999".into()));
        app.value_change_tracker.put(results[0].addr, vec![17]);
        app.changed_addresses.insert(results[0].addr, std::time::Instant::now());
        app.app_state = screen;
        if screen == AppState::MemoryEditor {
            app.memory_editor_result_index = Some(0);
        }

        app.set_persistence_enabled(false);

        assert!(!app.enable_persistence);
        assert_eq!(app.state.searches.len(), 4);
        assert_eq!(app.state.current_search, 0);
        assert_eq!(app.app_state, AppState::InProcess);
        assert!(app.memory_editor_result_index.is_none());
        assert!(app.address_editor.is_none());
        assert!(!app.pointer_scanner.open);
        assert!(app.pointer_scanner.target.is_none());
        assert!(app.pointer_scanner.candidates.is_none());
        assert!(app.pointer_scanner.job.is_none());
        assert!(app.pointer_scanner.error.is_none());
        assert!(app.pointer_scanner.status.is_empty());
        assert!(app.cheat_table_status.is_empty());
        assert!(app.cheat_table_status_at.is_none());
        assert_result_interaction_cleared(&app);
        assert!(app.value_change_tracker.is_empty());
        assert!(app.changed_addresses.is_empty());
        // One release per exclusively owned address, none for the shared normal freeze.
        assert_eq!(rx.try_iter().count(), 3);
        for search in &mut app.state.searches[1..] {
            assert!(search.address_overrides.is_empty());
            assert!(search.unresolved_addresses.is_empty());
            assert!(search.freezed_addresses.is_empty());
            assert!(search.old_results.is_empty());
            assert!(search.memory_snapshot.read().unwrap().is_empty());
            assert!(!search.search_complete.load(Ordering::Acquire));
            assert_eq!(search.searching, SearchMode::None);
            assert_eq!(search.get_result_count(), 0);
            search.undo_last_search();
            assert_eq!(search.get_result_count(), 0, "Undo must not resurrect persisted addresses");
        }
        let normal = &app.state.searches[0];
        assert_eq!(normal.description, "Normal search");
        assert_eq!(normal.search_value_text, "17");
        assert_eq!(normal.search_type, SearchType::Int);
        assert_eq!(normal.numeric_filter_lower, "10");
        assert_eq!(normal.get_result_count(), 1);
        assert_eq!(
            result_identity(normal.collect_results().first().copied()),
            Some((results[0].addr, SearchType::Int))
        );
        assert_eq!(normal.old_results.len(), 1);
        assert_eq!(normal.memory_snapshot.read().unwrap().len(), 1);
        assert!(normal.search_complete.load(Ordering::Acquire));
        assert_eq!(normal.freezed_addresses.len(), 1);
        assert!(normal.freezed_addresses.contains(&results[0].addr));
        assert_eq!(*values, [17, 42, 99, 123]);
        app.set_persistence_enabled(true);
        assert!(app.state.searches[1..].iter().all(|search| search.get_result_count() == 0));
        assert!(!app.pointer_scanner.open);
        assert!(app.address_editor.is_none());
    }
}

#[test]
fn disabling_persistence_also_clears_definitions_only_present_in_undo() {
    let mut app = App::default();
    app.set_persistence_enabled(true);
    let search = &mut app.state.searches[0];
    search.set_cached_results(vec![SearchResult::new(0x1000, SearchType::Int)]);
    search
        .address_overrides
        .insert((0x1000, SearchType::Int), game_cheetah::AddressSpec::absolute(0x1000));
    search.push_undo_state(search.collect_results());
    search.address_overrides.clear();
    assert!(search.has_persistent_address_state());
    app.set_persistence_enabled(false);
    let search = &mut app.state.searches[0];
    search.undo_last_search();
    assert!(!search.has_persistent_address_state());
    assert_eq!(search.get_result_count(), 0);
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
fn freezing_without_a_process_is_rejected() {
    let mut app = create_test_app();

    let result = SearchResult::new(0x1000, SearchType::Int);
    let _ = app.state.searches[0].results_sender.send(vec![result]);

    app.toggle_freeze(0);
    assert!(!app.state.searches[0].freezed_addresses.contains(&0x1000));
    assert!(app.state.current_error().is_some());
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
