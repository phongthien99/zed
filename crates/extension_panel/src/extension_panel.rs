use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use anyhow::Result;
use extension::{
    Extension, ExtensionHostProxy, PanelInstanceId, PanelManifestEntry, PanelManifestPosition,
    UiEvent, UiNode, UiSpacing, UiTree,
};
use gpui::{
    Action, AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, ListAlignment,
    ListState, SharedString, Subscription, Task, Window, list,
};
use project::git_store::{GitStore, GitStoreEvent, RepositoryEvent};
use schemars::JsonSchema;
use serde::Deserialize;
use settings::SettingsStore;
use ui::prelude::*;
use workspace::Workspace;
use workspace::dock::{DockPosition, Panel, PanelEvent};

#[cfg(test)]
mod extension_panel_tests;
mod git_proxy;
mod list_item;
mod registry;
mod text_inputs;
mod ui_tree;

pub use registry::find_extension_panel;
use registry::{
    ExtensionPanelRegistry, ExtensionPanelRegistryProxy, add_registered_panel, extension_panels,
    toggle_extension_panel, workspaces_by_window,
};
use text_inputs::TextInputs;
use ui_tree::{ListRow, UiTreeRenderer, icon_for_name, list_rows};

/// Panels share a dock with built-in panels, which require unique activation priorities,
/// so extension panels are numbered from a range that built-in panels don't use.
static NEXT_ACTIVATION_PRIORITY: AtomicU32 = AtomicU32::new(1000);

static NEXT_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);

/// Toggles a panel provided by an extension.
#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = extension_panel)]
#[serde(deny_unknown_fields)]
pub struct ToggleExtensionPanel {
    pub extension_id: String,
    pub panel_id: String,
}

pub fn init(cx: &mut App) {
    cx.set_global(ExtensionPanelRegistry::default());
    let proxy = ExtensionHostProxy::default_global(cx);
    proxy.register_panel_proxy(ExtensionPanelRegistryProxy);
    proxy.register_git_proxy(git_proxy::ExtensionPanelGitProxy);

    cx.observe_new(|workspace: &mut Workspace, window, cx| {
        let Some(window) = window else {
            return;
        };

        workspace.register_action(|workspace, action: &ToggleExtensionPanel, window, cx| {
            toggle_extension_panel(
                workspace,
                &action.extension_id,
                &action.panel_id,
                window,
                cx,
            );
        });

        let registered_panels = cx
            .try_global::<ExtensionPanelRegistry>()
            .map(|registry| registry.panels.clone())
            .unwrap_or_default();
        for registered_panel in registered_panels {
            add_registered_panel(workspace, registered_panel, window, cx);
        }
    })
    .detach();
}

pub(crate) fn persistent_name(extension_id: &str, panel_id: &str) -> SharedString {
    format!("extension:{extension_id}:{panel_id}").into()
}

pub struct ExtensionPanel {
    extension: Arc<dyn Extension>,
    extension_id: Arc<str>,
    panel_id: Arc<str>,
    instance: PanelInstanceId,
    persistent_name: SharedString,
    manifest_entry: PanelManifestEntry,
    icon: IconName,
    position: DockPosition,
    activation_priority: u32,
    focus_handle: FocusHandle,
    tree: Option<UiTree>,
    /// The rows of `tree` displayed by `list_state`; see [`list_rows`].
    rows: Vec<ListRow>,
    list_state: ListState,
    error: Option<SharedString>,
    inputs: TextInputs,
    pending_events: Vec<UiEvent>,
    render_task: Option<Task<()>>,
    dirty: bool,
    git_store_subscription: Option<Subscription>,
}

impl ExtensionPanel {
    pub fn new(
        extension: Arc<dyn Extension>,
        panel_id: Arc<str>,
        manifest_entry: PanelManifestEntry,
        cx: &mut Context<Self>,
    ) -> Self {
        let extension_id = extension.manifest().id.clone();
        let icon = manifest_entry
            .icon
            .as_deref()
            .and_then(icon_for_name)
            .unwrap_or(IconName::Blocks);
        let position = match manifest_entry.default_position {
            PanelManifestPosition::Left => DockPosition::Left,
            PanelManifestPosition::Right => DockPosition::Right,
            PanelManifestPosition::Bottom => DockPosition::Bottom,
        };

        let mut this = Self {
            persistent_name: persistent_name(&extension_id, &panel_id),
            extension,
            extension_id,
            panel_id,
            instance: NEXT_INSTANCE_ID.fetch_add(1, Ordering::Relaxed),
            manifest_entry,
            icon,
            position,
            activation_priority: NEXT_ACTIVATION_PRIORITY.fetch_add(1, Ordering::Relaxed),
            focus_handle: cx.focus_handle(),
            tree: None,
            rows: Vec::new(),
            list_state: ListState::new(0, ListAlignment::Top, px(1000.)),
            error: None,
            inputs: TextInputs::default(),
            pending_events: Vec::new(),
            render_task: None,
            dirty: false,
            git_store_subscription: None,
        };
        cx.on_release({
            let extension = this.extension.clone();
            let panel_id = this.panel_id.clone();
            let instance = this.instance;
            move |_, cx| {
                cx.background_spawn(
                    async move { extension.panel_release(panel_id, instance).await },
                )
                .detach_and_log_err(cx);
            }
        })
        .detach();
        this.schedule_render(cx);
        this
    }

    pub fn extension_id(&self) -> &Arc<str> {
        &self.extension_id
    }

    pub fn panel_id(&self) -> &Arc<str> {
        &self.panel_id
    }

    pub fn instance(&self) -> PanelInstanceId {
        self.instance
    }

    pub fn tree(&self) -> Option<&UiTree> {
        self.tree.as_ref()
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    pub fn dispatch_event(&mut self, event: UiEvent, cx: &mut Context<Self>) {
        self.pending_events.push(event);
        self.schedule_render(cx);
    }

    pub fn schedule_render(&mut self, cx: &mut Context<Self>) {
        if self.render_task.is_some() {
            self.dirty = true;
            return;
        }

        let events = std::mem::take(&mut self.pending_events);
        let extension = self.extension.clone();
        let panel_id = self.panel_id.clone();
        let instance = self.instance;
        self.render_task = Some(cx.spawn(async move |this, cx| {
            let result = handle_events_and_render(extension, panel_id, instance, events).await;
            this.update(cx, |this, cx| {
                this.render_task = None;
                this.apply_render_result(result, cx);
                if std::mem::take(&mut this.dirty) {
                    this.schedule_render(cx);
                }
            })
            .ok();
        }));
    }

    fn apply_render_result(&mut self, result: Result<UiTree>, cx: &mut Context<Self>) {
        match result {
            Ok(tree) => {
                self.rows = list_rows(&tree);
                // Splicing rather than resetting keeps the scroll position.
                self.list_state
                    .splice(0..self.list_state.item_count(), self.rows.len());
                self.tree = Some(tree);
                self.error = None;
                // A tree rendered before the latest edits reached the extension would
                // revert text the user has typed since.
                self.inputs
                    .tree_changed(!self.dirty && self.pending_events.is_empty());
            }
            Err(error) => {
                log::error!(
                    "extension panel `{}` failed to render: {error:#}",
                    self.persistent_name
                );
                self.error = Some(format!("{error:#}").into());
            }
        }
        cx.notify();
    }

    /// Re-renders the panel whenever the Git state it may display changes. Only panels
    /// that use the Git API observe it, so other panels don't re-render on every change.
    fn observe_git_store(&mut self, git_store: &Entity<GitStore>, cx: &mut Context<Self>) {
        if self.git_store_subscription.is_some() {
            return;
        }
        self.git_store_subscription = Some(cx.subscribe(
            git_store,
            |this, _, event: &GitStoreEvent, cx| {
                let affects_panel = match event {
                    GitStoreEvent::ActiveRepositoryChanged(_)
                    | GitStoreEvent::RepositoryAdded
                    | GitStoreEvent::RepositoryRemoved(_) => true,
                    GitStoreEvent::RepositoryUpdated(_, event, _) => matches!(
                        event,
                        RepositoryEvent::StatusesChanged
                            | RepositoryEvent::HeadChanged
                            | RepositoryEvent::BranchListChanged
                    ),
                    _ => false,
                };
                if affects_panel {
                    this.schedule_render(cx);
                }
            },
        ));
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        self.schedule_render(cx);
        cx.notify();
    }

    fn input_edited(&mut self, id: &SharedString, cx: &mut Context<Self>) {
        let Some(value) = self.inputs.take_unreported_value(id, cx) else {
            return;
        };
        self.dispatch_event(
            UiEvent::InputChanged {
                id: id.to_string(),
                value,
            },
            cx,
        );
    }

    fn submit_input(&mut self, id: &SharedString, cx: &mut Context<Self>) {
        let Some(value) = self.inputs.value(id, cx) else {
            return;
        };
        self.dispatch_event(
            UiEvent::InputSubmitted {
                id: id.to_string(),
                value,
            },
            cx,
        );
    }
}

async fn handle_events_and_render(
    extension: Arc<dyn Extension>,
    panel_id: Arc<str>,
    instance: PanelInstanceId,
    events: Vec<UiEvent>,
) -> Result<UiTree> {
    for event in events {
        extension
            .panel_handle_event(panel_id.clone(), instance, event)
            .await?;
    }
    extension.panel_render(panel_id, instance).await
}

impl EventEmitter<PanelEvent> for ExtensionPanel {}

impl Focusable for ExtensionPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ExtensionPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(tree) = self.tree.as_ref() {
            self.inputs.sync(tree, window, cx);
        }

        let content = if let Some(error) = self.error.clone() {
            v_flex()
                .gap_2()
                .child(Label::new(error).color(Color::Error))
                .child(
                    Button::new("retry", "Retry")
                        .on_click(cx.listener(|this, _, _, cx| this.retry(cx))),
                )
                .into_any_element()
        } else if self.tree.is_some() {
            list(
                self.list_state.clone(),
                cx.processor(|this, index: usize, _window, cx| this.render_row(index, cx)),
            )
            .size_full()
            .into_any_element()
        } else {
            Label::new("Loading…")
                .color(Color::Muted)
                .into_any_element()
        };

        v_flex()
            .id("extension-panel")
            .key_context("ExtensionPanel")
            .track_focus(&self.focus_handle)
            .size_full()
            .p_2()
            .child(content)
    }
}

impl ExtensionPanel {
    fn render_row(&mut self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let (Some(tree), Some(row)) = (self.tree.as_ref(), self.rows.get(index).copied()) else {
            return div().into_any_element();
        };
        let root_gap = match tree.node(tree.root) {
            Some(UiNode::VStack(root)) => root.gap,
            _ => UiSpacing::None,
        };
        let element = UiTreeRenderer {
            tree,
            inputs: &self.inputs,
        }
        .render_node(row.node, 1, cx)
        .unwrap_or_else(|| div().into_any_element());
        let row_container = div().w_full().child(element);
        if row.gap_after {
            match root_gap {
                UiSpacing::None => row_container,
                UiSpacing::Small => row_container.pb_1(),
                UiSpacing::Medium => row_container.pb_2(),
                UiSpacing::Large => row_container.pb_4(),
            }
            .into_any_element()
        } else {
            row_container.into_any_element()
        }
    }
}

impl Panel for ExtensionPanel {
    fn persistent_name() -> &'static str {
        "ExtensionPanel"
    }

    fn panel_key() -> &'static str {
        "ExtensionPanel"
    }

    fn instance_persistent_name(&self) -> SharedString {
        self.persistent_name.clone()
    }

    fn instance_panel_key(&self) -> SharedString {
        self.persistent_name.clone()
    }

    fn position(&self, _window: &Window, _cx: &App) -> DockPosition {
        self.position
    }

    fn position_is_valid(&self, _position: DockPosition) -> bool {
        true
    }

    fn set_position(
        &mut self,
        position: DockPosition,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.position = position;
        // Docks move panels in response to settings changes, so notify settings observers
        // even though the position is stored on the panel rather than in settings.
        cx.update_global::<SettingsStore, _>(|_, _| {});
    }

    fn default_size(&self, _window: &Window, _cx: &App) -> Pixels {
        px(300.)
    }

    fn icon(&self, _window: &Window, _cx: &App) -> Option<IconName> {
        Some(self.icon)
    }

    fn icon_label(&self, _window: &Window, _cx: &App) -> Option<String> {
        // The dock only shows labels that parse as a count.
        Some(self.tree.as_ref()?.badge?.to_string())
    }

    fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
        None
    }

    fn instance_icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<SharedString> {
        Some(self.manifest_entry.title.clone().into())
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleExtensionPanel {
            extension_id: self.extension_id.to_string(),
            panel_id: self.panel_id.to_string(),
        })
    }

    fn activation_priority(&self) -> u32 {
        self.activation_priority
    }
}
