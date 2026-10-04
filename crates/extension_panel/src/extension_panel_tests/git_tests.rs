use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use agent_settings::AgentSettings;
use extension::git::{FileStatus, Operation, StatusCode, StatusEntry};
use extension::{
    Extension, ExtensionGitProxy, PanelInstanceId, PanelManifestEntry, PanelManifestPosition,
    UiEvent,
};
use fs::FakeFs;
use git::status::StatusCode as GitStatusCode;
use gpui::{AppContext as _, TestAppContext};
use language_model::LanguageModelRegistry;
use project::Project;
use serde_json::json;
use settings::{Settings as _, SettingsStore};
use util::path;
use workspace::Workspace;

use crate::ExtensionPanel;
use crate::git_proxy::ExtensionPanelGitProxy;

use super::{FakePanelExtension, PANEL_ID, RENDER_DELAY};

/// A workspace with a Git repository at `/project` and an extension panel shown in it,
/// after the panel's first render.
struct GitPanelSetup<'a> {
    extension: Arc<FakePanelExtension>,
    extension_id: Arc<str>,
    instance: PanelInstanceId,
    cx: &'a mut gpui::VisualTestContext,
}

async fn setup_git_panel(cx: &mut TestAppContext) -> GitPanelSetup<'_> {
    cx.update(|cx| {
        let settings_store = SettingsStore::test(cx);
        cx.set_global(settings_store);
        cx.set_global(db::AppDatabase::test_new());
        theme_settings::init(theme::LoadThemes::JustBase, cx);
    });

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/project"),
        json!({
            ".git": {},
            "modified.rs": "modified\n",
            "staged.rs": "staged\n",
        }),
    )
    .await;
    fs.set_status_for_repo(
        path!("/project/.git").as_ref(),
        &[
            ("modified.rs", GitStatusCode::Modified.worktree()),
            ("staged.rs", GitStatusCode::Modified.index()),
        ],
    );
    let project = Project::test(fs.clone(), [Path::new(path!("/project"))], cx).await;
    let (workspace, cx) =
        cx.add_window_view(|window, cx| Workspace::test_new(project.clone(), window, cx));
    cx.run_until_parked();

    let extension = Arc::new(FakePanelExtension::new(cx.executor()));
    let extension_id = extension.manifest().id.clone();
    let panel = workspace.update_in(cx, |workspace, window, cx| {
        let panel = cx.new(|cx| {
            ExtensionPanel::new(
                extension.clone(),
                PANEL_ID.into(),
                PanelManifestEntry {
                    title: "Source Control".to_string(),
                    icon: None,
                    default_position: PanelManifestPosition::Left,
                },
                cx,
            )
        });
        workspace.add_panel(panel.clone(), window, cx);
        panel
    });
    let instance = panel.read_with(cx, |panel, _| panel.instance());
    cx.run_until_parked();
    cx.executor().advance_clock(RENDER_DELAY);
    cx.run_until_parked();

    GitPanelSetup {
        extension,
        extension_id,
        instance,
        cx,
    }
}

#[gpui::test]
async fn test_generate_commit_message(cx: &mut TestAppContext) {
    let GitPanelSetup {
        extension,
        extension_id,
        instance,
        cx,
    } = setup_git_panel(cx).await;
    cx.update(|_, cx| {
        AgentSettings::register(cx);
        LanguageModelRegistry::test(cx);
    });
    let repository = cx
        .cx
        .update(|cx| ExtensionPanelGitProxy.repositories(extension_id.clone(), instance, cx))
        .await
        .unwrap()[0]
        .id;

    cx.cx
        .update(|cx| {
            ExtensionPanelGitProxy.generate_commit_message(
                extension_id.clone(),
                instance,
                repository,
                "message".into(),
                cx,
            )
        })
        .unwrap();
    cx.run_until_parked();

    // The panel stays interactive while the model writes the message.
    assert!(extension.events.lock().is_empty());
    let model = cx.update(|_, cx| LanguageModelRegistry::read_global(cx).fake_model());
    model
        .as_fake()
        .send_last_completion_stream_text_chunk("Update ");
    model
        .as_fake()
        .send_last_completion_stream_text_chunk("modified.rs\n");
    model.as_fake().end_last_completion_stream();
    cx.run_until_parked();

    assert_eq!(
        *extension.events.lock(),
        vec![UiEvent::TaskCompleted {
            id: "message".into(),
            result: Ok("Update modified.rs".into()),
        }]
    );
}

#[gpui::test]
async fn test_generate_commit_message_without_model(cx: &mut TestAppContext) {
    let GitPanelSetup {
        extension_id,
        instance,
        cx,
        ..
    } = setup_git_panel(cx).await;
    cx.update(|_, cx| {
        AgentSettings::register(cx);
        LanguageModelRegistry::test(cx);
        LanguageModelRegistry::global(cx)
            .update(cx, |registry, cx| registry.set_default_model(None, cx));
    });
    let repository = cx
        .cx
        .update(|cx| ExtensionPanelGitProxy.repositories(extension_id.clone(), instance, cx))
        .await
        .unwrap()[0]
        .id;

    let error = cx
        .cx
        .update(|cx| {
            ExtensionPanelGitProxy.generate_commit_message(
                extension_id.clone(),
                instance,
                repository,
                "message".into(),
                cx,
            )
        })
        .unwrap_err();
    assert!(error.to_string().contains("no model"), "{error}");
}

#[gpui::test]
async fn test_git_proxy(cx: &mut TestAppContext) {
    let GitPanelSetup {
        extension,
        extension_id,
        instance,
        cx,
    } = setup_git_panel(cx).await;

    let repositories = cx
        .cx
        .update(|cx| ExtensionPanelGitProxy.repositories(extension_id.clone(), instance, cx))
        .await
        .unwrap();
    assert_eq!(repositories.len(), 1);
    assert_eq!(repositories[0].work_directory, path!("/project"));
    assert!(repositories[0].active);
    let repository = repositories[0].id;

    let status = |cx: &mut gpui::VisualTestContext| {
        cx.cx.update(|cx| {
            ExtensionPanelGitProxy.status(extension_id.clone(), instance, repository, cx)
        })
    };
    assert_eq!(
        status(cx).await.unwrap(),
        vec![
            StatusEntry {
                path: "modified.rs".into(),
                status: FileStatus::Tracked {
                    index: StatusCode::Unmodified,
                    worktree: StatusCode::Modified,
                },
            },
            StatusEntry {
                path: "staged.rs".into(),
                status: FileStatus::Tracked {
                    index: StatusCode::Modified,
                    worktree: StatusCode::Unmodified,
                },
            },
        ]
    );

    // Panel instances are only reachable by the extension that owns them.
    assert!(
        cx.cx
            .update(|cx| ExtensionPanelGitProxy.repositories(
                "other-extension".into(),
                instance,
                cx
            ))
            .await
            .is_err()
    );

    // Paths must stay within the repository.
    for path in ["../outside.rs", path!("/project/modified.rs")] {
        assert!(
            cx.cx
                .update(|cx| {
                    ExtensionPanelGitProxy.operation(
                        extension_id.clone(),
                        instance,
                        repository,
                        Operation::Stage(vec![path.to_string()]),
                        cx,
                    )
                })
                .await
                .is_err(),
            "staging {path} should fail"
        );
    }

    let render_count = extension.render_count.load(Ordering::SeqCst);
    cx.cx
        .update(|cx| {
            ExtensionPanelGitProxy.operation(
                extension_id.clone(),
                instance,
                repository,
                Operation::Stage(vec!["modified.rs".into()]),
                cx,
            )
        })
        .await
        .unwrap();
    cx.run_until_parked();
    cx.executor().advance_clock(RENDER_DELAY);
    cx.run_until_parked();

    assert_eq!(
        status(cx).await.unwrap().first(),
        Some(&StatusEntry {
            path: "modified.rs".into(),
            status: FileStatus::Tracked {
                index: StatusCode::Modified,
                worktree: StatusCode::Unmodified,
            },
        })
    );
    // The panel used the Git API, so it re-renders when the repository changes.
    assert!(extension.render_count.load(Ordering::SeqCst) > render_count);

    let commit_with_empty_message = cx.cx.update(|cx| {
        ExtensionPanelGitProxy.operation(
            extension_id.clone(),
            instance,
            repository,
            Operation::Commit {
                message: "  ".into(),
                options: Default::default(),
            },
            cx,
        )
    });
    assert!(commit_with_empty_message.await.is_err());
}
