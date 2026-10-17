//! Isolated practice game used by the interactive tutorial.
//!
//! The practice game runs as a separate child process, so Game Cheetah never
//! edits its own memory. Its memory deliberately contains what makes real
//! scans tedious:
//! - unrelated values covering every possible health value, so any first
//!   search finds many candidates,
//! - a lagging HUD copy of health that only a second value change separates
//!   from the real health value.
//!
//! The health-bar lesson instead hides a `f32` health value behind a bar and
//! runs a background thread whose values change on their own, like
//! animations and timers in real games.
use std::{
    io::{self, BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicI32, AtomicU32, Ordering},
    time::Duration,
};

use crossbeam_channel::{Receiver, unbounded};
use process_memory::{TryIntoProcessHandle, copy_address};

pub const TARGET_ARGUMENT: &str = "--tutorial-target";
/// Selects the health-bar lesson; combined with [`TARGET_ARGUMENT`].
pub const HEALTH_BAR_ARGUMENT: &str = "--health-bar";
pub const MAX_HEALTH: i32 = 100;
pub const MAX_BAR: f32 = 100.0;
/// Bar damage and potions in tenths, so the hidden value has decimals.
const BAR_DAMAGE: std::ops::RangeInclusive<i32> = 70..=230;
const BAR_HEAL: std::ops::RangeInclusive<i32> = 100..=300;
/// Values that keep changing on their own in the health-bar lesson.
const MOVING_VALUES: usize = 64;
const MOVING_INTERVAL: Duration = Duration::from_millis(100);
const DAMAGE: std::ops::RangeInclusive<i32> = 7..=23;
const HEAL: std::ops::RangeInclusive<i32> = 10..=30;
/// Three unrelated copies of every value from 1 to [`MAX_HEALTH`].
const DECOYS: usize = 3 * MAX_HEALTH as usize;
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(3);

struct Rng(u64);

impl Rng {
    fn seeded() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos() as u64);
        Self((nanos ^ u64::from(std::process::id()).rotate_left(32)) | 1)
    }

    fn range(&mut self, range: std::ops::RangeInclusive<i32>) -> i32 {
        // xorshift64: unpredictable enough that players must read the value.
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        let span = (range.end() - range.start() + 1) as u64;
        range.start() + (self.0 % span) as i32
    }
}

/// Health after taking `amount` damage; an empty health bar respawns at full health.
pub fn damaged(health: i32, amount: i32) -> i32 {
    let health = health.saturating_sub(amount);
    if health <= 0 { MAX_HEALTH } else { health }
}

/// Potions never heal above the maximum, but also never lower boosted health.
pub fn healed(health: i32, amount: i32) -> i32 {
    if health >= MAX_HEALTH { health } else { (health + amount).min(MAX_HEALTH) }
}

/// Same rules for the hidden bar value. Garbage (e.g. an integer written into
/// the float) is repaired by respawning, so the lesson can always continue.
pub fn bar_damaged(health: f32, amount: f32) -> f32 {
    let health = health - amount;
    if !health.is_finite() || health <= 0.0 { MAX_BAR } else { health }
}

pub fn bar_healed(health: f32, amount: f32) -> f32 {
    if !health.is_finite() {
        MAX_BAR
    } else if health >= MAX_BAR {
        health
    } else {
        (health.max(0.0) + amount).min(MAX_BAR)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lesson {
    /// Health is a visible integer: search for exact values.
    #[default]
    KnownValue,
    /// Health is a hidden float behind a bar: compare unknown values.
    HealthBar,
}

struct Game {
    health: AtomicI32,
    hud: AtomicI32,
    changes: u32,
    rng: Rng,
}

impl Game {
    /// Kept out of line so its locals live in a frame that [`scrub_stack`] overwrites.
    #[inline(never)]
    fn apply(&mut self, command: &str) -> bool {
        let current = self.health.load(Ordering::SeqCst);
        let next = match command {
            "damage" => damaged(current, self.rng.range(DAMAGE)),
            "heal" => healed(current, self.rng.range(HEAL)),
            _ => return false,
        };
        if next != current {
            self.health.store(next, Ordering::SeqCst);
            self.changes += 1;
            // The HUD only redraws on every other change, like a lazy UI copy.
            if self.changes % 2 == 1 {
                self.hud.store(next, Ordering::SeqCst);
            }
        }
        true
    }
}

/// Stale stack slots would otherwise keep matching copies of health that no
/// amount of narrowing removes, leaving learners stuck with extra candidates.
#[inline(never)]
fn scrub_stack() {
    std::hint::black_box(&mut [0_u8; 16 * 1024]);
}

/// Holds nothing but the hidden health value. A persistent random generator
/// would change on every click too, and survive comparisons by pure chance.
struct Bar {
    health: AtomicU32,
}

impl Bar {
    #[inline(never)]
    fn apply(&self, command: &str) -> bool {
        // Seeded per command, so its state only lives on the scrubbed stack.
        let mut rng = Rng::seeded();
        let current = f32::from_bits(self.health.load(Ordering::SeqCst));
        let next = match command {
            "damage" => bar_damaged(current, rng.range(BAR_DAMAGE) as f32 / 10.0),
            "heal" => bar_healed(current, rng.range(BAR_HEAL) as f32 / 10.0),
            _ => return false,
        };
        self.health.store(next.to_bits(), Ordering::SeqCst);
        true
    }
}

/// Random walks: each value moves up or down on every tick, so only the
/// "unchanged" comparison removes all of them at once.
fn spawn_moving_values() {
    let values: &'static [AtomicU32; MOVING_VALUES] = Box::leak(Box::new(std::array::from_fn(|index| {
        let start = (index as f32 * 1.5) % MAX_BAR + 1.0;
        AtomicU32::new(if index % 2 == 0 { start.to_bits() } else { start as u32 })
    })));
    std::thread::spawn(move || {
        let mut rng = Rng::seeded();
        loop {
            for (index, value) in values.iter().enumerate() {
                let step = rng.range(1..=40) * if rng.range(0..=1) == 0 { -1 } else { 1 };
                let old = value.load(Ordering::Relaxed);
                let new = if index % 2 == 0 {
                    let moved = f32::from_bits(old) + step as f32 / 8.0;
                    (if (1.0..MAX_BAR).contains(&moved) {
                        moved
                    } else {
                        f32::from_bits(old) - step as f32 / 8.0
                    })
                    .to_bits()
                } else {
                    let moved = old as i32 + step;
                    (if (1..10_000).contains(&moved) { moved } else { old as i32 - step }) as u32
                };
                value.store(new, Ordering::Relaxed);
            }
            std::thread::sleep(MOVING_INTERVAL);
        }
    });
}

/// Runs before GUI initialization. EOF on stdin ends the practice process.
pub fn run_target(lesson: Lesson) -> io::Result<()> {
    let mut output = io::stdout().lock();
    match lesson {
        Lesson::KnownValue => {
            let mut game = Box::new(Game {
                health: AtomicI32::new(MAX_HEALTH),
                hud: AtomicI32::new(MAX_HEALTH),
                changes: 0,
                rng: Rng::seeded(),
            });
            let decoys = Box::new(std::array::from_fn::<_, DECOYS, _>(|index| AtomicI32::new(index as i32 % MAX_HEALTH + 1)));
            writeln!(output, "{:X} {:X}", game.health.as_ptr() as usize, game.hud.as_ptr() as usize)?;
            serve(&mut output, |command| game.apply(command))?;
            std::hint::black_box(&decoys);
        }
        Lesson::HealthBar => {
            let bar = Box::new(Bar {
                health: AtomicU32::new(MAX_BAR.to_bits()),
            });
            spawn_moving_values();
            writeln!(output, "{:X} 0", bar.health.as_ptr() as usize)?;
            serve(&mut output, |command| bar.apply(command))?;
        }
    }
    Ok(())
}

fn serve(output: &mut impl Write, mut apply: impl FnMut(&str) -> bool) -> io::Result<()> {
    output.flush()?;
    for line in io::stdin().lock().lines() {
        let known = apply(&line?);
        scrub_stack();
        if !known {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Unknown tutorial command"));
        }
        writeln!(output, "ok")?;
        output.flush()?;
    }
    Ok(())
}

/// Owns the child, including cleanup on errors and application shutdown.
pub struct TutorialProcess {
    lesson: Lesson,
    child: Child,
    responses: Receiver<Result<String, String>>,
    pid: process_memory::Pid,
    health_address: usize,
    /// Zero when the lesson has no HUD copy.
    hud_address: usize,
}

impl TutorialProcess {
    pub fn spawn(executable: &Path, lesson: Lesson) -> Result<Self, String> {
        let mut command = Command::new(executable);
        command.arg(TARGET_ARGUMENT);
        if lesson == Lesson::HealthBar {
            command.arg(HEALTH_BAR_ARGUMENT);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| error.to_string())?;
        let output = child.stdout.take().expect("piped tutorial stdout");
        let (sender, responses) = unbounded();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                if sender.send(line.map_err(|error| error.to_string())).is_err() {
                    break;
                }
            }
        });
        // Establish ownership before any fallible handshake so Drop always reaps it.
        let mut target = Self {
            lesson,
            child,
            responses,
            pid: 0,
            health_address: 0,
            hud_address: 0,
        };
        target.pid = process_memory::Pid::try_from(target.child.id()).map_err(|error| error.to_string())?;
        let response = target.response()?;
        let mut addresses = response.split_whitespace().map(|text| usize::from_str_radix(text, 16));
        match (addresses.next(), addresses.next()) {
            (Some(Ok(health)), Some(Ok(hud))) if health != 0 && (hud != 0 || lesson == Lesson::HealthBar) => {
                target.health_address = health;
                target.hud_address = hud;
                Ok(target)
            }
            _ => Err(format!("Unexpected tutorial handshake: {response}")),
        }
    }

    fn response(&self) -> Result<String, String> {
        self.responses.recv_timeout(RESPONSE_TIMEOUT).map_err(|error| error.to_string())?
    }

    pub fn lesson(&self) -> Lesson {
        self.lesson
    }

    pub fn pid(&self) -> process_memory::Pid {
        self.pid
    }

    pub fn health_address(&self) -> usize {
        self.health_address
    }

    /// Address of the lagging HUD copy, which looks like health but is not.
    pub fn hud_address(&self) -> Option<usize> {
        (self.hud_address != 0).then_some(self.hud_address)
    }

    fn read(&self, address: usize) -> Result<[u8; 4], String> {
        let handle = self.pid.try_into_process_handle().map_err(|error| error.to_string())?;
        let bytes = copy_address(address, 4, &crate::state::memory_reader::ExactProcessReader(&handle)).map_err(|error| error.to_string())?;
        bytes.try_into().map_err(|_| "Incomplete tutorial read".to_owned())
    }

    /// Health as the game interprets it: an `i32` or the hidden bar `f32`.
    pub fn health(&self) -> Result<f32, String> {
        let bytes = self.read(self.health_address)?;
        Ok(match self.lesson {
            Lesson::KnownValue => i32::from_ne_bytes(bytes) as f32,
            Lesson::HealthBar => f32::from_ne_bytes(bytes),
        })
    }

    pub fn hud(&self) -> Result<Option<f32>, String> {
        self.hud_address()
            .map(|address| self.read(address).map(|bytes| i32::from_ne_bytes(bytes) as f32))
            .transpose()
    }

    /// Acknowledgement proves the change happened, even if a freeze already restored health.
    fn command(&mut self, command: &[u8]) -> Result<(), String> {
        let input = self.child.stdin.as_mut().ok_or("Tutorial input is closed")?;
        input.write_all(command).and_then(|()| input.flush()).map_err(|error| error.to_string())?;
        if self.response()? != "ok" {
            return Err("Unexpected tutorial response".into());
        }
        Ok(())
    }

    pub fn damage(&mut self) -> Result<(), String> {
        self.command(b"damage\n")
    }

    pub fn heal(&mut self) -> Result<(), String> {
        self.command(b"heal\n")
    }
}

impl Drop for TutorialProcess {
    fn drop(&mut self) {
        match self.child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => {
                if let Err(error) = self.child.kill() {
                    eprintln!("Failed to stop tutorial target: {error}");
                }
            }
            Err(error) => eprintln!("Failed to check tutorial target: {error}"),
        }
        if let Err(error) = self.child.wait() {
            eprintln!("Failed to reap tutorial target: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_rules_always_change_damage_and_cap_potions() {
        assert_eq!(damaged(100, 23), 77);
        assert_eq!(damaged(5, 7), MAX_HEALTH, "an empty health bar respawns");
        assert_eq!(healed(90, 30), MAX_HEALTH);
        assert_eq!(healed(MAX_HEALTH, 10), MAX_HEALTH);
        assert_eq!(healed(999, 10), 999, "potions never lower edited health");
        let mut rng = Rng(1);
        for _ in 0..1000 {
            assert!(DAMAGE.contains(&rng.range(DAMAGE)));
            assert!(HEAL.contains(&rng.range(HEAL)));
        }
    }

    #[test]
    fn bar_rules_repair_garbage_and_never_lower_boosted_health() {
        assert_eq!(bar_damaged(50.5, 7.0), 43.5);
        assert_eq!(bar_damaged(5.0, 7.0), MAX_BAR);
        assert_eq!(bar_damaged(f32::NAN, 7.0), MAX_BAR);
        assert_eq!(bar_healed(95.0, 10.0), MAX_BAR);
        assert_eq!(bar_healed(500.0, 10.0), 500.0);
        assert_eq!(bar_healed(f32::INFINITY, 10.0), MAX_BAR);
        assert_eq!(bar_healed(-3.0, 10.0), 10.0);
    }
}
