use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use extension::{
    Extension, PanelManifestEntry, PanelManifestPosition, UiButton, UiEvent, UiLabel, UiListItem,
    UiNode, UiSpacing, UiStack, UiTree,
};
use fs::FakeFs;
use gpui::{AppContext as _, Entity, TestAppContext, px};
use project::Project;
use settings::SettingsStore;
use workspace::Workspace;
use workspace::dock::{DockPosition, PanelHandle, PanelSizeState};

use crate::{ExtensionPanel, find_extension_panel, list_rows, toggle_extension_panel};

mod fake_extension;
mod git_tests;

use fake_extension::FakePanelExtension;

const RENDER_DELAY: Duration = Duration::from_millis(100);
const PANEL_ID: &str = "counter";
const INCREMENT_BUTTON_ID: &str = "increment";

fn counter_tree(clicks: usize) -> UiTree {
    UiTree {
        nodes: vec![
            UiNode::VStack(UiStack::new(vec![1, 2])),
            UiNode::Label(UiLabel {
                text: format!("clicks: {clicks}"),
                ..UiLabel::default()
            }),
            UiNode::Button(UiButton {
                id: INCREMENT_BUTTON_ID.to_string(),
                label: "Increment".to_string(),
                ..UiButton::default()
            }),
        ],
        root: 0,
        badge: None,
    }
}

fn create_panel(
    extension: Arc<FakePanelExtension>,
    cx: &mut TestAppContext,
) -> Entity<ExtensionPanel> {
    cx.new(|cx| {
        ExtensionPanel::new(
            extension,
            PANEL_ID.into(),
            PanelManifestEntry {
                title: "Counter".to_string(),
                icon: Some("file_tree".to_string()),
                default_position: PanelManifestPosition::Left,
            },
            cx,
        )
    })
}

fn finish_pending_render(cx: &mut TestAppContext) {
    cx.run_until_parked();
    cx.executor().advance_clock(RENDER_DELAY);
    cx.run_until_parked();
}

fn click_increment(panel: &Entity<ExtensionPanel>, cx: &mut TestAppContext) {
    panel.update(cx, |panel, cx| {
        panel.dispatch_event(UiEvent::Clicked(INCREMENT_BUTTON_ID.to_string()), cx)
    });
}

#[gpui::test]
async fn test_click_renders_new_tree(cx: &mut TestAppContext) {
    let extension = Arc::new(FakePanelExtension::new(cx.executor()));
    let panel = create_panel(extension.clone(), cx);

    panel.read_with(cx, |panel, _| assert!(panel.tree().is_none()));
    finish_pending_render(cx);
    panel.read_with(cx, |panel, _| {
        assert_eq!(panel.tree(), Some(&counter_tree(0)));
        assert!(panel.error().is_none());
    });

    click_increment(&panel, cx);
    finish_pending_render(cx);
    panel.read_with(cx, |panel, _| {
        assert_eq!(panel.tree(), Some(&counter_tree(1)));
        assert!(panel.error().is_none());
    });
    assert_eq!(extension.render_count.load(Ordering::SeqCst), 2);
    assert_eq!(extension.event_count.load(Ordering::SeqCst), 1);
}

#[test]
fn test_list_rows_flatten_lists() {
    let item = |id: &str| {
        UiNode::ListItem(UiListItem {
            id: id.to_string(),
            ..UiListItem::default()
        })
    };
    let tree = UiTree {
        nodes: vec![
            UiNode::VStack(UiStack::new(vec![1, 2, 5])),
            UiNode::Label(UiLabel::default()),
            UiNode::VStack(UiStack {
                children: vec![3, 4],
                gap: UiSpacing::None,
            }),
            item("a"),
            item("b"),
            // A stack with a gap is laid out as a whole, since rows have no gaps between them.
            UiNode::VStack(UiStack::new(vec![6])),
            item("c"),
        ],
        root: 0,
        badge: None,
    };
    let rows = list_rows(&tree)
        .into_iter()
        .map(|row| (row.node, row.gap_after))
        .collect::<Vec<_>>();
    assert_eq!(rows, vec![(1, true), (3, false), (4, true), (5, true)]);
}

#[gpui::test]
async fn test_dropped_panel_is_released(cx: &mut TestAppContext) {
    let extension = Arc::new(FakePanelExtension::new(cx.executor()));
    let panel = create_panel(extension.clone(), cx);
    finish_pending_render(cx);
    let instance = panel.read_with(cx, |panel, _| panel.instance());
    assert!(extension.released_instances.lock().is_empty());

    drop(panel);
    // Entities are released when effects are flushed at the end of an update.
    cx.update(|_| {});
    cx.run_until_parked();
    assert_eq!(*extension.released_instances.lock(), vec![instance]);
}

#[gpui::test]
async fn test_events_during_render_are_coalesced(cx: &mut TestAppContext) {
    let extension = Arc::new(FakePanelExtension::new(cx.executor()));
    let panel = create_panel(extension.clone(), cx);
    finish_pending_render(cx);
    assert_eq!(extension.render_count.load(Ordering::SeqCst), 1);

    click_increment(&panel, cx);
    cx.run_until_parked();
    click_increment(&panel, cx);
    click_increment(&panel, cx);
    cx.run_until_parked();
    assert_eq!(extension.event_count.load(Ordering::SeqCst), 1);

    finish_pending_render(cx);
    assert_eq!(extension.render_count.load(Ordering::SeqCst), 2);
    panel.read_with(cx, |panel, _| {
        assert_eq!(panel.tree(), Some(&counter_tree(1)));
    });

    finish_pending_render(cx);
    assert_eq!(extension.render_count.load(Ordering::SeqCst), 3);
    assert_eq!(extension.event_count.load(Ordering::SeqCst), 3);
    panel.read_with(cx, |panel, _| {
        assert_eq!(panel.tree(), Some(&counter_tree(3)));
    });

    finish_pending_render(cx);
    assert_eq!(extension.render_count.load(Ordering::SeqCst), 3);
}

#[gpui::test]
async fn test_panels_of_same_type_are_independent(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let settings_store = SettingsStore::test(cx);
        cx.set_global(settings_store);
        cx.set_global(db::AppDatabase::test_new());
        theme_settings::init(theme::LoadThemes::JustBase, cx);
    });

    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;
    let (workspace, cx) =
        cx.add_window_view(|window, cx| Workspace::test_new(project.clone(), window, cx));

    let extension = Arc::new(FakePanelExtension::new(cx.executor()));
    let extension_id = extension.manifest().id.clone();
    let [counter_panel, logs_panel] = ["counter", "logs"].map(|panel_id| {
        workspace.update_in(cx, |workspace, window, cx| {
            let panel = cx.new(|cx| {
                ExtensionPanel::new(
                    extension.clone(),
                    panel_id.into(),
                    PanelManifestEntry {
                        title: panel_id.to_string(),
                        icon: None,
                        default_position: PanelManifestPosition::Left,
                    },
                    cx,
                )
            });
            workspace.add_panel(panel.clone(), window, cx);
            panel
        })
    });
    cx.run_until_parked();

    workspace.read_with(cx, |workspace, cx| {
        assert_eq!(
            find_extension_panel(workspace, &extension_id, "counter", cx),
            Some(counter_panel.clone())
        );
        assert_eq!(
            find_extension_panel(workspace, &extension_id, "logs", cx),
            Some(logs_panel.clone())
        );
        assert_ne!(
            PanelHandle::persistent_name(&counter_panel, cx),
            PanelHandle::persistent_name(&logs_panel, cx)
        );
        assert_ne!(
            PanelHandle::panel_key(&counter_panel, cx),
            PanelHandle::panel_key(&logs_panel, cx)
        );
    });

    let active_panel_id = |workspace: &Entity<Workspace>, cx: &mut gpui::VisualTestContext| {
        workspace.read_with(cx, |workspace, cx| {
            let dock = workspace.dock_at_position(DockPosition::Left).read(cx);
            dock.is_open()
                .then(|| dock.active_panel().map(|panel| panel.panel_id()))
                .flatten()
        })
    };

    workspace.update_in(cx, |workspace, window, cx| {
        toggle_extension_panel(workspace, &extension_id, "counter", window, cx)
    });
    assert_eq!(
        active_panel_id(&workspace, cx),
        Some(counter_panel.entity_id())
    );

    workspace.update_in(cx, |workspace, window, cx| {
        toggle_extension_panel(workspace, &extension_id, "logs", window, cx)
    });
    assert_eq!(
        active_panel_id(&workspace, cx),
        Some(logs_panel.entity_id())
    );

    workspace.update_in(cx, |workspace, window, cx| {
        toggle_extension_panel(workspace, &extension_id, "logs", window, cx)
    });
    assert_eq!(active_panel_id(&workspace, cx), None);

    let counter_size = PanelSizeState {
        size: Some(px(420.)),
        flex: None,
    };
    let logs_size = PanelSizeState {
        size: Some(px(240.)),
        flex: None,
    };
    workspace.update_in(cx, |workspace, _window, cx| {
        workspace
            .dock_at_position(DockPosition::Left)
            .update(cx, |dock, cx| {
                dock.set_panel_size_state(&counter_panel, counter_size, cx);
                dock.set_panel_size_state(&logs_panel, logs_size, cx);
            });
    });
    workspace.read_with(cx, |workspace, cx| {
        let dock = workspace.dock_at_position(DockPosition::Left).read(cx);
        assert_eq!(
            dock.stored_panel_size_state(&counter_panel),
            Some(counter_size)
        );
        assert_eq!(dock.stored_panel_size_state(&logs_panel), Some(logs_size));
    });
}
