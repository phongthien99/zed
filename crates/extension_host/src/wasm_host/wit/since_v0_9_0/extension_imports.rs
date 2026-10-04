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

impl ExtensionImports for WasmState {
    async fn get_settings(
        &mut self,
        location: Option<self::SettingsLocation>,
        category: String,
        key: Option<String>,
    ) -> wasmtime::Result<Result<String, String>> {
        self.on_main_thread(|cx| {
            async move {
                let path = location.as_ref().and_then(|location| {
                    RelPath::new(Path::new(&location.path), PathStyle::Unix).ok()
                });
                let location = path
                    .as_ref()
                    .zip(location.as_ref())
                    .map(|(path, location)| ::settings::SettingsLocation {
                        worktree_id: WorktreeId::from_proto(location.worktree_id),
                        path,
                    });

                cx.update(|cx| match category.as_str() {
                    "language" => {
                        let key = key.map(|k| LanguageName::new(&k));
                        let settings = AllLanguageSettings::get(location, cx).language(
                            location,
                            key.as_ref(),
                            cx,
                        );
                        Ok(serde_json::to_string(&settings::LanguageSettings {
                            tab_size: settings.tab_size,
                            hard_tabs: settings.hard_tabs,
                            preferred_line_length: settings.preferred_line_length,
                        })?)
                    }
                    "lsp" => {
                        let settings = key
                            .and_then(|key| {
                                ProjectSettings::get(location, cx)
                                    .lsp
                                    .get(&::lsp::LanguageServerName::from_proto(key))
                            })
                            .cloned()
                            .unwrap_or_default();
                        Ok(serde_json::to_string(&settings::LspSettings {
                            binary: settings.binary.map(|binary| settings::CommandSettings {
                                path: binary.path,
                                arguments: binary.arguments,
                                env: binary.env.map(|env| env.into_iter().collect()),
                            }),
                            settings: settings.settings,
                            initialization_options: settings.initialization_options,
                        })?)
                    }
                    "context_servers" => {
                        let settings = key
                            .and_then(|key| {
                                ProjectSettings::get(location, cx)
                                    .context_servers
                                    .get(key.as_str())
                            })
                            .cloned()
                            .unwrap_or_else(|| {
                                project::project_settings::ContextServerSettings::default_extension(
                                )
                            });

                        match settings {
                            project::project_settings::ContextServerSettings::Stdio {
                                enabled: _,
                                command,
                                ..
                            } => Ok(serde_json::to_string(&settings::ContextServerSettings {
                                command: Some(settings::CommandSettings {
                                    path: command.path.to_str().map(|path| path.to_string()),
                                    arguments: Some(command.args),
                                    env: command.env.map(|env| env.into_iter().collect()),
                                }),
                                settings: None,
                            })?),
                            project::project_settings::ContextServerSettings::Extension {
                                enabled: _,
                                settings,
                                ..
                            } => Ok(serde_json::to_string(&settings::ContextServerSettings {
                                command: None,
                                settings: Some(settings),
                            })?),
                            project::project_settings::ContextServerSettings::Http { .. } => {
                                bail!("remote context server settings not supported in 0.6.0")
                            }
                        }
                    }
                    _ => {
                        bail!("Unknown settings category: {}", category);
                    }
                })
            }
            .boxed_local()
        })
        .await
        .to_wasmtime_result()
    }

    async fn set_language_server_installation_status(
        &mut self,
        server_name: String,
        status: LanguageServerInstallationStatus,
    ) -> wasmtime::Result<()> {
        let status = match status {
            LanguageServerInstallationStatus::CheckingForUpdate => BinaryStatus::CheckingForUpdate,
            LanguageServerInstallationStatus::Downloading => BinaryStatus::Downloading,
            LanguageServerInstallationStatus::None => BinaryStatus::None,
            LanguageServerInstallationStatus::Failed(error) => BinaryStatus::Failed { error },
        };

        self.host
            .proxy
            .update_language_server_status(::lsp::LanguageServerName(server_name.into()), status);

        Ok(())
    }

    async fn download_file(
        &mut self,
        url: String,
        path: String,
        file_type: DownloadedFileType,
    ) -> wasmtime::Result<Result<(), String>> {
        maybe!(async {
            let parsed_url = Url::parse(&url)?;
            self.capability_granter.grant_download_file(&parsed_url)?;

            let path = PathBuf::from(path);
            let extension_work_dir = self.host.work_dir.join(self.manifest.id.as_ref());

            self.host.fs.create_dir(&extension_work_dir).await?;

            let destination_path = self
                .host
                .writeable_path_from_extension(&self.manifest.id, &path)
                .await?;

            let mut response = self
                .host
                .http_client
                .get(&url, Default::default(), true)
                .await
                .context("downloading release")?;

            anyhow::ensure!(
                response.status().is_success(),
                "download failed with status {}",
                response.status()
            );
            let mut body = BufReader::new(response.body_mut());

            match file_type {
                DownloadedFileType::Uncompressed => {
                    futures::pin_mut!(body);
                    self.host
                        .fs
                        .create_file_with(&destination_path, body)
                        .await?;
                }
                DownloadedFileType::Gzip => {
                    let body = GzipDecoder::new(body);
                    futures::pin_mut!(body);
                    self.host
                        .fs
                        .create_file_with(&destination_path, body)
                        .await?;
                }
                DownloadedFileType::GzipTar => {
                    let mut tar_gz_bytes = Vec::new();
                    body.read_to_end(&mut tar_gz_bytes).await?;
                    let decompressed_bytes =
                        GzipDecoder::new(BufReader::new(tar_gz_bytes.as_slice()));
                    futures::pin_mut!(decompressed_bytes);
                    self.host
                        .fs
                        .extract_tar_file(&destination_path, Archive::new(decompressed_bytes))
                        .await?;
                }
                DownloadedFileType::Zip => {
                    futures::pin_mut!(body);
                    extract_zip(&destination_path, body)
                        .await
                        .with_context(|| format!("unzipping {path:?} archive"))?;
                }
            }

            Ok(())
        })
        .await
        .to_wasmtime_result()
    }

    async fn make_file_executable(&mut self, path: String) -> wasmtime::Result<Result<(), String>> {
        let path = self
            .host
            .writeable_path_from_extension(&self.manifest.id, Path::new(&path))
            .await
            .into_wasmtime_result()?;

        make_file_executable(&path)
            .await
            .with_context(|| format!("setting permissions for path {path:?}"))
            .to_wasmtime_result()
    }
}
