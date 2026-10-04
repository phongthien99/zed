use extension::{
    UiButtonStyle, UiColor, UiEvent, UiIconButton, UiLabel, UiLabelSize, UiMenuEntry, UiNode,
    UiNodeId, UiSpacing, UiTree,
};
use gpui::{
    Anchor, AnyElement, App, Context, Div, Entity, FontWeight, SharedString, WeakEntity, Window,
};
use ui::{
    Checkbox, ContextMenu, ContextMenuEntry, Divider, IconButton, PopoverMenu, TintColor, Tooltip,
    prelude::*,
};
use util::ResultExt as _;

use crate::{ExtensionPanel, list_item::render_list_item, text_inputs::TextInputs};

/// Deeply nested trees are cut off when rendering to avoid exhausting the stack, since
/// `UiTree::validate` bounds the number of nodes but not the nesting depth.
pub(crate) const MAX_RENDER_DEPTH: usize = 64;

pub(crate) fn icon_for_name(name: &str) -> Option<IconName> {
    let icon = match name {
        "arrow_down" => IconName::ArrowDown,
        "arrow_up" => IconName::ArrowUp,
        "bell" => IconName::Bell,
        "blocks" => IconName::Blocks,
        "book" => IconName::Book,
        "box" => IconName::Box,
        "check" => IconName::Check,
        "check_double" => IconName::CheckDouble,
        "chevron_down" => IconName::ChevronDown,
        "chevron_right" => IconName::ChevronRight,
        "circle" => IconName::Circle,
        "close" => IconName::Close,
        "code" => IconName::Code,
        "copy" => IconName::Copy,
        "dash" => IconName::Dash,
        "diff" => IconName::Diff,
        "ellipsis" => IconName::Ellipsis,
        "file" => IconName::File,
        "file_diff" => IconName::FileDiff,
        "file_git" => IconName::FileGit,
        "file_tree" => IconName::FileTree,
        "folder" => IconName::Folder,
        "folder_open" => IconName::FolderOpen,
        "git_branch" => IconName::GitBranch,
        "git_commit" => IconName::GitCommit,
        "hash" => IconName::Hash,
        "info" => IconName::Info,
        "list_collapse" => IconName::ListCollapse,
        "list_todo" => IconName::ListTodo,
        "list_tree" => IconName::ListTree,
        "menu" => IconName::Menu,
        "pencil" => IconName::Pencil,
        "plus" => IconName::Plus,
        "rotate_ccw" => IconName::RotateCcw,
        "rotate_cw" => IconName::RotateCw,
        "search" => IconName::MagnifyingGlass,
        "settings" => IconName::Settings,
        "sparkle" => IconName::Sparkle,
        "split" => IconName::Split,
        "star" => IconName::Star,
        "terminal" => IconName::Terminal,
        "trash" => IconName::Trash,
        "undo" => IconName::Undo,
        "warning" => IconName::Warning,
        _ => return None,
    };
    Some(icon)
}

pub(crate) fn color(color: UiColor) -> Color {
    match color {
        UiColor::Default => Color::Default,
        UiColor::Muted => Color::Muted,
        UiColor::Accent => Color::Accent,
        UiColor::Success => Color::Success,
        UiColor::Warning => Color::Warning,
        UiColor::Error => Color::Error,
        UiColor::Created => Color::Created,
        UiColor::Modified => Color::Modified,
        UiColor::Deleted => Color::Deleted,
        UiColor::Conflict => Color::Conflict,
        UiColor::Ignored => Color::Ignored,
    }
}

pub(crate) fn label_size(size: UiLabelSize) -> LabelSize {
    match size {
        UiLabelSize::Default => LabelSize::Default,
        UiLabelSize::Small => LabelSize::Small,
        UiLabelSize::XSmall => LabelSize::XSmall,
    }
}

pub(crate) fn with_gap(stack: Div, gap: UiSpacing) -> Div {
    match gap {
        UiSpacing::None => stack,
        UiSpacing::Small => stack.gap_1(),
        UiSpacing::Medium => stack.gap_2(),
        UiSpacing::Large => stack.gap_4(),
    }
}

pub(crate) fn render_label(label: &UiLabel) -> Label {
    Label::new(label.text.clone())
        .color(color(label.color))
        .size(label_size(label.size))
        .when(label.bold, |this| this.weight(FontWeight::BOLD))
        .when(label.strikethrough, |this| this.strikethrough())
}

/// A row of the virtualized list that displays a panel's tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ListRow {
    pub(crate) node: UiNodeId,
    /// Whether the root stack's gap follows the row.
    pub(crate) gap_after: bool,
}

/// Splits a tree into rows for a virtualized list, so that panels with thousands of list
/// items only lay out the visible ones.
///
/// Each child of a root vertical stack is a row, except that a child stack consisting only
/// of list items contributes each item as a row of its own.
pub(crate) fn list_rows(tree: &UiTree) -> Vec<ListRow> {
    let Some(UiNode::VStack(root)) = tree.node(tree.root) else {
        return vec![ListRow {
            node: tree.root,
            gap_after: false,
        }];
    };

    let mut rows = Vec::new();
    for child in &root.children {
        match tree.node(*child) {
            Some(UiNode::VStack(stack))
                if stack.gap == UiSpacing::None
                    && !stack.children.is_empty()
                    && stack
                        .children
                        .iter()
                        .all(|item| matches!(tree.node(*item), Some(UiNode::ListItem(_)))) =>
            {
                rows.extend(stack.children.iter().map(|item| ListRow {
                    node: *item,
                    gap_after: false,
                }));
                if let Some(last) = rows.last_mut() {
                    last.gap_after = true;
                }
            }
            _ => rows.push(ListRow {
                node: *child,
                gap_after: true,
            }),
        }
    }
    rows
}

pub(crate) struct UiTreeRenderer<'a> {
    pub(crate) tree: &'a UiTree,
    pub(crate) inputs: &'a TextInputs,
}

impl UiTreeRenderer<'_> {
    pub(crate) fn render_node(
        &self,
        node_id: UiNodeId,
        depth: usize,
        cx: &Context<ExtensionPanel>,
    ) -> Option<AnyElement> {
        if depth > MAX_RENDER_DEPTH {
            return None;
        }

        let element = match self.tree.node(node_id)? {
            UiNode::VStack(stack) => with_gap(v_flex(), stack.gap)
                .children(
                    stack
                        .children
                        .iter()
                        .filter_map(|child| self.render_node(*child, depth + 1, cx)),
                )
                .into_any_element(),
            UiNode::HStack(stack) => with_gap(h_flex(), stack.gap)
                .children(
                    stack
                        .children
                        .iter()
                        .filter_map(|child| self.render_node(*child, depth + 1, cx)),
                )
                .into_any_element(),
            UiNode::Label(label) => render_label(label).into_any_element(),
            UiNode::Button(button) => {
                let id = button.id.clone();
                let style = match button.style {
                    UiButtonStyle::Default => ButtonStyle::Filled,
                    UiButtonStyle::Filled => ButtonStyle::Tinted(TintColor::Accent),
                    UiButtonStyle::Subtle => ButtonStyle::Subtle,
                };
                let icon = button.icon.as_deref().and_then(icon_for_name);
                let element = Button::new(
                    SharedString::from(format!("button-{id}")),
                    button.label.clone(),
                )
                .style(style)
                .disabled(button.disabled)
                .when_some(icon, |this, icon| {
                    this.start_icon(Icon::new(icon).size(IconSize::Small))
                })
                .when(button.full_width, |this| this.full_width())
                .when_some(button.tooltip.clone(), |this, tooltip| {
                    this.tooltip(Tooltip::text(tooltip))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.dispatch_event(UiEvent::Clicked(id.clone()), cx)
                }));
                if button.menu.is_empty() {
                    element.into_any_element()
                } else {
                    let entries = button.menu.clone();
                    let panel = cx.entity().downgrade();
                    h_flex()
                        .when(button.full_width, |this| this.w_full())
                        .gap_px()
                        .child(
                            div()
                                .when(button.full_width, |this| this.flex_1())
                                .child(element),
                        )
                        .child(
                            PopoverMenu::new(SharedString::from(format!(
                                "button-menu-{}",
                                button.id
                            )))
                            .trigger(
                                IconButton::new(
                                    SharedString::from(format!(
                                        "button-menu-trigger-{}",
                                        button.id
                                    )),
                                    IconName::ChevronDown,
                                )
                                .icon_size(IconSize::Small)
                                .style(style)
                                .disabled(button.disabled),
                            )
                            .anchor(Anchor::TopRight)
                            .menu(move |window, cx| {
                                Some(build_menu(&entries, panel.clone(), window, cx))
                            }),
                        )
                        .into_any_element()
                }
            }
            UiNode::IconButton(button) => render_icon_button(
                button,
                SharedString::from(format!("icon-button-{}", button.id)),
                cx,
            ),
            UiNode::TextInput(input) => {
                let id = SharedString::from(input.id.clone());
                let editor = self.inputs.editor(&id)?.clone();
                // Inputs fill the rest of a horizontal stack, such as next to a button.
                let container = div()
                    .flex_grow_1()
                    .min_w_0()
                    .px_1()
                    .border_1()
                    .border_color(cx.theme().colors().border)
                    .rounded_sm();
                if input.multi_line {
                    container
                        .key_context("ExtensionPanelTextArea")
                        .py_0p5()
                        .on_action(cx.listener(move |this, _: &menu::SecondaryConfirm, _, cx| {
                            this.submit_input(&id, cx)
                        }))
                        .child(editor)
                        .into_any_element()
                } else {
                    container
                        .on_action(cx.listener(move |this, _: &menu::Confirm, _, cx| {
                            this.submit_input(&id, cx)
                        }))
                        .child(editor)
                        .into_any_element()
                }
            }
            UiNode::Checkbox(checkbox) => {
                let id = checkbox.id.clone();
                Checkbox::new(
                    SharedString::from(format!("checkbox-{id}")),
                    checkbox.checked.into(),
                )
                .label(checkbox.label.clone())
                .on_click(cx.listener(move |this, state: &ToggleState, _, cx| {
                    this.dispatch_event(
                        UiEvent::CheckboxToggled {
                            id: id.clone(),
                            checked: state.selected(),
                        },
                        cx,
                    )
                }))
                .into_any_element()
            }
            UiNode::ListItem(item) => render_list_item(item, cx),
            UiNode::Divider => Divider::horizontal().into_any_element(),
            UiNode::Spacer => div().flex_1().into_any_element(),
        };
        Some(element)
    }
}

pub(crate) fn render_icon_button(
    button: &UiIconButton,
    element_id: SharedString,
    cx: &Context<ExtensionPanel>,
) -> AnyElement {
    let icon_button = IconButton::new(
        element_id.clone(),
        icon_for_name(&button.icon).unwrap_or(IconName::Blocks),
    )
    .icon_size(IconSize::Small)
    .disabled(button.disabled);

    if button.menu.is_empty() {
        let id = button.id.clone();
        icon_button
            .when_some(button.tooltip.clone(), |this, tooltip| {
                this.tooltip(Tooltip::text(tooltip))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.dispatch_event(UiEvent::Clicked(id.clone()), cx)
            }))
            .into_any_element()
    } else {
        let entries = button.menu.clone();
        let panel = cx.entity().downgrade();
        let menu = PopoverMenu::new(SharedString::from(format!("{element_id}-menu")))
            .anchor(Anchor::TopRight)
            .menu(move |window, cx| Some(build_menu(&entries, panel.clone(), window, cx)));
        match button.tooltip.clone() {
            Some(tooltip) => menu
                .trigger_with_tooltip(icon_button, Tooltip::text(tooltip))
                .into_any_element(),
            None => menu.trigger(icon_button).into_any_element(),
        }
    }
}

pub(crate) fn build_menu(
    entries: &[UiMenuEntry],
    panel: WeakEntity<ExtensionPanel>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<ContextMenu> {
    ContextMenu::build(window, cx, |mut menu, _, _| {
        for entry in entries {
            menu = match entry {
                UiMenuEntry::Separator => menu.separator(),
                UiMenuEntry::Header(text) => menu.header(text.clone()),
                UiMenuEntry::Item(item) => {
                    let id = item.id.clone();
                    let panel = panel.clone();
                    let mut menu_entry = ContextMenuEntry::new(item.label.clone())
                        .disabled(item.disabled)
                        .handler(move |_, cx| {
                            panel
                                .update(cx, |panel, cx| {
                                    panel.dispatch_event(UiEvent::Clicked(id.clone()), cx)
                                })
                                .log_err();
                        });
                    if let Some(icon) = item.icon.as_deref().and_then(icon_for_name) {
                        menu_entry = menu_entry.icon(icon);
                    }
                    if let Some(checked) = item.checked {
                        menu_entry = menu_entry.toggle(IconPosition::Start, checked);
                    }
                    menu.item(menu_entry)
                }
            };
        }
        menu
    })
}
