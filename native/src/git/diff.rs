use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use git2::{DiffFlags, DiffFormat, DiffOptions, ErrorCode, Repository};

use super::{FileChange, Side};
use crate::error::{Error, Result};

pub const MAX_DIFF_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_DIFF_LINES: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Addition,
    Deletion,
    Context,
    Header,
}

#[derive(Debug)]
pub struct DiffLine {
    pub bytes: Range<usize>,
    pub kind: LineKind,
    pub old_number: Option<u32>,
    pub new_number: Option<u32>,
}

#[derive(Debug, Default)]
pub struct DiffPreview {
    pub buffer: Vec<u8>,
    pub lines: Vec<DiffLine>,
    pub notice: Option<String>,
}

impl DiffPreview {
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            notice: Some(message.into()),
            ..Self::default()
        }
    }
}

pub fn preview(repo: &Repository, file: &FileChange, cancel: &AtomicBool) -> Result<DiffPreview> {
    super::compatibility::safe_relative(&file.path)?;
    if file.conflicted {
        return Ok(DiffPreview::message(
            "Merge conflict: resolve using Git or your editor first.",
        ));
    }
    // Check both sides before asking libgit2 to materialize a patch.
    let index = repo.index()?;
    if let Some(entry) = index.get_path(&file.path, 0) {
        if entry.mode & 0o170000 == 0o160000 {
            return Ok(DiffPreview::message(
                "Submodule changes: textual preview is not supported yet.",
            ));
        }
        if entry.id != git2::Oid::zero() && repo.find_blob(entry.id)?.size() > MAX_DIFF_BYTES {
            return Ok(DiffPreview::message(
                "File exceeds the 2 MiB preview limit. Open it in your editor.",
            ));
        }
    }
    if file.side == Side::Unstaged {
        if let Some(root) = repo.workdir() {
            match std::fs::symlink_metadata(root.join(&file.path)) {
                Ok(meta) if meta.len() > MAX_DIFF_BYTES as u64 => {
                    return Ok(DiffPreview::message(
                        "File exceeds the 2 MiB preview limit. Open it in your editor.",
                    ));
                }
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(err.into()),
            }
        }
    }
    let head_tree = match repo.head() {
        Ok(head) => Some(head.peel_to_tree()?),
        Err(err) if matches!(err.code(), ErrorCode::UnbornBranch | ErrorCode::NotFound) => None,
        Err(err) => return Err(err.into()),
    };
    if file.side == Side::Staged {
        let old_path: &Path = file.old_path.as_deref().unwrap_or(&file.path);
        if let Some(tree) = &head_tree {
            if let Ok(entry) = tree.get_path(old_path) {
                if entry.kind() == Some(git2::ObjectType::Blob)
                    && repo.find_blob(entry.id())?.size() > MAX_DIFF_BYTES
                {
                    return Ok(DiffPreview::message(
                        "Previous file version exceeds the 2 MiB preview limit.",
                    ));
                }
            }
        }
    }
    let mut options = DiffOptions::new();
    options
        .pathspec(&file.path)
        .disable_pathspec_match(true)
        .context_lines(3)
        .max_size(MAX_DIFF_BYTES as i64)
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .show_untracked_content(true);
    if let Some(old_path) = &file.old_path {
        options.pathspec(old_path);
    }
    let diff = match file.side {
        Side::Staged => {
            repo.diff_tree_to_index(head_tree.as_ref(), Some(&index), Some(&mut options))?
        }
        Side::Unstaged => repo.diff_index_to_workdir(Some(&index), Some(&mut options))?,
    };
    collect(&diff, cancel)
}

pub(super) fn collect(diff: &git2::Diff<'_>, cancel: &AtomicBool) -> Result<DiffPreview> {
    let mut result = DiffPreview::default();
    let mut truncated = false;
    let print_result = diff.print(DiffFormat::Patch, |delta, _, line| {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        if delta.flags().contains(DiffFlags::BINARY) {
            result.notice = Some("Binary file: textual preview is unavailable.".into());
            return true;
        }
        let content = line.content();
        if result.buffer.len() + content.len() > MAX_DIFF_BYTES
            || result.lines.len() >= MAX_DIFF_LINES
        {
            truncated = true;
            return false;
        }
        let start = result.buffer.len();
        result.buffer.extend_from_slice(content);
        result.lines.push(DiffLine {
            bytes: start..result.buffer.len(),
            kind: match line.origin() {
                '+' => LineKind::Addition,
                '-' => LineKind::Deletion,
                ' ' => LineKind::Context,
                _ => LineKind::Header,
            },
            old_number: line.old_lineno(),
            new_number: line.new_lineno(),
        });
        true
    });
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    if truncated {
        result.notice = Some("Preview truncated at 2 MiB / 20,000 lines. Open the file in your editor for full content.".into());
    } else {
        print_result?;
    }
    if result.lines.is_empty() && result.notice.is_none() {
        result.notice = Some(
            "No text changes. The file may have changed externally; refresh the repository.".into(),
        );
    }
    Ok(result)
}
