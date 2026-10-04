use anyhow::{Result, bail};

pub type UiNodeId = u32;

/// Panels render list items lazily, so this mainly bounds the cost of sending a tree from
/// the extension.
pub const MAX_UI_TREE_NODES: usize = 20_000;
pub const MAX_UI_STRING_BYTES: usize = 64 * 1024;
/// List item actions are stored inline rather than as nodes, so they need their own limit.
pub const MAX_UI_LIST_ITEM_ACTIONS: usize = 8;
/// Menu entries are stored inline rather than as nodes, so they need their own limit.
pub const MAX_UI_MENU_ENTRIES: usize = 100;

/// Identifies one instance of a panel. Every workspace gets its own instance of each panel.
pub type PanelInstanceId = u64;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UiColor {
    #[default]
    Default,
    Muted,
    Accent,
    Success,
    Warning,
    Error,
    Created,
    Modified,
    Deleted,
    Conflict,
    Ignored,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UiSpacing {
    None,
    #[default]
    Small,
    Medium,
    Large,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiStack {
    pub children: Vec<UiNodeId>,
    pub gap: UiSpacing,
}

impl UiStack {
    pub fn new(children: Vec<UiNodeId>) -> Self {
        Self {
            children,
            gap: UiSpacing::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UiLabelSize {
    #[default]
    Default,
    Small,
    XSmall,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiLabel {
    pub text: String,
    pub color: UiColor,
    pub size: UiLabelSize,
    pub bold: bool,
    pub strikethrough: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiIcon {
    Named(String),
    /// The icon theme's icon for the file at this path.
    File(String),
    /// The icon theme's icon for the directory at this path.
    Folder(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UiButtonStyle {
    #[default]
    Default,
    Filled,
    Subtle,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiMenuItem {
    pub id: String,
    pub label: String,
    pub icon: Option<String>,
    pub disabled: bool,
    /// `None` for plain items, `Some(checked)` for items showing a check mark.
    pub checked: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiMenuEntry {
    Item(UiMenuItem),
    Separator,
    Header(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiButton {
    pub id: String,
    pub label: String,
    pub disabled: bool,
    pub icon: Option<String>,
    pub style: UiButtonStyle,
    pub full_width: bool,
    pub tooltip: Option<String>,
    /// Shown in a dropdown attached to the button when not empty.
    pub menu: Vec<UiMenuEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiIconButton {
    pub id: String,
    pub icon: String,
    pub tooltip: Option<String>,
    pub disabled: bool,
    /// Opened by clicking the button when not empty.
    pub menu: Vec<UiMenuEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiTextInput {
    pub id: String,
    pub placeholder: String,
    pub value: String,
    pub multi_line: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiCheckbox {
    pub id: String,
    pub label: String,
    pub checked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiDecoration {
    pub text: String,
    pub color: UiColor,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiListItem {
    pub id: String,
    pub label: String,
    pub indent: u32,
    pub icon: Option<UiIcon>,
    pub selected: bool,
    /// `None` for leaf items, `Some(expanded)` for items that can be expanded or collapsed.
    pub expanded: Option<bool>,
    pub description: Option<String>,
    pub label_color: UiColor,
    pub strikethrough: bool,
    pub decoration: Option<UiDecoration>,
    /// Buttons shown while the item is hovered.
    pub actions: Vec<UiIconButton>,
    pub tooltip: Option<String>,
    /// Opened by right-clicking the item when not empty.
    pub context_menu: Vec<UiMenuEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiNode {
    VStack(UiStack),
    HStack(UiStack),
    Label(UiLabel),
    Button(UiButton),
    IconButton(UiIconButton),
    TextInput(UiTextInput),
    Checkbox(UiCheckbox),
    ListItem(UiListItem),
    Divider,
    Spacer,
}

/// A UI tree produced by an extension.
///
/// WIT has no recursive types, so the tree is stored as an arena where container
/// nodes reference their children by index into `nodes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiTree {
    pub nodes: Vec<UiNode>,
    pub root: UiNodeId,
    /// A count shown on the panel's icon in the dock while the panel is closed.
    pub badge: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiEvent {
    Clicked(String),
    InputChanged {
        id: String,
        value: String,
    },
    InputSubmitted {
        id: String,
        value: String,
    },
    CheckboxToggled {
        id: String,
        checked: bool,
    },
    ListItemToggled(String),
    /// A long-running host operation started by the extension finished.
    TaskCompleted {
        id: String,
        result: Result<String, String>,
    },
}

impl UiTree {
    pub fn node(&self, id: UiNodeId) -> Option<&UiNode> {
        self.nodes.get(id as usize)
    }

    /// Validates a tree received from an untrusted extension.
    ///
    /// Every node must be reachable from the root at most once, which rules out
    /// cycles and shared children, and all strings must be within size limits.
    pub fn validate(&self) -> Result<()> {
        if self.nodes.len() > MAX_UI_TREE_NODES {
            bail!(
                "UI tree has {} nodes, exceeding the limit of {MAX_UI_TREE_NODES}",
                self.nodes.len()
            );
        }
        if self.node(self.root).is_none() {
            bail!("UI tree root {} does not exist", self.root);
        }

        let mut visited = vec![false; self.nodes.len()];
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            let Some(node) = self.node(id) else {
                bail!("UI tree references missing node {id}");
            };
            let Some(seen) = visited.get_mut(id as usize) else {
                bail!("UI tree references missing node {id}");
            };
            if *seen {
                bail!("UI tree node {id} is referenced more than once");
            }
            *seen = true;

            for text in node_strings(node) {
                if text.len() > MAX_UI_STRING_BYTES {
                    bail!("UI tree node {id} has a string exceeding {MAX_UI_STRING_BYTES} bytes");
                }
            }

            if let UiNode::ListItem(item) = node
                && item.actions.len() > MAX_UI_LIST_ITEM_ACTIONS
            {
                bail!(
                    "UI tree node {id} has {} actions, exceeding the limit of {MAX_UI_LIST_ITEM_ACTIONS}",
                    item.actions.len()
                );
            }

            for menu in node_menus(node) {
                if menu.len() > MAX_UI_MENU_ENTRIES {
                    bail!(
                        "UI tree node {id} has a menu with {} entries, exceeding the limit of {MAX_UI_MENU_ENTRIES}",
                        menu.len()
                    );
                }
            }

            if let UiNode::VStack(children) | UiNode::HStack(children) = node {
                stack.extend(children.children.iter().copied());
            }
        }
        Ok(())
    }
}

fn node_menus(node: &UiNode) -> Vec<&[UiMenuEntry]> {
    match node {
        UiNode::Button(button) => vec![&button.menu],
        UiNode::IconButton(button) => vec![&button.menu],
        UiNode::ListItem(item) => {
            let mut menus = vec![item.context_menu.as_slice()];
            menus.extend(item.actions.iter().map(|action| action.menu.as_slice()));
            menus
        }
        _ => Vec::new(),
    }
}

fn menu_strings(menu: &[UiMenuEntry]) -> impl Iterator<Item = &str> {
    menu.iter().flat_map(|entry| match entry {
        UiMenuEntry::Item(item) => {
            let mut strings = vec![item.id.as_str(), item.label.as_str()];
            strings.extend(item.icon.as_deref());
            strings
        }
        UiMenuEntry::Separator => Vec::new(),
        UiMenuEntry::Header(text) => vec![text.as_str()],
    })
}

fn node_strings(node: &UiNode) -> Vec<&str> {
    match node {
        UiNode::VStack(_) | UiNode::HStack(_) | UiNode::Divider | UiNode::Spacer => Vec::new(),
        UiNode::Label(label) => vec![&label.text],
        UiNode::Button(button) => {
            let mut strings = vec![button.id.as_str(), button.label.as_str()];
            strings.extend(button.icon.as_deref());
            strings.extend(button.tooltip.as_deref());
            strings.extend(menu_strings(&button.menu));
            strings
        }
        UiNode::IconButton(button) => icon_button_strings(button),
        UiNode::TextInput(input) => vec![&input.id, &input.placeholder, &input.value],
        UiNode::Checkbox(checkbox) => vec![&checkbox.id, &checkbox.label],
        UiNode::ListItem(item) => {
            let mut strings = vec![item.id.as_str(), item.label.as_str()];
            if let Some(UiIcon::Named(text) | UiIcon::File(text) | UiIcon::Folder(text)) =
                &item.icon
            {
                strings.push(text);
            }
            strings.extend(item.description.as_deref());
            strings.extend(
                item.decoration
                    .as_ref()
                    .map(|decoration| decoration.text.as_str()),
            );
            strings.extend(item.tooltip.as_deref());
            strings.extend(item.actions.iter().flat_map(icon_button_strings));
            strings.extend(menu_strings(&item.context_menu));
            strings
        }
    }
}

fn icon_button_strings(button: &UiIconButton) -> Vec<&str> {
    let mut strings = vec![button.id.as_str(), button.icon.as_str()];
    strings.extend(button.tooltip.as_deref());
    strings.extend(menu_strings(&button.menu));
    strings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_accepts_valid_tree() {
        let tree = UiTree {
            nodes: vec![
                UiNode::VStack(UiStack::new(vec![1, 2])),
                UiNode::Label(UiLabel {
                    text: "hello".into(),
                    ..UiLabel::default()
                }),
                UiNode::Divider,
            ],
            root: 0,
            badge: None,
        };
        assert!(tree.validate().is_ok());
    }

    #[test]
    fn test_validate_rejects_missing_root() {
        let tree = UiTree {
            nodes: vec![UiNode::Divider],
            root: 1,
            badge: None,
        };
        assert!(tree.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_missing_child() {
        let tree = UiTree {
            nodes: vec![UiNode::VStack(UiStack::new(vec![5]))],
            root: 0,
            badge: None,
        };
        assert!(tree.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_cycle() {
        let tree = UiTree {
            nodes: vec![
                UiNode::VStack(UiStack::new(vec![1])),
                UiNode::VStack(UiStack::new(vec![0])),
            ],
            root: 0,
            badge: None,
        };
        assert!(tree.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_too_many_list_item_actions() {
        let tree = UiTree {
            nodes: vec![UiNode::ListItem(UiListItem {
                id: "item".into(),
                actions: vec![UiIconButton::default(); MAX_UI_LIST_ITEM_ACTIONS + 1],
                ..UiListItem::default()
            })],
            root: 0,
            badge: None,
        };
        assert!(tree.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_too_many_menu_entries() {
        let tree = UiTree {
            nodes: vec![UiNode::ListItem(UiListItem {
                id: "item".into(),
                context_menu: vec![UiMenuEntry::Separator; MAX_UI_MENU_ENTRIES + 1],
                ..UiListItem::default()
            })],
            root: 0,
            badge: None,
        };
        assert!(tree.validate().is_err());

        let tree = UiTree {
            nodes: vec![UiNode::IconButton(UiIconButton {
                id: "more".into(),
                menu: vec![UiMenuEntry::Separator; MAX_UI_MENU_ENTRIES],
                ..UiIconButton::default()
            })],
            root: 0,
            badge: None,
        };
        assert!(tree.validate().is_ok());
    }

    #[test]
    fn test_validate_rejects_too_many_nodes() {
        let tree = UiTree {
            nodes: vec![UiNode::Divider; MAX_UI_TREE_NODES + 1],
            root: 0,
            badge: None,
        };
        assert!(tree.validate().is_err());
    }
}
