use game_cheetah::{NumericComparison as Op, NumericFilter, SearchType};

fn matches(op: Op, lower: &str, upper: &str, ty: SearchType, value: &str) -> bool {
    NumericFilter::parse(op, lower, upper).unwrap().matches(ty, &ty.from_string(value).unwrap().1)
}

#[test]
fn every_operator_works_for_every_numeric_type() {
    for ty in [
        SearchType::Byte,
        SearchType::Short,
        SearchType::Int,
        SearchType::Int64,
        SearchType::Float,
        SearchType::Double,
    ] {
        for (op, expected) in [
            (Op::Equal, vec![5]),
            (Op::NotEqual, vec![4, 6, 7]),
            (Op::Less, vec![4]),
            (Op::LessEqual, vec![4, 5]),
            (Op::Greater, vec![6, 7]),
            (Op::GreaterEqual, vec![5, 6, 7]),
            (Op::Between, vec![5, 6]),
        ] {
            let actual: Vec<_> = (4..=7).filter(|n| matches(op, "5", "6", ty, &n.to_string())).collect();
            assert_eq!(actual, expected, "{ty:?} {op:?}");
        }
    }
}

#[test]
fn integer_limits_and_fractional_bounds_are_not_rounded() {
    for value in ["-9223372036854775808", "9007199254740993", "9223372036854775807"] {
        assert!(matches(Op::Equal, value, "", SearchType::Int64, value));
    }
    assert!(!matches(Op::Equal, "9007199254740992", "", SearchType::Int64, "9007199254740993"));
    assert!(matches(Op::Greater, "9007199254740992.0", "", SearchType::Int64, "9007199254740993"));
    assert!(matches(Op::Less, "9223372036854775808.0", "", SearchType::Int64, "9223372036854775807"));
    assert!(matches(Op::Greater, "-9223372036854777856.0", "", SearchType::Int64, "-9223372036854775808"));
    assert!(matches(Op::Less, "-0.5", "", SearchType::Int, "-1"));
    assert!(matches(Op::Greater, "-0.5", "", SearchType::Int, "0"));
    assert!(matches(Op::Less, "0.5", "", SearchType::Int, "0"));
    assert!(matches(Op::Greater, "0.5", "", SearchType::Int, "1"));
    // A threshold need not fit the storage type: every byte is less than 300.
    assert!(matches(Op::Less, "300", "", SearchType::Byte, "255"));
    assert!(!matches(Op::GreaterEqual, "0", "", SearchType::Int, "-1"));
}

#[test]
fn float_comparisons_are_exact_in_storage_precision() {
    assert!(matches(Op::Equal, "0.1", "", SearchType::Float, "0.1"));
    assert!(matches(Op::Equal, "0.1", "", SearchType::Double, "0.1"));
    assert!(!matches(Op::Equal, "42", "", SearchType::Float, "42.0001"));
    assert!(matches(Op::Equal, "0", "", SearchType::Double, "-0"));
    assert!(matches(Op::Less, "9007199254740993", "", SearchType::Double, "9007199254740992"));
    assert!(matches(Op::Greater, "9223372036854775807", "", SearchType::Double, "9223372036854775808"));
    for op in Op::ALL {
        for value in ["NaN", "inf", "-inf"] {
            assert!(!matches(op, "0", "1", SearchType::Double, value));
        }
    }
}

#[test]
fn invalid_inputs_ranges_and_non_numeric_values_are_rejected() {
    for input in ["", "abc", "NaN", "inf", "1e999", "9223372036854775808", "1,5"] {
        assert!(NumericFilter::parse(Op::Equal, input, "").is_err(), "{input}");
    }
    assert!(NumericFilter::parse(Op::Between, "5", "4").is_err());
    assert!(NumericFilter::parse(Op::Between, "9007199254740993", "9007199254740992.0").is_err());
    assert!(NumericFilter::parse(Op::Between, "0", "").is_err());
    assert!(NumericFilter::parse(Op::Equal, " 42 ", "unused").is_ok());
    let filter = NumericFilter::parse(Op::NotEqual, "0", "").unwrap();
    assert!(!filter.matches(SearchType::String, b"123"));
    assert!(!filter.matches(SearchType::Int64, &[1, 2]));
}

#[test]
fn integer_overflow_explains_limits_without_changing_types() {
    for (ty, min, max) in [
        (SearchType::Byte, "0", "255"),
        (SearchType::Short, "-32768", "32767"),
        (SearchType::Int, "-2147483648", "2147483647"),
        (SearchType::Int64, "-9223372036854775808", "9223372036854775807"),
    ] {
        assert_eq!(ty.from_string(min).unwrap().0, ty);
        assert_eq!(ty.from_string(max).unwrap().0, ty);
        let above = (max.parse::<i128>().unwrap() + 1).to_string();
        let error = ty.from_string(&above).unwrap_err();
        assert!(error.contains(min) && error.contains(max), "{error}");
        assert_ne!(error, ty.from_string("not a number").unwrap_err());
    }
    assert!(SearchType::Int.from_string("1800000000").is_ok());
    assert!(SearchType::Int.from_string("3000000000").is_err());
    assert!(SearchType::Int64.from_string("3000000000").is_ok());
}

#[cfg(target_os = "linux")]
mod own_process {
    use super::*;
    use game_cheetah::{App, SearchMode, SearchResult, UnknownComparison};
    use std::{
        sync::atomic::Ordering,
        time::{Duration, Instant},
    };

    fn finish(app: &mut App) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let search = &mut app.state.searches[0];
        while !search.search_complete.load(Ordering::Acquire) {
            search.collect_results(); // Drain the bounded result channel.
            assert!(Instant::now() < deadline, "filter worker did not finish");
            std::thread::yield_now();
        }
        search.collect_results();
        search.update_search_mode();
    }

    #[test]
    fn inline_and_background_filters_read_all_hits_and_undo_without_writes() {
        for count in [12, 205_000] {
            let values: Vec<i64> = (0..count).map(|i| i as i64 - 6).collect();
            let mut app = App::default();
            app.state.pid = std::process::id() as process_memory::Pid;
            let search = &mut app.state.searches[0];
            search.search_type = SearchType::Guess;
            let original: Vec<_> = values.iter().map(|v| SearchResult::new(v as *const i64 as usize, SearchType::Int64)).collect();
            search.set_cached_results(original.clone());
            search.search_complete.store(true, Ordering::Release);
            search.numeric_comparison = Op::GreaterEqual;
            search.numeric_filter_lower = "0".to_owned();
            app.apply_numeric_filter();
            finish(&mut app);
            let search = &app.state.searches[0];
            let filtered = search.collect_results();
            assert_eq!(filtered.len(), count - 6);
            assert_eq!(filtered[0].addr, original[6].addr);
            assert_eq!(search.searching, SearchMode::None);
            assert_eq!(search.current_bytes.load(Ordering::Acquire), count);
            app.undo_search();
            assert_eq!(app.state.searches[0].get_result_count(), count);
            assert!(app.state.searches[0].old_results.is_empty());
            assert!(values.iter().enumerate().all(|(i, &v)| v == i as i64 - 6));
        }
    }

    #[test]
    fn filter_preserves_unknown_baseline_and_undo_after_zero_hits() {
        let value = Box::new(42_i32);
        let address = &*value as *const i32 as usize;
        let mut app = App::default();
        app.state.pid = std::process::id() as process_memory::Pid;
        let search = &mut app.state.searches[0];
        search.search_type = SearchType::Unknown;
        search.unknown_comparison = Some(UnknownComparison::Increased);
        search.search_complete.store(true, Ordering::Release);
        search.set_cached_results(vec![SearchResult::new(address, SearchType::Int)]);
        let mut previous = [0; 8];
        previous[..4].copy_from_slice(&40_i32.to_le_bytes());
        search.previous_unknown_values.write().unwrap().insert((address, SearchType::Int), previous);
        search.numeric_filter_lower = "100".to_owned();
        app.apply_numeric_filter();
        assert_eq!(app.state.searches[0].get_result_count(), 0);
        assert_eq!(
            app.state.searches[0].previous_unknown_values.read().unwrap()[&(address, SearchType::Int)],
            previous
        );
        app.undo_search();
        assert_eq!(app.state.searches[0].get_result_count(), 1);
        assert_eq!(app.state.searches[0].unknown_comparison, Some(UnknownComparison::Increased));
        app.unknown_search(UnknownComparison::Increased);
        finish(&mut app);
        assert_eq!(app.state.searches[0].get_result_count(), 1);
        assert_eq!(*value, 42);
    }

    #[test]
    fn type_filter_alone_retains_negative_values_and_distinguishes_same_address_types() {
        let value = Box::new(-1_i64);
        let address = &*value as *const i64 as usize;
        let mut app = App::default();
        app.state.pid = std::process::id() as process_memory::Pid;
        let search = &mut app.state.searches[0];
        search.set_cached_results(vec![SearchResult::new(address, SearchType::Int), SearchResult::new(address, SearchType::Int64)]);
        search.search_complete.store(true, Ordering::Release);
        search.numeric_filter_enabled = false;
        search.numeric_filter_lower = "invalid but disabled".to_owned();
        search.type_filter_enabled = true;
        search.filter_types = [false, false, false, true, false, false];
        app.apply_numeric_filter();
        let results = app.state.searches[0].collect_results();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].addr, address);
        assert_eq!(results[0].search_type, SearchType::Int64);
        assert_eq!(*value, -1);
        app.undo_search();
        assert_eq!(app.state.searches[0].get_result_count(), 2);
    }

    #[test]
    fn stability_observes_the_full_interval_and_is_undoable() {
        // More than the visible rows; stable filtering must read every candidate.
        let values = vec![-7_i64; 4096];
        let mut app = App::default();
        app.state.pid = std::process::id() as process_memory::Pid;
        let search = &mut app.state.searches[0];
        search.search_type = SearchType::Unknown;
        search.unknown_comparison = Some(UnknownComparison::Unchanged);
        search.set_cached_results(values.iter().map(|v| SearchResult::new(v as *const i64 as usize, SearchType::Int64)).collect());
        search.search_complete.store(true, Ordering::Release);
        search.numeric_filter_enabled = false;
        search.stable_filter_enabled = true;
        search.stable_filter_seconds = 1;
        let start = Instant::now();
        app.apply_numeric_filter();
        assert_eq!(app.state.searches[0].searching, SearchMode::Stability);
        finish(&mut app);
        assert!(start.elapsed() >= Duration::from_secs(1));
        assert_eq!(app.state.searches[0].get_result_count(), values.len());
        assert_eq!(app.state.searches[0].current_bytes.load(Ordering::Acquire), 1000);
        assert_eq!(app.state.searches[0].unknown_comparison, Some(UnknownComparison::Unchanged));
        app.undo_search();
        assert_eq!(app.state.searches[0].get_result_count(), values.len());
        assert!(app.state.searches[0].old_results.is_empty());
        assert!(values.iter().all(|&v| v == -7));
    }

    #[test]
    fn cancelling_stability_restores_results_and_isolates_worker_generations() {
        let value = Box::new(42_i64);
        let address = &*value as *const i64 as usize;
        let mut app = App::default();
        app.state.pid = std::process::id() as process_memory::Pid;
        let search = &mut app.state.searches[0];
        search.set_cached_results(vec![SearchResult::new(address, SearchType::Int64)]);
        search.search_complete.store(true, Ordering::Release);
        search.stable_filter_enabled = true;
        search.stable_filter_seconds = 30;
        app.apply_numeric_filter();
        let old_complete = app.state.searches[0].search_complete.clone();
        let old_progress = app.state.searches[0].current_bytes.clone();
        app.undo_search();
        assert_eq!(app.state.searches[0].get_result_count(), 1);
        assert_eq!(app.state.searches[0].searching, SearchMode::None);
        assert!(app.state.searches[0].old_results.is_empty());
        assert!(!std::sync::Arc::ptr_eq(&old_complete, &app.state.searches[0].search_complete));
        assert!(!std::sync::Arc::ptr_eq(&old_progress, &app.state.searches[0].current_bytes));
        app.clear_results();
        old_complete.store(true, Ordering::Release);
        old_progress.store(30000, Ordering::Release);
        assert!(!app.state.searches[0].search_complete.load(Ordering::Acquire));
        assert_eq!(app.state.searches[0].get_result_count(), 0);
    }

    #[test]
    fn invalid_and_busy_filters_preserve_results_history_and_freezes() {
        let mut app = App::default();
        app.state.pid = std::process::id() as process_memory::Pid;
        let search = &mut app.state.searches[0];
        search.set_cached_results(vec![SearchResult::new(0x1000, SearchType::Int)]);
        search.freezed_addresses.insert(0x1000);
        search.numeric_filter_lower = "invalid".to_owned();
        app.apply_numeric_filter();
        assert!(app.state.current_error().is_some());
        app.state.searches[0].numeric_filter_lower = "0".to_owned();
        app.state.searches[0].searching = SearchMode::Percent;
        app.apply_numeric_filter();
        app.state.filter_numeric_results(0, NumericFilter::parse(Op::Equal, "0", "").unwrap());
        let search = &app.state.searches[0];
        assert_eq!(search.get_result_count(), 1);
        assert!(search.old_results.is_empty());
        assert!(search.freezed_addresses.contains(&0x1000));
    }
}

#[test]
fn disabled_predicates_do_not_validate_and_empty_types_or_invalid_duration_are_rejected() {
    let mut search = game_cheetah::SearchContext::new("filter".to_owned());
    assert_eq!(search.stable_filter_seconds, 3);
    search.numeric_filter_lower = "invalid".to_owned();
    search.numeric_filter_enabled = false;
    assert!(search.result_filter().is_err());
    search.type_filter_enabled = true;
    assert!(search.result_filter().is_ok());
    search.filter_types.fill(false);
    assert!(search.result_filter().is_err());
    search.type_filter_enabled = false;
    search.stable_filter_enabled = true;
    assert!(search.result_filter().is_ok());
    for seconds in [0, 31, u32::MAX] {
        search.stable_filter_seconds = seconds;
        assert!(search.result_filter().is_err());
    }
    search.stable_filter_seconds = 30;
    assert!(search.result_filter().is_ok());
}
