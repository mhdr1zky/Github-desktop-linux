use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicBool;

use git2::Repository;
use gitrust_desktop::git::{FileChange, GitBackend, LineKind, LocalGit, Mutation, Side, Snapshot};
use tempfile::TempDir;

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("Git CLI is required as an independent oracle");
    assert!(
        output.status.success(),
        "{:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn fixture() -> TempDir {
    // All tests in this binary enter this OnceLock before any libgit2 access.
    // Isolate the oracle fixtures from the developer's global hooks/LFS/identity.
    static CONFIG: std::sync::OnceLock<TempDir> = std::sync::OnceLock::new();
    CONFIG.get_or_init(|| {
        let config = tempfile::tempdir().unwrap();
        for level in [
            git2::ConfigLevel::System,
            git2::ConfigLevel::Global,
            git2::ConfigLevel::XDG,
            git2::ConfigLevel::ProgramData,
        ] {
            // SAFETY: initialization completes before any test can access Git.
            unsafe {
                git2::opts::set_search_path(level, config.path()).unwrap();
            }
        }
        config
    });
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    let mut config = repo.config().unwrap();
    config.set_str("user.name", "Native Test").unwrap();
    config.set_str("user.email", "native@example.test").unwrap();
    config.set_bool("commit.gpgsign", false).unwrap();
    config
        .set_str(
            "core.hooksPath",
            dir.path().join("no-hooks").to_str().unwrap(),
        )
        .unwrap();
    dir
}

fn snapshot(root: &Path) -> Snapshot {
    LocalGit.snapshot(root, &AtomicBool::new(false)).unwrap()
}

fn selected(snapshot: &Snapshot, path: &str, side: Side) -> FileChange {
    snapshot
        .files
        .iter()
        .find(|file| file.path == Path::new(path) && file.side == side)
        .unwrap()
        .clone()
}

fn mutate(root: &Path, mutation: Mutation) {
    LocalGit
        .mutate(root, &mutation, snapshot(root).revision)
        .unwrap();
}

fn initial_commit(root: &Path) {
    std::fs::write(root.join("tracked.txt"), "original\n").unwrap();
    git(root, &["add", "tracked.txt"]);
    git(
        root,
        &["-c", "core.hooksPath=/dev/null", "commit", "-m", "initial"],
    );
}

#[test]
fn staged_and_unstaged_diff_use_different_sources() {
    let dir = fixture();
    let root = dir.path();
    initial_commit(root);
    std::fs::write(root.join("tracked.txt"), "staged\n").unwrap();
    git(root, &["add", "tracked.txt"]);
    std::fs::write(root.join("tracked.txt"), "working\n").unwrap();
    let state = snapshot(root);
    assert_eq!(state.files.len(), 2);
    assert_eq!(git(root, &["status", "--porcelain=v1"]), "MM tracked.txt\n");
    let staged = LocalGit
        .diff(
            root,
            &selected(&state, "tracked.txt", Side::Staged),
            &AtomicBool::new(false),
        )
        .unwrap();
    let unstaged = LocalGit
        .diff(
            root,
            &selected(&state, "tracked.txt", Side::Unstaged),
            &AtomicBool::new(false),
        )
        .unwrap();
    let added = |diff: &gitrust_desktop::git::DiffPreview| -> Vec<String> {
        diff.lines
            .iter()
            .filter(|line| line.kind == LineKind::Addition)
            .map(|line| String::from_utf8_lossy(&diff.buffer[line.bytes.clone()]).into_owned())
            .collect()
    };
    assert_eq!(added(&staged), ["staged\n"]);
    assert_eq!(added(&unstaged), ["working\n"]);
}

#[test]
fn commit_preserves_terminal_staging_and_unstaged_work() {
    let dir = fixture();
    let root = dir.path();
    initial_commit(root);
    std::fs::write(root.join("tracked.txt"), "staged\n").unwrap();
    git(root, &["add", "tracked.txt"]);
    std::fs::write(root.join("tracked.txt"), "not staged\n").unwrap();
    std::fs::write(root.join("untracked.txt"), "do not commit\n").unwrap();
    let index_before = git(root, &["ls-files", "--stage"]);
    mutate(
        root,
        Mutation::Commit {
            summary: "keep index".into(),
            description: "body".into(),
        },
    );
    assert_eq!(git(root, &["show", "HEAD:tracked.txt"]), "staged\n");
    assert_eq!(git(root, &["ls-files", "--stage"]), index_before);
    assert_eq!(
        std::fs::read_to_string(root.join("tracked.txt")).unwrap(),
        "not staged\n"
    );
    assert_eq!(
        git(root, &["status", "--porcelain=v1"]),
        " M tracked.txt\n?? untracked.txt\n"
    );
    assert_eq!(
        git(root, &["log", "-1", "--format=%B"]),
        "keep index\n\nbody\n"
    );
}

#[test]
fn unborn_stage_unstage_and_initial_commit() {
    let dir = fixture();
    let root = dir.path();
    std::fs::write(root.join("new.txt"), "hello\n").unwrap();
    let state = snapshot(root);
    mutate(
        root,
        Mutation::Stage(selected(&state, "new.txt", Side::Unstaged)),
    );
    assert_eq!(git(root, &["status", "--porcelain=v1"]), "A  new.txt\n");
    let state = snapshot(root);
    mutate(
        root,
        Mutation::Unstage(selected(&state, "new.txt", Side::Staged)),
    );
    assert_eq!(git(root, &["status", "--porcelain=v1"]), "?? new.txt\n");
    assert_eq!(
        std::fs::read_to_string(root.join("new.txt")).unwrap(),
        "hello\n"
    );
    let state = snapshot(root);
    mutate(
        root,
        Mutation::Stage(selected(&state, "new.txt", Side::Unstaged)),
    );
    mutate(
        root,
        Mutation::Commit {
            summary: "first".into(),
            description: String::new(),
        },
    );
    assert_eq!(git(root, &["rev-list", "--count", "HEAD"]), "1\n");
    assert!(snapshot(root).files.is_empty());
}

#[test]
fn stage_deletion_and_unstage_rename_preserve_working_files() {
    let dir = fixture();
    let root = dir.path();
    initial_commit(root);
    git(root, &["mv", "tracked.txt", "renamed.txt"]);
    let state = snapshot(root);
    let renamed = selected(&state, "renamed.txt", Side::Staged);
    assert_eq!(renamed.old_path.as_deref(), Some(Path::new("tracked.txt")));
    mutate(root, Mutation::Unstage(renamed));
    assert_eq!(
        git(root, &["status", "--porcelain=v1"]),
        " D tracked.txt\n?? renamed.txt\n"
    );
    let state = snapshot(root);
    mutate(
        root,
        Mutation::Stage(selected(&state, "tracked.txt", Side::Unstaged)),
    );
    assert_eq!(
        git(root, &["diff", "--cached", "--name-status"]),
        "D\ttracked.txt\n"
    );
    assert!(root.join("renamed.txt").exists());
}

#[test]
fn external_index_change_rejects_stale_commit_without_losing_staging() {
    let dir = fixture();
    let root = dir.path();
    initial_commit(root);
    let before = snapshot(root);
    let head = git(root, &["rev-parse", "HEAD"]);
    std::fs::write(root.join("tracked.txt"), "external\n").unwrap();
    git(root, &["add", "tracked.txt"]);
    let err = LocalGit
        .mutate(
            root,
            &Mutation::Commit {
                summary: "stale".into(),
                description: String::new(),
            },
            before.revision,
        )
        .unwrap_err();
    assert!(err.to_string().contains("externally"));
    assert_eq!(git(root, &["rev-parse", "HEAD"]), head);
    assert_eq!(git(root, &["show", ":tracked.txt"]), "external\n");
}

#[test]
fn hooks_signing_filters_and_index_lock_fail_closed() {
    let dir = fixture();
    let root = dir.path();
    initial_commit(root);
    let repo = Repository::open(root).unwrap();
    let mut config = repo.config().unwrap();
    config.set_bool("commit.gpgsign", true).unwrap();
    assert!(
        snapshot(root)
            .write_blockers
            .iter()
            .any(|s| s.contains("Signed"))
    );
    let mutation = Mutation::CreateBranch("blocked".into());
    assert!(
        LocalGit
            .mutate(root, &mutation, snapshot(root).revision)
            .is_err()
    );
    config.set_bool("commit.gpgsign", false).unwrap();
    config
        .set_str("filter.lfs.clean", "git-lfs clean -- %f")
        .unwrap();
    assert!(
        snapshot(root)
            .write_blockers
            .iter()
            .any(|s| s.contains("filters"))
    );
    config.remove("filter.lfs.clean").unwrap();
    std::fs::create_dir(root.join("no-hooks")).unwrap();
    let hook = root.join("no-hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert!(
        snapshot(root)
            .write_blockers
            .iter()
            .any(|s| s.contains("hooks"))
    );
    std::fs::remove_file(hook).unwrap();
    std::fs::write(repo.path().join("index.lock"), "external lock").unwrap();
    assert!(
        LocalGit
            .mutate(root, &mutation, snapshot(root).revision)
            .unwrap_err()
            .to_string()
            .contains("index.lock")
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("index.lock")).unwrap(),
        "external lock"
    );
}

#[test]
fn checkout_blocks_dirty_tree_and_can_switch_clean_branches() {
    let dir = fixture();
    let root = dir.path();
    initial_commit(root);
    mutate(root, Mutation::CreateBranch("feature/native".into()));
    std::fs::write(root.join("tracked.txt"), "keep this edit\n").unwrap();
    let error = LocalGit
        .mutate(
            root,
            &Mutation::Checkout("feature/native".into()),
            snapshot(root).revision,
        )
        .unwrap_err();
    assert!(error.to_string().contains("local changes"));
    assert_eq!(
        std::fs::read_to_string(root.join("tracked.txt")).unwrap(),
        "keep this edit\n"
    );
    git(root, &["restore", "tracked.txt"]);
    mutate(root, Mutation::Checkout("feature/native".into()));
    assert_eq!(git(root, &["branch", "--show-current"]), "feature/native\n");
}

#[test]
fn binary_large_file_and_line_limit_are_bounded() {
    let dir = fixture();
    let root = dir.path();
    std::fs::write(root.join("binary.bin"), b"hello\0binary\xff").unwrap();
    std::fs::write(
        root.join("large.txt"),
        vec![b'x'; gitrust_desktop::git::MAX_DIFF_BYTES + 1],
    )
    .unwrap();
    std::fs::write(root.join("lines.txt"), "line\n".repeat(25_000)).unwrap();
    let state = snapshot(root);
    for name in ["binary.bin", "large.txt", "lines.txt"] {
        let preview = LocalGit
            .diff(
                root,
                &selected(&state, name, Side::Unstaged),
                &AtomicBool::new(false),
            )
            .unwrap();
        assert!(preview.buffer.len() <= gitrust_desktop::git::MAX_DIFF_BYTES);
        assert!(preview.lines.len() <= gitrust_desktop::git::MAX_DIFF_LINES);
        assert!(
            preview.notice.is_some(),
            "{name}: {:?}",
            preview.lines.len()
        );
    }
}

#[test]
fn history_is_paged_and_commit_detail_is_on_demand() {
    let dir = fixture();
    let root = dir.path();
    initial_commit(root);
    let repo = Repository::open(root).unwrap();
    for n in 0..104 {
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        let signature = repo.signature().unwrap();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            &format!("commit {n}"),
            &parent.tree().unwrap(),
            &[&parent],
        )
        .unwrap();
    }
    let page = LocalGit.history(root, 0, &AtomicBool::new(false)).unwrap();
    assert_eq!(page.commits.len(), 100);
    assert!(page.has_more);
    assert_eq!(
        page.commits[0].oid,
        git(root, &["rev-parse", "HEAD"]).trim()
    );
    let last = LocalGit
        .history(root, 100, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(last.commits.len(), 5);
    assert!(!last.has_more);
    let detail = LocalGit
        .commit_diff(
            root,
            &last.commits.last().unwrap().oid,
            &AtomicBool::new(false),
        )
        .unwrap();
    assert!(
        detail
            .lines
            .iter()
            .any(|line| line.kind == LineKind::Addition)
    );
}

#[test]
fn cancelled_reads_do_not_modify_repository() {
    let dir = fixture();
    let root = dir.path();
    initial_commit(root);
    let before = git(root, &["status", "--porcelain=v1"]);
    assert!(matches!(
        LocalGit.snapshot(root, &AtomicBool::new(true)),
        Err(gitrust_desktop::error::Error::Cancelled)
    ));
    assert_eq!(git(root, &["status", "--porcelain=v1"]), before);
}

#[cfg(unix)]
#[test]
fn non_utf8_file_paths_and_symlinks_survive_stage_and_commit() {
    use std::os::unix::{ffi::OsStringExt, fs::symlink};
    let dir = fixture();
    let root = dir.path();
    let path = std::path::PathBuf::from(std::ffi::OsString::from_vec(b"native-\xff.txt".to_vec()));
    std::fs::write(root.join(&path), "native bytes\n").unwrap();
    symlink("missing-target", root.join("link")).unwrap();
    for file in snapshot(root).files {
        mutate(root, Mutation::Stage(file));
    }
    mutate(
        root,
        Mutation::Commit {
            summary: "native paths".into(),
            description: String::new(),
        },
    );
    assert!(snapshot(root).files.is_empty());
    let repo = Repository::open(root).unwrap();
    let tree = repo.head().unwrap().peel_to_tree().unwrap();
    assert!(tree.get_path(&path).is_ok());
    assert_eq!(
        tree.get_path(Path::new("link")).unwrap().filemode(),
        0o120000
    );
}

#[test]
fn missing_identity_rejects_commit_and_retains_index() {
    let dir = fixture();
    let root = dir.path();
    std::fs::write(root.join("first.txt"), "staged\n").unwrap();
    git(root, &["add", "first.txt"]);
    let repo = Repository::open(root).unwrap();
    // Empty identity is invalid even if the developer's global config has a fallback.
    repo.config().unwrap().set_str("user.name", "").unwrap();
    repo.config().unwrap().set_str("user.email", "").unwrap();
    let before = git(root, &["ls-files", "--stage"]);
    let result = LocalGit.mutate(
        root,
        &Mutation::Commit {
            summary: "identity missing".into(),
            description: String::new(),
        },
        snapshot(root).revision,
    );
    assert!(result.is_err());
    assert_eq!(git(root, &["ls-files", "--stage"]), before);
}
