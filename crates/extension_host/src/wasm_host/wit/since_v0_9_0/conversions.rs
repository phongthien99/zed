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

impl From<Range> for std::ops::Range<usize> {
    fn from(range: Range) -> Self {
        let start = range.start as usize;
        let end = range.end as usize;
        start..end
    }
}

impl From<Command> for extension::Command {
    fn from(value: Command) -> Self {
        Self {
            command: value.command.into(),
            args: value.args,
            env: value.env,
        }
    }
}

impl From<StartDebuggingRequestArgumentsRequest>
    for extension::StartDebuggingRequestArgumentsRequest
{
    fn from(value: StartDebuggingRequestArgumentsRequest) -> Self {
        match value {
            StartDebuggingRequestArgumentsRequest::Launch => Self::Launch,
            StartDebuggingRequestArgumentsRequest::Attach => Self::Attach,
        }
    }
}
impl TryFrom<dap::StartDebuggingRequestArguments> for extension::StartDebuggingRequestArguments {
    type Error = anyhow::Error;

    fn try_from(value: dap::StartDebuggingRequestArguments) -> Result<Self, Self::Error> {
        Ok(Self {
            configuration: serde_json::from_str(&value.configuration)?,
            request: value.request.into(),
        })
    }
}
impl From<dap::IpAddress> for IpAddr {
    fn from(value: dap::IpAddress) -> Self {
        match value {
            dap::IpAddress::Ipv4((a, b, c, d)) => IpAddr::V4(Ipv4Addr::new(a, b, c, d)),
            dap::IpAddress::Ipv6((a, b, c, d, e, f, g, h)) => {
                IpAddr::V6(Ipv6Addr::new(a, b, c, d, e, f, g, h))
            }
        }
    }
}

impl From<IpAddr> for dap::IpAddress {
    fn from(value: IpAddr) -> Self {
        match value {
            IpAddr::V4(v4) => {
                let [a, b, c, d] = v4.octets();
                Self::Ipv4((a, b, c, d))
            }
            IpAddr::V6(v6) => {
                let [a, b, c, d, e, f, g, h] = v6.segments();
                Self::Ipv6((a, b, c, d, e, f, g, h))
            }
        }
    }
}

impl From<dap::TcpArguments> for extension::TcpArguments {
    fn from(value: dap::TcpArguments) -> Self {
        Self {
            host: value.host.into(),
            port: value.port,
            timeout: value.timeout,
        }
    }
}

impl From<extension::TcpArgumentsTemplate> for dap::TcpArgumentsTemplate {
    fn from(value: extension::TcpArgumentsTemplate) -> Self {
        Self {
            host: value.host.map(Into::into),
            port: value.port,
            timeout: value.timeout,
        }
    }
}

impl From<dap::TcpArgumentsTemplate> for extension::TcpArgumentsTemplate {
    fn from(value: dap::TcpArgumentsTemplate) -> Self {
        Self {
            host: value.host.map(Into::into),
            port: value.port,
            timeout: value.timeout,
        }
    }
}

impl TryFrom<extension::DebugTaskDefinition> for DebugTaskDefinition {
    type Error = anyhow::Error;
    fn try_from(value: extension::DebugTaskDefinition) -> Result<Self, Self::Error> {
        Ok(Self {
            label: value.label.to_string(),
            adapter: value.adapter.to_string(),
            config: value.config.to_string(),
            tcp_connection: value.tcp_connection.map(Into::into),
        })
    }
}

impl From<task::DebugRequest> for DebugRequest {
    fn from(value: task::DebugRequest) -> Self {
        match value {
            task::DebugRequest::Launch(launch_request) => Self::Launch(launch_request.into()),
            task::DebugRequest::Attach(attach_request) => Self::Attach(attach_request.into()),
        }
    }
}

impl From<DebugRequest> for task::DebugRequest {
    fn from(value: DebugRequest) -> Self {
        match value {
            DebugRequest::Launch(launch_request) => Self::Launch(launch_request.into()),
            DebugRequest::Attach(attach_request) => Self::Attach(attach_request.into()),
        }
    }
}

impl From<task::LaunchRequest> for LaunchRequest {
    fn from(value: task::LaunchRequest) -> Self {
        Self {
            program: value.program,
            cwd: value.cwd.map(|p| p.to_string_lossy().into_owned()),
            args: value.args,
            envs: value.env.into_iter().collect(),
        }
    }
}

impl From<task::AttachRequest> for AttachRequest {
    fn from(value: task::AttachRequest) -> Self {
        Self {
            process_id: value.process_id,
        }
    }
}

impl From<LaunchRequest> for task::LaunchRequest {
    fn from(value: LaunchRequest) -> Self {
        Self {
            program: value.program,
            cwd: value.cwd.map(|p| p.into()),
            args: value.args,
            env: value.envs.into_iter().collect(),
        }
    }
}
impl From<AttachRequest> for task::AttachRequest {
    fn from(value: AttachRequest) -> Self {
        Self {
            process_id: value.process_id,
        }
    }
}

impl From<ZedDebugConfig> for DebugConfig {
    fn from(value: ZedDebugConfig) -> Self {
        Self {
            label: value.label.into(),
            adapter: value.adapter.into(),
            request: value.request.into(),
            stop_on_entry: value.stop_on_entry,
        }
    }
}
impl TryFrom<DebugAdapterBinary> for extension::DebugAdapterBinary {
    type Error = anyhow::Error;
    fn try_from(value: DebugAdapterBinary) -> Result<Self, Self::Error> {
        Ok(Self {
            command: value.command,
            arguments: value.arguments,
            envs: value.envs.into_iter().collect(),
            cwd: value.cwd.map(|s| s.into()),
            connection: value.connection.map(Into::into),
            request_args: value.request_args.try_into()?,
        })
    }
}

impl From<dap::BuildTaskDefinition> for extension::BuildTaskDefinition {
    fn from(value: dap::BuildTaskDefinition) -> Self {
        match value {
            dap::BuildTaskDefinition::ByName(name) => Self::ByName(name.into()),
            dap::BuildTaskDefinition::Template(build_task_template) => Self::Template {
                task_template: build_task_template.template.into(),
                locator_name: build_task_template.locator_name.map(SharedString::from),
            },
        }
    }
}

impl From<extension::BuildTaskDefinition> for dap::BuildTaskDefinition {
    fn from(value: extension::BuildTaskDefinition) -> Self {
        match value {
            extension::BuildTaskDefinition::ByName(name) => Self::ByName(name.into()),
            extension::BuildTaskDefinition::Template {
                task_template,
                locator_name,
            } => Self::Template(dap::BuildTaskDefinitionTemplatePayload {
                template: task_template.into(),
                locator_name: locator_name.map(String::from),
            }),
        }
    }
}
impl From<BuildTaskTemplate> for extension::BuildTaskTemplate {
    fn from(value: BuildTaskTemplate) -> Self {
        Self {
            label: value.label,
            command: value.command,
            args: value.args,
            env: value.env.into_iter().collect(),
            cwd: value.cwd,
            ..Default::default()
        }
    }
}
impl From<extension::BuildTaskTemplate> for BuildTaskTemplate {
    fn from(value: extension::BuildTaskTemplate) -> Self {
        Self {
            label: value.label,
            command: value.command,
            args: value.args,
            env: value.env.into_iter().collect(),
            cwd: value.cwd,
        }
    }
}

impl TryFrom<DebugScenario> for extension::DebugScenario {
    type Error = anyhow::Error;

    fn try_from(value: DebugScenario) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            adapter: value.adapter.into(),
            label: value.label.into(),
            build: value.build.map(Into::into),
            config: serde_json::Value::from_str(&value.config)?,
            tcp_connection: value.tcp_connection.map(Into::into),
        })
    }
}

impl From<extension::DebugScenario> for DebugScenario {
    fn from(value: extension::DebugScenario) -> Self {
        Self {
            adapter: value.adapter.into(),
            label: value.label.into(),
            build: value.build.map(Into::into),
            config: value.config.to_string(),
            tcp_connection: value.tcp_connection.map(Into::into),
        }
    }
}

impl TryFrom<SpawnInTerminal> for ResolvedTask {
    type Error = anyhow::Error;

    fn try_from(value: SpawnInTerminal) -> Result<Self, Self::Error> {
        Ok(Self {
            label: value.label,
            command: value.command.context("missing command")?,
            args: value.args,
            env: value.env.into_iter().collect(),
            cwd: value.cwd.map(|s| {
                let s = s.to_string_lossy();
                if cfg!(target_os = "windows") {
                    s.replace('\\', "/")
                } else {
                    s.into_owned()
                }
            }),
        })
    }
}

impl From<CodeLabel> for extension::CodeLabel {
    fn from(value: CodeLabel) -> Self {
        Self {
            code: value.code,
            spans: value.spans.into_iter().map(Into::into).collect(),
            filter_range: value.filter_range.into(),
        }
    }
}

impl From<CodeLabelSpan> for extension::CodeLabelSpan {
    fn from(value: CodeLabelSpan) -> Self {
        match value {
            CodeLabelSpan::CodeRange(range) => Self::CodeRange(range.into()),
            CodeLabelSpan::Literal(literal) => Self::Literal(literal.into()),
        }
    }
}

impl From<CodeLabelSpanLiteral> for extension::CodeLabelSpanLiteral {
    fn from(value: CodeLabelSpanLiteral) -> Self {
        Self {
            text: value.text,
            highlight_name: value.highlight_name,
        }
    }
}

impl From<extension::Completion> for Completion {
    fn from(value: extension::Completion) -> Self {
        Self {
            label: value.label,
            label_details: value.label_details.map(Into::into),
            detail: value.detail,
            kind: value.kind.map(Into::into),
            insert_text_format: value.insert_text_format.map(Into::into),
        }
    }
}

impl From<extension::CompletionLabelDetails> for CompletionLabelDetails {
    fn from(value: extension::CompletionLabelDetails) -> Self {
        Self {
            detail: value.detail,
            description: value.description,
        }
    }
}

impl From<extension::CompletionKind> for CompletionKind {
    fn from(value: extension::CompletionKind) -> Self {
        match value {
            extension::CompletionKind::Text => Self::Text,
            extension::CompletionKind::Method => Self::Method,
            extension::CompletionKind::Function => Self::Function,
            extension::CompletionKind::Constructor => Self::Constructor,
            extension::CompletionKind::Field => Self::Field,
            extension::CompletionKind::Variable => Self::Variable,
            extension::CompletionKind::Class => Self::Class,
            extension::CompletionKind::Interface => Self::Interface,
            extension::CompletionKind::Module => Self::Module,
            extension::CompletionKind::Property => Self::Property,
            extension::CompletionKind::Unit => Self::Unit,
            extension::CompletionKind::Value => Self::Value,
            extension::CompletionKind::Enum => Self::Enum,
            extension::CompletionKind::Keyword => Self::Keyword,
            extension::CompletionKind::Snippet => Self::Snippet,
            extension::CompletionKind::Color => Self::Color,
            extension::CompletionKind::File => Self::File,
            extension::CompletionKind::Reference => Self::Reference,
            extension::CompletionKind::Folder => Self::Folder,
            extension::CompletionKind::EnumMember => Self::EnumMember,
            extension::CompletionKind::Constant => Self::Constant,
            extension::CompletionKind::Struct => Self::Struct,
            extension::CompletionKind::Event => Self::Event,
            extension::CompletionKind::Operator => Self::Operator,
            extension::CompletionKind::TypeParameter => Self::TypeParameter,
            extension::CompletionKind::Other(value) => Self::Other(value),
        }
    }
}

impl From<extension::InsertTextFormat> for InsertTextFormat {
    fn from(value: extension::InsertTextFormat) -> Self {
        match value {
            extension::InsertTextFormat::PlainText => Self::PlainText,
            extension::InsertTextFormat::Snippet => Self::Snippet,
            extension::InsertTextFormat::Other(value) => Self::Other(value),
        }
    }
}

impl From<extension::Symbol> for Symbol {
    fn from(value: extension::Symbol) -> Self {
        Self {
            kind: value.kind.into(),
            name: value.name,
            container_name: value.container_name,
        }
    }
}

impl From<extension::SymbolKind> for SymbolKind {
    fn from(value: extension::SymbolKind) -> Self {
        match value {
            extension::SymbolKind::File => Self::File,
            extension::SymbolKind::Module => Self::Module,
            extension::SymbolKind::Namespace => Self::Namespace,
            extension::SymbolKind::Package => Self::Package,
            extension::SymbolKind::Class => Self::Class,
            extension::SymbolKind::Method => Self::Method,
            extension::SymbolKind::Property => Self::Property,
            extension::SymbolKind::Field => Self::Field,
            extension::SymbolKind::Constructor => Self::Constructor,
            extension::SymbolKind::Enum => Self::Enum,
            extension::SymbolKind::Interface => Self::Interface,
            extension::SymbolKind::Function => Self::Function,
            extension::SymbolKind::Variable => Self::Variable,
            extension::SymbolKind::Constant => Self::Constant,
            extension::SymbolKind::String => Self::String,
            extension::SymbolKind::Number => Self::Number,
            extension::SymbolKind::Boolean => Self::Boolean,
            extension::SymbolKind::Array => Self::Array,
            extension::SymbolKind::Object => Self::Object,
            extension::SymbolKind::Key => Self::Key,
            extension::SymbolKind::Null => Self::Null,
            extension::SymbolKind::EnumMember => Self::EnumMember,
            extension::SymbolKind::Struct => Self::Struct,
            extension::SymbolKind::Event => Self::Event,
            extension::SymbolKind::Operator => Self::Operator,
            extension::SymbolKind::TypeParameter => Self::TypeParameter,
            extension::SymbolKind::Other(value) => Self::Other(value),
        }
    }
}

impl From<extension::SlashCommand> for SlashCommand {
    fn from(value: extension::SlashCommand) -> Self {
        Self {
            name: value.name,
            description: value.description,
            tooltip_text: value.tooltip_text,
            requires_argument: value.requires_argument,
        }
    }
}

impl From<SlashCommandOutput> for extension::SlashCommandOutput {
    fn from(value: SlashCommandOutput) -> Self {
        Self {
            text: value.text,
            sections: value.sections.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<SlashCommandOutputSection> for extension::SlashCommandOutputSection {
    fn from(value: SlashCommandOutputSection) -> Self {
        Self {
            range: value.range.start as usize..value.range.end as usize,
            label: value.label,
        }
    }
}

impl From<SlashCommandArgumentCompletion> for extension::SlashCommandArgumentCompletion {
    fn from(value: SlashCommandArgumentCompletion) -> Self {
        Self {
            label: value.label,
            new_text: value.new_text,
            run_command: value.run_command,
        }
    }
}

impl TryFrom<ContextServerConfiguration> for extension::ContextServerConfiguration {
    type Error = anyhow::Error;

    fn try_from(value: ContextServerConfiguration) -> Result<Self, Self::Error> {
        let settings_schema: serde_json::Value = serde_json::from_str(&value.settings_schema)
            .context("Failed to parse settings_schema")?;

        Ok(Self {
            installation_instructions: value.installation_instructions,
            default_settings: value.default_settings,
            settings_schema,
        })
    }
}
