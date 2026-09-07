//! Per-search cancellation and completion, independent of the lifetime of a tab.
use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use process_memory::CopyAddress;

use crate::AppError;

/// The UI owns the sole cancellation sender. Dropping it wakes every worker,
/// including workers blocked on a full result channel.
pub(crate) struct SearchTask {
    _cancel: Sender<()>,
    pub worker: SearchWorker,
}

#[derive(Clone)]
pub(crate) struct SearchWorker {
    pub cancel: Receiver<()>,
    complete: Arc<AtomicBool>,
    error: Arc<Mutex<Option<AppError>>>,
    failed: Arc<AtomicBool>,
    read_any: Arc<AtomicBool>,
    attempted_read: Arc<AtomicBool>,
    pid: process_memory::Pid,
    start_time: u64,
    name: String,
}

impl SearchTask {
    pub fn new(pid: process_memory::Pid, start_time: u64, name: String, complete: Arc<AtomicBool>) -> Self {
        let (sender, cancel) = crossbeam_channel::bounded(1);
        Self {
            _cancel: sender,
            worker: SearchWorker {
                cancel,
                complete,
                error: Arc::new(Mutex::new(None)),
                failed: Arc::new(AtomicBool::new(false)),
                read_any: Arc::new(AtomicBool::new(false)),
                attempted_read: Arc::new(AtomicBool::new(false)),
                pid,
                start_time,
                name,
            },
        }
    }
}

impl SearchWorker {
    pub fn cancelled(&self) -> bool {
        !matches!(self.cancel.try_recv(), Err(TryRecvError::Empty))
    }

    pub fn stopped(&self) -> bool {
        self.cancelled() || self.failed.load(Ordering::Acquire)
    }

    pub fn fail(&self, error: AppError) {
        if let Ok(mut slot) = self.error.lock() {
            if matches!(error, AppError::ProcessExited { .. }) {
                *slot = Some(error);
            } else {
                slot.get_or_insert(error);
            }
        }
        self.failed.store(true, Ordering::Release);
    }

    pub fn error(&self) -> Option<AppError> {
        self.error.lock().ok().and_then(|slot| slot.clone())
    }

    pub fn send<T: Send>(&self, sender: &Sender<T>, batch: T) -> bool {
        if self.stopped() {
            return false;
        }
        crossbeam_channel::select! {
            recv(self.cancel) -> _ => false,
            send(sender, batch) -> sent => sent.is_ok(),
        }
    }

    pub fn record_read(&self, success: bool) {
        self.attempted_read.store(true, Ordering::Relaxed);
        if success {
            self.read_any.store(true, Ordering::Relaxed);
        }
    }

    /// Fresh, PID-specific lookup: never turn an exited/recycled process into
    /// a successful empty result due to a stale UI process cache.
    pub fn check_process(&self) -> bool {
        if self.cancelled() {
            return false;
        }
        let mut system = sysinfo::System::new();
        let pid = sysinfo::Pid::from(self.pid as usize);
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
        let alive = self.pid != 0
            && system.process(pid).is_some_and(|process| {
                (self.start_time == 0 || process.start_time() == self.start_time)
                    && !matches!(process.status(), sysinfo::ProcessStatus::Zombie | sysinfo::ProcessStatus::Dead)
            });
        if !alive {
            self.fail(AppError::ProcessExited { name: self.name.clone() });
        }
        alive
    }

    pub fn finish(&self) {
        if self.cancelled() {
            return;
        }
        self.check_process();
        if !self.stopped() && self.attempted_read.load(Ordering::Relaxed) && !self.read_any.load(Ordering::Relaxed) {
            self.fail(AppError::SearchReadFailed);
        }
        self.complete.store(true, Ordering::Release);
    }

    pub fn reader<T: CopyAddress>(&self, inner: T) -> SearchReader<'_, T> {
        SearchReader { worker: self, inner }
    }
}

pub(crate) struct SearchReader<'a, T> {
    worker: &'a SearchWorker,
    inner: T,
}

impl<T: CopyAddress> CopyAddress for SearchReader<'_, T> {
    fn copy_address(&self, addr: usize, bytes: &mut [u8]) -> io::Result<()> {
        if self.worker.stopped() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "search cancelled"));
        }
        let result = self.inner.copy_address(addr, bytes);
        self.worker.record_read(result.is_ok());
        result
    }

    fn get_pointer_width(&self) -> process_memory::Architecture {
        self.inner.get_pointer_width()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, time::Duration};

    #[test]
    fn tab_drop_wakes_a_sender_blocked_on_a_full_channel() {
        let task = SearchTask::new(0, 0, "test".into(), Arc::new(AtomicBool::new(false)));
        let worker = task.worker.clone();
        let (sender, _receiver) = crossbeam_channel::bounded(1);
        sender.send(1).unwrap();
        let (done_tx, done_rx) = crossbeam_channel::bounded(1);
        let thread = std::thread::spawn(move || done_tx.send(worker.send(&sender, 2)).unwrap());
        drop(task);
        assert!(!done_rx.recv_timeout(Duration::from_secs(2)).unwrap());
        thread.join().unwrap();
    }

    #[test]
    fn cancelled_reader_never_touches_process_memory() {
        struct Reader(Cell<usize>);
        impl CopyAddress for Reader {
            fn copy_address(&self, _: usize, bytes: &mut [u8]) -> io::Result<()> {
                self.0.set(self.0.get() + 1);
                bytes.fill(42);
                Ok(())
            }
            fn get_pointer_width(&self) -> process_memory::Architecture {
                process_memory::Architecture::Arch64Bit
            }
        }
        let task = SearchTask::new(0, 0, "test".into(), Arc::new(AtomicBool::new(false)));
        let worker = task.worker.clone();
        let reader = worker.reader(Reader(Cell::new(0)));
        reader.copy_address(0, &mut [0; 8]).unwrap();
        drop(task);
        assert_eq!(reader.copy_address(0, &mut [0; 8]).unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert_eq!(reader.inner.0.get(), 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_process_exiting_during_a_search_is_not_a_successful_empty_result() {
        use std::process::{Command, Stdio};
        // cat waits on our pipe, giving the test deterministic control of exit.
        let mut child = Command::new("cat").stdin(Stdio::piped()).stdout(Stdio::null()).spawn().unwrap();
        let complete = Arc::new(AtomicBool::new(false));
        let task = SearchTask::new(child.id() as process_memory::Pid, 0, "test child".into(), complete.clone());
        assert!(task.worker.check_process());
        drop(child.stdin.take());
        child.wait().unwrap();
        task.worker.record_read(false);
        task.worker.finish();
        assert!(complete.load(Ordering::Acquire));
        assert!(matches!(task.worker.error(), Some(AppError::ProcessExited { .. })));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn reused_pid_with_a_different_start_time_is_rejected() {
        let task = SearchTask::new(
            std::process::id() as process_memory::Pid,
            u64::MAX,
            "replaced".into(),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(!task.worker.check_process());
        assert!(matches!(task.worker.error(), Some(AppError::ProcessExited { .. })));
    }
}
