use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context as _, anyhow};
use futures::{AsyncWriteExt as _, FutureExt, StreamExt, future::BoxFuture, stream::BoxStream};
use gpui::{AnyView, App, AsyncApp, Context, Entity, SharedString, Task, Window};
use language_model::{
    AuthenticateError, IconOrSvg, InlineDescription, InlineProviderSettings, LanguageModel,
    LanguageModelCompletionError, LanguageModelCompletionEvent, LanguageModelId, LanguageModelName,
    LanguageModelProvider, LanguageModelProviderId, LanguageModelProviderName,
    LanguageModelProviderState, LanguageModelRequest, LanguageModelToolChoice,
    ProviderSettingsView, Role, StopReason,
};
use ui::prelude::*;
use util::command::{Stdio, new_command};

const PROVIDER_ID: LanguageModelProviderId = LanguageModelProviderId::new("claude_code_cli");
const PROVIDER_NAME: LanguageModelProviderName =
    LanguageModelProviderName::new("Claude Code CLI (Pro/Max)");

const DESCRIPTION: &str = "Runs the `claude` CLI in print mode, so requests use the Claude Pro/Max \
    subscription you signed in to with Claude Code. Install Claude Code and run `claude` once to sign in.";

/// Model aliases accepted by `claude --model`.
const MODELS: &[(&str, &str)] = &[
    ("haiku", "Claude Haiku (via Claude Code)"),
    ("sonnet", "Claude Sonnet (via Claude Code)"),
    ("opus", "Claude Opus (via Claude Code)"),
];
const DEFAULT_MODEL: &str = "sonnet";
const DEFAULT_FAST_MODEL: &str = "haiku";

pub struct ClaudeCodeCliLanguageModelProvider {
    state: Entity<State>,
}

pub struct State {
    cli_path: Option<PathBuf>,
}

impl State {
    fn refresh_cli_path(&mut self, cx: &mut Context<Self>) {
        let cli_path = find_claude_cli();
        if cli_path != self.cli_path {
            self.cli_path = cli_path;
            cx.notify();
        }
    }
}

fn find_claude_cli() -> Option<PathBuf> {
    let file_name = if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    };
    let from_path = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|directory| directory.join(file_name))
            .find(|path| path.is_file())
    });
    // Zed launched from a desktop entry may not inherit the shell PATH, and the
    // Claude Code native installer puts the binary in ~/.local/bin.
    from_path.or_else(|| {
        let path = util::paths::home_dir().join(".local/bin").join(file_name);
        path.is_file().then_some(path)
    })
}

impl ClaudeCodeCliLanguageModelProvider {
    pub fn new(cx: &mut App) -> Self {
        let state = cx.new(|_| State {
            cli_path: find_claude_cli(),
        });
        Self { state }
    }

    fn create_language_model(
        &self,
        model: &'static str,
        cx: &App,
    ) -> Option<Arc<dyn LanguageModel>> {
        let (id, display_name) = MODELS.iter().find(|(id, _)| *id == model)?;
        let cli_path = self.state.read(cx).cli_path.clone()?;
        Some(Arc::new(ClaudeCodeCliLanguageModel {
            id,
            display_name,
            cli_path,
        }))
    }
}

impl LanguageModelProviderState for ClaudeCodeCliLanguageModelProvider {
    type ObservableEntity = State;

    fn observable_entity(&self) -> Option<Entity<Self::ObservableEntity>> {
        Some(self.state.clone())
    }
}

impl LanguageModelProvider for ClaudeCodeCliLanguageModelProvider {
    fn id(&self) -> LanguageModelProviderId {
        PROVIDER_ID
    }

    fn name(&self) -> LanguageModelProviderName {
        PROVIDER_NAME
    }

    fn icon(&self) -> IconOrSvg {
        IconOrSvg::Icon(IconName::AiClaude)
    }

    fn default_model(&self, cx: &App) -> Option<Arc<dyn LanguageModel>> {
        self.create_language_model(DEFAULT_MODEL, cx)
    }

    fn default_fast_model(&self, cx: &App) -> Option<Arc<dyn LanguageModel>> {
        self.create_language_model(DEFAULT_FAST_MODEL, cx)
    }

    fn provided_models(&self, cx: &App) -> Vec<Arc<dyn LanguageModel>> {
        MODELS
            .iter()
            .filter_map(|(id, _)| self.create_language_model(id, cx))
            .collect()
    }

    fn is_authenticated(&self, cx: &App) -> bool {
        self.state.read(cx).cli_path.is_some()
    }

    fn authenticate(&self, cx: &mut App) -> Task<Result<(), AuthenticateError>> {
        self.state
            .update(cx, |state, cx| state.refresh_cli_path(cx));
        if self.is_authenticated(cx) {
            Task::ready(Ok(()))
        } else {
            Task::ready(Err(AuthenticateError::CredentialsNotFound))
        }
    }

    fn settings_view(&self, _cx: &mut App) -> Option<ProviderSettingsView> {
        let state = self.state.clone();
        Some(ProviderSettingsView::Inline(InlineProviderSettings {
            title: None,
            description: Some(InlineDescription::Text(DESCRIPTION.into())),
            create_view: Arc::new(move |_window: &mut Window, cx: &mut App| -> AnyView {
                let state = state.clone();
                cx.new(|_| ConfigurationView { state }).into()
            }),
        }))
    }

    fn missing_credentials_error_message(&self) -> SharedString {
        "The `claude` CLI was not found. Install Claude Code and run `claude` once to sign in \
        with your Claude Pro/Max subscription."
            .into()
    }
}

struct ConfigurationView {
    state: Entity<State>,
}

impl Render for ConfigurationView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.state.read(cx).cli_path {
            Some(path) => Label::new(format!("Using {}", path.display())).color(Color::Muted),
            None => Label::new("`claude` CLI not found").color(Color::Error),
        }
    }
}

struct ClaudeCodeCliLanguageModel {
    id: &'static str,
    display_name: &'static str,
    cli_path: PathBuf,
}

impl LanguageModel for ClaudeCodeCliLanguageModel {
    fn id(&self) -> LanguageModelId {
        LanguageModelId::from(self.id.to_string())
    }

    fn name(&self) -> LanguageModelName {
        LanguageModelName::from(self.display_name.to_string())
    }

    fn provider_id(&self) -> LanguageModelProviderId {
        PROVIDER_ID
    }

    fn provider_name(&self) -> LanguageModelProviderName {
        PROVIDER_NAME
    }

    fn telemetry_id(&self) -> String {
        format!("claude_code_cli/{}", self.id)
    }

    fn supports_images(&self) -> bool {
        false
    }

    // Requests are sent as a single plain-text prompt, so Zed's tools can't be forwarded.
    fn supports_tools(&self) -> bool {
        false
    }

    fn supports_tool_choice(&self, _choice: LanguageModelToolChoice) -> bool {
        false
    }

    fn max_token_count(&self) -> u64 {
        200_000
    }

    fn stream_completion(
        &self,
        request: LanguageModelRequest,
        cx: &AsyncApp,
    ) -> BoxFuture<
        'static,
        Result<
            BoxStream<'static, Result<LanguageModelCompletionEvent, LanguageModelCompletionError>>,
            LanguageModelCompletionError,
        >,
    > {
        let (system_prompt, prompt) = request_to_prompt(&request);
        let cli_path = self.cli_path.clone();
        let model = self.id;
        let task = cx.background_spawn(async move {
            run_claude_cli(&cli_path, model, system_prompt.as_deref(), &prompt).await
        });
        async move {
            let text = task.await.map_err(LanguageModelCompletionError::Other)?;
            let events: Vec<Result<LanguageModelCompletionEvent, LanguageModelCompletionError>> = vec![
                Ok(LanguageModelCompletionEvent::Text(text)),
                Ok(LanguageModelCompletionEvent::Stop(StopReason::EndTurn)),
            ];
            Ok(futures::stream::iter(events).boxed())
        }
        .boxed()
    }
}

fn request_to_prompt(request: &LanguageModelRequest) -> (Option<String>, String) {
    let mut system_prompt = String::new();
    let conversation: Vec<_> = request
        .messages
        .iter()
        .filter_map(|message| {
            let text = message.string_contents();
            match message.role {
                Role::System => {
                    system_prompt.push_str(&text);
                    None
                }
                Role::User | Role::Assistant => Some((message.role, text)),
            }
        })
        .collect();

    // A single user message (the commit message case) is passed through verbatim;
    // longer conversations are flattened into a transcript since `claude -p` takes one prompt.
    let prompt = match conversation.as_slice() {
        [(Role::User, text)] => text.clone(),
        _ => conversation
            .iter()
            .map(|(role, text)| {
                let speaker = if *role == Role::Assistant {
                    "Assistant"
                } else {
                    "User"
                };
                format!("{speaker}: {text}")
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
    };

    let system_prompt = (!system_prompt.is_empty()).then_some(system_prompt);
    (system_prompt, prompt)
}

async fn run_claude_cli(
    cli_path: &PathBuf,
    model: &str,
    system_prompt: Option<&str>,
    prompt: &str,
) -> anyhow::Result<String> {
    let mut command = new_command(cli_path);
    command
        .arg("--print")
        .args(["--output-format", "text"])
        .args(["--model", model])
        // Disable Claude Code's own tools: the prompt already contains everything needed.
        .args(["--tools", ""])
        .arg("--no-session-persistence")
        .args([
            "--system-prompt",
            system_prompt.unwrap_or("Follow the user's instructions exactly."),
        ])
        // Run outside the project so Claude Code doesn't load the project's CLAUDE.md on
        // top of the project rules Zed already includes in the prompt.
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command
        .spawn()
        .with_context(|| format!("failed to run {}", cli_path.display()))?;

    // The prompt goes through stdin because a large diff could exceed the argument length limit.
    let mut stdin = child
        .stdin
        .take()
        .context("failed to open stdin of the claude CLI")?;
    stdin.write_all(prompt.as_bytes()).await?;
    stdin.close().await?;
    drop(stdin);

    let output = child.output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let details = if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        return Err(anyhow!(
            "claude CLI exited with {}: {details}",
            output.status
        ));
    }

    Ok(String::from_utf8(output.stdout)
        .context("claude CLI output was not valid UTF-8")?
        .trim()
        .to_string())
}
