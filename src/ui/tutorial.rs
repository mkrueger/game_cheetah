//! Interactive tutorial: a guided lesson against the isolated practice game.
//!
//! Progress is derived from what actually happened — completed scans in the
//! lesson's search tab and the practice game's real memory — never from
//! button clicks alone.
use std::time::{Duration, Instant};

use i18n_embed_fl::fl;

use crate::{
    App, AppError, ProcessInfo, SearchContext, SearchMode, SearchReport, SearchType,
    tutorial::{MAX_HEALTH, TutorialProcess},
};

/// The narrowing loop ends once at most this many candidates remain.
const TARGET_CANDIDATES: usize = 2;
/// Spurious matches can survive; offer to continue with a short list.
const SKIP_CANDIDATES: usize = 5;
const EDIT_VALUE: i32 = 999;
/// Longer than several 125 ms freeze-worker cycles.
const FREEZE_SETTLE: Duration = Duration::from_millis(500);
const FREEZE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
enum Step {
    #[default]
    Intro,
    Search,
    Narrow,
    Edit,
    Freeze,
    TestFreeze,
    Complete,
}

const STEPS: [Step; 6] = [Step::Intro, Step::Search, Step::Narrow, Step::Edit, Step::Freeze, Step::TestFreeze];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hint {
    SearchMissed,
    NarrowLost,
    NothingRemoved,
    TabClosed,
    CopyEdited,
    WrongFreeze,
    FreezeValue,
    FreezeFailed,
}

/// One reading of the practice game's memory.
#[derive(Clone, Copy, Debug)]
struct Sample {
    health_address: usize,
    health: i32,
    hud: i32,
}

#[derive(Debug, Default)]
struct Progress {
    step: Step,
    hint: Option<Hint>,
    /// The tab in which the first successful search happened.
    search_id: Option<u64>,
    last_report: Option<SearchReport>,
    scanned_candidates: Option<usize>,
    candidates: usize,
    target_found: bool,
    refinements: usize,
    freeze_test: Option<Instant>,
}

impl Progress {
    fn step(&self) -> Step {
        self.step
    }

    fn advance(&mut self, step: Step) {
        self.step = step;
        self.hint = None;
    }

    fn observe(&mut self, searches: &[Box<SearchContext>], current: usize, sample: Sample, now: Instant) {
        if matches!(self.step(), Step::Intro | Step::Complete) {
            return;
        }
        let search = match self.search_id {
            Some(id) => {
                let Some(search) = searches.iter().find(|search| search.view_id == id) else {
                    *self = Self {
                        step: Step::Search,
                        hint: Some(Hint::TabClosed),
                        ..Self::default()
                    };
                    return;
                };
                search
            }
            None => {
                let Some(search) = searches.get(current) else { return };
                search
            }
        };
        if search.searching != SearchMode::None {
            return;
        }
        let results = search.collect_results();
        let has_target = results
            .binary_search_by_key(&(sample.health_address, SearchType::Int as u8), |result| {
                (result.addr, result.search_type as u8)
            })
            .is_ok();
        self.candidates = results.len();
        self.target_found = has_target;
        let finished = search.search_complete.load(std::sync::atomic::Ordering::Acquire)
            && search.completed_report.is_some()
            && search.completed_report != self.last_report;
        if finished {
            self.last_report = search.completed_report;
            let previous = self.scanned_candidates.replace(results.len());
            match self.step() {
                Step::Search if has_target => {
                    self.search_id = Some(search.view_id);
                    self.refinements = 0;
                    self.advance(Step::Narrow);
                }
                Step::Search => self.hint = Some(Hint::SearchMissed),
                Step::Narrow if has_target => {
                    self.refinements += 1;
                    self.hint = (previous == Some(results.len())).then_some(Hint::NothingRemoved);
                }
                Step::Narrow => self.hint = Some(Hint::NarrowLost),
                _ => {}
            }
        }

        let frozen = search.freezed_addresses.contains(&sample.health_address);
        match self.step() {
            Step::Narrow if has_target && self.refinements > 0 && results.len() <= TARGET_CANDIDATES => self.advance(Step::Edit),
            Step::Edit if sample.health == EDIT_VALUE => self.advance(Step::Freeze),
            Step::Edit if sample.hud == EDIT_VALUE => self.hint = Some(Hint::CopyEdited),
            Step::Freeze if frozen && sample.health == EDIT_VALUE => {
                self.freeze_test = None;
                self.advance(Step::TestFreeze);
            }
            Step::Freeze if sample.health != EDIT_VALUE => self.hint = Some(Hint::FreezeValue),
            Step::Freeze if !search.freezed_addresses.is_empty() => self.hint = Some(Hint::WrongFreeze),
            Step::Freeze => self.hint = None,
            Step::TestFreeze if !frozen => {
                self.freeze_test = None;
                self.advance(Step::Freeze);
            }
            Step::TestFreeze => {
                if let Some(started) = self.freeze_test {
                    let elapsed = now.saturating_duration_since(started);
                    if elapsed >= FREEZE_SETTLE && sample.health == EDIT_VALUE {
                        self.freeze_test = None;
                        self.advance(Step::Complete);
                    } else if elapsed >= FREEZE_TIMEOUT {
                        self.freeze_test = None;
                        self.hint = Some(Hint::FreezeFailed);
                    }
                }
            }
            _ => {}
        }
    }

    fn can_skip_narrowing(&self) -> bool {
        self.step() == Step::Narrow && self.target_found && self.refinements > 0 && self.candidates <= SKIP_CANDIDATES
    }
}

pub(crate) struct Tutorial {
    target: TutorialProcess,
    progress: Progress,
    health: Option<i32>,
    error: Option<String>,
}

enum Action {
    Begin,
    SkipNarrowing,
    Restart,
    Exit,
}

impl App {
    pub fn start_tutorial(&mut self) {
        match std::env::current_exe() {
            Ok(executable) => self.start_tutorial_with(&executable),
            Err(error) => self.state.push_error(AppError::Generic {
                message: plain(fl!(crate::LANGUAGE_LOADER, "tutorial-error", error = error.to_string())),
            }),
        }
    }

    /// `executable` must be a Game Cheetah binary that understands
    /// [`crate::tutorial::TARGET_ARGUMENT`].
    pub fn start_tutorial_with(&mut self, executable: &std::path::Path) {
        match TutorialProcess::spawn(executable) {
            Ok(target) => {
                self.back_to_main_menu();
                let process = ProcessInfo {
                    pid: target.pid(),
                    start_time: 0,
                    executable: String::new(),
                    name: fl!(crate::LANGUAGE_LOADER, "tutorial-target"),
                    cmd: String::new(),
                    user: String::new(),
                    memory: 0,
                    instances: Vec::new(),
                };
                self.select_process(&process);
                self.state.searches[0].search_type = SearchType::Int;
                self.tutorial = Some(Tutorial {
                    target,
                    progress: Progress::default(),
                    health: None,
                    error: None,
                });
            }
            Err(error) => self.state.push_error(AppError::Generic {
                message: plain(fl!(crate::LANGUAGE_LOADER, "tutorial-error", error = error)),
            }),
        }
    }

    /// Re-reads the practice game and advances the lesson; runs every frame.
    pub fn poll_tutorial(&mut self) {
        let Some(tutorial) = self.tutorial.as_mut() else { return };
        if tutorial.error.is_some() {
            return;
        }
        let sample = tutorial.target.health().and_then(|health| {
            Ok(Sample {
                health_address: tutorial.target.health_address(),
                health,
                hud: tutorial.target.hud()?,
            })
        });
        match sample {
            Ok(sample) => {
                tutorial.health = Some(sample.health);
                tutorial
                    .progress
                    .observe(&self.state.searches, self.state.current_search, sample, Instant::now());
            }
            Err(error) => {
                tutorial.health = None;
                tutorial.error = Some(error);
            }
        }
    }
}

/// Fluent wraps placeables in Unicode isolation marks, which the bundled
/// fonts render as boxes.
fn plain(text: String) -> String {
    text.replace(['\u{2068}', '\u{2069}'], "")
}

pub(crate) fn show(app: &mut App, ui: &mut egui::Ui) {
    let Some(tutorial) = app.tutorial.as_mut() else { return };
    let mut action = None;
    egui::Panel::right("tutorial_panel")
        .default_size(420.0)
        .min_size(280.0)
        .max_size(600.0)
        .frame(egui::Frame::new().fill(ui.visuals().panel_fill).inner_margin(egui::Margin::symmetric(14, 12)))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("tutorial_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    header(ui, &mut action);
                    ui.add_space(8.0);
                    if let Some(error) = &tutorial.error {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            plain(fl!(crate::LANGUAGE_LOADER, "tutorial-error", error = error.clone())),
                        );
                        return;
                    }
                    game_card(ui, tutorial);
                    ui.add_space(10.0);
                    step_overview(ui, tutorial.progress.step());
                    ui.add_space(6.0);
                    step_card(ui, tutorial, &mut action);
                    ui.add_space(12.0);
                    egui::CollapsingHeader::new(fl!(crate::LANGUAGE_LOADER, "tutorial-glossary-title"))
                        .id_salt("tutorial_glossary")
                        .show(ui, |ui| ui.label(fl!(crate::LANGUAGE_LOADER, "tutorial-glossary")));
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "tutorial-safety")).small().weak());
                });
        });
    match action {
        Some(Action::Begin) => {
            if let Some(tutorial) = app.tutorial.as_mut() {
                tutorial.progress.advance(Step::Search);
            }
            app.search_value_request_focus = true;
        }
        Some(Action::SkipNarrowing) => {
            if let Some(tutorial) = app.tutorial.as_mut() {
                tutorial.progress.advance(Step::Edit);
            }
        }
        Some(Action::Restart) => app.start_tutorial(),
        Some(Action::Exit) => app.back_to_main_menu(),
        None => {}
    }
}

fn header(ui: &mut egui::Ui, action: &mut Option<Action>) {
    ui.horizontal(|ui| {
        ui.heading(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "tutorial-title")).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(fl!(crate::LANGUAGE_LOADER, "tutorial-exit")).clicked() {
                *action = Some(Action::Exit);
            }
            if ui.button(fl!(crate::LANGUAGE_LOADER, "tutorial-restart")).clicked() {
                *action = Some(Action::Restart);
            }
        });
    });
}

fn card(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui);
        });
}

fn game_card(ui: &mut egui::Ui, tutorial: &mut Tutorial) {
    card(ui, |ui| {
        ui.label(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "tutorial-game-title")).strong());
        ui.add_space(6.0);
        let health = tutorial.health.unwrap_or(0);
        let fill = if health > MAX_HEALTH {
            egui::Color32::from_rgb(214, 170, 60)
        } else if health > MAX_HEALTH / 2 {
            egui::Color32::from_rgb(70, 160, 90)
        } else if health > MAX_HEALTH / 4 {
            egui::Color32::from_rgb(210, 140, 50)
        } else {
            egui::Color32::from_rgb(200, 70, 70)
        };
        ui.add(
            egui::ProgressBar::new((health as f32 / MAX_HEALTH as f32).clamp(0.0, 1.0))
                .fill(fill)
                .desired_height(22.0)
                .text(
                    egui::RichText::new(plain(fl!(crate::LANGUAGE_LOADER, "tutorial-health", health = health, max = MAX_HEALTH)))
                        .strong()
                        .color(egui::Color32::WHITE),
                ),
        );
        ui.add_space(6.0);
        let mut damaged = false;
        let mut result = Ok(());
        ui.horizontal_wrapped(|ui| {
            if ui.button(fl!(crate::LANGUAGE_LOADER, "tutorial-damage-button")).clicked() {
                result = tutorial.target.damage();
                damaged = true;
            }
            let can_heal = health < MAX_HEALTH;
            if ui
                .add_enabled(can_heal, egui::Button::new(fl!(crate::LANGUAGE_LOADER, "tutorial-heal-button")))
                .on_disabled_hover_text(fl!(crate::LANGUAGE_LOADER, "tutorial-heal-full"))
                .clicked()
            {
                result = tutorial.target.heal();
            }
        });
        ui.label(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "tutorial-game-rules")).small().weak());
        match result {
            Ok(()) if damaged && tutorial.progress.step() == Step::TestFreeze => {
                tutorial.progress.freeze_test = Some(Instant::now());
                tutorial.progress.hint = None;
            }
            Ok(()) => {}
            Err(error) => tutorial.error = Some(error),
        }
    });
}

fn step_name(step: Step) -> String {
    match step {
        Step::Intro => fl!(crate::LANGUAGE_LOADER, "tutorial-intro-title"),
        Step::Search => fl!(crate::LANGUAGE_LOADER, "tutorial-search-title"),
        Step::Narrow => fl!(crate::LANGUAGE_LOADER, "tutorial-narrow-title"),
        Step::Edit => fl!(crate::LANGUAGE_LOADER, "tutorial-edit-title"),
        Step::Freeze => fl!(crate::LANGUAGE_LOADER, "tutorial-freeze-title"),
        Step::TestFreeze => fl!(crate::LANGUAGE_LOADER, "tutorial-test-title"),
        Step::Complete => fl!(crate::LANGUAGE_LOADER, "tutorial-complete-title"),
    }
}

/// Compact progress row, so the current instructions stay above the fold.
fn step_overview(ui: &mut egui::Ui, current: Step) {
    let accent = ui.visuals().selection.bg_fill;
    let done = egui::Color32::from_rgb(90, 180, 110);
    let pending = ui.visuals().weak_text_color();
    let number = STEPS.iter().position(|&step| step == current).map_or(STEPS.len(), |index| index + 1);
    ui.horizontal(|ui| {
        for step in STEPS {
            // Painted markers: the bundled fonts lack check-mark glyphs.
            let (rect, response) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            let center = rect.center();
            if step < current {
                ui.painter().circle_filled(center, 5.0, done);
            } else if step == current {
                ui.painter().circle_filled(center, 6.0, accent);
            } else {
                ui.painter().circle_stroke(center, 4.5, egui::Stroke::new(1.0, pending));
            }
            response.on_hover_text(step_name(step));
        }
        ui.add_space(6.0);
        ui.label(egui::RichText::new(plain(fl!(crate::LANGUAGE_LOADER, "tutorial-progress", step = number, total = STEPS.len()))).weak());
    });
}

fn section(ui: &mut egui::Ui, heading: String, body: String) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(heading).strong().color(crate::ui::theme::ACCENT_TEXT));
    ui.label(body);
}

fn step_card(ui: &mut egui::Ui, tutorial: &Tutorial, action: &mut Option<Action>) {
    let progress = &tutorial.progress;
    let health = tutorial.health.unwrap_or(0);
    let step = progress.step();
    card(ui, |ui| {
        ui.label(egui::RichText::new(step_name(step)).size(17.0).strong());
        let why = fl!(crate::LANGUAGE_LOADER, "tutorial-why");
        let how = fl!(crate::LANGUAGE_LOADER, "tutorial-how");
        match step {
            Step::Intro => {
                ui.add_space(4.0);
                ui.label(fl!(crate::LANGUAGE_LOADER, "tutorial-intro-text"));
                ui.add_space(10.0);
                if ui
                    .add(egui::Button::new(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "tutorial-begin")).strong()).min_size(egui::vec2(160.0, 30.0)))
                    .clicked()
                {
                    *action = Some(Action::Begin);
                }
            }
            Step::Search => {
                section(ui, why, fl!(crate::LANGUAGE_LOADER, "tutorial-search-why"));
                section(ui, how, fl!(crate::LANGUAGE_LOADER, "tutorial-search-how"));
                status(ui, plain(fl!(crate::LANGUAGE_LOADER, "tutorial-search-status", health = health)));
            }
            Step::Narrow => {
                section(ui, why, fl!(crate::LANGUAGE_LOADER, "tutorial-narrow-why"));
                section(ui, how, fl!(crate::LANGUAGE_LOADER, "tutorial-narrow-how"));
                status(
                    ui,
                    plain(fl!(
                        crate::LANGUAGE_LOADER,
                        "tutorial-narrow-status",
                        health = health,
                        count = progress.candidates,
                        goal = TARGET_CANDIDATES
                    )),
                );
                if progress.can_skip_narrowing() {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "tutorial-skip-hint")).small().weak());
                    if ui.button(fl!(crate::LANGUAGE_LOADER, "tutorial-skip")).clicked() {
                        *action = Some(Action::SkipNarrowing);
                    }
                }
            }
            Step::Edit => {
                section(ui, why, fl!(crate::LANGUAGE_LOADER, "tutorial-edit-why"));
                section(ui, how, fl!(crate::LANGUAGE_LOADER, "tutorial-edit-how"));
                status(ui, plain(fl!(crate::LANGUAGE_LOADER, "tutorial-edit-status", count = progress.candidates)));
            }
            Step::Freeze => {
                section(ui, why, fl!(crate::LANGUAGE_LOADER, "tutorial-freeze-why"));
                section(ui, how, fl!(crate::LANGUAGE_LOADER, "tutorial-freeze-how"));
            }
            Step::TestFreeze => {
                section(ui, why, fl!(crate::LANGUAGE_LOADER, "tutorial-test-why"));
                section(ui, how, fl!(crate::LANGUAGE_LOADER, "tutorial-test-how"));
                if progress.freeze_test.is_some() {
                    status(ui, fl!(crate::LANGUAGE_LOADER, "tutorial-test-status"));
                }
            }
            Step::Complete => {
                ui.add_space(4.0);
                ui.label(fl!(crate::LANGUAGE_LOADER, "tutorial-complete-text"));
                section(
                    ui,
                    fl!(crate::LANGUAGE_LOADER, "tutorial-tips-title"),
                    fl!(crate::LANGUAGE_LOADER, "tutorial-tips"),
                );
                ui.add_space(10.0);
                if ui.button(fl!(crate::LANGUAGE_LOADER, "tutorial-exit")).clicked() {
                    *action = Some(Action::Exit);
                }
            }
        }
        if let Some(hint) = progress.hint {
            let text = match hint {
                Hint::SearchMissed => fl!(crate::LANGUAGE_LOADER, "tutorial-hint-search-missed"),
                Hint::NarrowLost => fl!(crate::LANGUAGE_LOADER, "tutorial-hint-narrow-lost"),
                Hint::NothingRemoved => fl!(crate::LANGUAGE_LOADER, "tutorial-hint-nothing-removed"),
                Hint::TabClosed => fl!(crate::LANGUAGE_LOADER, "tutorial-hint-tab-closed"),
                Hint::CopyEdited => fl!(crate::LANGUAGE_LOADER, "tutorial-hint-copy-edited"),
                Hint::WrongFreeze => fl!(crate::LANGUAGE_LOADER, "tutorial-hint-wrong-freeze"),
                Hint::FreezeValue => fl!(crate::LANGUAGE_LOADER, "tutorial-hint-freeze-value"),
                Hint::FreezeFailed => fl!(crate::LANGUAGE_LOADER, "tutorial-hint-freeze-failed"),
            };
            ui.add_space(8.0);
            egui::Frame::new()
                .fill(egui::Color32::from_rgba_unmultiplied(200, 150, 40, 28))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(170, 130, 50)))
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(text);
                });
        }
    });
}

fn status(ui: &mut egui::Ui, text: String) {
    ui.add_space(8.0);
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(text).strong());
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SearchResult;
    use std::sync::atomic::Ordering;

    const HEALTH: usize = 0x2000;
    const HUD: usize = 0x3000;

    fn sample(health: i32, hud: i32) -> Sample {
        Sample {
            health_address: HEALTH,
            health,
            hud,
        }
    }

    fn complete_scan(search: &mut SearchContext, addresses: &[usize], seconds: u64) {
        search.set_cached_results(addresses.iter().map(|&addr| SearchResult::new(addr, SearchType::Int)).collect());
        search.searching = SearchMode::None;
        search.search_complete.store(true, Ordering::Release);
        search.completed_report = Some(SearchReport {
            elapsed: Duration::from_secs(seconds),
            bytes_read: 8,
            incomplete_reads: 0,
            matches: Some(addresses.len()),
        });
    }

    // Mirrors the engine's `searches` storage.
    #[allow(clippy::vec_box)]
    fn started() -> (Progress, Vec<Box<SearchContext>>) {
        let mut progress = Progress::default();
        progress.advance(Step::Search);
        let mut search = SearchContext::new("Tutorial test".into());
        search.search_type = SearchType::Int;
        (progress, vec![Box::new(search)])
    }

    #[test]
    fn narrowing_is_a_repeatable_loop_until_copies_are_eliminated() {
        let now = Instant::now();
        let (mut progress, mut searches) = started();
        progress.observe(&searches, 0, sample(100, 100), now);
        assert_eq!(progress.step(), Step::Search, "no scan has completed yet");

        searches[0].searching = SearchMode::Memory;
        progress.observe(&searches, 0, sample(100, 100), now);
        assert_eq!(progress.step(), Step::Search);

        complete_scan(&mut searches[0], &[0x1000, HEALTH, HUD, 0x4000, 0x5000, 0x6000], 1);
        progress.observe(&searches, 0, sample(100, 100), now);
        assert_eq!(progress.step(), Step::Narrow);
        assert!(!progress.can_skip_narrowing(), "skipping needs at least one narrowing");

        // Narrowing with an unchanged value removes nothing.
        complete_scan(&mut searches[0], &[0x1000, HEALTH, HUD, 0x4000, 0x5000, 0x6000], 2);
        progress.observe(&searches, 0, sample(100, 100), now);
        assert_eq!(progress.hint, Some(Hint::NothingRemoved));

        // The first change leaves the HUD copy (and a spurious hit) next to health.
        complete_scan(&mut searches[0], &[HEALTH, HUD, 0x6000], 3);
        progress.observe(&searches, 0, sample(80, 80), now);
        assert_eq!(progress.step(), Step::Narrow, "three candidates still need another round");
        assert_eq!(progress.hint, None);
        assert!(progress.can_skip_narrowing());

        complete_scan(&mut searches[0], &[HEALTH], 4);
        progress.observe(&searches, 0, sample(65, 80), now);
        assert_eq!(progress.step(), Step::Edit);
    }

    #[test]
    fn a_short_first_result_list_still_requires_narrowing() {
        let now = Instant::now();
        let (mut progress, mut searches) = started();
        complete_scan(&mut searches[0], &[HEALTH, HUD], 1);
        progress.observe(&searches, 0, sample(87, 87), now);
        assert_eq!(progress.step(), Step::Narrow, "the lesson must practice narrowing at least once");
        complete_scan(&mut searches[0], &[HEALTH], 2);
        progress.observe(&searches, 0, sample(70, 87), now);
        assert_eq!(progress.step(), Step::Edit);
    }

    #[test]
    fn wrong_values_lose_the_target_and_undo_recovers() {
        let now = Instant::now();
        let (mut progress, mut searches) = started();
        complete_scan(&mut searches[0], &[0x1000, 0x4000, 0x5000], 1);
        progress.observe(&searches, 0, sample(100, 100), now);
        assert_eq!((progress.step(), progress.hint), (Step::Search, Some(Hint::SearchMissed)));

        complete_scan(&mut searches[0], &[0x1000, HEALTH, HUD, 0x4000, 0x5000], 2);
        progress.observe(&searches, 0, sample(100, 100), now);
        assert_eq!((progress.step(), progress.hint), (Step::Narrow, None));

        complete_scan(&mut searches[0], &[0x4000], 3);
        progress.observe(&searches, 0, sample(80, 80), now);
        assert_eq!(progress.hint, Some(Hint::NarrowLost));
        assert_eq!(progress.step(), Step::Narrow, "losing the target must not advance");

        complete_scan(&mut searches[0], &[0x1000, HEALTH, HUD, 0x4000, 0x5000], 2);
        progress.observe(&searches, 0, sample(80, 80), now);
        assert_eq!(progress.hint, None);
    }

    #[test]
    fn edit_and_freeze_are_verified_in_game_memory() {
        let now = Instant::now();
        let (mut progress, mut searches) = started();
        complete_scan(&mut searches[0], &[0x1000, HEALTH, HUD, 0x4000], 1);
        progress.observe(&searches, 0, sample(100, 100), now);
        complete_scan(&mut searches[0], &[HEALTH, HUD], 2);
        progress.observe(&searches, 0, sample(80, 80), now);
        assert_eq!(progress.step(), Step::Edit);

        progress.observe(&searches, 0, sample(80, EDIT_VALUE), now);
        assert_eq!((progress.step(), progress.hint), (Step::Edit, Some(Hint::CopyEdited)));
        progress.observe(&searches, 0, sample(EDIT_VALUE, EDIT_VALUE), now);
        assert_eq!((progress.step(), progress.hint), (Step::Freeze, None));

        searches[0].freezed_addresses.insert(HUD);
        progress.observe(&searches, 0, sample(EDIT_VALUE, EDIT_VALUE), now);
        assert_eq!((progress.step(), progress.hint), (Step::Freeze, Some(Hint::WrongFreeze)));
        searches[0].freezed_addresses.insert(HEALTH);
        progress.observe(&searches, 0, sample(EDIT_VALUE, EDIT_VALUE), now);
        assert_eq!(progress.step(), Step::TestFreeze);

        progress.observe(&searches, 0, sample(EDIT_VALUE, EDIT_VALUE), now + Duration::from_secs(5));
        assert_eq!(progress.step(), Step::TestFreeze, "a highlighted snowflake alone proves nothing");
        progress.freeze_test = Some(now);
        progress.observe(&searches, 0, sample(EDIT_VALUE, 0), now + Duration::from_millis(499));
        assert_eq!(progress.step(), Step::TestFreeze);
        progress.observe(&searches, 0, sample(980, 0), now + Duration::from_secs(1));
        assert_eq!(progress.step(), Step::TestFreeze);
        progress.observe(&searches, 0, sample(980, 0), now + Duration::from_secs(3));
        assert_eq!(progress.hint, Some(Hint::FreezeFailed));

        progress.freeze_test = Some(now);
        progress.observe(&searches, 0, sample(EDIT_VALUE, 0), now + Duration::from_secs(1));
        assert_eq!(progress.step(), Step::Complete);
    }

    #[test]
    fn unfreezing_or_closing_the_lesson_tab_steps_back() {
        let now = Instant::now();
        let (mut progress, mut searches) = started();
        complete_scan(&mut searches[0], &[HEALTH, HUD, 0x4000], 1);
        progress.observe(&searches, 0, sample(100, 100), now);
        progress.advance(Step::TestFreeze);
        progress.freeze_test = Some(now);
        progress.observe(&searches, 0, sample(EDIT_VALUE, 0), now + Duration::from_secs(1));
        assert_eq!(progress.step(), Step::Freeze);
        assert!(progress.freeze_test.is_none());

        let other = SearchContext::new("Other".into());
        progress.observe(&[Box::new(other)], 0, sample(EDIT_VALUE, 0), now);
        assert_eq!((progress.step(), progress.hint), (Step::Search, Some(Hint::TabClosed)));
        assert!(progress.search_id.is_none());
    }
}
