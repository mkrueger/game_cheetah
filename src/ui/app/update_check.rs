//! Once-per-launch GitHub release check.
//!
//! Runs on a worker thread so the UI never blocks on the network.
//! Results are delivered through a `crossbeam_channel`, polled each frame.

use super::App;

impl App {
    /// Kick off the once-per-launch update check on a worker thread.
    pub(super) fn start_update_check_if_needed(&mut self) {
        if !self.check_for_updates || self.update_check_rx.is_some() || self.latest_version.is_some() {
            return;
        }
        let (tx, rx) = crossbeam_channel::bounded::<Option<String>>(1);
        std::thread::spawn(move || {
            let result = crate::update_check::fetch_latest_version();
            let _ = tx.send(result);
        });
        self.update_check_rx = Some(rx);
    }

    pub(super) fn poll_update_check(&mut self) {
        if let Some(rx) = &self.update_check_rx
            && let Ok(latest) = rx.try_recv()
        {
            self.update_check_rx = None;
            if let Some(tag) = latest
                && crate::update_check::is_newer(&tag, crate::VERSION)
            {
                self.latest_version = Some(tag);
            }
        }
    }
}
