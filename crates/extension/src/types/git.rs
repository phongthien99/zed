//! Git data exchanged between extension panels and the host's Git integration.

pub type RepositoryId = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upstream {
    pub name: String,
    pub ahead: u32,
    pub behind: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    pub id: RepositoryId,
    pub work_directory: String,
    pub branch: Option<String>,
    pub upstream: Option<Upstream>,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    pub is_head: bool,
    pub is_remote: bool,
    pub upstream: Option<Upstream>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusCode {
    Unmodified,
    Modified,
    TypeChanged,
    Added,
    Deleted,
    Renamed,
    Copied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Untracked,
    Ignored,
    Conflicted,
    Tracked {
        index: StatusCode,
        worktree: StatusCode,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    /// Relative to the repository's working directory, using `/` as the separator.
    pub path: String,
    pub status: FileStatus,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommitOptions {
    pub amend: bool,
    pub signoff: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DiffKind {
    /// `HEAD` against the working tree.
    #[default]
    Uncommitted,
    /// `HEAD` against the index.
    Staged,
    /// The index against the working tree.
    Unstaged,
}

/// An operation on a repository that produces no data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    Stage(Vec<String>),
    Unstage(Vec<String>),
    StageAll,
    UnstageAll,
    Commit {
        message: String,
        options: CommitOptions,
    },
    OpenDiff {
        path: String,
        kind: DiffKind,
    },
    OpenFile(String),
    Fetch,
    Pull {
        rebase: bool,
    },
    Push {
        force: bool,
    },
    CheckoutBranch(String),
    CreateBranch(String),
    StashAll,
    StashPop,
}
