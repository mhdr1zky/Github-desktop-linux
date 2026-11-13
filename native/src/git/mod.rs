mod compatibility;
mod diff;
mod local;

pub use diff::{DiffLine, DiffPreview, LineKind, MAX_DIFF_BYTES, MAX_DIFF_LINES};
pub use local::LocalGit;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::error::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Staged,
    Unstaged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    /// Native paths are retained for operations. Lossy conversion is display-only.
    pub path: PathBuf,
    pub old_path: Option<PathBuf>,
    pub side: Side,
    pub label: &'static str,
    pub conflicted: bool,
}

#[derive(Clone, Debug)]
pub struct Branch {
    pub name: String,
    pub current: bool,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub root: PathBuf,
    pub head: String,
    pub files: Vec<FileChange>,
    pub branches: Vec<Branch>,
    pub write_blockers: Vec<String>,
    pub revision: u64,
}

#[derive(Clone, Debug)]
pub struct CommitSummary {
    pub oid: String,
    pub summary: String,
    pub author: String,
}

#[derive(Debug)]
pub struct HistoryPage {
    pub commits: Vec<CommitSummary>,
    pub has_more: bool,
    pub head: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Mutation {
    Stage(FileChange),
    Unstage(FileChange),
    Commit {
        summary: String,
        description: String,
    },
    CreateBranch(String),
    Checkout(String),
}

/// All methods are blocking and must only be called by a worker, never the UI.
pub trait GitBackend: Send + 'static {
    fn snapshot(&self, path: &Path, cancel: &AtomicBool) -> Result<Snapshot>;
    fn diff(&self, path: &Path, file: &FileChange, cancel: &AtomicBool) -> Result<DiffPreview>;
    fn history(&self, path: &Path, skip: usize, cancel: &AtomicBool) -> Result<HistoryPage>;
    fn commit_diff(&self, path: &Path, oid: &str, cancel: &AtomicBool) -> Result<DiffPreview>;
    fn mutate(&self, path: &Path, mutation: &Mutation, expected_revision: u64) -> Result<String>;
}
