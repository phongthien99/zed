use std::collections::BTreeMap;

use zed_extension_api::{git, ui};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Section {
    Merge,
    Staged,
    Changes,
}

impl Section {
    pub(crate) const ALL: [Section; 3] = [Section::Merge, Section::Staged, Section::Changes];

    pub(crate) fn key(self) -> &'static str {
        match self {
            Section::Merge => "merge",
            Section::Staged => "staged",
            Section::Changes => "changes",
        }
    }

    pub(crate) fn from_key(key: &str) -> Option<Self> {
        Section::ALL
            .into_iter()
            .find(|section| section.key() == key)
    }

    pub(crate) fn title(self) -> &'static str {
        match self {
            Section::Merge => "Merge Changes",
            Section::Staged => "Staged Changes",
            Section::Changes => "Changes",
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ViewMode {
    #[default]
    List,
    Tree,
}

/// A changed file as displayed in one section of the panel.
pub(crate) struct Change {
    pub(crate) path: String,
    pub(crate) letter: &'static str,
    pub(crate) color: ui::Color,
    pub(crate) deleted: bool,
}

impl Change {
    pub(crate) fn file_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    pub(crate) fn directory(&self) -> Option<&str> {
        self.path.rsplit_once('/').map(|(directory, _)| directory)
    }
}

#[derive(Default)]
pub(crate) struct Changes {
    pub(crate) merge: Vec<Change>,
    pub(crate) staged: Vec<Change>,
    pub(crate) changes: Vec<Change>,
}

impl Changes {
    pub(crate) fn new(entries: Vec<git::StatusEntry>) -> Self {
        let mut changes = Changes::default();
        for entry in entries {
            match entry.status {
                git::FileStatus::Conflicted => changes.merge.push(Change {
                    path: entry.path,
                    letter: "!",
                    color: ui::Color::Conflict,
                    deleted: false,
                }),
                git::FileStatus::Untracked => changes.changes.push(Change {
                    path: entry.path,
                    letter: "U",
                    color: ui::Color::Created,
                    deleted: false,
                }),
                git::FileStatus::Ignored => {}
                git::FileStatus::Tracked(tracked) => {
                    if let Some(change) = Change::for_code(&entry.path, tracked.index) {
                        changes.staged.push(change);
                    }
                    if let Some(change) = Change::for_code(&entry.path, tracked.worktree) {
                        changes.changes.push(change);
                    }
                }
            }
        }
        for section in [
            &mut changes.merge,
            &mut changes.staged,
            &mut changes.changes,
        ] {
            section.sort_by(|a, b| a.path.cmp(&b.path));
        }
        changes
    }

    pub(crate) fn section(&self, section: Section) -> &[Change] {
        match section {
            Section::Merge => &self.merge,
            Section::Staged => &self.staged,
            Section::Changes => &self.changes,
        }
    }
}

impl Change {
    pub(crate) fn for_code(path: &str, code: git::StatusCode) -> Option<Self> {
        let (letter, color) = match code {
            git::StatusCode::Unmodified => return None,
            git::StatusCode::Modified => ("M", ui::Color::Modified),
            git::StatusCode::TypeChanged => ("T", ui::Color::Modified),
            git::StatusCode::Added => ("A", ui::Color::Created),
            git::StatusCode::Deleted => ("D", ui::Color::Deleted),
            git::StatusCode::Renamed => ("R", ui::Color::Modified),
            git::StatusCode::Copied => ("C", ui::Color::Created),
        };
        Some(Self {
            path: path.to_string(),
            letter,
            color,
            deleted: code == git::StatusCode::Deleted,
        })
    }
}

/// The directories and files below one directory of a section's tree view.
#[derive(Default)]
pub(crate) struct DirectoryNode<'a> {
    pub(crate) directories: BTreeMap<String, DirectoryNode<'a>>,
    pub(crate) files: Vec<&'a Change>,
}

impl<'a> DirectoryNode<'a> {
    pub(crate) fn new(changes: &'a [Change]) -> Self {
        let mut root = DirectoryNode::default();
        for change in changes {
            let mut node = &mut root;
            if let Some(directory) = change.directory() {
                for component in directory.split('/') {
                    node = node.directories.entry(component.to_string()).or_default();
                }
            }
            node.files.push(change);
        }
        root.compact();
        root
    }

    /// Merges directories that only contain a single directory into it, the way VS Code's
    /// compact folders display `src/components` as one row.
    pub(crate) fn compact(&mut self) {
        let directories = std::mem::take(&mut self.directories);
        for (mut name, mut node) in directories {
            while node.files.is_empty() && node.directories.len() == 1 {
                let Some((child_name, child)) = node.directories.pop_first() else {
                    break;
                };
                name = format!("{name}/{child_name}");
                node = child;
            }
            node.compact();
            self.directories.insert(name, node);
        }
    }
}

pub(crate) fn diff_kind(section: Section) -> git::DiffKind {
    match section {
        Section::Merge => git::DiffKind::Uncommitted,
        Section::Staged => git::DiffKind::Staged,
        Section::Changes => git::DiffKind::Unstaged,
    }
}

pub(crate) fn file_actions(section: Section, path: &str) -> Vec<ui::IconButton> {
    let open = ui::action(format!("open:{path}"), "file").tooltip("Open File");
    match section {
        Section::Merge => vec![
            open,
            ui::action(format!("stage:{path}"), "plus").tooltip("Stage Changes"),
        ],
        Section::Staged => vec![
            open,
            ui::action(format!("unstage:{path}"), "dash").tooltip("Unstage Changes"),
        ],
        Section::Changes => vec![
            open,
            ui::action(format!("discard:{path}"), "undo").tooltip("Discard Changes"),
            ui::action(format!("stage:{path}"), "plus").tooltip("Stage Changes"),
        ],
    }
}

pub(crate) fn file_context_menu(section: Section, path: &str) -> Vec<ui::MenuEntry> {
    let mut entries = vec![
        ui::menu_item(format!("file:{}:{path}", section.key()), "Open Changes").icon("diff"),
        ui::menu_item(format!("open:{path}"), "Open File").icon("file"),
        ui::menu_separator(),
    ];
    match section {
        Section::Merge => {
            entries.push(ui::menu_item(format!("stage:{path}"), "Stage Changes").icon("plus"));
        }
        Section::Staged => {
            entries.push(ui::menu_item(format!("unstage:{path}"), "Unstage Changes").icon("dash"));
        }
        Section::Changes => {
            entries.push(ui::menu_item(format!("stage:{path}"), "Stage Changes").icon("plus"));
            entries.push(ui::menu_item(format!("discard:{path}"), "Discard Changes").icon("undo"));
        }
    }
    entries
}

pub(crate) fn group_actions(section: Section, target: &str) -> Vec<ui::IconButton> {
    match section {
        Section::Merge => {
            vec![ui::action(format!("stage-group:{target}"), "plus").tooltip("Stage All Changes")]
        }
        Section::Staged => {
            vec![
                ui::action(format!("unstage-group:{target}"), "dash")
                    .tooltip("Unstage All Changes"),
            ]
        }
        Section::Changes => vec![
            ui::action(format!("discard-group:{target}"), "undo").tooltip("Discard All Changes"),
            ui::action(format!("stage-group:{target}"), "plus").tooltip("Stage All Changes"),
        ],
    }
}

pub(crate) fn group_context_menu(section: Section, target: &str) -> Vec<ui::MenuEntry> {
    match section {
        Section::Merge => {
            vec![ui::menu_item(format!("stage-group:{target}"), "Stage All Changes").icon("plus")]
        }
        Section::Staged => vec![
            ui::menu_item(format!("unstage-group:{target}"), "Unstage All Changes").icon("dash"),
        ],
        Section::Changes => vec![
            ui::menu_item(format!("stage-group:{target}"), "Stage All Changes").icon("plus"),
            ui::menu_item(format!("discard-group:{target}"), "Discard All Changes").icon("undo"),
        ],
    }
}
