//! Declarative UI for extension panels.
//!
//! An extension describes the contents of a panel by returning a [`Tree`] from
//! [`crate::Extension::panel_render`], and receives user interactions through
//! [`crate::Extension::panel_handle_event`].

use crate::wit::zed::extension::ui as wit_ui;

pub use wit_ui::{ButtonStyle, Color, LabelSize, Spacing};

mod element;

pub use element::*;

/// Identifies one instance of a panel.
///
/// Every workspace window gets its own instance of each panel, all served by the same
/// extension, so state that belongs to one window should be keyed by its instance.
pub type PanelInstance = wit_ui::PanelInstance;

/// An icon displayed by a list item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Icon {
    /// One of Zed's named icons, such as `"git_branch"` or `"folder"`.
    Named(String),
    /// The icon that the current icon theme uses for the file at the given path.
    File(String),
    /// The icon that the current icon theme uses for the directory at the given path.
    Folder(String),
}

impl From<Icon> for wit_ui::Icon {
    fn from(icon: Icon) -> Self {
        match icon {
            Icon::Named(name) => Self::Named(name),
            Icon::File(path) => Self::File(path),
            Icon::Folder(path) => Self::Folder(path),
        }
    }
}

impl From<&str> for Icon {
    fn from(name: &str) -> Self {
        Self::Named(name.to_string())
    }
}

impl From<String> for Icon {
    fn from(name: String) -> Self {
        Self::Named(name)
    }
}

/// An entry in a menu, created with [`menu_item`], [`menu_separator`] or [`menu_header`].
#[derive(Debug, Clone)]
pub struct MenuEntry(wit_ui::MenuEntry);

impl MenuEntry {
    /// Sets the named icon displayed before an item's label. Only has an effect on items.
    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        if let wit_ui::MenuEntry::Item(item) = &mut self.0 {
            item.icon = Some(icon.into());
        }
        self
    }

    /// Sets whether an item is disabled. Only has an effect on items.
    pub fn disabled(mut self, disabled: bool) -> Self {
        if let wit_ui::MenuEntry::Item(item) = &mut self.0 {
            item.disabled = disabled;
        }
        self
    }

    /// Shows a check mark next to an item, filled in when `checked` is true. Only has an
    /// effect on items.
    pub fn checked(mut self, checked: bool) -> Self {
        if let wit_ui::MenuEntry::Item(item) = &mut self.0 {
            item.checked = Some(checked);
        }
        self
    }
}

/// Returns a menu item that produces [`Event::Clicked`] with the given `id` when selected.
pub fn menu_item(id: impl Into<String>, label: impl Into<String>) -> MenuEntry {
    MenuEntry(wit_ui::MenuEntry::Item(wit_ui::MenuItem {
        id: id.into(),
        label: label.into(),
        icon: None,
        disabled: false,
        checked: None,
    }))
}

/// Returns a line separating groups of menu items.
pub fn menu_separator() -> MenuEntry {
    MenuEntry(wit_ui::MenuEntry::Separator)
}

/// Returns a heading for the menu items that follow.
pub fn menu_header(text: impl Into<String>) -> MenuEntry {
    MenuEntry(wit_ui::MenuEntry::Header(text.into()))
}

fn menu_entries(entries: impl IntoIterator<Item = MenuEntry>) -> Vec<wit_ui::MenuEntry> {
    entries.into_iter().map(|entry| entry.0).collect()
}

/// A button that only displays an icon, used for list item actions.
#[derive(Debug, Clone)]
pub struct IconButton(wit_ui::IconButton);

impl IconButton {
    /// Sets the text displayed when the button is hovered.
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.0.tooltip = Some(tooltip.into());
        self
    }

    /// Sets whether the button is disabled.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.0.disabled = disabled;
        self
    }

    /// Makes the button open a menu instead of producing [`Event::Clicked`].
    pub fn menu(mut self, entries: impl IntoIterator<Item = MenuEntry>) -> Self {
        self.0.menu = menu_entries(entries);
        self
    }
}

/// Returns an icon button for use as a list item action, which produces
/// [`Event::Clicked`] with the given `id` when clicked.
pub fn action(id: impl Into<String>, icon: impl Into<String>) -> IconButton {
    IconButton(wit_ui::IconButton {
        id: id.into(),
        icon: icon.into(),
        tooltip: None,
        disabled: false,
        menu: Vec::new(),
    })
}

/// A UI tree to be displayed in a panel.
///
/// Construct one by building a nested [`Element`] and calling [`Element::build`].
#[derive(Debug, Clone)]
pub struct Tree {
    nodes: Vec<wit_ui::Node>,
    root: u32,
    badge: Option<u32>,
}

impl Tree {
    /// Sets a count shown on the panel's icon in the dock while the panel is closed, such
    /// as the number of pending items.
    pub fn badge(mut self, count: u32) -> Self {
        self.badge = Some(count);
        self
    }
}

impl From<Tree> for wit_ui::UiTree {
    fn from(tree: Tree) -> Self {
        Self {
            nodes: tree.nodes,
            root: tree.root,
            badge: tree.badge,
        }
    }
}

/// An event produced by user interaction with a panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The button, icon button, list item or list item action with the given identifier
    /// was clicked.
    Clicked(String),
    /// The value of a text input changed.
    InputChanged { id: String, value: String },
    /// A text input was submitted.
    InputSubmitted { id: String, value: String },
    /// A checkbox was toggled.
    CheckboxToggled { id: String, checked: bool },
    /// The list item with the given identifier was expanded or collapsed.
    ListItemToggled(String),
    /// A long-running operation started for this panel instance, such as
    /// [`crate::git::generate_commit_message`], finished with the given output or error.
    TaskCompleted {
        id: String,
        result: Result<String, String>,
    },
}

impl From<wit_ui::UiEvent> for Event {
    fn from(event: wit_ui::UiEvent) -> Self {
        match event {
            wit_ui::UiEvent::Clicked(id) => Self::Clicked(id),
            wit_ui::UiEvent::InputChanged(input) => Self::InputChanged {
                id: input.id,
                value: input.value,
            },
            wit_ui::UiEvent::InputSubmitted(input) => Self::InputSubmitted {
                id: input.id,
                value: input.value,
            },
            wit_ui::UiEvent::CheckboxToggled(checkbox) => Self::CheckboxToggled {
                id: checkbox.id,
                checked: checkbox.checked,
            },
            wit_ui::UiEvent::ListItemToggled(id) => Self::ListItemToggled(id),
            wit_ui::UiEvent::TaskCompleted(task) => Self::TaskCompleted {
                id: task.id,
                result: task.output,
            },
        }
    }
}

/// Requests that Zed call [`crate::Extension::panel_render`] again for every instance of
/// the given panel.
///
/// This is useful when the panel's state changes for reasons other than a UI event.
pub fn request_render(panel_id: &str) {
    wit_ui::request_render(panel_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_single_element() {
        let tree = label("hello").build();
        assert_eq!(tree.root, 0);
        assert_eq!(tree.nodes.len(), 1);
        assert!(matches!(&tree.nodes[0], wit_ui::Node::Label(label) if label.text == "hello"));
    }

    #[test]
    fn test_build_flattens_nested_children() {
        let tree = v_stack()
            .child(label("title"))
            .child(h_stack().children([button("ok", "OK"), divider()]))
            .child(checkbox("flag", "Flag", true))
            .build();

        assert_eq!(tree.root, 0);
        assert_eq!(tree.nodes.len(), 6);
        assert!(
            matches!(&tree.nodes[0], wit_ui::Node::VStack(stack) if stack.children == [1, 2, 5])
        );
        assert!(matches!(&tree.nodes[1], wit_ui::Node::Label(label) if label.text == "title"));
        assert!(matches!(&tree.nodes[2], wit_ui::Node::HStack(stack) if stack.children == [3, 4]));
        assert!(matches!(&tree.nodes[3], wit_ui::Node::Button(button) if button.id == "ok"));
        assert!(matches!(&tree.nodes[4], wit_ui::Node::Divider));
        assert!(
            matches!(&tree.nodes[5], wit_ui::Node::Checkbox(checkbox) if checkbox.id == "flag" && checkbox.checked)
        );
    }

    #[test]
    fn test_child_is_ignored_on_non_stacks() {
        let tree = label("leaf").child(label("ignored")).build();
        assert_eq!(tree.nodes.len(), 1);
    }

    #[test]
    fn test_modifiers_apply_to_matching_elements() {
        let tree = v_stack()
            .child(button("save", "Save").disabled(true))
            .child(
                list_item("src", "src")
                    .indent(2)
                    .icon("folder")
                    .selected(true)
                    .expanded(Some(false)),
            )
            .build();

        assert!(matches!(&tree.nodes[1], wit_ui::Node::Button(button) if button.disabled));
        let wit_ui::Node::ListItem(item) = &tree.nodes[2] else {
            panic!("expected a list item");
        };
        assert_eq!(item.indent, 2);
        assert!(matches!(&item.icon, Some(wit_ui::Icon::Named(name)) if name == "folder"));
        assert!(item.selected);
        assert_eq!(item.expanded, Some(false));
    }

    #[test]
    fn test_list_item_decorations() {
        let tree = list_item("file", "main.rs")
            .icon(Icon::File("src/main.rs".into()))
            .description("src")
            .decoration("M", Color::Modified)
            .color(Color::Modified)
            .strikethrough(true)
            .action(action("stage", "plus").tooltip("Stage Changes"))
            .build();

        let wit_ui::Node::ListItem(item) = &tree.nodes[0] else {
            panic!("expected a list item");
        };
        assert!(matches!(&item.icon, Some(wit_ui::Icon::File(path)) if path == "src/main.rs"));
        assert_eq!(item.description.as_deref(), Some("src"));
        assert!(
            matches!(&item.decoration, Some(decoration) if decoration.text == "M" && decoration.color == Color::Modified)
        );
        assert_eq!(item.label_color, Color::Modified);
        assert!(item.strikethrough);
        assert_eq!(item.actions.len(), 1);
        assert_eq!(item.actions[0].tooltip.as_deref(), Some("Stage Changes"));
    }

    #[test]
    fn test_menus() {
        let tree = v_stack()
            .child(
                button("commit", "Commit").menu([
                    menu_item("commit-amend", "Commit (Amend)").icon("pencil"),
                    menu_separator(),
                    menu_header("Sign-off"),
                    menu_item("signoff", "Sign Off")
                        .checked(true)
                        .disabled(true),
                ]),
            )
            .child(icon_button("more", "ellipsis").menu([menu_item("pull", "Pull")]))
            .child(list_item("file", "main.rs").context_menu([menu_item("open", "Open File")]))
            .build();

        let wit_ui::Node::Button(commit) = &tree.nodes[1] else {
            panic!("expected a button");
        };
        assert_eq!(commit.menu.len(), 4);
        assert!(matches!(
            &commit.menu[0],
            wit_ui::MenuEntry::Item(item) if item.id == "commit-amend" && item.icon.as_deref() == Some("pencil")
        ));
        assert!(matches!(&commit.menu[1], wit_ui::MenuEntry::Separator));
        assert!(matches!(&commit.menu[2], wit_ui::MenuEntry::Header(text) if text == "Sign-off"));
        assert!(matches!(
            &commit.menu[3],
            wit_ui::MenuEntry::Item(item) if item.checked == Some(true) && item.disabled
        ));
        assert!(
            matches!(&tree.nodes[2], wit_ui::Node::IconButton(button) if button.menu.len() == 1)
        );
        assert!(
            matches!(&tree.nodes[3], wit_ui::Node::ListItem(item) if item.context_menu.len() == 1)
        );
    }

    #[test]
    fn test_button_and_label_styles() {
        let tree = v_stack()
            .child(
                button("commit", "Commit")
                    .icon("check")
                    .style(ButtonStyle::Filled)
                    .full_width(true),
            )
            .child(label("Changes").size(LabelSize::Small).bold(true))
            .child(
                h_stack()
                    .gap(Spacing::None)
                    .child(spacer())
                    .child(icon_button("refresh", "rotate_cw")),
            )
            .child(text_input("message", "Message", "").multi_line(true))
            .build();

        assert!(matches!(
            &tree.nodes[1],
            wit_ui::Node::Button(button)
                if button.icon.as_deref() == Some("check")
                    && button.style == ButtonStyle::Filled
                    && button.full_width
        ));
        assert!(matches!(
            &tree.nodes[2],
            wit_ui::Node::Label(label) if label.size == LabelSize::Small && label.bold
        ));
        assert!(
            matches!(&tree.nodes[3], wit_ui::Node::HStack(stack) if stack.gap == Spacing::None)
        );
        assert!(matches!(&tree.nodes[4], wit_ui::Node::Spacer));
        assert!(
            matches!(&tree.nodes[5], wit_ui::Node::IconButton(button) if button.id == "refresh")
        );
        assert!(matches!(&tree.nodes[6], wit_ui::Node::TextInput(input) if input.multi_line));
    }

    #[test]
    fn test_tree_converts_to_wit_tree() {
        let tree = v_stack().child(divider()).build();
        let wit_tree: wit_ui::UiTree = tree.into();
        assert_eq!(wit_tree.root, 0);
        assert_eq!(wit_tree.nodes.len(), 2);
    }
}
