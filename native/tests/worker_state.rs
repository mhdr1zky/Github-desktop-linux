use std::path::Path;
use std::sync::{Arc, atomic::AtomicBool, mpsc};
use std::time::Duration;

use gitrust_desktop::{
    command::{Command, Event, Payload},
    error::{Error, Result},
    git::{DiffPreview, FileChange, GitBackend, HistoryPage, Mutation, Snapshot},
    state::{AppState, Pending},
    worker::Worker,
};

struct BlockedBackend {
    entered: mpsc::Sender<()>,
    release: std::sync::Mutex<mpsc::Receiver<()>>,
}
impl GitBackend for BlockedBackend {
    fn snapshot(&self, _path: &Path, _cancel: &AtomicBool) -> Result<Snapshot> {
        self.entered.send(()).unwrap();
        self.release.lock().unwrap().recv().unwrap();
        Err(Error::Invalid("finished deliberately blocked read".into()))
    }
    fn diff(&self, _: &Path, _: &FileChange, _: &AtomicBool) -> Result<DiffPreview> {
        unreachable!()
    }
    fn history(&self, _: &Path, _: usize, _: &AtomicBool) -> Result<HistoryPage> {
        unreachable!()
    }
    fn commit_diff(&self, _: &Path, _: &str, _: &AtomicBool) -> Result<DiffPreview> {
        unreachable!()
    }
    fn mutate(&self, _: &Path, _: &Mutation, _: u64) -> Result<String> {
        unreachable!()
    }
}

#[test]
fn full_queue_never_blocks_ui_and_drop_does_not_join_io() {
    let (entered, wait_entered) = mpsc::channel();
    let (release, wait_release) = mpsc::channel();
    let (wake, wait_wake) = mpsc::channel();
    let backend = BlockedBackend {
        entered,
        release: std::sync::Mutex::new(wait_release),
    };
    let worker = Worker::spawn(backend, None, move || {
        let _ = wake.send(());
    })
    .unwrap();
    // Drive the API from a separate thread so a regression cannot hang the test suite forever.
    let (done, wait_done) = mpsc::channel();
    let worker = Arc::new(std::sync::Mutex::new(Some(worker)));
    let owned = worker.clone();
    std::thread::spawn(move || {
        let mut guard = owned.lock().unwrap();
        let worker = guard.as_mut().unwrap();
        worker.submit(1, Command::Open("/first".into())).unwrap();
        wait_entered.recv_timeout(Duration::from_secs(5)).unwrap();
        worker.submit(2, Command::Open("/second".into())).unwrap();
        assert!(worker.submit(3, Command::Open("/third".into())).is_err());
        drop(guard.take());
        done.send(()).unwrap();
    });
    wait_done
        .recv_timeout(Duration::from_secs(5))
        .expect("UI submission/drop must not wait for blocked Git I/O");
    release.send(()).unwrap();
    // The event receiver was dropped; no repaint should be emitted after shutdown.
    assert!(wait_wake.recv_timeout(Duration::from_secs(5)).is_err());
}

#[test]
fn stale_generation_and_superseded_request_are_rejected() {
    let mut state = AppState {
        generation: 2,
        pending: Some(Pending {
            id: 9,
            command: Command::Preferences,
        }),
        ..Default::default()
    };
    let mut event = Event {
        id: 9,
        generation: 1,
        result: Ok(Payload::Preferences(Default::default())),
        warning: None,
    };
    assert!(!state.accepts(&event));
    event.generation = 2;
    event.id = 8;
    assert!(!state.accepts(&event));
    event.id = 9;
    assert!(state.accepts(&event));
    state.pending = None;
    assert!(!state.accepts(&event));
}
