use crate::wasm_host::wit::since_v0_6_0::slash_command::SlashCommandOutputSection;
use crate::wasm_host::wit::{CompletionKind, CompletionLabelDetails, InsertTextFormat, SymbolKind};
use crate::wasm_host::{
    WasmState,
    wit::{IntoWasmtimeResult, ToWasmtimeResult},
};
use ::http_client::{AsyncBody, HttpRequestExt};
use ::settings::{Settings, WorktreeId};
use anyhow::{Context as _, Result, bail};
use async_compression::futures::bufread::GzipDecoder;
use async_tar::Archive;
use async_trait::async_trait;
use extension::{
    ExtensionGitProxy, ExtensionHostProxy, ExtensionLanguageServerProxy, ExtensionPanelProxy,
    KeyValueStoreDelegate, ProjectDelegate, WorktreeDelegate,
};
use futures::{AsyncReadExt, lock::Mutex};
use futures::{FutureExt as _, io::BufReader};
use gpui::{BackgroundExecutor, SharedString};
use language::{BinaryStatus, LanguageName, language_settings::AllLanguageSettings};
use project::project_settings::ProjectSettings;
use semver::Version;
use std::{
    env,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, OnceLock},
};
use task::{SpawnInTerminal, ZedDebugConfig};
use url::Url;
use util::{
    archive::extract_zip, fs::make_file_executable, maybe, paths::PathStyle, rel_path::RelPath,
};
use wasmtime::component::{Linker, Resource};

use super::*;

impl From<ui::UiTree> for extension::UiTree {
    fn from(value: ui::UiTree) -> Self {
        Self {
            nodes: value.nodes.into_iter().map(Into::into).collect(),
            root: value.root,
            badge: value.badge,
        }
    }
}

impl From<ui::Stack> for extension::UiStack {
    fn from(value: ui::Stack) -> Self {
        Self {
            children: value.children,
            gap: match value.gap {
                ui::Spacing::None => extension::UiSpacing::None,
                ui::Spacing::Small => extension::UiSpacing::Small,
                ui::Spacing::Medium => extension::UiSpacing::Medium,
                ui::Spacing::Large => extension::UiSpacing::Large,
            },
        }
    }
}

impl From<ui::Color> for extension::UiColor {
    fn from(value: ui::Color) -> Self {
        match value {
            ui::Color::Default => Self::Default,
            ui::Color::Muted => Self::Muted,
            ui::Color::Accent => Self::Accent,
            ui::Color::Success => Self::Success,
            ui::Color::Warning => Self::Warning,
            ui::Color::Error => Self::Error,
            ui::Color::Created => Self::Created,
            ui::Color::Modified => Self::Modified,
            ui::Color::Deleted => Self::Deleted,
            ui::Color::Conflict => Self::Conflict,
            ui::Color::Ignored => Self::Ignored,
        }
    }
}

impl From<ui::Icon> for extension::UiIcon {
    fn from(value: ui::Icon) -> Self {
        match value {
            ui::Icon::Named(name) => Self::Named(name),
            ui::Icon::File(path) => Self::File(path),
            ui::Icon::Folder(path) => Self::Folder(path),
        }
    }
}

impl From<ui::IconButton> for extension::UiIconButton {
    fn from(value: ui::IconButton) -> Self {
        Self {
            id: value.id,
            icon: value.icon,
            tooltip: value.tooltip,
            disabled: value.disabled,
            menu: value.menu.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<ui::MenuEntry> for extension::UiMenuEntry {
    fn from(value: ui::MenuEntry) -> Self {
        match value {
            ui::MenuEntry::Item(item) => Self::Item(extension::UiMenuItem {
                id: item.id,
                label: item.label,
                icon: item.icon,
                disabled: item.disabled,
                checked: item.checked,
            }),
            ui::MenuEntry::Separator => Self::Separator,
            ui::MenuEntry::Header(text) => Self::Header(text),
        }
    }
}

impl From<ui::Node> for extension::UiNode {
    fn from(value: ui::Node) -> Self {
        match value {
            ui::Node::VStack(stack) => Self::VStack(stack.into()),
            ui::Node::HStack(stack) => Self::HStack(stack.into()),
            ui::Node::Label(label) => Self::Label(extension::UiLabel {
                text: label.text,
                color: label.color.into(),
                size: match label.size {
                    ui::LabelSize::Default => extension::UiLabelSize::Default,
                    ui::LabelSize::Small => extension::UiLabelSize::Small,
                    ui::LabelSize::XSmall => extension::UiLabelSize::XSmall,
                },
                bold: label.bold,
                strikethrough: label.strikethrough,
            }),
            ui::Node::Button(button) => Self::Button(extension::UiButton {
                id: button.id,
                label: button.label,
                disabled: button.disabled,
                icon: button.icon,
                style: match button.style {
                    ui::ButtonStyle::Default => extension::UiButtonStyle::Default,
                    ui::ButtonStyle::Filled => extension::UiButtonStyle::Filled,
                    ui::ButtonStyle::Subtle => extension::UiButtonStyle::Subtle,
                },
                full_width: button.full_width,
                tooltip: button.tooltip,
                menu: button.menu.into_iter().map(Into::into).collect(),
            }),
            ui::Node::IconButton(button) => Self::IconButton(button.into()),
            ui::Node::TextInput(input) => Self::TextInput(extension::UiTextInput {
                id: input.id,
                placeholder: input.placeholder,
                value: input.value,
                multi_line: input.multi_line,
            }),
            ui::Node::Checkbox(checkbox) => Self::Checkbox(extension::UiCheckbox {
                id: checkbox.id,
                label: checkbox.label,
                checked: checkbox.checked,
            }),
            ui::Node::ListItem(item) => Self::ListItem(extension::UiListItem {
                id: item.id,
                label: item.label,
                indent: item.indent,
                icon: item.icon.map(Into::into),
                selected: item.selected,
                expanded: item.expanded,
                description: item.description,
                label_color: item.label_color.into(),
                strikethrough: item.strikethrough,
                decoration: item.decoration.map(|decoration| extension::UiDecoration {
                    text: decoration.text,
                    color: decoration.color.into(),
                }),
                actions: item.actions.into_iter().map(Into::into).collect(),
                tooltip: item.tooltip,
                context_menu: item.context_menu.into_iter().map(Into::into).collect(),
            }),
            ui::Node::Divider => Self::Divider,
            ui::Node::Spacer => Self::Spacer,
        }
    }
}

impl From<extension::UiEvent> for ui::UiEvent {
    fn from(value: extension::UiEvent) -> Self {
        match value {
            extension::UiEvent::Clicked(id) => Self::Clicked(id),
            extension::UiEvent::InputChanged { id, value } => {
                Self::InputChanged(ui::InputEvent { id, value })
            }
            extension::UiEvent::InputSubmitted { id, value } => {
                Self::InputSubmitted(ui::InputEvent { id, value })
            }
            extension::UiEvent::CheckboxToggled { id, checked } => {
                Self::CheckboxToggled(ui::CheckboxEvent { id, checked })
            }
            extension::UiEvent::ListItemToggled(id) => Self::ListItemToggled(id),
            extension::UiEvent::TaskCompleted { id, result } => {
                Self::TaskCompleted(ui::TaskResult { id, output: result })
            }
        }
    }
}

impl ui::Host for WasmState {
    async fn request_render(&mut self, panel_id: String) -> wasmtime::Result<()> {
        let extension_id = self.manifest.id.clone();
        let proxy = self.host.proxy.clone();
        self.on_main_thread(move |cx| {
            async move {
                cx.update(|cx| {
                    proxy.request_panel_render(extension_id, panel_id.into(), cx);
                });
            }
            .boxed_local()
        })
        .await;
        Ok(())
    }
}
