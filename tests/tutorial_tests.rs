use std::{
    path::Path,
    time::{Duration, Instant},
};

use game_cheetah::{
    App, ProcessInfo, SearchMode, SearchType, UnknownComparison,
    tutorial::{Lesson, MAX_BAR, TutorialProcess},
};

const EXE: &str = env!("CARGO_BIN_EXE_game-cheetah");

fn attach(app: &mut App, target: &TutorialProcess) {
    app.select_process(&ProcessInfo {
        pid: target.pid(),
        start_time: 0,
        executable: String::new(),
        name: "Tutorial test".into(),
        cmd: String::new(),
        user: String::new(),
        memory: 0,
        instances: Vec::new(),
    });
    assert!(app.state.current_error().is_none(), "{:?}", app.state.current_error());
}

fn assert_reaped(pid: process_memory::Pid) {
    let system = sysinfo::System::new_all();
    assert!(
        system.process(sysinfo::Pid::from_u32(pid.try_into().unwrap())).is_none(),
        "Tutorial child must be terminated and reaped"
    );
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !condition() {
        assert!(Instant::now() < deadline, "Tutorial operation timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn finish_search(app: &mut App) {
    wait_until(|| {
        app.state.poll_searches();
        app.state.searches[0].searching == SearchMode::None
    });
    assert!(app.state.current_error().is_none(), "{:?}", app.state.current_error());
    assert!(app.state.searches[0].completed_report.is_some());
}

fn narrow_to_current_health(app: &mut App, target: &TutorialProcess) -> Vec<usize> {
    app.state.searches[0].search_value_text = target.health().unwrap().to_string();
    app.start_search();
    finish_search(app);
    app.state.searches[0].collect_results().iter().map(|result| result.addr).collect()
}

#[test]
#[cfg_attr(target_os = "macos", ignore = "reading another process needs root or a debugger entitlement on macOS")]
fn practice_game_needs_repeated_narrowing_and_supports_edit_and_freeze() {
    let mut target = TutorialProcess::spawn(Path::new(EXE), Lesson::KnownValue).unwrap();
    let pid = target.pid();
    assert_eq!(target.health().unwrap(), 100.0);
    let mut app = App::default();
    attach(&mut app, &target);
    app.state.searches[0].search_type = SearchType::Int;
    let initial = narrow_to_current_health(&mut app, &target);
    assert!(initial.len() >= 5, "Health, its HUD copy and three decoys must actually be discoverable");
    assert!(initial.contains(&target.health_address()));

    // One change separates health from the decoys, but not from its HUD copy.
    target.damage().unwrap();
    assert!(target.health().unwrap() < 100.0);
    assert_eq!(target.hud().unwrap(), Some(target.health().unwrap()));
    let first = narrow_to_current_health(&mut app, &target);
    assert!(first.len() < initial.len());
    assert!(first.contains(&target.health_address()) && first.contains(&target.hud_address().unwrap()));
    assert!(first.len() <= 3, "{first:x?}");

    // The lagging copy only drops out after a second change.
    target.heal().unwrap();
    assert_ne!(target.hud().unwrap(), Some(target.health().unwrap()));
    let second = narrow_to_current_health(&mut app, &target);
    assert_eq!(second, [target.health_address()], "no stale copy may survive two changes");

    let index = second.iter().position(|&addr| addr == target.health_address()).unwrap();
    assert!(app.commit_result_value(index, "999"));
    assert_eq!(target.health().unwrap(), 999.0);
    app.toggle_freeze(index);
    // Let the freeze worker consume the command before testing it.
    std::thread::sleep(Duration::from_millis(200));
    target.damage().unwrap();
    wait_until(|| target.health().unwrap() == 999.0);
    app.toggle_freeze(index);
    std::thread::sleep(Duration::from_millis(200));
    target.damage().unwrap();
    assert!((976.0..=992.0).contains(&target.health().unwrap()));
    target.heal().unwrap();
    assert!(target.health().unwrap() < 999.0, "potions never raise or reset boosted health");
    app.back_to_main_menu();
    drop(target);
    assert_reaped(pid);
}

fn compare(app: &mut App, comparison: UnknownComparison) -> Vec<(usize, SearchType)> {
    app.unknown_search(comparison);
    finish_search(app);
    app.state.searches[0]
        .collect_results()
        .iter()
        .map(|result| (result.addr, result.search_type))
        .collect()
}

fn addresses(rows: &[(usize, SearchType)]) -> usize {
    let mut addresses: Vec<_> = rows.iter().map(|&(addr, _)| addr).collect();
    addresses.dedup();
    addresses.len()
}

/// Follows the lesson's instructions literally: damage/Decreased,
/// potion/Increased and wait/Unchanged until at most two addresses remain.
#[test]
#[cfg_attr(target_os = "macos", ignore = "reading another process needs root or a debugger entitlement on macOS")]
fn health_bar_is_found_with_unknown_comparisons_and_overcharged_as_float() {
    let mut target = TutorialProcess::spawn(Path::new(EXE), Lesson::HealthBar).unwrap();
    let pid = target.pid();
    assert_eq!(target.health().unwrap(), MAX_BAR);
    assert!(target.hud_address().is_none());
    let mut app = App::default();
    attach(&mut app, &target);
    app.state.searches[0].search_type = SearchType::Unknown;
    app.start_search();
    finish_search(&mut app);
    assert_eq!(app.state.searches[0].completed_report.unwrap().matches, None, "a snapshot has no match count");

    let health = (target.health_address(), SearchType::Float);
    let mut rounds = Vec::new();
    for round in 0..12 {
        // People need a moment to click, so values that move on their own do move.
        std::thread::sleep(Duration::from_millis(300));
        let before = target.health().unwrap();
        let comparison = match round % 3 {
            0 => {
                target.damage().unwrap();
                if target.health().unwrap() > before {
                    UnknownComparison::Increased
                } else {
                    UnknownComparison::Decreased
                }
            }
            1 => {
                target.heal().unwrap();
                UnknownComparison::Increased
            }
            _ => {
                std::thread::sleep(Duration::from_millis(250));
                UnknownComparison::Unchanged
            }
        };
        let rows = compare(&mut app, comparison);
        assert!(rows.contains(&health), "round {round}: {comparison:?} lost the bar");
        rounds.push(addresses(&rows));
        if addresses(&rows) <= 2 {
            break;
        }
    }
    assert!(rounds.len() >= 3, "only \"Unchanged\" removes the values that move on their own: {rounds:?}");
    assert!(*rounds.last().unwrap() <= 2, "the lesson goal must be reachable: {rounds:?}");
    let rows = app.state.searches[0].collect_results();
    assert!(
        rows.iter().any(|row| row.addr == health.0 && row.search_type == SearchType::Int),
        "the Int reading of the same bytes survives, which the lesson explains"
    );

    // Writing 200 into the Int row produces a near-zero float.
    let int_row = rows.iter().position(|row| row.addr == health.0 && row.search_type == SearchType::Int).unwrap();
    assert!(app.commit_result_value(int_row, "200"));
    let garbage = target.health().unwrap();
    assert!(garbage > 0.0 && garbage < 1e-6, "{garbage}");
    let float_row = rows.iter().position(|row| (row.addr, row.search_type) == health).unwrap();
    assert!(app.commit_result_value(float_row, "200"));
    assert_eq!(target.health().unwrap(), 200.0);
    app.toggle_freeze(float_row);
    std::thread::sleep(Duration::from_millis(200));
    target.damage().unwrap();
    wait_until(|| target.health().unwrap() == 200.0);
    app.back_to_main_menu();
    drop(target);
    assert_reaped(pid);
}

#[test]
fn missing_practice_executable_reports_a_start_error() {
    for lesson in [Lesson::KnownValue, Lesson::HealthBar] {
        assert!(TutorialProcess::spawn(Path::new("/__missing_game_cheetah_tutorial__/game-cheetah"), lesson).is_err());
    }
}

#[test]
fn practice_target_needs_no_display_and_exits_on_control_pipe_eof() {
    let output = std::process::Command::new(EXE)
        .arg(game_cheetah::tutorial::TARGET_ARGUMENT)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let addresses = String::from_utf8(output.stdout).unwrap();
    let addresses: Vec<_> = addresses.split_whitespace().map(|text| usize::from_str_radix(text, 16).unwrap()).collect();
    assert_eq!(addresses.len(), 2);
    assert!(addresses.iter().all(|&address| address != 0));
}

mod panel {
    use super::*;
    use i18n_embed_fl::fl;

    fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
        fn collect(shape: &egui::epaint::Shape, out: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::epaint::Shape::Text(text) => out.push((text.galley.job.text.clone(), text.galley.rect.translate(text.pos.to_vec2()))),
                egui::epaint::Shape::Vec(children) => children.iter().for_each(|child| collect(child, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        output.shapes.iter().for_each(|shape| collect(&shape.shape, &mut out));
        out
    }

    struct Ui {
        ctx: egui::Context,
        app: App,
    }

    impl Ui {
        fn frame(&mut self, events: Vec<egui::Event>) -> Vec<(String, egui::Rect)> {
            self.app.state.poll_searches();
            self.app.poll_tutorial();
            let mut output = self.ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 720.0))),
                    events,
                    ..Default::default()
                },
                |ui| game_cheetah::ui::in_process_view::view_in_process(&mut self.app, ui),
            );
            output.textures_delta.clear();
            let texts = texts(&output);
            // Only the right-hand tutorial panel; the workspace is checked elsewhere.
            for (text, _) in texts.iter().filter(|(_, rect)| rect.min.x > 670.0) {
                assert!(!text.contains(['\u{2068}', '\u{2069}']), "isolation marks render as boxes: {text:?}");
            }
            texts
        }

        fn settle(&mut self) -> Vec<(String, egui::Rect)> {
            for _ in 0..3 {
                self.frame(vec![]);
            }
            self.frame(vec![])
        }

        fn shows(&mut self, needle: &str) -> bool {
            self.settle().iter().any(|(text, _)| text.contains(needle))
        }

        fn click(&mut self, label: &str) {
            let texts = self.settle();
            let rect = texts.iter().find(|(text, _)| text == label).unwrap_or_else(|| panic!("{label:?} not shown")).1;
            let pos = rect.center();
            self.frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ]);
            self.frame(vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }]);
        }

        fn health(&mut self) -> i32 {
            let prefix = fl!(game_cheetah::LANGUAGE_LOADER, "tutorial-health", health = 0, max = 0).replace(['\u{2068}', '\u{2069}'], "");
            let prefix = prefix.split('0').next().unwrap().to_owned();
            let texts = self.settle();
            let text = &texts.iter().find(|(text, _)| text.starts_with(&prefix)).expect("health bar").0;
            text[prefix.len()..].split_whitespace().next().unwrap().parse().unwrap()
        }

        fn narrow(&mut self) {
            let health = self.health();
            self.app.state.searches[0].search_value_text = health.to_string();
            self.app.start_search();
            wait_until(|| {
                self.app.state.poll_searches();
                self.app.state.searches[0].searching == SearchMode::None
            });
        }
    }

    #[test]
    #[cfg_attr(target_os = "macos", ignore = "reading another process needs root or a debugger entitlement on macOS")]
    fn panel_guides_through_intro_search_and_repeated_narrowing() {
        let ctx = egui::Context::default();
        game_cheetah::ui::theme::apply(&ctx);
        let mut app = App::default();
        app.start_tutorial_with(Path::new(EXE), Lesson::KnownValue);
        assert!(app.state.current_error().is_none(), "{:?}", app.state.current_error());
        let mut ui = Ui { ctx, app };

        assert!(ui.shows(&fl!(game_cheetah::LANGUAGE_LOADER, "tutorial-intro-text")));
        assert_eq!(ui.health(), 100);
        // The game can be played at any time, even before the first search.
        ui.click(&fl!(game_cheetah::LANGUAGE_LOADER, "tutorial-damage-button"));
        let health = ui.health();
        assert!((77..=93).contains(&health), "{health}");

        ui.click(&fl!(game_cheetah::LANGUAGE_LOADER, "tutorial-begin"));
        assert!(ui.shows(&fl!(game_cheetah::LANGUAGE_LOADER, "tutorial-search-how")));
        ui.narrow();
        let narrow_how = fl!(game_cheetah::LANGUAGE_LOADER, "tutorial-narrow-how");
        assert!(ui.shows(&narrow_how), "a successful first search starts the narrowing loop");

        // Each round: change health in the game, then narrow with the new value.
        for _ in 0..6 {
            if !ui.shows(&narrow_how) {
                break;
            }
            ui.click(&fl!(game_cheetah::LANGUAGE_LOADER, "tutorial-damage-button"));
            ui.narrow();
        }
        assert!(ui.shows(&fl!(game_cheetah::LANGUAGE_LOADER, "tutorial-edit-how")));
        assert!(ui.app.state.searches[0].get_result_count() <= 2);

        let health_address = ui.app.tutorial_health_address().unwrap();
        let row = ui.app.state.searches[0]
            .collect_results()
            .iter()
            .position(|row| row.addr == health_address)
            .unwrap();
        assert!(ui.app.commit_result_value(row, "999"));
        assert!(ui.shows(&fl!(game_cheetah::LANGUAGE_LOADER, "tutorial-freeze-how")));
        ui.app.toggle_freeze(row);
        std::thread::sleep(Duration::from_millis(200));
        ui.click(&fl!(game_cheetah::LANGUAGE_LOADER, "tutorial-damage-button"));
        let complete = fl!(game_cheetah::LANGUAGE_LOADER, "tutorial-complete-text");
        wait_until(|| ui.shows(&complete));
        let pid = ui.app.state.pid;
        ui.click(&fl!(game_cheetah::LANGUAGE_LOADER, "tutorial-next-lesson"));
        assert!(ui.shows(&fl!(game_cheetah::LANGUAGE_LOADER, "tutorial-bar-intro-text")));
        assert_reaped(pid);

        // The top bar's Close also ends the tutorial.
        ui.click(&fl!(game_cheetah::LANGUAGE_LOADER, "close-button"));
        assert!(ui.app.state.pid == 0, "ending the tutorial detaches from the practice game");
    }

    fn wait_idle(app: &mut App) {
        wait_until(|| {
            app.state.poll_searches();
            app.state.searches[0].searching == SearchMode::None
        });
    }

    #[test]
    #[cfg_attr(target_os = "macos", ignore = "reading another process needs root or a debugger entitlement on macOS")]
    fn health_bar_lesson_is_completed_through_the_panel_without_showing_the_number() {
        let ctx = egui::Context::default();
        game_cheetah::ui::theme::apply(&ctx);
        let mut app = App::default();
        app.start_tutorial_with(Path::new(EXE), Lesson::KnownValue);
        let first_pid = app.state.pid;
        let mut ui = Ui { ctx, app };
        let tr = |id: &str| game_cheetah::LANGUAGE_LOADER.get(id);

        // Switching lessons replaces the practice process.
        ui.click(&tr("tutorial-lesson-bar"));
        assert_ne!(ui.app.state.pid, first_pid);
        assert_reaped(first_pid);
        assert!(ui.shows(&tr("tutorial-bar-intro-text")));
        let number_prefix = tr("tutorial-health").split('{').next().unwrap().to_owned();
        assert!(
            !ui.settle().iter().any(|(text, rect)| rect.min.x > 670.0 && text.starts_with(&number_prefix)),
            "the health-bar lesson must not reveal the number"
        );

        ui.click(&tr("tutorial-begin"));
        assert!(ui.shows(&tr("tutorial-bar-search-how")));
        // A value search is the wrong tool for a bar.
        ui.app.state.searches[0].search_type = SearchType::Int;
        ui.app.state.searches[0].search_value_text = "100".into();
        ui.app.start_search();
        wait_idle(&mut ui.app);
        assert!(ui.shows(&tr("tutorial-hint-choose-unknown")));
        ui.app.clear_results();
        ui.app.state.searches[0].search_type = SearchType::Unknown;
        ui.app.start_search();
        wait_idle(&mut ui.app);
        assert!(ui.shows(&tr("tutorial-bar-narrow-ready")));

        // Random walks can survive one round by chance, so repeat like the lesson says.
        let edit_how = tr("tutorial-bar-edit-how");
        for _ in 0..4 {
            for (button, comparison) in [
                (Some("tutorial-damage-button"), UnknownComparison::Decreased),
                (Some("tutorial-heal-button"), UnknownComparison::Increased),
                (None, UnknownComparison::Unchanged),
            ] {
                std::thread::sleep(Duration::from_millis(300));
                if let Some(button) = button {
                    ui.click(&tr(button));
                }
                ui.app.unknown_search(comparison);
                wait_idle(&mut ui.app);
            }
            if ui.shows(&edit_how) {
                break;
            }
        }
        assert!(ui.shows(&edit_how));

        let rows = ui.app.state.searches[0].collect_results();
        let row = |kind: SearchType| rows.iter().position(|row| row.search_type == kind).unwrap();
        assert!(ui.app.commit_result_value(row(SearchType::Int), "200"));
        assert!(ui.shows(&tr("tutorial-hint-wrong-type")));
        assert!(ui.app.commit_result_value(row(SearchType::Float), "200"));
        assert!(ui.shows(&tr("tutorial-bar-freeze-how")));
        ui.app.toggle_freeze(row(SearchType::Float));
        assert!(ui.shows(&tr("tutorial-bar-test-how")));
        std::thread::sleep(Duration::from_millis(200));
        ui.click(&tr("tutorial-damage-button"));
        let complete = tr("tutorial-bar-complete-text");
        wait_until(|| ui.shows(&complete));

        ui.click(&tr("close-button"));
        assert_eq!(ui.app.state.pid, 0);
    }

    #[test]
    #[cfg_attr(target_os = "macos", ignore = "reading another process needs root or a debugger entitlement on macOS")]
    fn header_and_exit_button_fit_at_the_default_window_size() {
        let ctx = egui::Context::default();
        game_cheetah::ui::theme::apply(&ctx);
        let mut app = App::default();
        app.start_tutorial_with(Path::new(EXE), Lesson::KnownValue);
        let mut ui = Ui { ctx, app };
        let tr = |id: &str| game_cheetah::LANGUAGE_LOADER.get(id);
        let texts = ui.settle();
        let rect = |label: &str| texts.iter().find(|(text, _)| text == label).unwrap_or_else(|| panic!("{label:?} not shown")).1;

        let known = rect(&tr("tutorial-lesson-known"));
        let bar = rect(&tr("tutorial-lesson-bar"));
        assert_eq!(known.center().y, bar.center().y, "both lessons fit on one row");
        // The panel ends 14 px before the window edge; the restart icon takes 30 px.
        assert!(bar.right() <= 1100.0 - 14.0 - 30.0, "lesson tab runs into the restart icon: {bar:?}");
        assert!(!texts.iter().any(|(text, _)| *text == tr("tutorial-restart")), "restart is an icon");

        let exit = rect(&tr("tutorial-exit"));
        assert!(exit.bottom() <= 720.0 - 8.0, "exit button is cut off: {exit:?}");
        // Pinned: still visible while the lesson text is long.
        ui.click(&tr("tutorial-begin"));
        let texts = ui.settle();
        assert!(texts.iter().any(|(text, rect)| *text == tr("tutorial-exit") && rect.bottom() <= 712.0));
    }
}
