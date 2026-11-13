pub mod components;

use crate::git::{FileChange, Mutation};
use std::path::PathBuf;

pub enum Action {
    Open(PathBuf),
    Refresh,
    Changes,
    SelectFile(FileChange),
    History(usize),
    CommitDiff(String),
    Mutate(Mutation),
    Cancel,
}
