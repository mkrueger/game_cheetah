use std::{
    path::Path,
    time::{Duration, Instant},
};

use game_cheetah::{App, ProcessInfo, SearchMode, SearchType, tutorial::TutorialProcess};

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
fn practice_game_needs_repeated_narrowing_and_supports_edit_and_freeze() {
    let mut target = TutorialProcess::spawn(Path::new(env!("CARGO_BIN_EXE_game-cheetah"))).unwrap();
    let pid = target.pid();
    assert_eq!(target.health().unwrap(), game_cheetah::tutorial::MAX_HEALTH);
    let mut app = App::default();
    app.select_process(&ProcessInfo {
        pid,
        start_time: 0,
        executable: String::new(),
        name: "Tutorial test".into(),
        cmd: String::new(),
        user: String::new(),
        memory: 0,
        instances: Vec::new(),
    });
    assert!(app.state.current_error().is_none(), "{:?}", app.state.current_error());
    app.state.searches[0].search_type = SearchType::Int;
    let initial = narrow_to_current_health(&mut app, &target);
    assert!(initial.len() >= 5, "Health, its HUD copy and three decoys must actually be discoverable");
    assert!(initial.contains(&target.health_address()));

    // One change separates health from the decoys, but not from its HUD copy.
    target.damage().unwrap();
    assert!(target.health().unwrap() < 100);
    assert_eq!(target.hud().unwrap(), target.health().unwrap());
    let first = narrow_to_current_health(&mut app, &target);
    assert!(first.len() < initial.len());
    assert!(first.contains(&target.health_address()) && first.contains(&target.hud_address()));
    assert!(first.len() <= 3, "{first:x?}");

    // The lagging copy only drops out after a second change.
    target.heal().unwrap();
    assert_ne!(target.hud().unwrap(), target.health().unwrap());
    let second = narrow_to_current_health(&mut app, &target);
    assert_eq!(second, [target.health_address()], "no stale copy may survive two changes");

    let index = second.iter().position(|&addr| addr == target.health_address()).unwrap();
    assert!(app.commit_result_value(index, "999"));
    assert_eq!(target.health().unwrap(), 999);
    app.toggle_freeze(index);
    // Let the freeze worker consume the command before testing it.
    std::thread::sleep(Duration::from_millis(200));
    target.damage().unwrap();
    wait_until(|| target.health().unwrap() == 999);
    app.toggle_freeze(index);
    std::thread::sleep(Duration::from_millis(200));
    target.damage().unwrap();
    assert!((976..=992).contains(&target.health().unwrap()));
    target.heal().unwrap();
    assert!(target.health().unwrap() < 999, "potions never raise or reset boosted health");
    app.back_to_main_menu();
    drop(target);
    let system = sysinfo::System::new_all();
    assert!(
        system.process(sysinfo::Pid::from_u32(pid.try_into().unwrap())).is_none(),
        "Tutorial child must be terminated and reaped"
    );
}

#[test]
fn missing_practice_executable_reports_a_start_error() {
    assert!(TutorialProcess::spawn(Path::new("/__missing_game_cheetah_tutorial__/game-cheetah")).is_err());
}

#[test]
fn practice_target_needs_no_display_and_exits_on_control_pipe_eof() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_game-cheetah"))
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
    fn panel_guides_through_intro_search_and_repeated_narrowing() {
        let ctx = egui::Context::default();
        game_cheetah::ui::theme::apply(&ctx);
        let mut app = App::default();
        app.start_tutorial_with(Path::new(env!("CARGO_BIN_EXE_game-cheetah")));
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

        ui.click(&fl!(game_cheetah::LANGUAGE_LOADER, "tutorial-exit"));
        assert!(ui.app.state.pid == 0, "ending the tutorial detaches from the practice game");
    }
}
