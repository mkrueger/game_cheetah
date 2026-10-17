//! Interactive tutorial: guided lessons against the isolated practice game.
//!
//! Progress is derived from what actually happened — completed scans in the
//! lesson's search tab and the practice game's real memory — never from
//! button clicks alone.
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use i18n_embed_fl::fl;

use crate::{
    App, AppError, ProcessInfo, SearchContext, SearchMode, SearchReport, SearchType,
    tutorial::{Lesson, MAX_BAR, MAX_HEALTH, TutorialProcess},
};

/// The narrowing loop ends once at most this many addresses remain.
const TARGET_CANDIDATES: usize = 2;
/// Spurious matches can survive; offer to continue with a short list.
const SKIP_CANDIDATES: usize = 5;
const EDIT_VALUE: f32 = 999.0;
/// Potions and respawns never exceed a full bar, so only an edit can overfill it.
const OVERFILLED_BAR: f32 = MAX_BAR * 1.01;
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
    ChooseUnknown,
    NarrowLost,
    NothingRemoved,
    TabClosed,
    CopyEdited,
    WrongType,
    WrongFreeze,
    FreezeValue,
    FreezeFailed,
}

/// One reading of the practice game's memory.
#[derive(Clone, Copy, Debug)]
struct Sample {
    health_address: usize,
    health: f32,
    hud: Option<f32>,
}

/// Bytes the game cannot use as health, e.g. an integer written into the float.
fn is_garbage(health: f32) -> bool {
    !health.is_finite() || health < 0.0 || (health != 0.0 && health.abs() < 1e-6) || health > 1e7
}

fn max_health(lesson: Lesson) -> f32 {
    match lesson {
        Lesson::KnownValue => MAX_HEALTH as f32,
        Lesson::HealthBar => MAX_BAR,
    }
}

#[derive(Debug)]
struct Progress {
    lesson: Lesson,
    step: Step,
    hint: Option<Hint>,
    /// The tab in which the lesson's search started.
    search_id: Option<u64>,
    last_report: Option<SearchReport>,
    scanned_candidates: Option<usize>,
    /// Distinct addresses; the unknown search lists each as Int and Float.
    candidates: usize,
    target_found: bool,
    refinements: usize,
    freeze_test: Option<Instant>,
}

impl Progress {
    fn new(lesson: Lesson) -> Self {
        Self {
            lesson,
            step: Step::Intro,
            hint: None,
            search_id: None,
            last_report: None,
            scanned_candidates: None,
            candidates: 0,
            target_found: false,
            refinements: 0,
            freeze_test: None,
        }
    }

    fn step(&self) -> Step {
        self.step
    }

    fn advance(&mut self, step: Step) {
        self.step = step;
        self.hint = None;
    }

    /// The row type whose edits the game actually understands.
    fn target_type(&self) -> SearchType {
        match self.lesson {
            Lesson::KnownValue => SearchType::Int,
            Lesson::HealthBar => SearchType::Float,
        }
    }

    fn edited(&self, health: f32) -> bool {
        match self.lesson {
            Lesson::KnownValue => health == EDIT_VALUE,
            Lesson::HealthBar => !is_garbage(health) && health >= OVERFILLED_BAR,
        }
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
                        ..Self::new(self.lesson)
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
            .binary_search_by_key(&(sample.health_address, self.target_type() as u8), |result| {
                (result.addr, result.search_type as u8)
            })
            .is_ok();
        // Results are sorted by address, so equal addresses are adjacent.
        self.candidates = results
            .iter()
            .enumerate()
            .filter(|&(index, result)| index == 0 || results[index - 1].addr != result.addr)
            .count();
        self.target_found = has_target;
        let finished = search.search_complete.load(std::sync::atomic::Ordering::Acquire)
            && search.completed_report.is_some()
            && search.completed_report != self.last_report;
        if finished {
            self.last_report = search.completed_report;
            // A snapshot reports no match count: it only captures memory.
            let snapshot = search.completed_report.is_some_and(|report| report.matches.is_none());
            let previous = if snapshot {
                self.scanned_candidates = None;
                None
            } else {
                self.scanned_candidates.replace(self.candidates)
            };
            match (self.lesson, self.step()) {
                (Lesson::KnownValue, Step::Search) if has_target => {
                    self.search_id = Some(search.view_id);
                    self.refinements = 0;
                    self.advance(Step::Narrow);
                }
                (Lesson::KnownValue, Step::Search) => self.hint = Some(Hint::SearchMissed),
                (Lesson::HealthBar, Step::Search) if snapshot && search.search_type == SearchType::Unknown => {
                    self.search_id = Some(search.view_id);
                    self.refinements = 0;
                    self.advance(Step::Narrow);
                }
                (Lesson::HealthBar, Step::Search) => self.hint = Some(Hint::ChooseUnknown),
                // Capturing again restarts the comparisons.
                (Lesson::HealthBar, Step::Narrow) if snapshot => {
                    self.refinements = 0;
                    self.hint = None;
                }
                (_, Step::Narrow) if has_target => {
                    self.refinements += 1;
                    self.hint = (previous == Some(self.candidates)).then_some(Hint::NothingRemoved);
                }
                (_, Step::Narrow) => self.hint = Some(Hint::NarrowLost),
                _ => {}
            }
        }

        let frozen = search.freezed_addresses.contains(&sample.health_address);
        let edited = self.edited(sample.health);
        let garbage = self.lesson == Lesson::HealthBar && is_garbage(sample.health);
        match self.step() {
            Step::Narrow if has_target && self.refinements > 0 && self.candidates <= TARGET_CANDIDATES => self.advance(Step::Edit),
            Step::Edit if edited => self.advance(Step::Freeze),
            Step::Edit if self.lesson == Lesson::HealthBar => self.hint = garbage.then_some(Hint::WrongType),
            Step::Edit if sample.hud == Some(EDIT_VALUE) => self.hint = Some(Hint::CopyEdited),
            Step::Freeze if frozen && edited => {
                self.freeze_test = None;
                self.advance(Step::TestFreeze);
            }
            Step::Freeze if garbage => self.hint = Some(Hint::WrongType),
            Step::Freeze if !edited => self.hint = Some(Hint::FreezeValue),
            Step::Freeze if !search.freezed_addresses.is_empty() => self.hint = Some(Hint::WrongFreeze),
            Step::Freeze => self.hint = None,
            Step::TestFreeze if !frozen => {
                self.freeze_test = None;
                self.advance(Step::Freeze);
            }
            Step::TestFreeze => {
                if let Some(started) = self.freeze_test {
                    let elapsed = now.saturating_duration_since(started);
                    if elapsed >= FREEZE_SETTLE && edited {
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
    executable: PathBuf,
    target: TutorialProcess,
    progress: Progress,
    health: Option<f32>,
    error: Option<String>,
}

enum Action {
    Begin,
    SkipNarrowing,
    Start(Lesson),
    Exit,
}

impl App {
    pub fn start_tutorial(&mut self) {
        match std::env::current_exe() {
            Ok(executable) => self.start_tutorial_with(&executable, Lesson::KnownValue),
            Err(error) => self.state.push_error(AppError::Generic {
                message: plain(fl!(crate::LANGUAGE_LOADER, "tutorial-error", error = error.to_string())),
            }),
        }
    }

    /// `executable` must be a Game Cheetah binary that understands
    /// [`crate::tutorial::TARGET_ARGUMENT`].
    pub fn start_tutorial_with(&mut self, executable: &Path, lesson: Lesson) {
        match TutorialProcess::spawn(executable, lesson) {
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
                // Choosing "Unknown" is part of the health-bar lesson.
                if lesson == Lesson::KnownValue {
                    self.state.searches[0].search_type = SearchType::Int;
                }
                self.tutorial = Some(Tutorial {
                    executable: executable.to_owned(),
                    target,
                    progress: Progress::new(lesson),
                    health: None,
                    error: None,
                });
            }
            Err(error) => self.state.push_error(AppError::Generic {
                message: plain(fl!(crate::LANGUAGE_LOADER, "tutorial-error", error = error)),
            }),
        }
    }

    /// Where the practice game keeps its real health value, while a tutorial runs.
    pub fn tutorial_health_address(&self) -> Option<usize> {
        self.tutorial.as_ref().map(|tutorial| tutorial.target.health_address())
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
                    header(ui, tutorial.progress.lesson, &mut action);
                    ui.add_space(6.0);
                    if let Some(error) = &tutorial.error {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            plain(fl!(crate::LANGUAGE_LOADER, "tutorial-error", error = error.clone())),
                        );
                        return;
                    }
                    game_card(ui, tutorial);
                    ui.add_space(10.0);
                    step_card(ui, tutorial, &mut action);
                    ui.add_space(12.0);
                    egui::CollapsingHeader::new(fl!(crate::LANGUAGE_LOADER, "tutorial-glossary-title"))
                        .id_salt("tutorial_glossary")
                        .show(ui, |ui| ui.label(fl!(crate::LANGUAGE_LOADER, "tutorial-glossary")));
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "tutorial-safety")).small().weak());
                    ui.add_space(8.0);
                    if ui.button(fl!(crate::LANGUAGE_LOADER, "tutorial-exit")).clicked() {
                        action = Some(Action::Exit);
                    }
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
        Some(Action::Start(lesson)) => {
            if let Some(executable) = app.tutorial.as_ref().map(|tutorial| tutorial.executable.clone()) {
                app.start_tutorial_with(&executable, lesson);
            }
        }
        Some(Action::Exit) => app.back_to_main_menu(),
        None => {}
    }
}

/// Lesson choice and restart share one row to keep the instructions visible;
/// the tutorial ends via the button at the bottom or the top bar's Close.
fn header(ui: &mut egui::Ui, lesson: Lesson, action: &mut Option<Action>) {
    ui.horizontal(|ui| {
        for (option, label) in [
            (Lesson::KnownValue, fl!(crate::LANGUAGE_LOADER, "tutorial-lesson-known")),
            (Lesson::HealthBar, fl!(crate::LANGUAGE_LOADER, "tutorial-lesson-bar")),
        ] {
            let response = ui.selectable_label(option == lesson, egui::RichText::new(label).strong());
            if option != lesson && response.on_hover_text(fl!(crate::LANGUAGE_LOADER, "tutorial-lesson-switch")).clicked() {
                *action = Some(Action::Start(option));
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button(fl!(crate::LANGUAGE_LOADER, "tutorial-restart"))
                .on_hover_text(fl!(crate::LANGUAGE_LOADER, "tutorial-lesson-switch"))
                .clicked()
            {
                *action = Some(Action::Start(lesson));
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
    let lesson = tutorial.progress.lesson;
    card(ui, |ui| {
        let max = max_health(lesson);
        let health = tutorial.health.filter(|health| !is_garbage(*health)).unwrap_or(0.0);
        let fill = if health > max {
            egui::Color32::from_rgb(214, 170, 60)
        } else if health > max / 2.0 {
            egui::Color32::from_rgb(70, 160, 90)
        } else if health > max / 4.0 {
            egui::Color32::from_rgb(210, 140, 50)
        } else {
            egui::Color32::from_rgb(200, 70, 70)
        };
        let mut bar = egui::ProgressBar::new((health / max).clamp(0.0, 1.0)).fill(fill).desired_height(22.0);
        // The health-bar lesson hides the number, as many games do.
        if lesson == Lesson::KnownValue {
            let shown = health as i32;
            bar = bar.text(
                egui::RichText::new(plain(fl!(crate::LANGUAGE_LOADER, "tutorial-health", health = shown, max = MAX_HEALTH)))
                    .strong()
                    .color(egui::Color32::WHITE),
            );
        }
        let mut damaged = false;
        let mut result = Ok(());
        ui.horizontal_wrapped(|ui| {
            if ui.button(fl!(crate::LANGUAGE_LOADER, "tutorial-damage-button")).clicked() {
                result = tutorial.target.damage();
                damaged = true;
            }
            if ui
                .add_enabled(health < max, egui::Button::new(fl!(crate::LANGUAGE_LOADER, "tutorial-heal-button")))
                .on_disabled_hover_text(fl!(crate::LANGUAGE_LOADER, "tutorial-heal-full"))
                .clicked()
            {
                result = tutorial.target.heal();
            }
        });
        ui.add_space(4.0);
        ui.add(bar);
        ui.add_space(2.0);
        let rules = match lesson {
            Lesson::KnownValue => fl!(crate::LANGUAGE_LOADER, "tutorial-game-rules"),
            Lesson::HealthBar => fl!(crate::LANGUAGE_LOADER, "tutorial-bar-game-rules"),
        };
        ui.label(egui::RichText::new(rules).small().weak());
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

fn step_name(lesson: Lesson, step: Step) -> String {
    match (lesson, step) {
        (_, Step::Intro) => fl!(crate::LANGUAGE_LOADER, "tutorial-intro-title"),
        (Lesson::KnownValue, Step::Search) => fl!(crate::LANGUAGE_LOADER, "tutorial-search-title"),
        (Lesson::HealthBar, Step::Search) => fl!(crate::LANGUAGE_LOADER, "tutorial-bar-search-title"),
        (Lesson::KnownValue, Step::Narrow) => fl!(crate::LANGUAGE_LOADER, "tutorial-narrow-title"),
        (Lesson::HealthBar, Step::Narrow) => fl!(crate::LANGUAGE_LOADER, "tutorial-bar-narrow-title"),
        (_, Step::Edit) => fl!(crate::LANGUAGE_LOADER, "tutorial-edit-title"),
        (_, Step::Freeze) => fl!(crate::LANGUAGE_LOADER, "tutorial-freeze-title"),
        (_, Step::TestFreeze) => fl!(crate::LANGUAGE_LOADER, "tutorial-test-title"),
        (_, Step::Complete) => fl!(crate::LANGUAGE_LOADER, "tutorial-complete-title"),
    }
}

/// Step dots next to the step title, so the instructions stay above the fold.
fn step_overview(ui: &mut egui::Ui, lesson: Lesson, current: Step) {
    let accent = ui.visuals().selection.bg_fill;
    let done = egui::Color32::from_rgb(90, 180, 110);
    let pending = ui.visuals().weak_text_color();
    let number = STEPS.iter().position(|&step| step == current).map_or(STEPS.len(), |index| index + 1);
    ui.label(egui::RichText::new(plain(fl!(crate::LANGUAGE_LOADER, "tutorial-progress", step = number, total = STEPS.len()))).weak())
        .on_hover_text(STEPS.map(|step| step_name(lesson, step)).join("\n"));
    // Right-to-left layout: paint the last step first.
    for step in STEPS.into_iter().rev() {
        // Painted markers: the bundled fonts lack check-mark glyphs.
        let (rect, response) = ui.allocate_exact_size(egui::vec2(12.0, 14.0), egui::Sense::hover());
        let center = rect.center();
        if step < current {
            ui.painter().circle_filled(center, 4.0, done);
        } else if step == current {
            ui.painter().circle_filled(center, 5.0, accent);
        } else {
            ui.painter().circle_stroke(center, 3.5, egui::Stroke::new(1.0, pending));
        }
        response.on_hover_text(step_name(lesson, step));
    }
}

fn section(ui: &mut egui::Ui, heading: String, body: String) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(heading).strong().color(crate::ui::theme::ACCENT_TEXT));
    ui.label(body);
}

/// Instructions first, so they are visible without scrolling; the
/// background explanation follows for those who want to understand more.
fn explain(ui: &mut egui::Ui, why: String, how: String) {
    section(ui, fl!(crate::LANGUAGE_LOADER, "tutorial-how"), how);
    section(ui, fl!(crate::LANGUAGE_LOADER, "tutorial-why"), why);
}

fn primary(ui: &mut egui::Ui, label: String) -> bool {
    ui.add(egui::Button::new(egui::RichText::new(label).strong()).min_size(egui::vec2(160.0, 30.0)))
        .clicked()
}

fn step_card(ui: &mut egui::Ui, tutorial: &Tutorial, action: &mut Option<Action>) {
    let progress = &tutorial.progress;
    let lesson = progress.lesson;
    let health = tutorial.health.unwrap_or(0.0) as i32;
    let step = progress.step();
    let known = lesson == Lesson::KnownValue;
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(step_name(lesson, step)).size(17.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| step_overview(ui, lesson, step));
        });
        hint(ui, progress);
        match step {
            Step::Intro => {
                ui.add_space(4.0);
                ui.label(if known {
                    fl!(crate::LANGUAGE_LOADER, "tutorial-intro-text")
                } else {
                    fl!(crate::LANGUAGE_LOADER, "tutorial-bar-intro-text")
                });
                ui.add_space(10.0);
                if primary(ui, fl!(crate::LANGUAGE_LOADER, "tutorial-begin")) {
                    *action = Some(Action::Begin);
                }
            }
            Step::Search if known => {
                status(ui, plain(fl!(crate::LANGUAGE_LOADER, "tutorial-search-status", health = health)));
                explain(
                    ui,
                    fl!(crate::LANGUAGE_LOADER, "tutorial-search-why"),
                    fl!(crate::LANGUAGE_LOADER, "tutorial-search-how"),
                );
            }
            Step::Search => explain(
                ui,
                fl!(crate::LANGUAGE_LOADER, "tutorial-bar-search-why"),
                fl!(crate::LANGUAGE_LOADER, "tutorial-bar-search-how"),
            ),
            Step::Narrow => {
                let current = if known {
                    plain(fl!(
                        crate::LANGUAGE_LOADER,
                        "tutorial-narrow-status",
                        health = health,
                        count = progress.candidates,
                        goal = TARGET_CANDIDATES
                    ))
                } else if progress.refinements == 0 {
                    fl!(crate::LANGUAGE_LOADER, "tutorial-bar-narrow-ready")
                } else {
                    plain(fl!(
                        crate::LANGUAGE_LOADER,
                        "tutorial-bar-narrow-status",
                        count = progress.candidates,
                        goal = TARGET_CANDIDATES
                    ))
                };
                status(ui, current);
                if progress.can_skip_narrowing() {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "tutorial-skip-hint")).small().weak());
                    if ui.button(fl!(crate::LANGUAGE_LOADER, "tutorial-skip")).clicked() {
                        *action = Some(Action::SkipNarrowing);
                    }
                }
                if known {
                    explain(
                        ui,
                        fl!(crate::LANGUAGE_LOADER, "tutorial-narrow-why"),
                        fl!(crate::LANGUAGE_LOADER, "tutorial-narrow-how"),
                    );
                } else {
                    explain(
                        ui,
                        fl!(crate::LANGUAGE_LOADER, "tutorial-bar-narrow-why"),
                        fl!(crate::LANGUAGE_LOADER, "tutorial-bar-narrow-how"),
                    );
                }
            }
            Step::Edit if known => {
                status(ui, plain(fl!(crate::LANGUAGE_LOADER, "tutorial-edit-status", count = progress.candidates)));
                explain(
                    ui,
                    fl!(crate::LANGUAGE_LOADER, "tutorial-edit-why"),
                    fl!(crate::LANGUAGE_LOADER, "tutorial-edit-how"),
                );
            }
            Step::Edit => explain(
                ui,
                fl!(crate::LANGUAGE_LOADER, "tutorial-bar-edit-why"),
                fl!(crate::LANGUAGE_LOADER, "tutorial-bar-edit-how"),
            ),
            Step::Freeze if known => explain(
                ui,
                fl!(crate::LANGUAGE_LOADER, "tutorial-freeze-why"),
                fl!(crate::LANGUAGE_LOADER, "tutorial-freeze-how"),
            ),
            Step::Freeze => explain(
                ui,
                fl!(crate::LANGUAGE_LOADER, "tutorial-bar-freeze-why"),
                fl!(crate::LANGUAGE_LOADER, "tutorial-bar-freeze-how"),
            ),
            Step::TestFreeze => {
                if progress.freeze_test.is_some() {
                    status(
                        ui,
                        if known {
                            fl!(crate::LANGUAGE_LOADER, "tutorial-test-status")
                        } else {
                            fl!(crate::LANGUAGE_LOADER, "tutorial-bar-test-status")
                        },
                    );
                }
                explain(
                    ui,
                    fl!(crate::LANGUAGE_LOADER, "tutorial-test-why"),
                    if known {
                        fl!(crate::LANGUAGE_LOADER, "tutorial-test-how")
                    } else {
                        fl!(crate::LANGUAGE_LOADER, "tutorial-bar-test-how")
                    },
                );
            }
            Step::Complete => {
                ui.add_space(4.0);
                ui.label(if known {
                    fl!(crate::LANGUAGE_LOADER, "tutorial-complete-text")
                } else {
                    fl!(crate::LANGUAGE_LOADER, "tutorial-bar-complete-text")
                });
                if known {
                    ui.add_space(8.0);
                    if primary(ui, fl!(crate::LANGUAGE_LOADER, "tutorial-next-lesson")) {
                        *action = Some(Action::Start(Lesson::HealthBar));
                    }
                }
                section(
                    ui,
                    fl!(crate::LANGUAGE_LOADER, "tutorial-tips-title"),
                    fl!(crate::LANGUAGE_LOADER, "tutorial-tips"),
                );
            }
        }
    });
}

/// Shown right below the step title, so it is visible without scrolling.
fn hint(ui: &mut egui::Ui, progress: &Progress) {
    let known = progress.lesson == Lesson::KnownValue;
    if let Some(hint) = progress.hint {
        let text = match hint {
            Hint::SearchMissed => fl!(crate::LANGUAGE_LOADER, "tutorial-hint-search-missed"),
            Hint::ChooseUnknown => fl!(crate::LANGUAGE_LOADER, "tutorial-hint-choose-unknown"),
            Hint::NarrowLost if known => fl!(crate::LANGUAGE_LOADER, "tutorial-hint-narrow-lost"),
            Hint::NarrowLost => fl!(crate::LANGUAGE_LOADER, "tutorial-bar-hint-narrow-lost"),
            Hint::NothingRemoved if known => fl!(crate::LANGUAGE_LOADER, "tutorial-hint-nothing-removed"),
            Hint::NothingRemoved => fl!(crate::LANGUAGE_LOADER, "tutorial-bar-hint-nothing-removed"),
            Hint::TabClosed => fl!(crate::LANGUAGE_LOADER, "tutorial-hint-tab-closed"),
            Hint::CopyEdited => fl!(crate::LANGUAGE_LOADER, "tutorial-hint-copy-edited"),
            Hint::WrongType => fl!(crate::LANGUAGE_LOADER, "tutorial-hint-wrong-type"),
            Hint::WrongFreeze => fl!(crate::LANGUAGE_LOADER, "tutorial-hint-wrong-freeze"),
            Hint::FreezeValue if known => fl!(crate::LANGUAGE_LOADER, "tutorial-hint-freeze-value"),
            Hint::FreezeValue => fl!(crate::LANGUAGE_LOADER, "tutorial-bar-hint-freeze-value"),
            Hint::FreezeFailed => fl!(crate::LANGUAGE_LOADER, "tutorial-hint-freeze-failed"),
        };
        ui.add_space(6.0);
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
            health: health as f32,
            hud: Some(hud as f32),
        }
    }

    fn bar(health: f32) -> Sample {
        Sample {
            health_address: HEALTH,
            health,
            hud: None,
        }
    }

    fn complete_scan(search: &mut SearchContext, addresses: &[usize], seconds: u64) {
        complete_typed_scan(search, &addresses.iter().map(|&addr| (addr, SearchType::Int)).collect::<Vec<_>>(), seconds);
    }

    fn complete_typed_scan(search: &mut SearchContext, rows: &[(usize, SearchType)], seconds: u64) {
        let addresses: Vec<_> = rows.iter().map(|&(addr, _)| addr).collect();
        search.set_cached_results(rows.iter().map(|&(addr, kind)| SearchResult::new(addr, kind)).collect());
        search.searching = SearchMode::None;
        search.search_complete.store(true, Ordering::Release);
        search.completed_report = Some(SearchReport {
            elapsed: Duration::from_secs(seconds),
            bytes_read: 8,
            incomplete_reads: 0,
            matches: Some(addresses.len()),
        });
    }

    #[allow(clippy::vec_box)]
    fn started() -> (Progress, Vec<Box<SearchContext>>) {
        started_lesson(Lesson::KnownValue)
    }

    // Mirrors the engine's `searches` storage.
    #[allow(clippy::vec_box)]
    fn started_lesson(lesson: Lesson) -> (Progress, Vec<Box<SearchContext>>) {
        let mut progress = Progress::new(lesson);
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

        progress.observe(&searches, 0, sample(80, 999), now);
        assert_eq!((progress.step(), progress.hint), (Step::Edit, Some(Hint::CopyEdited)));
        progress.observe(&searches, 0, sample(999, 999), now);
        assert_eq!((progress.step(), progress.hint), (Step::Freeze, None));

        searches[0].freezed_addresses.insert(HUD);
        progress.observe(&searches, 0, sample(999, 999), now);
        assert_eq!((progress.step(), progress.hint), (Step::Freeze, Some(Hint::WrongFreeze)));
        searches[0].freezed_addresses.insert(HEALTH);
        progress.observe(&searches, 0, sample(999, 999), now);
        assert_eq!(progress.step(), Step::TestFreeze);

        progress.observe(&searches, 0, sample(999, 999), now + Duration::from_secs(5));
        assert_eq!(progress.step(), Step::TestFreeze, "a highlighted snowflake alone proves nothing");
        progress.freeze_test = Some(now);
        progress.observe(&searches, 0, sample(999, 0), now + Duration::from_millis(499));
        assert_eq!(progress.step(), Step::TestFreeze);
        progress.observe(&searches, 0, sample(980, 0), now + Duration::from_secs(1));
        assert_eq!(progress.step(), Step::TestFreeze);
        progress.observe(&searches, 0, sample(980, 0), now + Duration::from_secs(3));
        assert_eq!(progress.hint, Some(Hint::FreezeFailed));

        progress.freeze_test = Some(now);
        progress.observe(&searches, 0, sample(999, 0), now + Duration::from_secs(1));
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
        progress.observe(&searches, 0, sample(999, 0), now + Duration::from_secs(1));
        assert_eq!(progress.step(), Step::Freeze);
        assert!(progress.freeze_test.is_none());

        let other = SearchContext::new("Other".into());
        progress.observe(&[Box::new(other)], 0, sample(999, 0), now);
        assert_eq!((progress.step(), progress.hint), (Step::Search, Some(Hint::TabClosed)));
        assert!(progress.search_id.is_none());
    }

    fn snapshot(search: &mut SearchContext, seconds: u64) {
        search.search_type = SearchType::Unknown;
        complete_typed_scan(search, &[], seconds);
        search.completed_report.as_mut().unwrap().matches = None;
    }

    #[test]
    fn health_bar_lesson_requires_unknown_snapshot_and_comparisons() {
        let now = Instant::now();
        let (mut progress, mut searches) = started_lesson(Lesson::HealthBar);
        searches[0].search_type = SearchType::Float;
        complete_typed_scan(&mut searches[0], &[(HEALTH, SearchType::Float)], 1);
        progress.observe(&searches, 0, bar(100.0), now);
        assert_eq!((progress.step(), progress.hint), (Step::Search, Some(Hint::ChooseUnknown)));

        snapshot(&mut searches[0], 2);
        progress.observe(&searches, 0, bar(100.0), now);
        assert_eq!((progress.step(), progress.hint), (Step::Narrow, None));
        assert_eq!(progress.refinements, 0);

        // Health as Int and Float plus a moving value: two addresses, three rows.
        let rows = [
            (HEALTH, SearchType::Int),
            (HEALTH, SearchType::Float),
            (0x4000, SearchType::Float),
            (0x5000, SearchType::Int),
        ];
        complete_typed_scan(&mut searches[0], &rows, 3);
        progress.observe(&searches, 0, bar(81.4), now);
        assert_eq!(progress.candidates, 3);
        assert_eq!(progress.step(), Step::Narrow);

        // A wrong comparison loses the Float row.
        complete_typed_scan(&mut searches[0], &[(0x4000, SearchType::Float)], 4);
        progress.observe(&searches, 0, bar(81.4), now);
        assert_eq!(progress.hint, Some(Hint::NarrowLost));

        // Capturing again restarts the comparisons without a stale hint.
        snapshot(&mut searches[0], 5);
        progress.observe(&searches, 0, bar(81.4), now);
        assert_eq!((progress.step(), progress.hint, progress.refinements), (Step::Narrow, None, 0));

        complete_typed_scan(&mut searches[0], &rows[..3], 6);
        progress.observe(&searches, 0, bar(70.1), now);
        assert_eq!(progress.candidates, 2, "the Int and Float rows count as one address");
        assert_eq!(progress.step(), Step::Edit);
    }

    #[test]
    fn health_bar_must_be_overfilled_as_float_not_by_potions_or_respawns() {
        let now = Instant::now();
        let (mut progress, mut searches) = started_lesson(Lesson::HealthBar);
        snapshot(&mut searches[0], 1);
        progress.observe(&searches, 0, bar(100.0), now);
        complete_typed_scan(&mut searches[0], &[(HEALTH, SearchType::Int), (HEALTH, SearchType::Float)], 2);
        progress.observe(&searches, 0, bar(80.0), now);
        assert_eq!(progress.step(), Step::Edit);

        progress.observe(&searches, 0, bar(MAX_BAR), now);
        assert_eq!(progress.step(), Step::Edit, "a full bar from a potion or respawn is not an edit");
        // Writing the integer 200 into the float gives a tiny denormal.
        progress.observe(&searches, 0, bar(f32::from_bits(200)), now);
        assert_eq!((progress.step(), progress.hint), (Step::Edit, Some(Hint::WrongType)));
        progress.observe(&searches, 0, bar(MAX_BAR), now);
        assert_eq!(progress.hint, None, "the hint disappears once the bar is repaired");
        progress.observe(&searches, 0, bar(200.0), now);
        assert_eq!(progress.step(), Step::Freeze);

        searches[0].freezed_addresses.insert(HEALTH);
        progress.observe(&searches, 0, bar(200.0), now);
        assert_eq!(progress.step(), Step::TestFreeze);
        progress.freeze_test = Some(now);
        progress.observe(&searches, 0, bar(200.0), now + Duration::from_secs(1));
        assert_eq!(progress.step(), Step::Complete);
    }
}
