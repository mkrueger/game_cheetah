//! Isolated practice game used by the interactive tutorial.
//!
//! The practice game runs as a separate child process, so Game Cheetah never
//! edits its own memory. Its memory deliberately contains what makes real
//! scans tedious:
//! - unrelated values covering every possible health value, so any first
//!   search finds many candidates,
//! - a lagging HUD copy of health that only a second value change separates
//!   from the real health value.
use std::{
    io::{self, BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicI32, Ordering},
    time::Duration,
};

use crossbeam_channel::{Receiver, unbounded};
use process_memory::{TryIntoProcessHandle, copy_address};

pub const TARGET_ARGUMENT: &str = "--tutorial-target";
pub const MAX_HEALTH: i32 = 100;
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

/// Runs before GUI initialization. EOF on stdin ends the practice process.
pub fn run_target() -> io::Result<()> {
    let mut game = Box::new(Game {
        health: AtomicI32::new(MAX_HEALTH),
        hud: AtomicI32::new(MAX_HEALTH),
        changes: 0,
        rng: Rng::seeded(),
    });
    let decoys = Box::new(std::array::from_fn::<_, DECOYS, _>(|index| AtomicI32::new(index as i32 % MAX_HEALTH + 1)));
    let mut output = io::stdout().lock();
    writeln!(output, "{:X} {:X}", game.health.as_ptr() as usize, game.hud.as_ptr() as usize)?;
    output.flush()?;
    for line in io::stdin().lock().lines() {
        let known = game.apply(&line?);
        scrub_stack();
        if !known {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Unknown tutorial command"));
        }
        writeln!(output, "ok")?;
        output.flush()?;
        std::hint::black_box(&decoys);
    }
    Ok(())
}

/// Owns the child, including cleanup on errors and application shutdown.
pub struct TutorialProcess {
    child: Child,
    responses: Receiver<Result<String, String>>,
    pid: process_memory::Pid,
    health_address: usize,
    hud_address: usize,
}

impl TutorialProcess {
    pub fn spawn(executable: &Path) -> Result<Self, String> {
        let mut child = Command::new(executable)
            .arg(TARGET_ARGUMENT)
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
            (Some(Ok(health)), Some(Ok(hud))) if health != 0 && hud != 0 => {
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

    pub fn pid(&self) -> process_memory::Pid {
        self.pid
    }

    pub fn health_address(&self) -> usize {
        self.health_address
    }

    /// Address of the lagging HUD copy, which looks like health but is not.
    pub fn hud_address(&self) -> usize {
        self.hud_address
    }

    fn read(&self, address: usize) -> Result<i32, String> {
        let handle = self.pid.try_into_process_handle().map_err(|error| error.to_string())?;
        let bytes = copy_address(address, 4, &crate::state::memory_reader::ExactProcessReader(&handle)).map_err(|error| error.to_string())?;
        let bytes = bytes.try_into().map_err(|_| "Incomplete tutorial read".to_owned())?;
        Ok(i32::from_ne_bytes(bytes))
    }

    pub fn health(&self) -> Result<i32, String> {
        self.read(self.health_address)
    }

    pub fn hud(&self) -> Result<i32, String> {
        self.read(self.hud_address)
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
}
