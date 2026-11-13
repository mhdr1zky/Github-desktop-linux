use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use git2::{BranchType, ErrorCode, Repository, Status, StatusOptions};

use super::{Branch, CommitSummary, FileChange, GitBackend, HistoryPage, Mutation, Side, Snapshot};
use crate::error::{Error, Result};

pub const HISTORY_PAGE_SIZE: usize = 100;

#[derive(Default)]
pub struct LocalGit;

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

fn open(path: &Path) -> Result<Repository> {
    let repo = Repository::discover(path)?;
    if repo.is_bare() {
        return Err(Error::Unsupported(
            "Open a working-tree repository; bare repositories are not supported yet.".into(),
        ));
    }
    Ok(repo)
}

fn revision(repo: &Repository) -> Result<u64> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    repo.head()
        .ok()
        .and_then(|head| head.target())
        .hash(&mut hasher);
    // Include the symbolic HEAD as unborn branches have no target.
    std::fs::read(repo.path().join("HEAD"))?.hash(&mut hasher);
    match std::fs::File::open(repo.path().join("index")) {
        Ok(mut file) => {
            let mut buffer = [0u8; 8192];
            loop {
                let count = file.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                hasher.write(&buffer[..count]);
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    Ok(hasher.finish())
}

#[cfg(unix)]
fn native_path(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn native_path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).as_ref())
}

fn label(status: Status, staged: bool) -> &'static str {
    if status.is_conflicted() {
        "Conflict"
    } else if status.intersects(if staged {
        Status::INDEX_NEW
    } else {
        Status::WT_NEW
    }) {
        "Added"
    } else if status.intersects(if staged {
        Status::INDEX_DELETED
    } else {
        Status::WT_DELETED
    }) {
        "Deleted"
    } else if status.intersects(if staged {
        Status::INDEX_RENAMED
    } else {
        Status::WT_RENAMED
    }) {
        "Renamed"
    } else if status.intersects(if staged {
        Status::INDEX_TYPECHANGE
    } else {
        Status::WT_TYPECHANGE
    }) {
        "Type changed"
    } else {
        "Modified"
    }
}

impl GitBackend for LocalGit {
    fn snapshot(&self, path: &Path, cancel: &AtomicBool) -> Result<Snapshot> {
        let repo = open(path)?;
        check_cancel(cancel)?;
        let before = revision(&repo)?;
        let head = match repo.head() {
            Ok(head) if head.is_branch() => head.shorthand().unwrap_or("(non-UTF-8 branch)").into(),
            Ok(head) => format!(
                "Detached {}",
                head.target()
                    .map(|v| v.to_string()[..8].to_owned())
                    .unwrap_or_default()
            ),
            Err(err) if matches!(err.code(), ErrorCode::UnbornBranch | ErrorCode::NotFound) => repo
                .find_reference("HEAD")?
                .symbolic_target()
                .unwrap_or("unborn HEAD")
                .trim_start_matches("refs/heads/")
                .to_owned(),
            Err(err) => return Err(err.into()),
        };
        let mut options = StatusOptions::new();
        options
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .renames_head_to_index(true)
            .renames_index_to_workdir(true);
        let statuses = repo.statuses(Some(&mut options))?;
        let mut files = Vec::new();
        for entry in statuses.iter() {
            check_cancel(cancel)?;
            let status = entry.status();
            for side in [Side::Staged, Side::Unstaged] {
                let staged = side == Side::Staged;
                let mask = if staged {
                    Status::INDEX_NEW
                        | Status::INDEX_MODIFIED
                        | Status::INDEX_DELETED
                        | Status::INDEX_RENAMED
                        | Status::INDEX_TYPECHANGE
                } else {
                    Status::WT_NEW
                        | Status::WT_MODIFIED
                        | Status::WT_DELETED
                        | Status::WT_RENAMED
                        | Status::WT_TYPECHANGE
                        | Status::CONFLICTED
                };
                if !status.intersects(mask) {
                    continue;
                }
                let delta = if staged {
                    entry.head_to_index()
                } else {
                    entry.index_to_workdir()
                };
                let file_path = delta
                    .as_ref()
                    .and_then(|v| v.new_file().path_bytes())
                    .map(native_path)
                    .unwrap_or_else(|| native_path(entry.path_bytes()));
                let old_path = delta
                    .as_ref()
                    .and_then(|v| v.old_file().path_bytes())
                    .map(native_path)
                    .filter(|old| *old != file_path);
                files.push(FileChange {
                    path: file_path,
                    old_path,
                    side,
                    label: label(status, staged),
                    conflicted: status.is_conflicted(),
                });
            }
        }
        let mut branches = Vec::new();
        for branch in repo.branches(Some(BranchType::Local))? {
            check_cancel(cancel)?;
            let (branch, _) = branch?;
            if let Some(name) = branch.name()? {
                branches.push(Branch {
                    name: name.into(),
                    current: branch.is_head(),
                });
            }
        }
        branches.sort_by(|a, b| a.name.cmp(&b.name));
        let after = revision(&repo)?;
        if before != after {
            return Err(Error::Invalid(
                "Repository changed while reading. Refresh to try again.".into(),
            ));
        }
        Ok(Snapshot {
            root: repo.workdir().expect("working tree checked").to_owned(),
            head,
            files,
            branches,
            write_blockers: super::compatibility::write_blockers(&repo)?,
            revision: after,
        })
    }

    fn diff(
        &self,
        path: &Path,
        file: &FileChange,
        cancel: &AtomicBool,
    ) -> Result<super::DiffPreview> {
        check_cancel(cancel)?;
        super::diff::preview(&open(path)?, file, cancel)
    }

    fn history(&self, path: &Path, skip: usize, cancel: &AtomicBool) -> Result<HistoryPage> {
        let repo = open(path)?;
        check_cancel(cancel)?;
        let head = match repo.head() {
            Ok(head) => head.target(),
            Err(err) if matches!(err.code(), ErrorCode::UnbornBranch | ErrorCode::NotFound) => None,
            Err(err) => return Err(err.into()),
        };
        let Some(head) = head else {
            return Ok(HistoryPage {
                commits: Vec::new(),
                has_more: false,
                head: None,
            });
        };
        let mut walk = repo.revwalk()?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;
        walk.push(head)?;
        let mut commits = Vec::new();
        let mut has_more = false;
        for (offset, oid) in walk.enumerate() {
            check_cancel(cancel)?;
            let oid = oid?;
            if offset < skip {
                continue;
            }
            if commits.len() == HISTORY_PAGE_SIZE {
                has_more = true;
                break;
            }
            let commit = repo.find_commit(oid)?;
            commits.push(CommitSummary {
                oid: oid.to_string(),
                summary: String::from_utf8_lossy(commit.summary_bytes().unwrap_or_default())
                    .into_owned(),
                author: String::from_utf8_lossy(commit.author().name_bytes()).into_owned(),
            });
        }
        Ok(HistoryPage {
            commits,
            has_more,
            head: Some(head.to_string()),
        })
    }

    fn commit_diff(
        &self,
        path: &Path,
        oid: &str,
        cancel: &AtomicBool,
    ) -> Result<super::DiffPreview> {
        check_cancel(cancel)?;
        let repo = open(path)?;
        let commit = repo.find_commit(git2::Oid::from_str(oid)?)?;
        let tree = commit.tree()?;
        let parent = if commit.parent_count() > 0 {
            Some(commit.parent(0)?.tree()?)
        } else {
            None
        };
        let mut options = git2::DiffOptions::new();
        options
            .context_lines(3)
            .max_size(super::MAX_DIFF_BYTES as i64);
        let diff = repo.diff_tree_to_tree(parent.as_ref(), Some(&tree), Some(&mut options))?;
        let mut preview = super::diff::collect(&diff, cancel)?;
        if commit.parent_count() > 1 {
            preview.notice =
                Some("Merge commit: showing changes relative to the first parent.".into());
        }
        Ok(preview)
    }

    fn mutate(&self, path: &Path, mutation: &Mutation, expected_revision: u64) -> Result<String> {
        let repo = open(path)?;
        super::compatibility::ensure_writable(&repo)?;
        if revision(&repo)? != expected_revision {
            return Err(Error::Invalid(
                "HEAD or staging changed externally. Refresh and review before trying again."
                    .into(),
            ));
        }
        match mutation {
            Mutation::Stage(file) => {
                if file.conflicted {
                    return Err(Error::Unsupported(
                        "Resolve conflicts with Git first.".into(),
                    ));
                }
                if file.side != Side::Unstaged {
                    return Err(Error::Invalid("Select an unstaged file.".into()));
                }
                let path = super::compatibility::safe_relative(&file.path)?;
                let mut index = repo.index()?;
                match std::fs::symlink_metadata(repo.workdir().unwrap().join(&path)) {
                    Ok(meta) if meta.is_dir() => {
                        return Err(Error::Unsupported(
                            "Directory/submodule staging is not supported yet.".into(),
                        ));
                    }
                    Ok(_) => index.add_path(&path)?,
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                        index.remove_path(&path)?
                    }
                    Err(err) => return Err(err.into()),
                }
                if let Some(old_path) = &file.old_path {
                    index.remove_path(&super::compatibility::safe_relative(old_path)?)?;
                }
                index.write()?;
                Ok("File staged".into())
            }
            Mutation::Unstage(file) => {
                if file.side != Side::Staged {
                    return Err(Error::Invalid("Select a staged file.".into()));
                }
                let path = super::compatibility::safe_relative(&file.path)?;
                let commit = match repo.head() {
                    Ok(head) => Some(head.peel_to_commit()?),
                    Err(err)
                        if matches!(err.code(), ErrorCode::UnbornBranch | ErrorCode::NotFound) =>
                    {
                        None
                    }
                    Err(err) => return Err(err.into()),
                };
                let object = commit.as_ref().map(|commit| commit.as_object());
                let mut paths = vec![path];
                if let Some(old) = &file.old_path {
                    paths.push(super::compatibility::safe_relative(old)?);
                }
                repo.reset_default(object, paths.iter())?;
                Ok("File unstaged; working file unchanged".into())
            }
            Mutation::Commit {
                summary,
                description,
            } => {
                if summary.trim().is_empty()
                    || summary.contains(['\n', '\r', '\0'])
                    || description.contains('\0')
                {
                    return Err(Error::Invalid(
                        "Enter a non-empty, single-line commit summary.".into(),
                    ));
                }
                let mut index = repo.index()?;
                if index.has_conflicts() {
                    return Err(Error::Invalid(
                        "Resolve index conflicts before committing.".into(),
                    ));
                }
                let signature = repo.signature().map_err(|_| {
                    Error::Invalid(
                        "Configure Git user.name and user.email before committing.".into(),
                    )
                })?;
                let parent = match repo.head() {
                    Ok(head) => Some(head.peel_to_commit()?),
                    Err(err)
                        if matches!(err.code(), ErrorCode::UnbornBranch | ErrorCode::NotFound) =>
                    {
                        None
                    }
                    Err(err) => return Err(err.into()),
                };
                let tree_id = index.write_tree()?;
                let tree = repo.find_tree(tree_id)?;
                if parent
                    .as_ref()
                    .is_some_and(|commit| commit.tree_id() == tree_id)
                    || (parent.is_none() && tree.is_empty())
                {
                    return Err(Error::Invalid(
                        "There are no staged changes to commit.".into(),
                    ));
                }
                let message = if description.trim().is_empty() {
                    summary.trim().to_owned()
                } else {
                    format!("{}\n\n{}", summary.trim(), description.trim())
                };
                let parents: Vec<_> = parent.iter().collect();
                // Recheck after tree creation; never clear/re-stage the user's index.
                if revision(&repo)? != expected_revision {
                    return Err(Error::Invalid(
                        "Repository changed before commit. Refresh and review.".into(),
                    ));
                }
                let oid = repo.commit(
                    Some("HEAD"),
                    &signature,
                    &signature,
                    &message,
                    &tree,
                    &parents,
                )?;
                Ok(format!("Committed {}", &oid.to_string()[..8]))
            }
            Mutation::CreateBranch(name) => {
                if !git2::Reference::is_valid_name(&format!("refs/heads/{name}"))
                    || name.starts_with('-')
                {
                    return Err(Error::Invalid("Enter a valid Git branch name.".into()));
                }
                let commit = repo.head()?.peel_to_commit()?;
                repo.branch(name, &commit, false)?;
                Ok(format!("Created branch {name}; select it to switch"))
            }
            Mutation::Checkout(name) => {
                let mut status_options = StatusOptions::new();
                status_options
                    .include_untracked(true)
                    .recurse_untracked_dirs(true);
                if !repo.statuses(Some(&mut status_options))?.is_empty() {
                    return Err(Error::Invalid(
                        "Commit or stash all local changes with Git before switching branches."
                            .into(),
                    ));
                }
                let branch = repo.find_branch(name, BranchType::Local)?;
                let target = branch.get().peel_to_commit()?;
                let mut checkout = git2::build::CheckoutBuilder::new();
                checkout.safe();
                repo.checkout_tree(target.as_object(), Some(&mut checkout))?;
                let reference = branch.get().name().ok_or_else(|| {
                    Error::Unsupported("Non-UTF-8 branch names are not supported yet.".into())
                })?;
                repo.set_head(reference)?;
                Ok(format!("Switched to {name}"))
            }
        }
    }
}
