use anyhow::{Context as _, Result, anyhow};
use askpass::AskPassDelegate;
use extension::git as extension_git;
use futures::future::join_all;
use git::repository::{CommitOptions, PushOptions, Remote, RepoPath, Upstream, UpstreamTracking};
use git_ui::git_panel::GitStatusEntry;
use git_ui::picker_prompt;
use git_ui::solo_diff_view::SoloDiffView;
use git_ui::staged_diff::StagedDiff;
use git_ui::unstaged_diff::UnstagedDiff;
use git_ui_core::askpass_modal::AskPassModal;
use gpui::{
    AnyWindowHandle, App, AppContext as _, AsyncApp, Entity, SharedString, Task, WeakEntity,
};
use project::git_store::Repository;
use util::ResultExt as _;
use workspace::Workspace;

use super::PanelTarget;

pub(super) fn commit(
    target: &PanelTarget,
    repository: &Entity<Repository>,
    message: String,
    options: extension_git::CommitOptions,
    cx: &mut App,
) -> Result<Task<Result<()>>> {
    if message.trim().is_empty() {
        return Err(anyhow!("the commit message is empty"));
    }
    if repository
        .read(cx)
        .cached_status()
        .any(|entry| entry.status.is_conflicted())
    {
        return Err(anyhow!(
            "there are unresolved conflicts, which must be resolved and staged before committing"
        ));
    }

    let askpass = askpass_delegate(target, "git commit".into(), cx);
    let commit = repository.update(cx, |repository, cx| {
        repository.commit(
            message.into(),
            None,
            CommitOptions {
                amend: options.amend,
                signoff: options.signoff,
                allow_empty: false,
                no_verify: false,
            },
            askpass,
            cx,
        )
    });
    Ok(cx.background_spawn(async move { commit.await? }))
}

/// Shows Git's prompts, such as for a signing key passphrase, in a modal in the panel's
/// workspace.
pub(super) fn askpass_delegate(
    target: &PanelTarget,
    operation: SharedString,
    cx: &mut App,
) -> AskPassDelegate {
    workspace_askpass_delegate(target.window, target.workspace.downgrade(), operation, cx)
}

pub(super) fn workspace_askpass_delegate(
    window: AnyWindowHandle,
    workspace: WeakEntity<Workspace>,
    operation: SharedString,
    cx: &mut App,
) -> AskPassDelegate {
    AskPassDelegate::new_with_cancellation(
        &mut cx.to_async(),
        move |prompt, tx, cancellation, cx| {
            window
                .update(cx, |_, window, cx| {
                    workspace.update(cx, |workspace, cx| {
                        workspace.toggle_modal(window, cx, |window, cx| {
                            AskPassModal::new(
                                operation.clone(),
                                prompt.into(),
                                tx,
                                cancellation,
                                window,
                                cx,
                            )
                        });
                    })
                })
                .and_then(|result| result)
                .log_err();
        },
    )
}

/// Picks the remote to pull from or push to, letting the user choose when there are
/// several, the way the Git panel does.
pub(super) async fn pick_remote(
    repository: &Entity<Repository>,
    branch_name: String,
    is_push: bool,
    window: AnyWindowHandle,
    workspace: WeakEntity<Workspace>,
    cx: &mut AsyncApp,
) -> Result<Remote> {
    let remotes = repository
        .update(cx, |repository, _| {
            repository.get_remotes(Some(branch_name), is_push)
        })
        .await??;
    if remotes.is_empty() {
        return Err(anyhow!("the repository has no remotes"));
    }
    let names = remotes
        .into_iter()
        .map(|remote| remote.name)
        .collect::<Vec<_>>();
    let prompt = if is_push {
        "Pick which remote to push to"
    } else {
        "Pick which remote to pull from"
    };
    let selection = window
        .update(cx, |_, window, cx| {
            picker_prompt::prompt(prompt, names.clone(), workspace, window, cx)
        })?
        .await
        .context("no remote was selected")?;
    let name = names
        .get(selection)
        .cloned()
        .context("the selected remote does not exist")?;
    Ok(Remote { name })
}

pub(super) fn pull(
    target: &PanelTarget,
    repository: Entity<Repository>,
    rebase: bool,
    cx: &mut App,
) -> Result<Task<Result<()>>> {
    let branch = repository
        .read(cx)
        .branch
        .clone()
        .context("no branch is checked out")?;
    let window = target.window;
    let workspace = target.workspace.downgrade();
    Ok(cx.spawn(async move |cx| {
        let remote = pick_remote(
            &repository,
            branch.name().to_string(),
            false,
            window,
            workspace.clone(),
            cx,
        )
        .await?;
        let askpass = cx.update(|cx| {
            workspace_askpass_delegate(
                window,
                workspace,
                format!("git pull {}", remote.name).into(),
                cx,
            )
        });
        let branch_name = branch
            .upstream
            .is_none()
            .then(|| branch.name().to_owned().into());
        let pull = repository.update(cx, |repository, cx| {
            repository.pull(branch_name, remote.name.clone(), rebase, askpass, cx)
        });
        pull.await?.map(|_| ())
    }))
}

pub(super) fn push(
    target: &PanelTarget,
    repository: Entity<Repository>,
    force: bool,
    cx: &mut App,
) -> Result<Task<Result<()>>> {
    let branch = repository
        .read(cx)
        .branch
        .clone()
        .context("no branch is checked out")?;
    let options = if force {
        Some(PushOptions::Force)
    } else {
        match branch.upstream {
            Some(Upstream {
                tracking: UpstreamTracking::Gone,
                ..
            })
            | None => Some(PushOptions::SetUpstream),
            _ => None,
        }
    };
    let remote_branch: SharedString = branch
        .upstream
        .as_ref()
        .filter(|upstream| matches!(upstream.tracking, UpstreamTracking::Tracked(_)))
        .and_then(|upstream| upstream.branch_name())
        .unwrap_or_else(|| branch.name())
        .to_owned()
        .into();
    let window = target.window;
    let workspace = target.workspace.downgrade();
    Ok(cx.spawn(async move |cx| {
        let remote = pick_remote(
            &repository,
            branch.name().to_string(),
            true,
            window,
            workspace.clone(),
            cx,
        )
        .await?;
        let askpass = cx.update(|cx| {
            workspace_askpass_delegate(
                window,
                workspace,
                format!("git push {}", remote.name).into(),
                cx,
            )
        });
        let push = repository.update(cx, |repository, cx| {
            repository.push(
                branch.name().to_owned().into(),
                remote_branch,
                remote.name.clone(),
                options,
                askpass,
                cx,
            )
        });
        push.await?.map(|_| ())
    }))
}

pub(super) fn open_diff(
    target: &PanelTarget,
    repository: Entity<Repository>,
    path: &str,
    kind: extension_git::DiffKind,
    cx: &mut App,
) -> Result<Task<Result<()>>> {
    let repo_path = RepoPath::new(path).with_context(|| format!("invalid path {path:?}"))?;
    let status = repository
        .read(cx)
        .status_for_path(&repo_path)
        .with_context(|| format!("{path} has no changes"))?
        .status;
    let entry = GitStatusEntry::new(repo_path, status);

    if kind == extension_git::DiffKind::Uncommitted {
        let workspace = target.workspace.downgrade();
        let open = target.window.update(cx, |_, window, cx| {
            SoloDiffView::open_or_focus(entry, repository, workspace, window, cx)
        })?;
        return Ok(cx.background_spawn(async move { open.await.map(|_| ()) }));
    }

    // The staged and unstaged views show the workspace's active repository.
    repository.update(cx, |repository, cx| repository.set_as_active_repository(cx));
    let workspace = target.workspace.clone();
    target.window.update(cx, |_, window, cx| {
        workspace.update(cx, |workspace, cx| {
            if kind == extension_git::DiffKind::Staged {
                StagedDiff::deploy_at(workspace, Some(entry), window, cx);
            } else {
                UnstagedDiff::deploy_at(workspace, Some(entry), window, cx);
            }
        })
    })?;
    Ok(Task::ready(Ok(())))
}

pub(super) fn open_file(
    target: &PanelTarget,
    repository: &Entity<Repository>,
    path: &str,
    cx: &mut App,
) -> Result<Task<Result<()>>> {
    let repo_path = RepoPath::new(path).with_context(|| format!("invalid path {path:?}"))?;
    let project_path = repository
        .read(cx)
        .repo_path_to_project_path(&repo_path, cx)
        .with_context(|| format!("{path} is not in the project"))?;
    let workspace = target.workspace.clone();
    let open = target.window.update(cx, |_, window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.open_path_preview(project_path, None, true, false, true, window, cx)
        })
    })?;
    Ok(cx.background_spawn(async move { open.await.map(|_| ()) }))
}

/// Mirrors how the Git panel discards changes: staged changes are unstaged first, tracked
/// files are checked out from `HEAD`, and files that don't exist in `HEAD` are trashed.
pub(super) async fn discard_changes(
    target: PanelTarget,
    repository: Entity<Repository>,
    repo_paths: Vec<RepoPath>,
    cx: &mut AsyncApp,
) -> Result<()> {
    let (staged, created, tracked) = repository.read_with(cx, |repository, _| {
        let mut staged = Vec::new();
        let mut created = Vec::new();
        let mut tracked = Vec::new();
        for path in repo_paths {
            let Some(entry) = repository.status_for_path(&path) else {
                continue;
            };
            if entry.status.staging().has_staged() {
                staged.push(path.clone());
            }
            if entry.status.is_created() {
                created.push(path);
            } else {
                tracked.push(path);
            }
        }
        (staged, created, tracked)
    });

    if !staged.is_empty() {
        repository
            .update(cx, |repository, cx| repository.unstage_entries(staged, cx))
            .await?;
    }

    let project = target
        .workspace
        .read_with(cx, |workspace, _| workspace.project().clone());

    if !tracked.is_empty() {
        // Open buffers are reloaded after the checkout, since one with unsaved edits
        // wouldn't pick up the restored contents from disk on its own.
        let project_paths = cx.update(|cx| {
            let repository = repository.read(cx);
            tracked
                .iter()
                .filter_map(|path| repository.repo_path_to_project_path(path, cx))
                .collect::<Vec<_>>()
        });
        let open_buffers = project.update(cx, |project, cx| {
            project_paths
                .into_iter()
                .map(|project_path| project.open_buffer(project_path, cx))
                .collect::<Vec<_>>()
        });
        let buffers = join_all(open_buffers).await;

        repository
            .update(cx, |repository, cx| {
                repository.checkout_files("HEAD", tracked, cx)
            })
            .await?;

        let reloads = cx.update(|cx| {
            buffers
                .iter()
                .filter_map(|buffer| {
                    buffer.as_ref().ok()?.update(cx, |buffer, cx| {
                        buffer.is_dirty().then(|| buffer.reload(cx))
                    })
                })
                .collect::<Vec<_>>()
        });
        join_all(reloads).await;
    }

    for path in created {
        let trash = project.update(cx, |project, cx| {
            let project_path = repository.read(cx).repo_path_to_project_path(&path, cx)?;
            project.trash_file(project_path, cx)
        });
        if let Some(trash) = trash {
            trash.await?;
        }
    }

    Ok(())
}
