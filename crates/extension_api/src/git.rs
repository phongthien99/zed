//! Access to the Git repositories of the project that a panel is shown in.
//!
//! Every function takes the [`PanelInstance`] passed to
//! [`crate::Extension::panel_render`] or [`crate::Extension::panel_handle_event`], which
//! identifies the project to act on. The extension must declare the `git` capability in
//! its `extension.toml`:
//!
//! ```toml
//! [[capabilities]]
//! kind = "git"
//! ```

use crate::Result;
use crate::ui::PanelInstance;
use crate::wit::zed::extension::git as wit_git;

pub use wit_git::{
    Branch, CommitOptions, DiffKind, FileStatus, Repository, RepositoryId, StatusCode, StatusEntry,
    TrackedStatus, Upstream,
};

/// Returns the Git repositories in the project of the given panel instance.
pub fn repositories(instance: PanelInstance) -> Result<Vec<Repository>> {
    wit_git::repositories(instance)
}

/// Returns the files in the given repository that differ from `HEAD`.
pub fn status(instance: PanelInstance, repository: RepositoryId) -> Result<Vec<StatusEntry>> {
    wit_git::status(instance, repository)
}

/// Adds the given files, relative to the repository's working directory, to the index.
pub fn stage(instance: PanelInstance, repository: RepositoryId, paths: &[String]) -> Result<()> {
    wit_git::stage(instance, repository, paths)
}

/// Removes the given files, relative to the repository's working directory, from the index.
pub fn unstage(instance: PanelInstance, repository: RepositoryId, paths: &[String]) -> Result<()> {
    wit_git::unstage(instance, repository, paths)
}

/// Adds every changed file to the index.
pub fn stage_all(instance: PanelInstance, repository: RepositoryId) -> Result<()> {
    wit_git::stage_all(instance, repository)
}

/// Removes every file from the index.
pub fn unstage_all(instance: PanelInstance, repository: RepositoryId) -> Result<()> {
    wit_git::unstage_all(instance, repository)
}

/// Discards the changes to the given files after asking the user to confirm.
///
/// Tracked files are restored to their state in `HEAD`, and untracked files are moved to
/// the trash. Returns `false` if the user cancelled.
pub fn discard(
    instance: PanelInstance,
    repository: RepositoryId,
    paths: &[String],
) -> Result<bool> {
    wit_git::discard(instance, repository, paths)
}

/// Commits the staged changes with the given message.
pub fn commit(
    instance: PanelInstance,
    repository: RepositoryId,
    message: &str,
    options: CommitOptions,
) -> Result<()> {
    wit_git::commit(instance, repository, message, options)
}

/// Opens the changes to the given file in a diff view.
pub fn open_diff(
    instance: PanelInstance,
    repository: RepositoryId,
    path: &str,
    kind: DiffKind,
) -> Result<()> {
    wit_git::open_diff(instance, repository, path, kind)
}

/// Opens the given file in an editor.
pub fn open_file(instance: PanelInstance, repository: RepositoryId, path: &str) -> Result<()> {
    wit_git::open_file(instance, repository, path)
}

/// Fetches from all remotes.
pub fn fetch(instance: PanelInstance, repository: RepositoryId) -> Result<()> {
    wit_git::fetch(instance, repository)
}

/// Pulls the checked-out branch from its remote, rebasing local commits if `rebase`.
pub fn pull(instance: PanelInstance, repository: RepositoryId, rebase: bool) -> Result<()> {
    wit_git::pull(instance, repository, rebase)
}

/// Pushes the checked-out branch, setting its upstream if it has none.
pub fn push(instance: PanelInstance, repository: RepositoryId, force: bool) -> Result<()> {
    wit_git::push(instance, repository, force)
}

/// Returns the local and remote branches of the given repository.
pub fn branches(instance: PanelInstance, repository: RepositoryId) -> Result<Vec<Branch>> {
    wit_git::branches(instance, repository)
}

/// Checks out the given branch.
pub fn checkout_branch(
    instance: PanelInstance,
    repository: RepositoryId,
    name: &str,
) -> Result<()> {
    wit_git::checkout_branch(instance, repository, name)
}

/// Creates a branch from `HEAD` and checks it out.
pub fn create_branch(instance: PanelInstance, repository: RepositoryId, name: &str) -> Result<()> {
    wit_git::create_branch(instance, repository, name)
}

/// Stashes all changes, including untracked files.
pub fn stash_all(instance: PanelInstance, repository: RepositoryId) -> Result<()> {
    wit_git::stash_all(instance, repository)
}

/// Applies the latest stash and removes it from the stash list.
pub fn stash_pop(instance: PanelInstance, repository: RepositoryId) -> Result<()> {
    wit_git::stash_pop(instance, repository)
}

/// Starts writing a commit message for the given repository with the commit message model
/// configured in Zed's AI settings.
///
/// The message describes the staged changes, or all changes when nothing is staged. This
/// returns once the operation has started; the panel instance then receives
/// [`crate::ui::Event::TaskCompleted`] with `task_id` and the message.
pub fn generate_commit_message(
    instance: PanelInstance,
    repository: RepositoryId,
    task_id: &str,
) -> Result<()> {
    wit_git::generate_commit_message(instance, repository, task_id)
}

/// Creates a Git repository in the first folder of the project of the given panel instance.
pub fn init_repository(instance: PanelInstance) -> Result<()> {
    wit_git::init_repository(instance)
}

impl FileStatus {
    /// Whether the index differs from `HEAD` for this file.
    pub fn is_staged(&self) -> bool {
        matches!(self, FileStatus::Tracked(tracked) if tracked.index != StatusCode::Unmodified)
    }

    /// Whether the working tree differs from the index for this file.
    pub fn is_unstaged(&self) -> bool {
        match self {
            FileStatus::Untracked | FileStatus::Conflicted => true,
            FileStatus::Ignored => false,
            FileStatus::Tracked(tracked) => tracked.worktree != StatusCode::Unmodified,
        }
    }
}
