use zed_extension_api::{self as zed, git, ui};

use crate::changes::{
    Change, Changes, DirectoryNode, Section, ViewMode, file_actions, file_context_menu,
    group_actions, group_context_menu,
};
use crate::{
    BRANCH_QUERY_INPUT_ID, GENERATE_MESSAGE_TASK_ID, MAX_FILES_PER_SECTION, MESSAGE_INPUT_ID,
    PanelState, selected_repository,
};

pub(crate) fn repository_name(repository: &git::Repository) -> &str {
    repository
        .work_directory
        .rsplit(['/', '\\'])
        .find(|component| !component.is_empty())
        .unwrap_or(&repository.work_directory)
}

pub(crate) fn commit_shortcut() -> &'static str {
    match zed::current_platform().0 {
        zed::Os::Mac => "Cmd+Enter",
        zed::Os::Linux | zed::Os::Windows => "Ctrl+Enter",
    }
}

impl PanelState {
    pub(crate) fn render(&self, instance: ui::PanelInstance) -> zed::Result<ui::Tree> {
        let repositories = git::repositories(instance)?;
        let Some(repository) = selected_repository(self, &repositories) else {
            let mut root = ui::v_stack()
                .gap(ui::Spacing::Medium)
                .child(self.render_header(None, None))
                .child(
                    ui::label(
                        "The folder currently open doesn't have a Git repository. You can \
                         initialize a repository which will enable source control features \
                         powered by Git.",
                    )
                    .color(ui::Color::Muted),
                )
                .child(
                    ui::button("init-repository", "Initialize Repository")
                        .style(ui::ButtonStyle::Filled)
                        .full_width(true),
                );
            if let Some(error) = &self.error {
                root = root.child(self.render_error(error));
            }
            return Ok(root.build());
        };
        let changes = Changes::new(git::status(instance, repository.id)?);

        let mut root = ui::v_stack()
            .gap(ui::Spacing::Medium)
            .child(self.render_header(Some(&repository), Some(&changes)));
        if let Some(query) = &self.branch_query {
            root = root.children(self.render_branch_picker(instance, &repository, query)?);
        }
        if repositories.len() > 1 {
            root = root.child(self.render_repositories(&repositories, &repository));
        }
        root = root.child(self.render_commit_box(&repository, &changes));
        if let Some(error) = &self.error {
            root = root.child(self.render_error(error));
        }

        let mut list = ui::v_stack().gap(ui::Spacing::None);
        for section in Section::ALL {
            let section_changes = changes.section(section);
            // Like VS Code, the merge and staged sections only appear when they have
            // files, while the changes section is always shown.
            if section_changes.is_empty() && section != Section::Changes {
                continue;
            }
            list = list.children(self.render_section(section, section_changes));
        }
        let mut tree = root.child(list).build();

        let change_count = changes.merge.len() + changes.staged.len() + changes.changes.len();
        if change_count > 0 {
            tree = tree.badge(u32::try_from(change_count).unwrap_or(u32::MAX));
        }
        Ok(tree)
    }

    pub(crate) fn render_error(&self, error: &str) -> ui::Element {
        ui::h_stack()
            .child(ui::label(error).color(ui::Color::Error))
            .child(ui::spacer())
            .child(ui::icon_button("dismiss-error", "close").tooltip("Dismiss"))
    }

    pub(crate) fn render_header(
        &self,
        repository: Option<&git::Repository>,
        changes: Option<&Changes>,
    ) -> ui::Element {
        let mut header = ui::h_stack().gap(ui::Spacing::None).child(
            ui::label("SOURCE CONTROL")
                .size(ui::LabelSize::Small)
                .color(ui::Color::Muted)
                .bold(true),
        );
        header = header.child(ui::spacer());
        if let Some(changes) = changes {
            let (view_icon, view_tooltip) = match self.view_mode {
                ViewMode::List => ("list_tree", "View as Tree"),
                ViewMode::Tree => ("menu", "View as List"),
            };
            header = header
                .child(ui::icon_button("toggle-view", view_icon).tooltip(view_tooltip))
                .child(
                    ui::icon_button("commit", "check")
                        .tooltip("Commit")
                        .disabled(!self.can_commit(changes)),
                );
        }
        header = header.child(ui::icon_button("refresh", "rotate_cw").tooltip("Refresh"));
        if let Some(repository) = repository {
            header = header.child(
                ui::icon_button("more", "ellipsis")
                    .tooltip("More Actions…")
                    .menu(self.more_actions_menu(repository)),
            );
        }
        header
    }

    pub(crate) fn more_actions_menu(&self, repository: &git::Repository) -> Vec<ui::MenuEntry> {
        let has_branch = repository.branch.is_some();
        vec![
            ui::menu_header("Pull, Push"),
            ui::menu_item("pull", "Pull").disabled(!has_branch),
            ui::menu_item("pull-rebase", "Pull (Rebase)").disabled(!has_branch),
            ui::menu_item("push", "Push").disabled(!has_branch),
            ui::menu_item("fetch", "Fetch"),
            ui::menu_separator(),
            ui::menu_header("Branch"),
            ui::menu_item("open-branch-picker", "Checkout to…").icon("git_branch"),
            ui::menu_separator(),
            ui::menu_header("Stash"),
            ui::menu_item("stash-all", "Stash All Changes"),
            ui::menu_item("stash-pop", "Pop Latest Stash"),
            ui::menu_separator(),
            ui::menu_header("View"),
            ui::menu_item("view-list", "View as List").checked(self.view_mode == ViewMode::List),
            ui::menu_item("view-tree", "View as Tree").checked(self.view_mode == ViewMode::Tree),
        ]
    }

    /// An inline equivalent of VS Code's "Checkout to…" quick pick: typing filters the
    /// branches, and a name that matches no branch can be created.
    pub(crate) fn render_branch_picker(
        &self,
        instance: ui::PanelInstance,
        repository: &git::Repository,
        query: &str,
    ) -> zed::Result<Vec<ui::Element>> {
        let branches = git::branches(instance, repository.id)?;
        let query = query.trim();
        let query_lowercase = query.to_lowercase();

        let mut items = Vec::new();
        if !query.is_empty() && !branches.iter().any(|branch| branch.name == query) {
            items.push(
                ui::list_item(
                    format!("create-branch:{query}"),
                    format!("Create new branch '{query}'"),
                )
                .icon("plus"),
            );
        }
        let matches = branches
            .iter()
            .filter(|branch| branch.name.to_lowercase().contains(&query_lowercase));
        for branch in matches {
            let mut item = ui::list_item(format!("checkout:{}", branch.name), branch.name.as_str())
                .icon("git_branch")
                .selected(branch.is_head);
            if branch.is_head {
                item = item.description("current");
            } else if branch.is_remote {
                item = item.description("remote branch");
            }
            items.push(item);
        }

        Ok(vec![
            ui::h_stack()
                .child(
                    ui::label("Select a branch to checkout or type a new branch name")
                        .size(ui::LabelSize::Small)
                        .color(ui::Color::Muted),
                )
                .child(ui::spacer())
                .child(ui::icon_button("close-branch-picker", "close").tooltip("Close")),
            ui::text_input(BRANCH_QUERY_INPUT_ID, "Branch name", query),
            ui::v_stack().gap(ui::Spacing::None).children(items),
        ])
    }

    pub(crate) fn render_repositories(
        &self,
        repositories: &[git::Repository],
        selected: &git::Repository,
    ) -> ui::Element {
        let items = repositories.iter().map(|repository| {
            let mut item = ui::list_item(
                format!("repository:{}", repository.id),
                repository_name(repository),
            )
            .icon("git_branch")
            .selected(repository.id == selected.id)
            .tooltip(repository.work_directory.as_str());
            if let Some(branch) = &repository.branch {
                item = item.description(branch.as_str());
            }
            if let Some(upstream) = &repository.upstream
                && (upstream.ahead > 0 || upstream.behind > 0)
            {
                item = item.decoration(
                    format!("{}↓ {}↑", upstream.behind, upstream.ahead),
                    ui::Color::Muted,
                );
            }
            item
        });
        ui::v_stack().gap(ui::Spacing::None).children(items)
    }

    pub(crate) fn render_commit_box(
        &self,
        repository: &git::Repository,
        changes: &Changes,
    ) -> ui::Element {
        let placeholder = match &repository.branch {
            Some(branch) => format!("Message ({} to commit on '{branch}')", commit_shortcut()),
            None => format!("Message ({} to commit)", commit_shortcut()),
        };
        let input =
            ui::text_input(MESSAGE_INPUT_ID, placeholder, self.message.as_str()).multi_line(true);

        // Like VS Code, the commit button turns into a sync or publish button when there
        // is nothing to commit but the branch differs from its upstream.
        let has_changes =
            !changes.merge.is_empty() || !changes.staged.is_empty() || !changes.changes.is_empty();
        let main_button = match (&repository.branch, &repository.upstream) {
            (Some(_), Some(upstream))
                if !has_changes && (upstream.ahead > 0 || upstream.behind > 0) =>
            {
                ui::button(
                    "sync",
                    format!("Sync Changes {}↓ {}↑", upstream.behind, upstream.ahead),
                )
                .icon("rotate_cw")
                .tooltip(format!(
                    "Pull and push commits to and from '{}'",
                    upstream.name
                ))
            }
            (Some(branch), None) if !has_changes => ui::button("publish", "Publish Branch")
                .icon("arrow_up")
                .tooltip(format!("Publish '{branch}' to a remote")),
            _ => {
                let tooltip = if changes.staged.is_empty() {
                    "Stage all tracked changes and commit them"
                } else {
                    "Commit the staged changes"
                };
                ui::button("commit", "Commit")
                    .icon("check")
                    .tooltip(tooltip)
                    .disabled(!self.can_commit(changes))
                    .menu([
                        ui::menu_item("commit", "Commit").disabled(!self.can_commit(changes)),
                        ui::menu_item("commit-amend", "Commit (Amend)")
                            .disabled(self.message.trim().is_empty()),
                        ui::menu_separator(),
                        ui::menu_item("commit-push", "Commit & Push")
                            .disabled(!self.can_commit(changes)),
                        ui::menu_item("commit-sync", "Commit & Sync")
                            .disabled(!self.can_commit(changes)),
                    ])
            }
        };
        let has_changes_to_describe = !changes.staged.is_empty() || !changes.changes.is_empty();
        let generate_button = ui::icon_button(GENERATE_MESSAGE_TASK_ID, "sparkle")
            .tooltip(if self.generating_message {
                "Generating Commit Message…"
            } else {
                "Generate Commit Message"
            })
            .disabled(self.generating_message || !has_changes_to_describe);

        let mut commit_box = ui::v_stack().child(ui::h_stack().child(input).child(generate_button));
        if self.generating_message {
            commit_box = commit_box.child(
                ui::label("Generating commit message…")
                    .size(ui::LabelSize::Small)
                    .color(ui::Color::Muted),
            );
        }
        commit_box.child(main_button.style(ui::ButtonStyle::Filled).full_width(true))
    }

    pub(crate) fn can_commit(&self, changes: &Changes) -> bool {
        changes.merge.is_empty()
            && !self.message.trim().is_empty()
            && (!changes.staged.is_empty()
                || changes.changes.iter().any(|change| change.letter != "U"))
    }

    pub(crate) fn render_section(&self, section: Section, changes: &[Change]) -> Vec<ui::Element> {
        let expanded = !self.collapsed_sections.contains(&section);
        let mut header = ui::list_item(format!("section:{}", section.key()), section.title())
            .expanded(Some(expanded))
            .decoration(changes.len().to_string(), ui::Color::Muted);
        if !changes.is_empty() {
            for action in group_actions(section, section.key()) {
                header = header.action(action);
            }
            header = header.context_menu(group_context_menu(section, section.key()));
        }

        let mut rows = vec![header];
        if !expanded {
            return rows;
        }
        let shown = &changes[..changes.len().min(MAX_FILES_PER_SECTION)];
        match self.view_mode {
            ViewMode::List => {
                rows.extend(
                    shown
                        .iter()
                        .map(|change| self.render_file(section, change, 1, true)),
                );
            }
            ViewMode::Tree => {
                self.render_directory(section, &DirectoryNode::new(shown), "", 1, &mut rows);
            }
        }
        if changes.len() > shown.len() {
            rows.push(
                ui::list_item(
                    format!("hidden:{}", section.key()),
                    format!("{} more files not shown", changes.len() - shown.len()),
                )
                .indent(1)
                .color(ui::Color::Muted),
            );
        }
        rows
    }

    pub(crate) fn render_directory(
        &self,
        section: Section,
        node: &DirectoryNode,
        parent: &str,
        depth: u32,
        rows: &mut Vec<ui::Element>,
    ) {
        for (name, directory) in &node.directories {
            let path = if parent.is_empty() {
                name.clone()
            } else {
                format!("{parent}/{name}")
            };
            let key = format!("{}:{path}", section.key());
            let expanded = !self.collapsed_directories.contains(&key);
            let mut row = ui::list_item(format!("directory:{key}"), name.as_str())
                .indent(depth)
                .icon(ui::Icon::Folder(path.clone()))
                .expanded(Some(expanded))
                .context_menu(group_context_menu(section, &key));
            for action in group_actions(section, &key) {
                row = row.action(action);
            }
            rows.push(row);
            if expanded {
                self.render_directory(section, directory, &path, depth + 1, rows);
            }
        }
        rows.extend(
            node.files
                .iter()
                .map(|change| self.render_file(section, change, depth, false)),
        );
    }

    pub(crate) fn render_file(
        &self,
        section: Section,
        change: &Change,
        depth: u32,
        show_directory: bool,
    ) -> ui::Element {
        let id = format!("file:{}:{}", section.key(), change.path);
        let mut item = ui::list_item(id.as_str(), change.file_name())
            .indent(depth)
            .icon(ui::Icon::File(change.path.clone()))
            .color(change.color)
            .strikethrough(change.deleted)
            .decoration(change.letter, change.color)
            .selected(self.selected_item.as_deref() == Some(id.as_str()))
            .tooltip(change.path.as_str())
            .context_menu(file_context_menu(section, &change.path));
        if show_directory && let Some(directory) = change.directory() {
            item = item.description(directory);
        }
        for action in file_actions(section, &change.path) {
            item = item.action(action);
        }
        item
    }
}
