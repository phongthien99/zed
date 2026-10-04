use std::sync::Arc;

use anyhow::{Context as _, Result, anyhow};
use extension::{ExtensionGitProxy, PanelInstanceId, UiEvent, git as extension_git};
use git::repository::{FetchOptions, RepoPath, Upstream};
use git::status::{FileStatus, StatusCode};
use git_ui::git_panel::generate_commit_message_text;
use gpui::{AnyWindowHandle, App, AppContext as _, Entity, PromptLevel, Task};
use project::git_store::{Repository, RepositoryId};
use util::{ResultExt as _, maybe};
use workspace::Workspace;

use crate::{ExtensionPanel, extension_panels, workspaces_by_window};

mod operations;

use operations::{askpass_delegate, commit, discard_changes, open_diff, open_file, pull, push};

pub(crate) struct ExtensionPanelGitProxy;

/// The panel instance that a Git request came from, and the workspace it is shown in.
struct PanelTarget {
    window: AnyWindowHandle,
    workspace: Entity<Workspace>,
    panel: Entity<ExtensionPanel>,
}

impl PanelTarget {
    fn find(extension_id: &str, instance: PanelInstanceId, cx: &App) -> Result<Self> {
        for (window, workspaces) in workspaces_by_window(cx) {
            for workspace in workspaces {
                let panel = extension_panels(workspace.read(cx), cx)
                    .into_iter()
                    .find(|panel| {
                        let panel = panel.read(cx);
                        panel.instance == instance && panel.extension_id.as_ref() == extension_id
                    });
                if let Some(panel) = panel {
                    return Ok(Self {
                        window,
                        workspace,
                        panel,
                    });
                }
            }
        }
        Err(anyhow!("panel instance {instance} does not exist"))
    }

    /// Looks up a repository, and makes the panel re-render when the Git state changes
    /// since it is evidently displaying it.
    fn repository(
        &self,
        id: extension_git::RepositoryId,
        cx: &mut App,
    ) -> Result<Entity<Repository>> {
        self.observe_git_store(cx);
        self.workspace
            .read(cx)
            .project()
            .read(cx)
            .git_store()
            .read(cx)
            .repositories()
            .get(&RepositoryId::from_proto(id))
            .cloned()
            .with_context(|| format!("repository {id} does not exist"))
    }

    fn observe_git_store(&self, cx: &mut App) {
        let git_store = self
            .workspace
            .read(cx)
            .project()
            .read(cx)
            .git_store()
            .clone();
        self.panel
            .update(cx, |panel, cx| panel.observe_git_store(&git_store, cx));
    }
}

/// The name of a branch for new repositories when Git isn't configured with one.
const DEFAULT_BRANCH_NAME: &str = "main";

fn upstream(upstream: Option<&Upstream>) -> Option<extension_git::Upstream> {
    let upstream = upstream?;
    let status = upstream.tracking.status()?;
    Some(extension_git::Upstream {
        name: upstream
            .stripped_ref_name()
            .unwrap_or(&upstream.ref_name)
            .to_string(),
        ahead: status.ahead,
        behind: status.behind,
    })
}

fn repo_paths(paths: &[String]) -> Result<Vec<RepoPath>> {
    paths
        .iter()
        .map(|path| RepoPath::new(path).with_context(|| format!("invalid path {path:?}")))
        .collect()
}

fn status_code(code: StatusCode) -> extension_git::StatusCode {
    match code {
        StatusCode::Unmodified => extension_git::StatusCode::Unmodified,
        StatusCode::Modified => extension_git::StatusCode::Modified,
        StatusCode::TypeChanged => extension_git::StatusCode::TypeChanged,
        StatusCode::Added => extension_git::StatusCode::Added,
        StatusCode::Deleted => extension_git::StatusCode::Deleted,
        StatusCode::Renamed => extension_git::StatusCode::Renamed,
        StatusCode::Copied => extension_git::StatusCode::Copied,
    }
}

fn file_status(status: FileStatus) -> extension_git::FileStatus {
    match status {
        FileStatus::Untracked => extension_git::FileStatus::Untracked,
        FileStatus::Ignored => extension_git::FileStatus::Ignored,
        FileStatus::Unmerged(_) => extension_git::FileStatus::Conflicted,
        FileStatus::Tracked(tracked) => extension_git::FileStatus::Tracked {
            index: status_code(tracked.index_status),
            worktree: status_code(tracked.worktree_status),
        },
    }
}

impl ExtensionGitProxy for ExtensionPanelGitProxy {
    fn repositories(
        &self,
        extension_id: Arc<str>,
        instance: PanelInstanceId,
        cx: &mut App,
    ) -> Task<Result<Vec<extension_git::Repository>>> {
        Task::ready(maybe!({
            let target = PanelTarget::find(&extension_id, instance, cx)?;
            target.observe_git_store(cx);

            let git_store = target.workspace.read(cx).project().read(cx).git_store();
            let git_store = git_store.read(cx);
            let active_id = git_store
                .active_repository()
                .map(|repository| repository.read(cx).id);
            let mut repositories = git_store
                .repositories()
                .values()
                .map(|repository| {
                    let repository = repository.read(cx);
                    let branch = repository.branch.as_ref();
                    let upstream = upstream(branch.and_then(|branch| branch.upstream.as_ref()));
                    extension_git::Repository {
                        id: repository.id.to_proto(),
                        work_directory: repository
                            .work_directory_abs_path
                            .to_string_lossy()
                            .into_owned(),
                        branch: branch.map(|branch| branch.name().to_string()),
                        upstream,
                        active: Some(repository.id) == active_id,
                    }
                })
                .collect::<Vec<_>>();
            repositories.sort_by(|a, b| a.work_directory.cmp(&b.work_directory));
            Ok(repositories)
        }))
    }

    fn status(
        &self,
        extension_id: Arc<str>,
        instance: PanelInstanceId,
        repository: extension_git::RepositoryId,
        cx: &mut App,
    ) -> Task<Result<Vec<extension_git::StatusEntry>>> {
        Task::ready(maybe!({
            let repository =
                PanelTarget::find(&extension_id, instance, cx)?.repository(repository, cx)?;
            Ok(repository
                .read(cx)
                .cached_status()
                .map(|entry| extension_git::StatusEntry {
                    path: entry.repo_path.as_unix_str().to_string(),
                    status: file_status(entry.status),
                })
                .collect())
        }))
    }

    fn branches(
        &self,
        extension_id: Arc<str>,
        instance: PanelInstanceId,
        repository: extension_git::RepositoryId,
        cx: &mut App,
    ) -> Task<Result<Vec<extension_git::Branch>>> {
        let repository = match maybe!({
            PanelTarget::find(&extension_id, instance, cx)?.repository(repository, cx)
        }) {
            Ok(repository) => repository,
            Err(error) => return Task::ready(Err(error)),
        };
        let branches = repository.update(cx, |repository, _| repository.branches());
        cx.background_spawn(async move {
            Ok(branches
                .await??
                .branches
                .into_iter()
                .map(|branch| extension_git::Branch {
                    name: branch.name().to_string(),
                    is_head: branch.is_head,
                    is_remote: branch.is_remote(),
                    upstream: upstream(branch.upstream.as_ref()),
                })
                .collect())
        })
    }

    fn init_repository(
        &self,
        extension_id: Arc<str>,
        instance: PanelInstanceId,
        cx: &mut App,
    ) -> Task<Result<()>> {
        let result = maybe!({
            let target = PanelTarget::find(&extension_id, instance, cx)?;
            target.observe_git_store(cx);
            let project = target.workspace.read(cx).project().read(cx);
            let path = project
                .visible_worktrees(cx)
                .next()
                .context("the project has no folders")?
                .read(cx)
                .abs_path();
            Ok(project
                .git_store()
                .read(cx)
                .git_init(path, DEFAULT_BRANCH_NAME.to_string(), cx))
        });
        match result {
            Ok(task) => task,
            Err(error) => Task::ready(Err(error)),
        }
    }

    fn generate_commit_message(
        &self,
        extension_id: Arc<str>,
        instance: PanelInstanceId,
        repository: extension_git::RepositoryId,
        task_id: String,
        cx: &mut App,
    ) -> Result<()> {
        let target = PanelTarget::find(&extension_id, instance, cx)?;
        let repository = target.repository(repository, cx)?;
        let project = target.workspace.read(cx).project().clone();
        let generate = generate_commit_message_text(&repository, project, cx)?;
        let panel = target.panel.downgrade();
        cx.spawn(async move |cx| {
            let result = generate.await.map_err(|error| format!("{error:#}"));
            panel
                .update(cx, |panel, cx| {
                    panel.dispatch_event(
                        UiEvent::TaskCompleted {
                            id: task_id,
                            result,
                        },
                        cx,
                    )
                })
                .log_err();
        })
        .detach();
        Ok(())
    }

    fn discard(
        &self,
        extension_id: Arc<str>,
        instance: PanelInstanceId,
        repository: extension_git::RepositoryId,
        paths: Vec<String>,
        cx: &mut App,
    ) -> Task<Result<bool>> {
        let prepared = maybe!({
            let target = PanelTarget::find(&extension_id, instance, cx)?;
            let repository = target.repository(repository, cx)?;
            let repo_paths = repo_paths(&paths)?;
            let message = match repo_paths.as_slice() {
                [] => return Ok(None),
                [path] => format!(
                    "Are you sure you want to discard the changes in {}?",
                    path.file_name().unwrap_or(path.as_unix_str())
                ),
                paths => format!(
                    "Are you sure you want to discard the changes in {} files?",
                    paths.len()
                ),
            };
            let prompt = target.window.update(cx, |_, window, cx| {
                window.prompt(
                    PromptLevel::Warning,
                    &message,
                    Some("Tracked files are restored from HEAD and untracked files are moved to the trash."),
                    &["Discard Changes", "Cancel"],
                    cx,
                )
            })?;
            Ok(Some((target, repository, repo_paths, prompt)))
        });

        let (target, repository, repo_paths, prompt) = match prepared {
            Ok(Some(prepared)) => prepared,
            Ok(None) => return Task::ready(Ok(false)),
            Err(error) => return Task::ready(Err(error)),
        };
        cx.spawn(async move |cx| {
            if prompt.await? != 0 {
                return Ok(false);
            }
            discard_changes(target, repository, repo_paths, cx).await?;
            Ok(true)
        })
    }

    fn operation(
        &self,
        extension_id: Arc<str>,
        instance: PanelInstanceId,
        repository: extension_git::RepositoryId,
        operation: extension_git::Operation,
        cx: &mut App,
    ) -> Task<Result<()>> {
        let result = maybe!({
            let target = PanelTarget::find(&extension_id, instance, cx)?;
            let repository = target.repository(repository, cx)?;
            Ok(match operation {
                extension_git::Operation::Stage(paths) => {
                    let paths = repo_paths(&paths)?;
                    repository.update(cx, |repository, cx| repository.stage_entries(paths, cx))
                }
                extension_git::Operation::Unstage(paths) => {
                    let paths = repo_paths(&paths)?;
                    repository.update(cx, |repository, cx| repository.unstage_entries(paths, cx))
                }
                extension_git::Operation::StageAll => {
                    repository.update(cx, |repository, cx| repository.stage_all(cx))
                }
                extension_git::Operation::UnstageAll => {
                    repository.update(cx, |repository, cx| repository.unstage_all(cx))
                }
                extension_git::Operation::Commit { message, options } => {
                    commit(&target, &repository, message, options, cx)?
                }
                extension_git::Operation::OpenDiff { path, kind } => {
                    open_diff(&target, repository, &path, kind, cx)?
                }
                extension_git::Operation::OpenFile(path) => {
                    open_file(&target, &repository, &path, cx)?
                }
                extension_git::Operation::Fetch => {
                    let askpass = askpass_delegate(&target, "git fetch".into(), cx);
                    let fetch = repository.update(cx, |repository, cx| {
                        repository.fetch(FetchOptions::All, askpass, cx)
                    });
                    cx.background_spawn(async move { fetch.await?.map(|_| ()) })
                }
                extension_git::Operation::Pull { rebase } => pull(&target, repository, rebase, cx)?,
                extension_git::Operation::Push { force } => push(&target, repository, force, cx)?,
                extension_git::Operation::CheckoutBranch(name) => {
                    let checkout =
                        repository.update(cx, |repository, _| repository.change_branch(name));
                    cx.background_spawn(async move { checkout.await? })
                }
                extension_git::Operation::CreateBranch(name) => {
                    let name = name.trim().to_string();
                    if name.is_empty() {
                        return Err(anyhow!("the branch name is empty"));
                    }
                    let create =
                        repository.update(cx, |repository, _| repository.create_branch(name, None));
                    cx.background_spawn(async move { create.await? })
                }
                extension_git::Operation::StashAll => {
                    repository.update(cx, |repository, cx| repository.stash_all(None, cx))
                }
                extension_git::Operation::StashPop => {
                    repository.update(cx, |repository, cx| repository.stash_pop(None, cx))
                }
            })
        });
        match result {
            Ok(task) => task,
            Err(error) => Task::ready(Err(error)),
        }
    }
}
