use std::path::PathBuf;

use git2::{Repository, RepositoryState};

use crate::error::{Error, Result};

/// Conservative until Git CLI adapters for hooks, signing and filters exist.
pub fn write_blockers(repo: &Repository) -> Result<Vec<String>> {
    let mut blockers = Vec::new();
    if repo.state() != RepositoryState::Clean {
        blockers.push(
            "A merge/rebase or another Git operation is in progress. Finish it in Git first."
                .into(),
        );
    }
    let config = repo.config()?;
    let signing = match config.get_bool("commit.gpgsign") {
        Ok(value) => value,
        Err(err) if err.code() == git2::ErrorCode::NotFound => false,
        Err(err) => return Err(err.into()),
    };
    if signing {
        blockers.push(
            "Signed commits are not supported yet. Use Git to commit this repository.".into(),
        );
    }
    if config
        .get_string("extensions.objectformat")
        .is_ok_and(|v| v != "sha1")
    {
        blockers.push("This repository uses an unsupported object format.".into());
    }
    let mut entries = config.entries(Some("^filter\\..*"))?;
    if entries.next().is_some() {
        blockers.push(
            "Configured Git filters/LFS require the Git CLI adapter, which is not implemented yet."
                .into(),
        );
    }
    let default_hooks = repo.commondir().join("hooks");
    let hooks = match config.get_path("core.hookspath") {
        Ok(path) if path.is_absolute() => path,
        Ok(path) => repo.workdir().unwrap_or(repo.path()).join(path),
        Err(err) if err.code() == git2::ErrorCode::NotFound => default_hooks,
        Err(err) => return Err(err.into()),
    };
    match std::fs::read_dir(&hooks) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                if entry.file_name().to_string_lossy().ends_with(".sample") {
                    continue;
                }
                let metadata = std::fs::metadata(entry.path())?;
                #[cfg(unix)]
                let executable = {
                    use std::os::unix::fs::PermissionsExt;
                    metadata.permissions().mode() & 0o111 != 0
                };
                #[cfg(not(unix))]
                let executable = true;
                if metadata.is_file() && executable {
                    blockers.push("Executable Git hooks detected. Native writes are disabled to avoid bypassing hooks.".into());
                    break;
                }
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    Ok(blockers)
}

pub fn ensure_writable(repo: &Repository) -> Result<()> {
    let blockers = write_blockers(repo)?;
    if !blockers.is_empty() {
        return Err(Error::Unsupported(blockers.join("\n")));
    }
    if repo.workdir().is_none() {
        return Err(Error::Unsupported(
            "Bare repositories are read-only in this app.".into(),
        ));
    }
    if repo.path().join("index.lock").exists() {
        return Err(Error::Invalid("Git index.lock exists. Wait for the other Git operation to finish; do not delete its lock.".into()));
    }
    Ok(())
}

pub fn safe_relative(path: &std::path::Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(Error::Invalid(
            "Expected a repository-relative file path.".into(),
        ));
    }
    Ok(path.to_owned())
}
