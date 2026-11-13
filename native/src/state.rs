use crate::command::{Command, Event};
use crate::git::{CommitSummary, DiffPreview, FileChange, Snapshot};
use crate::storage::Preferences;

pub struct Pending {
    pub id: u64,
    pub command: Command,
}

#[derive(Default)]
pub struct AppState {
    pub generation: u64,
    pub preferences: Preferences,
    pub snapshot: Option<Snapshot>,
    pub selected: Option<FileChange>,
    pub diff: Option<DiffPreview>,
    pub history: Vec<CommitSummary>,
    pub history_head: Option<String>,
    pub history_offset: usize,
    pub history_has_more: bool,
    pub pending: Option<Pending>,
    pub error: Option<String>,
    pub warning: Option<String>,
    pub message: Option<String>,
}

impl AppState {
    pub fn accepts(&self, event: &Event) -> bool {
        event.generation == self.generation
            && self
                .pending
                .as_ref()
                .is_some_and(|pending| pending.id == event.id)
    }

    pub fn writing(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| pending.command.is_mutation())
    }
}
