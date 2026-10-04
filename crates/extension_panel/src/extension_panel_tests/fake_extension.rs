use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use collections::BTreeMap;
use extension::{
    BuildTaskTemplate, CodeLabel, Command, Completion, ContextServerConfiguration,
    DebugAdapterBinary, DebugRequest, DebugScenario, DebugTaskDefinition, Extension,
    ExtensionManifest, KeyValueStoreDelegate, LibManifestEntry, PanelInstanceId, ProjectDelegate,
    SchemaVersion, SlashCommand, SlashCommandArgumentCompletion, SlashCommandOutput,
    StartDebuggingRequestArgumentsRequest, Symbol, UiEvent, UiTree, WorktreeDelegate,
};
use gpui::BackgroundExecutor;
use language::LanguageName;
use lsp::LanguageServerName;
use task::{SpawnInTerminal, ZedDebugConfig};

use super::{INCREMENT_BUTTON_ID, RENDER_DELAY, counter_tree};

pub(super) struct FakePanelExtension {
    pub(super) executor: BackgroundExecutor,
    pub(super) clicks: AtomicUsize,
    pub(super) render_count: AtomicUsize,
    pub(super) event_count: AtomicUsize,
    pub(super) events: parking_lot::Mutex<Vec<UiEvent>>,
    pub(super) released_instances: parking_lot::Mutex<Vec<PanelInstanceId>>,
}

impl FakePanelExtension {
    pub(super) fn new(executor: BackgroundExecutor) -> Self {
        Self {
            executor,
            clicks: AtomicUsize::new(0),
            render_count: AtomicUsize::new(0),
            event_count: AtomicUsize::new(0),
            events: parking_lot::Mutex::new(Vec::new()),
            released_instances: parking_lot::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl Extension for FakePanelExtension {
    fn manifest(&self) -> Arc<ExtensionManifest> {
        Arc::new(ExtensionManifest {
            id: "fake-panel-extension".into(),
            name: "Fake Panel Extension".to_string(),
            version: "1.0.0".into(),
            schema_version: SchemaVersion(1),
            description: None,
            repository: None,
            authors: Vec::new(),
            lib: LibManifestEntry::default(),
            themes: Vec::new(),
            icon_themes: Vec::new(),
            languages: Vec::new(),
            grammars: BTreeMap::default(),
            language_servers: BTreeMap::default(),
            context_servers: BTreeMap::default(),
            slash_commands: BTreeMap::default(),
            snippets: None,
            capabilities: Vec::new(),
            debug_adapters: BTreeMap::default(),
            debug_locators: BTreeMap::default(),
            language_model_providers: BTreeMap::default(),
            panels: BTreeMap::default(),
        })
    }

    fn work_dir(&self) -> Arc<Path> {
        Arc::from(Path::new("/fake-panel-extension-work-dir"))
    }

    async fn panel_render(
        &self,
        _panel_id: Arc<str>,
        _instance: PanelInstanceId,
    ) -> anyhow::Result<UiTree> {
        self.executor.timer(RENDER_DELAY).await;
        self.render_count.fetch_add(1, Ordering::SeqCst);
        Ok(counter_tree(self.clicks.load(Ordering::SeqCst)))
    }

    async fn panel_handle_event(
        &self,
        _panel_id: Arc<str>,
        _instance: PanelInstanceId,
        event: UiEvent,
    ) -> anyhow::Result<()> {
        self.event_count.fetch_add(1, Ordering::SeqCst);
        self.events.lock().push(event.clone());
        if event == UiEvent::Clicked(INCREMENT_BUTTON_ID.to_string()) {
            self.clicks.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    }

    async fn panel_release(
        &self,
        _panel_id: Arc<str>,
        instance: PanelInstanceId,
    ) -> anyhow::Result<()> {
        self.released_instances.lock().push(instance);
        Ok(())
    }

    async fn language_server_command(
        &self,
        _language_server_id: LanguageServerName,
        _language_name: LanguageName,
        _worktree: Arc<dyn WorktreeDelegate>,
    ) -> anyhow::Result<Command> {
        anyhow::bail!("not supported by FakePanelExtension")
    }

    async fn language_server_initialization_options(
        &self,
        _language_server_id: LanguageServerName,
        _language_name: LanguageName,
        _worktree: Arc<dyn WorktreeDelegate>,
    ) -> anyhow::Result<Option<String>> {
        anyhow::bail!("not supported by FakePanelExtension")
    }

    async fn language_server_workspace_configuration(
        &self,
        _language_server_id: LanguageServerName,
        _worktree: Arc<dyn WorktreeDelegate>,
    ) -> anyhow::Result<Option<String>> {
        anyhow::bail!("not supported by FakePanelExtension")
    }

    async fn language_server_initialization_options_schema(
        &self,
        _language_server_id: LanguageServerName,
        _worktree: Arc<dyn WorktreeDelegate>,
    ) -> anyhow::Result<Option<String>> {
        anyhow::bail!("not supported by FakePanelExtension")
    }

    async fn language_server_workspace_configuration_schema(
        &self,
        _language_server_id: LanguageServerName,
        _worktree: Arc<dyn WorktreeDelegate>,
    ) -> anyhow::Result<Option<String>> {
        anyhow::bail!("not supported by FakePanelExtension")
    }

    async fn language_server_additional_initialization_options(
        &self,
        _language_server_id: LanguageServerName,
        _target_language_server_id: LanguageServerName,
        _worktree: Arc<dyn WorktreeDelegate>,
    ) -> anyhow::Result<Option<String>> {
        anyhow::bail!("not supported by FakePanelExtension")
    }

    async fn language_server_additional_workspace_configuration(
        &self,
        _language_server_id: LanguageServerName,
        _target_language_server_id: LanguageServerName,
        _worktree: Arc<dyn WorktreeDelegate>,
    ) -> anyhow::Result<Option<String>> {
        anyhow::bail!("not supported by FakePanelExtension")
    }

    async fn labels_for_completions(
        &self,
        _language_server_id: LanguageServerName,
        _completions: Vec<Completion>,
    ) -> anyhow::Result<Vec<Option<CodeLabel>>> {
        anyhow::bail!("not supported by FakePanelExtension")
    }

    async fn labels_for_symbols(
        &self,
        _language_server_id: LanguageServerName,
        _symbols: Vec<Symbol>,
    ) -> anyhow::Result<Vec<Option<CodeLabel>>> {
        anyhow::bail!("not supported by FakePanelExtension")
    }

    async fn complete_slash_command_argument(
        &self,
        _command: SlashCommand,
        _arguments: Vec<String>,
    ) -> anyhow::Result<Vec<SlashCommandArgumentCompletion>> {
        anyhow::bail!("not supported by FakePanelExtension")
    }

    async fn run_slash_command(
        &self,
        _command: SlashCommand,
        _arguments: Vec<String>,
        _worktree: Option<Arc<dyn WorktreeDelegate>>,
    ) -> anyhow::Result<SlashCommandOutput> {
        anyhow::bail!("not supported by FakePanelExtension")
    }

    async fn context_server_command(
        &self,
        _context_server_id: Arc<str>,
        _project: Arc<dyn ProjectDelegate>,
    ) -> anyhow::Result<Command> {
        anyhow::bail!("not supported by FakePanelExtension")
    }

    async fn context_server_configuration(
        &self,
        _context_server_id: Arc<str>,
        _project: Arc<dyn ProjectDelegate>,
    ) -> anyhow::Result<Option<ContextServerConfiguration>> {
        anyhow::bail!("not supported by FakePanelExtension")
    }

    async fn suggest_docs_packages(&self, _provider: Arc<str>) -> anyhow::Result<Vec<String>> {
        anyhow::bail!("not supported by FakePanelExtension")
    }

    async fn index_docs(
        &self,
        _provider: Arc<str>,
        _package_name: Arc<str>,
        _kv_store: Arc<dyn KeyValueStoreDelegate>,
    ) -> anyhow::Result<()> {
        anyhow::bail!("not supported by FakePanelExtension")
    }

    async fn get_dap_binary(
        &self,
        _dap_name: Arc<str>,
        _config: DebugTaskDefinition,
        _user_installed_path: Option<PathBuf>,
        _worktree: Arc<dyn WorktreeDelegate>,
    ) -> anyhow::Result<DebugAdapterBinary> {
        anyhow::bail!("not supported by FakePanelExtension")
    }

    async fn dap_request_kind(
        &self,
        _dap_name: Arc<str>,
        _config: serde_json::Value,
    ) -> anyhow::Result<StartDebuggingRequestArgumentsRequest> {
        anyhow::bail!("not supported by FakePanelExtension")
    }

    async fn dap_config_to_scenario(
        &self,
        _config: ZedDebugConfig,
    ) -> anyhow::Result<DebugScenario> {
        anyhow::bail!("not supported by FakePanelExtension")
    }

    async fn dap_locator_create_scenario(
        &self,
        _locator_name: String,
        _build_config_template: BuildTaskTemplate,
        _resolved_label: String,
        _debug_adapter_name: String,
    ) -> anyhow::Result<Option<DebugScenario>> {
        anyhow::bail!("not supported by FakePanelExtension")
    }

    async fn run_dap_locator(
        &self,
        _locator_name: String,
        _config: SpawnInTerminal,
    ) -> anyhow::Result<DebugRequest> {
        anyhow::bail!("not supported by FakePanelExtension")
    }
}
