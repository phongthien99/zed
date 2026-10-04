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

pub const MIN_VERSION: Version = Version::new(0, 9, 0);
pub const MAX_VERSION: Version = Version::new(0, 9, 0);

wasmtime::component::bindgen!({
    imports: {
        default: async | trappable,
    },
    exports: {
        default: async,
    },
    path: "../extension_api/wit/since_v0.9.0",
    with: {
         "worktree": ExtensionWorktree,
         "project": ExtensionProject,
         "key-value-store": ExtensionKeyValueStore,
         "zed:extension/http-client.http-response-stream": ExtensionHttpResponseStream
    },
});

pub use self::zed::extension::*;

mod conversions;
mod extension_imports;
mod git_host;
mod host;
mod panel_ui;

mod settings {
    #![allow(dead_code)]
    include!(concat!(env!("OUT_DIR"), "/since_v0.9.0/settings.rs"));
}

pub type ExtensionWorktree = Arc<dyn WorktreeDelegate>;
pub type ExtensionProject = Arc<dyn ProjectDelegate>;
pub type ExtensionKeyValueStore = Arc<dyn KeyValueStoreDelegate>;
pub type ExtensionHttpResponseStream = Arc<Mutex<::http_client::Response<AsyncBody>>>;

pub fn linker(executor: &BackgroundExecutor) -> &'static Linker<WasmState> {
    static LINKER: OnceLock<Linker<WasmState>> = OnceLock::new();
    LINKER.get_or_init(|| {
        super::new_linker(executor, |linker| {
            Extension::add_to_linker::<_, WasmState>(linker, |s| s)
        })
    })
}

