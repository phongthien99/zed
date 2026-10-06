use crate::wasm_host::{WasmState, wit::ToWasmtimeResult};
use anyhow::Result;
use extension::{ExtensionGitProxy, ExtensionHostProxy};
use futures::FutureExt as _;
use std::sync::Arc;

use super::*;

impl From<extension::git::StatusCode> for git::StatusCode {
    fn from(value: extension::git::StatusCode) -> Self {
        match value {
            extension::git::StatusCode::Unmodified => Self::Unmodified,
            extension::git::StatusCode::Modified => Self::Modified,
            extension::git::StatusCode::TypeChanged => Self::TypeChanged,
            extension::git::StatusCode::Added => Self::Added,
            extension::git::StatusCode::Deleted => Self::Deleted,
            extension::git::StatusCode::Renamed => Self::Renamed,
            extension::git::StatusCode::Copied => Self::Copied,
        }
    }
}

impl From<extension::git::StatusEntry> for git::StatusEntry {
    fn from(value: extension::git::StatusEntry) -> Self {
        Self {
            path: value.path,
            status: match value.status {
                extension::git::FileStatus::Untracked => git::FileStatus::Untracked,
                extension::git::FileStatus::Ignored => git::FileStatus::Ignored,
                extension::git::FileStatus::Conflicted => git::FileStatus::Conflicted,
                extension::git::FileStatus::Tracked { index, worktree } => {
                    git::FileStatus::Tracked(git::TrackedStatus {
                        index: index.into(),
                        worktree: worktree.into(),
                    })
                }
            },
        }
    }
}

impl From<extension::git::Repository> for git::Repository {
    fn from(value: extension::git::Repository) -> Self {
        Self {
            id: value.id,
            work_directory: value.work_directory,
            branch: value.branch,
            upstream: value.upstream.map(Into::into),
            active: value.active,
        }
    }
}

impl From<extension::git::Upstream> for git::Upstream {
    fn from(upstream: extension::git::Upstream) -> Self {
        Self {
            name: upstream.name,
            ahead: upstream.ahead,
            behind: upstream.behind,
        }
    }
}

impl WasmState {
    /// Runs a request against the Git proxy on the main thread, after checking that the
    /// extension was granted access to Git.
    async fn git_request<T: 'static + Send>(
        &mut self,
        request: impl 'static
        + Send
        + FnOnce(&ExtensionHostProxy, Arc<str>, &mut gpui::App) -> gpui::Task<Result<T>>,
    ) -> Result<T> {
        self.capability_granter.grant_git()?;
        let extension_id = self.manifest.id.clone();
        let proxy = self.host.proxy.clone();
        self.on_main_thread(move |cx| {
            async move {
                let task = cx.update(|cx| request(&proxy, extension_id, cx));
                task.await
            }
            .boxed_local()
        })
        .await
    }

    async fn git_operation(
        &mut self,
        instance: ui::PanelInstance,
        repository: git::RepositoryId,
        operation: extension::git::Operation,
    ) -> wasmtime::Result<Result<(), String>> {
        self.git_request(move |proxy, extension_id, cx| {
            proxy.operation(extension_id, instance, repository, operation, cx)
        })
        .await
        .to_wasmtime_result()
    }
}

impl git::Host for WasmState {
    async fn repositories(
        &mut self,
        instance: ui::PanelInstance,
    ) -> wasmtime::Result<Result<Vec<git::Repository>, String>> {
        self.git_request(move |proxy, extension_id, cx| {
            proxy.repositories(extension_id, instance, cx)
        })
        .await
        .map(|repositories| repositories.into_iter().map(Into::into).collect())
        .to_wasmtime_result()
    }

    async fn status(
        &mut self,
        instance: ui::PanelInstance,
        repository: git::RepositoryId,
    ) -> wasmtime::Result<Result<Vec<git::StatusEntry>, String>> {
        self.git_request(move |proxy, extension_id, cx| {
            proxy.status(extension_id, instance, repository, cx)
        })
        .await
        .map(|entries| entries.into_iter().map(Into::into).collect())
        .to_wasmtime_result()
    }

    async fn stage(
        &mut self,
        instance: ui::PanelInstance,
        repository: git::RepositoryId,
        paths: Vec<String>,
    ) -> wasmtime::Result<Result<(), String>> {
        self.git_operation(
            instance,
            repository,
            extension::git::Operation::Stage(paths),
        )
        .await
    }

    async fn unstage(
        &mut self,
        instance: ui::PanelInstance,
        repository: git::RepositoryId,
        paths: Vec<String>,
    ) -> wasmtime::Result<Result<(), String>> {
        self.git_operation(
            instance,
            repository,
            extension::git::Operation::Unstage(paths),
        )
        .await
    }

    async fn stage_all(
        &mut self,
        instance: ui::PanelInstance,
        repository: git::RepositoryId,
    ) -> wasmtime::Result<Result<(), String>> {
        self.git_operation(instance, repository, extension::git::Operation::StageAll)
            .await
    }

    async fn unstage_all(
        &mut self,
        instance: ui::PanelInstance,
        repository: git::RepositoryId,
    ) -> wasmtime::Result<Result<(), String>> {
        self.git_operation(instance, repository, extension::git::Operation::UnstageAll)
            .await
    }

    async fn discard(
        &mut self,
        instance: ui::PanelInstance,
        repository: git::RepositoryId,
        paths: Vec<String>,
    ) -> wasmtime::Result<Result<bool, String>> {
        self.git_request(move |proxy, extension_id, cx| {
            proxy.discard(extension_id, instance, repository, paths, cx)
        })
        .await
        .to_wasmtime_result()
    }

    async fn commit(
        &mut self,
        instance: ui::PanelInstance,
        repository: git::RepositoryId,
        message: String,
        options: git::CommitOptions,
    ) -> wasmtime::Result<Result<(), String>> {
        self.git_operation(
            instance,
            repository,
            extension::git::Operation::Commit {
                message,
                options: extension::git::CommitOptions {
                    amend: options.amend,
                    signoff: options.signoff,
                },
            },
        )
        .await
    }

    async fn open_diff(
        &mut self,
        instance: ui::PanelInstance,
        repository: git::RepositoryId,
        path: String,
        kind: git::DiffKind,
    ) -> wasmtime::Result<Result<(), String>> {
        let kind = match kind {
            git::DiffKind::Uncommitted => extension::git::DiffKind::Uncommitted,
            git::DiffKind::Staged => extension::git::DiffKind::Staged,
            git::DiffKind::Unstaged => extension::git::DiffKind::Unstaged,
        };
        self.git_operation(
            instance,
            repository,
            extension::git::Operation::OpenDiff { path, kind },
        )
        .await
    }

    async fn open_file(
        &mut self,
        instance: ui::PanelInstance,
        repository: git::RepositoryId,
        path: String,
    ) -> wasmtime::Result<Result<(), String>> {
        self.git_operation(
            instance,
            repository,
            extension::git::Operation::OpenFile(path),
        )
        .await
    }

    async fn fetch(
        &mut self,
        instance: ui::PanelInstance,
        repository: git::RepositoryId,
    ) -> wasmtime::Result<Result<(), String>> {
        self.git_operation(instance, repository, extension::git::Operation::Fetch)
            .await
    }

    async fn pull(
        &mut self,
        instance: ui::PanelInstance,
        repository: git::RepositoryId,
        rebase: bool,
    ) -> wasmtime::Result<Result<(), String>> {
        self.git_operation(
            instance,
            repository,
            extension::git::Operation::Pull { rebase },
        )
        .await
    }

    async fn push(
        &mut self,
        instance: ui::PanelInstance,
        repository: git::RepositoryId,
        force: bool,
    ) -> wasmtime::Result<Result<(), String>> {
        self.git_operation(
            instance,
            repository,
            extension::git::Operation::Push { force },
        )
        .await
    }

    async fn branches(
        &mut self,
        instance: ui::PanelInstance,
        repository: git::RepositoryId,
    ) -> wasmtime::Result<Result<Vec<git::Branch>, String>> {
        self.git_request(move |proxy, extension_id, cx| {
            proxy.branches(extension_id, instance, repository, cx)
        })
        .await
        .map(|branches| {
            branches
                .into_iter()
                .map(|branch| git::Branch {
                    name: branch.name,
                    is_head: branch.is_head,
                    is_remote: branch.is_remote,
                    upstream: branch.upstream.map(Into::into),
                })
                .collect()
        })
        .to_wasmtime_result()
    }

    async fn checkout_branch(
        &mut self,
        instance: ui::PanelInstance,
        repository: git::RepositoryId,
        name: String,
    ) -> wasmtime::Result<Result<(), String>> {
        self.git_operation(
            instance,
            repository,
            extension::git::Operation::CheckoutBranch(name),
        )
        .await
    }

    async fn create_branch(
        &mut self,
        instance: ui::PanelInstance,
        repository: git::RepositoryId,
        name: String,
    ) -> wasmtime::Result<Result<(), String>> {
        self.git_operation(
            instance,
            repository,
            extension::git::Operation::CreateBranch(name),
        )
        .await
    }

    async fn stash_all(
        &mut self,
        instance: ui::PanelInstance,
        repository: git::RepositoryId,
    ) -> wasmtime::Result<Result<(), String>> {
        self.git_operation(instance, repository, extension::git::Operation::StashAll)
            .await
    }

    async fn stash_pop(
        &mut self,
        instance: ui::PanelInstance,
        repository: git::RepositoryId,
    ) -> wasmtime::Result<Result<(), String>> {
        self.git_operation(instance, repository, extension::git::Operation::StashPop)
            .await
    }

    async fn generate_commit_message(
        &mut self,
        instance: ui::PanelInstance,
        repository: git::RepositoryId,
        task_id: String,
    ) -> wasmtime::Result<Result<(), String>> {
        self.git_request(move |proxy, extension_id, cx| {
            gpui::Task::ready(proxy.generate_commit_message(
                extension_id,
                instance,
                repository,
                task_id,
                cx,
            ))
        })
        .await
        .to_wasmtime_result()
    }

    async fn init_repository(
        &mut self,
        instance: ui::PanelInstance,
    ) -> wasmtime::Result<Result<(), String>> {
        self.git_request(move |proxy, extension_id, cx| {
            proxy.init_repository(extension_id, instance, cx)
        })
        .await
        .to_wasmtime_result()
    }
}
