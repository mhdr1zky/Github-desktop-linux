use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

use crate::command::{Command, Event, Payload};
use crate::error::Error;
use crate::git::GitBackend;
use crate::storage::{self, Preferences};

struct Request {
    id: u64,
    generation: u64,
    command: Command,
    cancel: Arc<AtomicBool>,
}

pub struct Worker {
    requests: mpsc::SyncSender<Request>,
    events: mpsc::Receiver<Event>,
    active_cancel: Option<Arc<AtomicBool>>,
    next_id: u64,
}

impl Worker {
    pub fn spawn(
        backend: impl GitBackend,
        preferences_path: Option<PathBuf>,
        wake: impl Fn() + Send + 'static,
    ) -> std::io::Result<Self> {
        // At most one queued request and one waiting result. A diff is <=2 MiB.
        let (requests, receive) = mpsc::sync_channel::<Request>(1);
        let (send_event, events) = mpsc::sync_channel(1);
        std::thread::Builder::new().name("gitrust-local".into()).spawn(move || {
            let mut preferences = Preferences::default();
            let mut persistence_enabled = preferences_path.is_some();
            let mut initialized = false;
            while let Ok(request) = receive.recv() {
                let mut warning = None;
                if !initialized {
                    if let Some(path) = &preferences_path {
                        match storage::load(path) {
                            Ok(loaded) => preferences = loaded,
                            Err(err) => {
                                persistence_enabled = false;
                                warning = Some(format!("Preferences could not be loaded; persistence disabled for this session: {err}"));
                            }
                        }
                    }
                    initialized = true;
                }
                let result = if request.cancel.load(Ordering::Relaxed) {
                    Err(Error::Cancelled)
                } else {
                    match &request.command {
                        Command::Preferences => Ok(Payload::Preferences(preferences.clone())),
                        Command::Open(path) => backend.snapshot(path, &request.cancel).map(|snapshot| {
                            preferences.remember(snapshot.root.clone());
                            if persistence_enabled {
                                if let Some(path) = &preferences_path {
                                    if let Err(err) = storage::save(path, &preferences) {
                                        warning = Some(format!("Repository opened, but preferences were not saved: {err}"));
                                    }
                                }
                            }
                            Payload::Snapshot(snapshot)
                        }),
                        Command::Diff { root, file } => backend.diff(root, file, &request.cancel).map(Payload::Diff),
                        Command::History { root, skip } => backend.history(root, *skip, &request.cancel).map(Payload::History),
                        Command::CommitDiff { root, oid } => backend.commit_diff(root, oid, &request.cancel).map(Payload::Diff),
                        Command::Mutate { root, mutation, revision } => backend.mutate(root, mutation, *revision).map(Payload::Mutation),
                    }
                };
                if send_event.send(Event { id: request.id, generation: request.generation, result, warning }).is_err() {
                    break;
                }
                wake();
            }
        })?;
        Ok(Self {
            requests,
            events,
            active_cancel: None,
            next_id: 1,
        })
    }

    /// Never blocks the UI. The caller only updates selection/pending state on success.
    pub fn submit(&mut self, generation: u64, command: Command) -> Result<u64, &'static str> {
        let cancel = Arc::new(AtomicBool::new(false));
        let id = self.next_id;
        let request = Request {
            id,
            generation,
            command,
            cancel: cancel.clone(),
        };
        match self.requests.try_send(request) {
            Ok(()) => {
                if let Some(previous) = self.active_cancel.replace(cancel) {
                    previous.store(true, Ordering::Relaxed);
                }
                self.next_id += 1;
                Ok(id)
            }
            Err(mpsc::TrySendError::Full(_)) => Err("Worker queue is busy. Try again in a moment."),
            Err(mpsc::TrySendError::Disconnected(_)) => {
                Err("Background worker stopped. Restart the application.")
            }
        }
    }

    pub fn try_event(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    pub fn cancel(&self) {
        if let Some(cancel) = &self.active_cancel {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Disconnect on drop; intentionally do not join a blocking Git operation on the UI thread.
        self.cancel();
    }
}
