use std::path::PathBuf;

use crate::git::{DiffPreview, FileChange, HistoryPage, Mutation, Snapshot};
use crate::storage::Preferences;

#[derive(Clone, Debug)]
pub enum Command {
    Preferences,
    Open(PathBuf),
    Diff {
        root: PathBuf,
        file: FileChange,
    },
    History {
        root: PathBuf,
        skip: usize,
    },
    CommitDiff {
        root: PathBuf,
        oid: String,
    },
    Mutate {
        root: PathBuf,
        mutation: Mutation,
        revision: u64,
    },
}

impl Command {
    pub fn is_mutation(&self) -> bool {
        matches!(self, Self::Mutate { .. })
    }
}

#[derive(Debug)]
pub enum Payload {
    Preferences(Preferences),
    Snapshot(Snapshot),
    Diff(DiffPreview),
    History(HistoryPage),
    /// A write is reported separately from the subsequent refresh, so a failed
    /// refresh never misrepresents an already-completed mutation as a failed write.
    Mutation(String),
}

#[derive(Debug)]
pub struct Event {
    pub id: u64,
    pub generation: u64,
    pub result: crate::error::Result<Payload>,
    pub warning: Option<String>,
}
