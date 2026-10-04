use std::sync::Arc;

use extension::{Extension, ExtensionPanelProxy, PanelManifestEntry};
use gpui::{AnyWindowHandle, App, Context, Entity, Global, Window};
use ui::prelude::*;
use util::ResultExt as _;
use workspace::{MultiWorkspace, Workspace};

use crate::{ExtensionPanel, persistent_name};

#[derive(Clone)]
pub(crate) struct RegisteredPanel {
    extension: Arc<dyn Extension>,
    extension_id: Arc<str>,
    panel_id: Arc<str>,
    entry: PanelManifestEntry,
}

#[derive(Default)]
pub(crate) struct ExtensionPanelRegistry {
    pub(crate) panels: Vec<RegisteredPanel>,
}

impl Global for ExtensionPanelRegistry {}

/// The registry itself lives in a GPUI global, which the `Send + Sync` proxy accesses
/// through the `App` it is handed.
pub(crate) struct ExtensionPanelRegistryProxy;

impl ExtensionPanelProxy for ExtensionPanelRegistryProxy {
    fn register_panel(
        &self,
        extension: Arc<dyn Extension>,
        panel_id: Arc<str>,
        entry: PanelManifestEntry,
        cx: &mut App,
    ) {
        let registered_panel = RegisteredPanel {
            extension_id: extension.manifest().id.clone(),
            extension,
            panel_id,
            entry,
        };

        let registry = cx.default_global::<ExtensionPanelRegistry>();
        registry.panels.retain(|panel| {
            panel.extension_id != registered_panel.extension_id
                || panel.panel_id != registered_panel.panel_id
        });
        registry.panels.push(registered_panel.clone());

        update_all_workspaces(cx, |workspace, window, cx| {
            remove_extension_panels(
                workspace,
                &registered_panel.extension_id,
                Some(registered_panel.panel_id.as_ref()),
                window,
                cx,
            );
            add_registered_panel(workspace, registered_panel.clone(), window, cx);
        });
    }

    fn unregister_panels(&self, extension_id: Arc<str>, cx: &mut App) {
        cx.default_global::<ExtensionPanelRegistry>()
            .panels
            .retain(|panel| panel.extension_id != extension_id);

        update_all_workspaces(cx, |workspace, window, cx| {
            remove_extension_panels(workspace, &extension_id, None, window, cx);
        });
    }

    fn request_panel_render(&self, extension_id: Arc<str>, panel_id: Arc<str>, cx: &mut App) {
        let panels = workspaces_by_window(cx)
            .into_iter()
            .flat_map(|(_, workspaces)| workspaces)
            .filter_map(|workspace| {
                find_extension_panel(workspace.read(cx), &extension_id, &panel_id, cx)
            })
            .collect::<Vec<_>>();
        for panel in panels {
            panel.update(cx, |panel, cx| panel.schedule_render(cx));
        }
    }
}

pub(crate) fn workspaces_by_window(cx: &App) -> Vec<(AnyWindowHandle, Vec<Entity<Workspace>>)> {
    cx.windows()
        .into_iter()
        .filter_map(|window| {
            if let Some(multi_workspace) = window.downcast::<MultiWorkspace>() {
                let workspaces = multi_workspace
                    .read(cx)
                    .log_err()?
                    .workspaces()
                    .cloned()
                    .collect();
                Some((window, workspaces))
            } else if let Some(workspace) = window.downcast::<Workspace>() {
                Some((window, vec![workspace.entity(cx).log_err()?]))
            } else {
                None
            }
        })
        .collect()
}

pub(crate) fn update_all_workspaces(
    cx: &mut App,
    mut update: impl FnMut(&mut Workspace, &mut Window, &mut Context<Workspace>),
) {
    for (window_handle, workspaces) in workspaces_by_window(cx) {
        window_handle
            .update(cx, |_, window, cx| {
                for workspace in workspaces {
                    workspace.update(cx, |workspace, cx| update(workspace, window, cx));
                }
            })
            .log_err();
    }
}

pub(crate) fn add_registered_panel(
    workspace: &mut Workspace,
    registered_panel: RegisteredPanel,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let panel = cx.new(|cx| {
        ExtensionPanel::new(
            registered_panel.extension,
            registered_panel.panel_id,
            registered_panel.entry,
            cx,
        )
    });
    workspace.add_panel(panel, window, cx);
}

pub(crate) fn remove_extension_panels(
    workspace: &mut Workspace,
    extension_id: &str,
    panel_id: Option<&str>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let panels = extension_panels(workspace, cx)
        .into_iter()
        .filter(|panel| {
            let panel = panel.read(cx);
            panel.extension_id.as_ref() == extension_id
                && panel_id.is_none_or(|panel_id| panel.panel_id.as_ref() == panel_id)
        })
        .collect::<Vec<_>>();
    for panel in panels {
        workspace.remove_panel(&panel, window, cx);
    }
}

pub(crate) fn extension_panels(workspace: &Workspace, cx: &App) -> Vec<Entity<ExtensionPanel>> {
    workspace
        .all_docks()
        .into_iter()
        .flat_map(|dock| {
            dock.read(cx)
                .panels()
                .filter_map(|panel| panel.to_any().downcast::<ExtensionPanel>().ok())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// `Workspace::panel` only returns the first panel of a given type, but every extension
/// panel shares the `ExtensionPanel` type.
pub fn find_extension_panel(
    workspace: &Workspace,
    extension_id: &str,
    panel_id: &str,
    cx: &App,
) -> Option<Entity<ExtensionPanel>> {
    extension_panels(workspace, cx).into_iter().find(|panel| {
        let panel = panel.read(cx);
        panel.extension_id.as_ref() == extension_id && panel.panel_id.as_ref() == panel_id
    })
}

pub(crate) fn toggle_extension_panel(
    workspace: &mut Workspace,
    extension_id: &str,
    panel_id: &str,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let persistent_name = persistent_name(extension_id, panel_id);
    let docks = workspace
        .all_docks()
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    for dock in docks {
        let Some(panel_index) = dock
            .read(cx)
            .panel_index_for_persistent_name(&persistent_name, cx)
        else {
            continue;
        };

        let closed_focused_panel = dock.update(cx, |dock, cx| {
            let is_visible = dock.is_open() && dock.active_panel_index() == Some(panel_index);
            if is_visible {
                let was_focused = dock
                    .active_panel()
                    .is_some_and(|panel| panel.panel_focus_handle(cx).contains_focused(window, cx));
                dock.set_open(false, window, cx);
                was_focused
            } else {
                dock.activate_panel(panel_index, window, cx);
                dock.set_open(true, window, cx);
                if let Some(panel) = dock.active_panel() {
                    panel.activation_focus_handle(cx).focus(window, cx);
                }
                false
            }
        });

        if closed_focused_panel {
            workspace.focus_center_pane(window, cx);
        }
        cx.notify();
        return;
    }
}
