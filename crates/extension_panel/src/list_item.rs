use std::path::Path;

use extension::{UiEvent, UiIcon, UiListItem};
use file_icons::FileIcons;
use gpui::{AnyElement, App, Context, SharedString};
use ui::{ListItem, Tooltip, prelude::*, right_click_menu};

use crate::{
    ExtensionPanel,
    ui_tree::{build_menu, color, icon_for_name, render_icon_button},
};

pub(crate) const LIST_ITEM_INDENT_STEP: Pixels = px(12.);
pub(crate) const LIST_ITEM_GROUP: &str = "extension-panel-list-item";

pub(crate) fn list_item_icon(item: &UiListItem, cx: &App) -> Option<Icon> {
    let icon = match item.icon.as_ref()? {
        UiIcon::Named(name) => Icon::new(icon_for_name(name)?),
        UiIcon::File(path) => FileIcons::get_icon(Path::new(path), cx)
            .map(Icon::from_path)
            .unwrap_or_else(|| Icon::new(IconName::File)),
        UiIcon::Folder(path) => {
            let expanded = item.expanded.unwrap_or(false);
            FileIcons::get_folder_icon(expanded, Path::new(path), cx)
                .map(Icon::from_path)
                .unwrap_or_else(|| {
                    Icon::new(if expanded {
                        IconName::FolderOpen
                    } else {
                        IconName::Folder
                    })
                })
        }
    };
    Some(icon.size(IconSize::Small).color(Color::Muted))
}

pub(crate) fn render_list_item(item: &UiListItem, cx: &Context<ExtensionPanel>) -> AnyElement {
    let click_id = item.id.clone();
    let toggle_id = item.id.clone();
    let decoration = || {
        item.decoration.as_ref().map(|decoration| {
            Label::new(decoration.text.clone())
                .size(LabelSize::Small)
                .color(color(decoration.color))
        })
    };

    let content = h_flex()
        .min_w_0()
        .gap_1p5()
        .child(
            Label::new(item.label.clone())
                .color(color(item.label_color))
                .when(item.strikethrough, |this| this.strikethrough())
                .truncate(),
        )
        .when_some(item.description.clone(), |this, description| {
            this.child(
                Label::new(description)
                    .size(LabelSize::Small)
                    .color(Color::Muted)
                    .truncate(),
            )
        });

    let list_item = ListItem::new(SharedString::from(format!("list-item-{}", item.id)))
        .indent_level(item.indent as usize)
        .indent_step_size(LIST_ITEM_INDENT_STEP)
        .toggle_state(item.selected)
        .toggle(item.expanded)
        .always_show_disclosure_icon(item.expanded.is_some())
        .when_some(list_item_icon(item, cx), |this, icon| this.start_slot(icon))
        .when_some(item.tooltip.clone(), |this, tooltip| {
            this.tooltip(Tooltip::text(tooltip))
        })
        .child(content)
        .end_slot::<Label>(decoration())
        .on_click(cx.listener(move |this, _, _, cx| {
            this.dispatch_event(UiEvent::Clicked(click_id.clone()), cx)
        }))
        .when(item.expanded.is_some(), |this| {
            this.on_toggle(cx.listener(move |this, _, _, cx| {
                this.dispatch_event(UiEvent::ListItemToggled(toggle_id.clone()), cx)
            }))
        });

    // Actions are drawn over the end of the item rather than in its end slot, so that
    // they don't take space from the label while hidden, like in VS Code.
    let row = div()
        .relative()
        .group(LIST_ITEM_GROUP)
        .child(list_item)
        .when(!item.actions.is_empty(), |this| {
            let colors = cx.theme().colors();
            this.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .visible_on_hover(LIST_ITEM_GROUP)
                    .bg(colors.panel_background)
                    .child(
                        h_flex()
                            .h_full()
                            .gap_0p5()
                            .px(DynamicSpacing::Base06.rems(cx))
                            .bg(colors.ghost_element_hover)
                            .children(item.actions.iter().map(|action| {
                                render_icon_button(
                                    action,
                                    SharedString::from(format!(
                                        "list-item-{}-action-{}",
                                        item.id, action.id
                                    )),
                                    cx,
                                )
                            }))
                            .children(decoration()),
                    ),
            )
        });

    if item.context_menu.is_empty() {
        return row.into_any_element();
    }
    let entries = item.context_menu.clone();
    let panel = cx.entity().downgrade();
    right_click_menu(SharedString::from(format!("list-item-menu-{}", item.id)))
        .trigger(move |_, _, _| row)
        .menu(move |window, cx| build_menu(&entries, panel.clone(), window, cx))
        .into_any_element()
}
