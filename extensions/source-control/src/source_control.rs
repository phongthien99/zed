use std::collections::{HashMap, HashSet};

use zed_extension_api::{self as zed, git, ui};

pub(crate) const MESSAGE_INPUT_ID: &str = "message";
pub(crate) const BRANCH_QUERY_INPUT_ID: &str = "branch-query";
pub(crate) const GENERATE_MESSAGE_TASK_ID: &str = "generate-commit-message";

/// Files beyond this many in a section aren't listed, to bound the size of the UI tree.
pub(crate) const MAX_FILES_PER_SECTION: usize = 2_000;

mod changes;
mod render;

use changes::{Changes, Section, ViewMode, diff_kind};

/// What to do after committing, chosen from the commit button's dropdown.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AfterCommit {
    Nothing,
    Push,
    Sync,
}

#[derive(Default)]
pub(crate) struct PanelState {
    repository: Option<git::RepositoryId>,
    message: String,
    view_mode: ViewMode,
    collapsed_sections: HashSet<Section>,
    /// Keys of the form `{section}:{directory}`.
    collapsed_directories: HashSet<String>,
    selected_item: Option<String>,
    /// The text typed into the branch picker, or `None` while it is closed.
    branch_query: Option<String>,
    /// Whether a commit message is being generated.
    generating_message: bool,
    error: Option<String>,
}

struct SourceControlExtension {
    states: HashMap<ui::PanelInstance, PanelState>,
}

pub(crate) fn selected_repository(
    state: &PanelState,
    repositories: &[git::Repository],
) -> Option<git::Repository> {
    state
        .repository
        .and_then(|id| repositories.iter().find(|repository| repository.id == id))
        .or_else(|| repositories.iter().find(|repository| repository.active))
        .or_else(|| repositories.first())
        .cloned()
}

impl PanelState {
    fn handle_event(&mut self, instance: ui::PanelInstance, event: ui::Event) -> zed::Result<()> {
        match event {
            ui::Event::InputChanged { id, value } if id == MESSAGE_INPUT_ID => {
                self.message = value;
                Ok(())
            }
            ui::Event::TaskCompleted { id, result } if id == GENERATE_MESSAGE_TASK_ID => {
                self.generating_message = false;
                self.message = result
                    .map_err(|error| format!("Failed to generate a commit message: {error}"))?;
                Ok(())
            }
            ui::Event::InputSubmitted { id, value } if id == MESSAGE_INPUT_ID => {
                self.message = value;
                self.commit(instance, false, AfterCommit::Nothing)
            }
            ui::Event::InputChanged { id, value } if id == BRANCH_QUERY_INPUT_ID => {
                self.branch_query = Some(value);
                Ok(())
            }
            ui::Event::InputSubmitted { id, value } if id == BRANCH_QUERY_INPUT_ID => {
                self.submit_branch_query(instance, value.trim())
            }
            ui::Event::Clicked(id) | ui::Event::ListItemToggled(id) => {
                self.handle_click(instance, &id)
            }
            _ => Ok(()),
        }
    }

    fn handle_click(&mut self, instance: ui::PanelInstance, id: &str) -> zed::Result<()> {
        let (kind, target) = id.split_once(':').unwrap_or((id, ""));
        match kind {
            "refresh" => Ok(()),
            "commit" => self.commit(instance, false, AfterCommit::Nothing),
            "commit-amend" => self.commit(instance, true, AfterCommit::Nothing),
            "commit-push" => self.commit(instance, false, AfterCommit::Push),
            "commit-sync" => self.commit(instance, false, AfterCommit::Sync),
            "sync" => self.sync(instance),
            "publish" | "push" => git::push(instance, self.repository_id(instance)?, false),
            "pull" => git::pull(instance, self.repository_id(instance)?, false),
            "pull-rebase" => git::pull(instance, self.repository_id(instance)?, true),
            "fetch" => git::fetch(instance, self.repository_id(instance)?),
            "stash-all" => git::stash_all(instance, self.repository_id(instance)?),
            "stash-pop" => git::stash_pop(instance, self.repository_id(instance)?),
            "init-repository" => git::init_repository(instance),
            GENERATE_MESSAGE_TASK_ID => {
                git::generate_commit_message(
                    instance,
                    self.repository_id(instance)?,
                    GENERATE_MESSAGE_TASK_ID,
                )?;
                self.generating_message = true;
                Ok(())
            }
            "open-branch-picker" => {
                self.branch_query = Some(String::new());
                Ok(())
            }
            "close-branch-picker" => {
                self.branch_query = None;
                Ok(())
            }
            "checkout" => {
                git::checkout_branch(instance, self.repository_id(instance)?, target)?;
                self.branch_query = None;
                Ok(())
            }
            "create-branch" => {
                git::create_branch(instance, self.repository_id(instance)?, target)?;
                self.branch_query = None;
                Ok(())
            }
            "dismiss-error" => {
                self.error = None;
                Ok(())
            }
            "toggle-view" => {
                self.view_mode = match self.view_mode {
                    ViewMode::List => ViewMode::Tree,
                    ViewMode::Tree => ViewMode::List,
                };
                Ok(())
            }
            "view-list" => {
                self.view_mode = ViewMode::List;
                Ok(())
            }
            "view-tree" => {
                self.view_mode = ViewMode::Tree;
                Ok(())
            }
            "repository" => {
                self.repository = target.parse().ok();
                Ok(())
            }
            "section" => {
                if let Some(section) = Section::from_key(target)
                    && !self.collapsed_sections.remove(&section)
                {
                    self.collapsed_sections.insert(section);
                }
                Ok(())
            }
            "directory" => {
                if !self.collapsed_directories.remove(target) {
                    self.collapsed_directories.insert(target.to_string());
                }
                Ok(())
            }
            "file" => {
                self.selected_item = Some(id.to_string());
                let Some((section_key, path)) = target.split_once(':') else {
                    return Ok(());
                };
                let kind = Section::from_key(section_key)
                    .map(diff_kind)
                    .unwrap_or(git::DiffKind::Uncommitted);
                git::open_diff(instance, self.repository_id(instance)?, path, kind)
            }
            "open" => git::open_file(instance, self.repository_id(instance)?, target),
            "stage" => git::stage(
                instance,
                self.repository_id(instance)?,
                &[target.to_string()],
            ),
            "unstage" => git::unstage(
                instance,
                self.repository_id(instance)?,
                &[target.to_string()],
            ),
            "discard" => git::discard(
                instance,
                self.repository_id(instance)?,
                &[target.to_string()],
            )
            .map(|_| ()),
            "stage-group" | "unstage-group" | "discard-group" => {
                self.handle_group_action(instance, kind, target)
            }
            _ => Ok(()),
        }
    }

    fn submit_branch_query(&mut self, instance: ui::PanelInstance, query: &str) -> zed::Result<()> {
        if query.is_empty() {
            return Ok(());
        }
        let repository = self.repository_id(instance)?;
        let branches = git::branches(instance, repository)?;
        if branches.iter().any(|branch| branch.name == query) {
            git::checkout_branch(instance, repository, query)?;
        } else {
            git::create_branch(instance, repository, query)?;
        }
        self.branch_query = None;
        Ok(())
    }

    /// Handles an action on a whole section, where `target` is the section's key, or on a
    /// directory within a section, where `target` is `{section}:{directory}`.
    fn handle_group_action(
        &mut self,
        instance: ui::PanelInstance,
        kind: &str,
        target: &str,
    ) -> zed::Result<()> {
        let (section_key, directory) = match target.split_once(':') {
            Some((section_key, directory)) => (section_key, Some(directory)),
            None => (target, None),
        };
        let Some(section) = Section::from_key(section_key) else {
            return Ok(());
        };
        let repository = self.repository_id(instance)?;
        let changes = Changes::new(git::status(instance, repository)?);
        let paths = changes
            .section(section)
            .iter()
            .filter(|change| {
                directory.is_none_or(|directory| {
                    change
                        .path
                        .strip_prefix(directory)
                        .is_some_and(|rest| rest.starts_with('/'))
                })
            })
            .map(|change| change.path.clone())
            .collect::<Vec<_>>();
        if paths.is_empty() {
            return Ok(());
        }

        match (kind, directory) {
            ("stage-group", None) if section == Section::Changes => {
                git::stage_all(instance, repository)
            }
            ("unstage-group", None) => git::unstage_all(instance, repository),
            ("stage-group", _) => git::stage(instance, repository, &paths),
            ("unstage-group", _) => git::unstage(instance, repository, &paths),
            ("discard-group", _) => git::discard(instance, repository, &paths).map(|_| ()),
            _ => Ok(()),
        }
    }

    fn repository_id(&self, instance: ui::PanelInstance) -> zed::Result<git::RepositoryId> {
        let repositories = git::repositories(instance)?;
        selected_repository(self, &repositories)
            .map(|repository| repository.id)
            .ok_or_else(|| "there is no Git repository".to_string())
    }

    fn sync(&mut self, instance: ui::PanelInstance) -> zed::Result<()> {
        let repository = self.repository_id(instance)?;
        git::pull(instance, repository, false)?;
        git::push(instance, repository, false)
    }

    /// Commits the staged changes, or like VS Code's smart commit, stages and commits all
    /// tracked changes when nothing is staged.
    fn commit(
        &mut self,
        instance: ui::PanelInstance,
        amend: bool,
        after: AfterCommit,
    ) -> zed::Result<()> {
        if self.message.trim().is_empty() {
            return Err("Please provide a commit message.".to_string());
        }
        let repository = self.repository_id(instance)?;
        let changes = Changes::new(git::status(instance, repository)?);
        if !changes.merge.is_empty() {
            return Err("Resolve and stage all merge conflicts before committing.".to_string());
        }
        if changes.staged.is_empty() {
            let tracked = changes
                .changes
                .iter()
                .filter(|change| change.letter != "U")
                .map(|change| change.path.clone())
                .collect::<Vec<_>>();
            // Amending only the message is allowed without any changes.
            if tracked.is_empty() && !amend {
                return Err("There are no changes to commit.".to_string());
            }
            if !tracked.is_empty() {
                git::stage(instance, repository, &tracked)?;
            }
        }
        git::commit(
            instance,
            repository,
            &self.message,
            git::CommitOptions {
                amend,
                signoff: false,
            },
        )?;
        self.message.clear();

        match after {
            AfterCommit::Nothing => Ok(()),
            AfterCommit::Push => git::push(instance, repository, false),
            AfterCommit::Sync => self.sync(instance),
        }
    }
}

impl zed::Extension for SourceControlExtension {
    fn new() -> Self {
        Self {
            states: HashMap::new(),
        }
    }

    fn panel_render(
        &mut self,
        _panel_id: &str,
        instance: ui::PanelInstance,
    ) -> zed::Result<ui::Tree> {
        self.states.entry(instance).or_default().render(instance)
    }

    fn panel_handle_event(
        &mut self,
        _panel_id: &str,
        instance: ui::PanelInstance,
        event: ui::Event,
    ) -> zed::Result<()> {
        let state = self.states.entry(instance).or_default();
        // Failed Git operations are shown inline, since returning them would replace the
        // whole panel with an error.
        match state.handle_event(instance, event) {
            Ok(()) => state.error = None,
            Err(error) => state.error = Some(error),
        }
        Ok(())
    }

    fn panel_release(&mut self, _panel_id: &str, instance: ui::PanelInstance) {
        self.states.remove(&instance);
    }
}

zed::register_extension!(SourceControlExtension);
