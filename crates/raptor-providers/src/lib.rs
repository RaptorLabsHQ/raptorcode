use std::env;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub const CRATE_PURPOSE: &str = "Provider integration boundaries for RaptorCode.";
pub const DEEPSEEK_API_KEY_ENV: &str = "DEEPSEEK_API_KEY";
pub const DEEPSEEK_API_KEY_URL: &str = "https://platform.deepseek.com/api_keys";
pub const DEEPSEEK_CHAT_COMPLETIONS_ENDPOINT: &str = "https://api.deepseek.com/chat/completions";
pub const DEEPSEEK_DEFAULT_MODEL: &str = "deepseek-v4-pro";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProviderId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelId(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderCapabilities {
    pub chat_completion: bool,
    pub local_execution: bool,
    pub network_required: bool,
    pub streaming: bool,
    pub system_prompt: bool,
    pub tool_calls: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionMessage {
    pub role: CompletionMessageRole,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionMessageRole {
    Assistant,
    System,
    Tool,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionRequest {
    pub provider_id: ProviderId,
    pub model_id: ModelId,
    pub messages: Vec<CompletionMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionResponse {
    pub provider_id: ProviderId,
    pub model_id: ModelId,
    pub content: String,
    pub finish_reason: Option<String>,
    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenUsage {
    pub completion_tokens: Option<u32>,
    pub prompt_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDefinition {
    id: ProviderId,
    display_name: String,
    default_model: Option<ModelId>,
    models: Vec<ModelId>,
    capabilities: ProviderCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderConfiguration {
    pub enabled: bool,
    pub provider_id: ProviderId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSummary {
    pub capabilities: ProviderCapabilities,
    pub configured: bool,
    pub default_model: Option<ModelId>,
    pub display_name: String,
    pub enabled: bool,
    pub provider_id: ProviderId,
    pub registered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderCatalog {
    providers: Vec<ProviderDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    AuthenticationFailed(String),
    ExecutionDisabled(String),
    HttpStatus { detail: String, status: u16 },
    HttpTransport(String),
    InvalidModelId(String),
    InvalidProviderId(String),
    MalformedResponse(String),
    MissingApiKey,
    RateLimited(String),
}

#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone)]
pub struct AgentMessage {
    pub role: String,
    pub content: Option<String>,
    pub reasoning_content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone)]
pub enum AgentStepResult {
    Text(String),
    ToolCalls(Vec<ToolCall>),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeepSeekChatRequest {
    pub model: String,
    pub messages: Vec<DeepSeekChatMessage>,
    pub stream: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeepSeekChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
struct DeepSeekChatResponse {
    choices: Vec<DeepSeekChoice>,
    model: Option<String>,
    usage: Option<DeepSeekUsage>,
}

#[derive(Debug, Deserialize)]
struct DeepSeekChoice {
    finish_reason: Option<String>,
    message: DeepSeekResponseMessage,
}

#[derive(Debug, Deserialize)]
struct DeepSeekResponseMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeepSeekUsage {
    completion_tokens: Option<u32>,
    prompt_tokens: Option<u32>,
    total_tokens: Option<u32>,
}

#[derive(Serialize)]
struct DeepSeekAgentRequest {
    model: String,
    messages: Vec<DeepSeekAgentRequestMessage>,
    tools: Vec<DeepSeekAgentToolSpec>,
    stream: bool,
}

#[derive(Serialize)]
struct DeepSeekAgentRequestMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<DeepSeekAgentToolCallSpec>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Serialize)]
struct DeepSeekAgentToolSpec {
    #[serde(rename = "type")]
    tool_type: String,
    function: DeepSeekAgentFunctionSpec,
}

#[derive(Serialize)]
struct DeepSeekAgentFunctionSpec {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Serialize)]
struct DeepSeekAgentToolCallSpec {
    id: String,
    #[serde(rename = "type")]
    tool_type: String,
    function: DeepSeekAgentToolCallFunctionSpec,
}

#[derive(Serialize)]
struct DeepSeekAgentToolCallFunctionSpec {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct DeepSeekAgentResponse {
    choices: Vec<DeepSeekAgentChoice>,
}

#[derive(Deserialize)]
struct DeepSeekAgentChoice {
    finish_reason: Option<String>,
    message: DeepSeekAgentResponseMessage,
}

#[derive(Deserialize)]
struct DeepSeekAgentResponseMessage {
    content: Option<String>,
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<DeepSeekAgentToolCallResponse>>,
}

#[derive(Deserialize)]
struct DeepSeekAgentToolCallResponse {
    id: String,
    function: DeepSeekAgentToolCallFunctionResponse,
}

#[derive(Deserialize)]
struct DeepSeekAgentToolCallFunctionResponse {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct DeepSeekModelsListResponse {
    data: Vec<DeepSeekModelEntry>,
}

#[derive(Deserialize)]
struct DeepSeekModelEntry {
    id: String,
}

pub struct DeepSeekProvider {
    api_key: String,
    client: reqwest::blocking::Client,
    definition: ProviderDefinition,
    endpoint: String,
}

impl std::fmt::Debug for DeepSeekProvider {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DeepSeekProvider")
            .field("api_key", &"[redacted]")
            .field("definition", &self.definition)
            .field("endpoint", &self.endpoint)
            .finish()
    }
}

pub trait Provider {
    fn id(&self) -> &ProviderId;
    fn display_name(&self) -> &str;
    fn capabilities(&self) -> &ProviderCapabilities;
    fn default_model(&self) -> Option<&ModelId>;
    fn models(&self) -> &[ModelId];
    fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, ProviderError>;
}

impl ProviderId {
    pub fn new(value: impl Into<String>) -> Result<Self, ProviderError> {
        let value = value.into();
        let valid = !value.is_empty()
            && !value.starts_with('-')
            && !value.ends_with('-')
            && !value.contains("..")
            && value.chars().all(|character| {
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
            });

        if valid {
            Ok(Self(value))
        } else {
            Err(ProviderError::InvalidProviderId(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl ModelId {
    pub fn new(value: impl Into<String>) -> Result<Self, ProviderError> {
        let value = value.into();
        let valid = !value.is_empty()
            && !value.starts_with('-')
            && !value.ends_with('-')
            && !value.contains("..")
            && value.chars().all(|character| {
                character.is_ascii_alphanumeric()
                    || matches!(character, '-' | '_' | '.' | ':' | '/')
            });

        if valid {
            Ok(Self(value))
        } else {
            Err(ProviderError::InvalidModelId(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl ProviderCapabilities {
    pub fn network_chat() -> Self {
        Self {
            chat_completion: true,
            local_execution: false,
            network_required: true,
            streaming: true,
            system_prompt: true,
            tool_calls: true,
        }
    }

    pub fn local_model() -> Self {
        Self {
            chat_completion: true,
            local_execution: true,
            network_required: false,
            streaming: true,
            system_prompt: true,
            tool_calls: false,
        }
    }

    pub fn unavailable() -> Self {
        Self {
            chat_completion: false,
            local_execution: false,
            network_required: false,
            streaming: false,
            system_prompt: false,
            tool_calls: false,
        }
    }

    pub fn labels(&self) -> Vec<&'static str> {
        let mut labels = Vec::new();

        if self.chat_completion {
            labels.push("chat");
        }
        if self.streaming {
            labels.push("streaming");
        }
        if self.system_prompt {
            labels.push("system-prompt");
        }
        if self.tool_calls {
            labels.push("tool-calls");
        }
        if self.local_execution {
            labels.push("local");
        }
        if self.network_required {
            labels.push("network-required");
        }

        labels
    }
}

impl CompletionMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: CompletionMessageRole::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: CompletionMessageRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: CompletionMessageRole::Assistant,
            content: content.into(),
        }
    }
}

impl CompletionMessageRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Assistant => "assistant",
            Self::System => "system",
            Self::Tool => "tool",
            Self::User => "user",
        }
    }
}

impl CompletionRequest {
    pub fn new(
        provider_id: ProviderId,
        model_id: ModelId,
        messages: Vec<CompletionMessage>,
    ) -> Self {
        Self {
            provider_id,
            model_id,
            messages,
        }
    }
}

impl ProviderDefinition {
    pub fn new(
        id: ProviderId,
        display_name: impl Into<String>,
        default_model: Option<ModelId>,
        models: Vec<ModelId>,
        capabilities: ProviderCapabilities,
    ) -> Self {
        Self {
            id,
            display_name: display_name.into(),
            default_model,
            models,
            capabilities,
        }
    }
}

impl DeepSeekProvider {
    pub fn from_env() -> Result<Self, ProviderError> {
        let api_key = env::var(DEEPSEEK_API_KEY_ENV).map_err(|_| ProviderError::MissingApiKey)?;

        Self::new(api_key)
    }

    pub fn new(api_key: impl Into<String>) -> Result<Self, ProviderError> {
        Self::with_endpoint(api_key, DEEPSEEK_CHAT_COMPLETIONS_ENDPOINT)
    }

    pub fn with_endpoint(
        api_key: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Result<Self, ProviderError> {
        let api_key = api_key.into();

        if api_key.trim().is_empty() {
            return Err(ProviderError::MissingApiKey);
        }

        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|error| ProviderError::HttpTransport(error.to_string()))?;

        Ok(Self {
            api_key,
            client,
            definition: deepseek_definition(),
            endpoint: endpoint.into(),
        })
    }

    pub fn complete_agent_step(
        &self,
        model_id: &ModelId,
        messages: &[AgentMessage],
        tools: &[ToolDefinition],
    ) -> Result<(AgentMessage, AgentStepResult), ProviderError> {
        let request_messages: Vec<DeepSeekAgentRequestMessage> = messages
            .iter()
            .map(|m| DeepSeekAgentRequestMessage {
                role: m.role.clone(),
                content: m.content.clone(),
                reasoning_content: m.reasoning_content.clone(),
                tool_calls: m.tool_calls.as_ref().map(|calls| {
                    calls
                        .iter()
                        .map(|c| DeepSeekAgentToolCallSpec {
                            id: c.id.clone(),
                            tool_type: "function".to_owned(),
                            function: DeepSeekAgentToolCallFunctionSpec {
                                name: c.name.clone(),
                                arguments: c.arguments.clone(),
                            },
                        })
                        .collect()
                }),
                tool_call_id: m.tool_call_id.clone(),
            })
            .collect();

        let tool_specs: Vec<DeepSeekAgentToolSpec> = tools
            .iter()
            .map(|t| DeepSeekAgentToolSpec {
                tool_type: "function".to_owned(),
                function: DeepSeekAgentFunctionSpec {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                },
            })
            .collect();

        let request_body = DeepSeekAgentRequest {
            model: model_id.as_str().to_owned(),
            messages: request_messages,
            tools: tool_specs,
            stream: false,
        };

        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&request_body)
            .send()
            .map_err(|e| ProviderError::HttpTransport(e.to_string()))?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .map_err(|e| ProviderError::HttpTransport(e.to_string()))?;

        if status != 200 {
            return Err(map_deepseek_error_status(status, &body));
        }

        let parsed: DeepSeekAgentResponse = serde_json::from_str(&body).map_err(|e| {
            ProviderError::MalformedResponse(format!("failed to parse agent response: {e}"))
        })?;
        let choice = parsed.choices.into_iter().next().ok_or_else(|| {
            ProviderError::MalformedResponse("missing choices[0] in agent response".to_owned())
        })?;

        let reasoning_content = choice.message.reasoning_content.clone();

        if choice.finish_reason.as_deref() == Some("tool_calls") {
            let raw_calls = choice.message.tool_calls.ok_or_else(|| {
                ProviderError::MalformedResponse(
                    "finish_reason=tool_calls but no tool_calls in message".to_owned(),
                )
            })?;
            let calls: Vec<ToolCall> = raw_calls
                .into_iter()
                .map(|tc| ToolCall {
                    id: tc.id,
                    name: tc.function.name,
                    arguments: tc.function.arguments,
                })
                .collect();
            let assistant_msg = AgentMessage {
                role: "assistant".to_owned(),
                content: None,
                reasoning_content,
                tool_calls: Some(calls.clone()),
                tool_call_id: None,
            };
            Ok((assistant_msg, AgentStepResult::ToolCalls(calls)))
        } else {
            let content = choice.message.content.clone().unwrap_or_default();
            let assistant_msg = AgentMessage {
                role: "assistant".to_owned(),
                content: Some(content.clone()),
                reasoning_content,
                tool_calls: None,
                tool_call_id: None,
            };
            Ok((assistant_msg, AgentStepResult::Text(content)))
        }
    }

    pub fn list_models(&self) -> Result<Vec<String>, ProviderError> {
        let response = self
            .client
            .get("https://api.deepseek.com/models")
            .bearer_auth(&self.api_key)
            .send()
            .map_err(|e| ProviderError::HttpTransport(e.to_string()))?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .map_err(|e| ProviderError::HttpTransport(e.to_string()))?;

        if status != 200 {
            return Err(map_deepseek_error_status(status, &body));
        }

        let parsed: DeepSeekModelsListResponse = serde_json::from_str(&body).map_err(|e| {
            ProviderError::MalformedResponse(format!("failed to parse models list: {e}"))
        })?;

        Ok(parsed.data.into_iter().map(|m| m.id).collect())
    }
}

impl Provider for DeepSeekProvider {
    fn id(&self) -> &ProviderId {
        self.definition.id()
    }

    fn display_name(&self) -> &str {
        self.definition.display_name()
    }

    fn capabilities(&self) -> &ProviderCapabilities {
        self.definition.capabilities()
    }

    fn default_model(&self) -> Option<&ModelId> {
        self.definition.default_model()
    }

    fn models(&self) -> &[ModelId] {
        self.definition.models()
    }

    fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        if request.provider_id.as_str() != "deepseek" {
            return Err(ProviderError::InvalidProviderId(
                request.provider_id.as_str().to_owned(),
            ));
        }

        let request_body = deepseek_chat_request_from_completion(&request)?;
        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&request_body)
            .send()
            .map_err(|error| ProviderError::HttpTransport(error.to_string()))?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .map_err(|error| ProviderError::HttpTransport(error.to_string()))?;

        parse_deepseek_chat_response(status, &body, &request.provider_id, &request.model_id)
    }
}

pub fn deepseek_chat_request_from_completion(
    request: &CompletionRequest,
) -> Result<DeepSeekChatRequest, ProviderError> {
    if request.provider_id.as_str() != "deepseek" {
        return Err(ProviderError::InvalidProviderId(
            request.provider_id.as_str().to_owned(),
        ));
    }

    Ok(DeepSeekChatRequest {
        model: request.model_id.as_str().to_owned(),
        messages: request
            .messages
            .iter()
            .map(|message| DeepSeekChatMessage {
                role: message.role.as_str().to_owned(),
                content: message.content.clone(),
            })
            .collect(),
        stream: false,
    })
}

pub fn parse_deepseek_chat_response(
    status: u16,
    body: &str,
    provider_id: &ProviderId,
    fallback_model_id: &ModelId,
) -> Result<CompletionResponse, ProviderError> {
    if status != 200 {
        return Err(map_deepseek_error_status(status, body));
    }

    let parsed: DeepSeekChatResponse = serde_json::from_str(body).map_err(|error| {
        ProviderError::MalformedResponse(format!("failed to parse DeepSeek response JSON: {error}"))
    })?;
    let choice = parsed
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| ProviderError::MalformedResponse("missing choices[0]".to_owned()))?;
    let content = choice.message.content.ok_or_else(|| {
        ProviderError::MalformedResponse("missing choices[0].message.content".to_owned())
    })?;
    let model_id = match parsed.model {
        Some(model) if !model.trim().is_empty() => ModelId::new(model)?,
        _ => fallback_model_id.clone(),
    };

    Ok(CompletionResponse {
        provider_id: provider_id.clone(),
        model_id,
        content,
        finish_reason: choice.finish_reason,
        usage: parsed.usage.map(|usage| TokenUsage {
            completion_tokens: usage.completion_tokens,
            prompt_tokens: usage.prompt_tokens,
            total_tokens: usage.total_tokens,
        }),
    })
}

impl Provider for ProviderDefinition {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn display_name(&self) -> &str {
        &self.display_name
    }

    fn capabilities(&self) -> &ProviderCapabilities {
        &self.capabilities
    }

    fn default_model(&self) -> Option<&ModelId> {
        self.default_model.as_ref()
    }

    fn models(&self) -> &[ModelId] {
        &self.models
    }

    fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        Err(ProviderError::ExecutionDisabled(
            self.id.as_str().to_owned(),
        ))
    }
}

impl ProviderConfiguration {
    pub fn new(provider_id: ProviderId, enabled: bool) -> Self {
        Self {
            enabled,
            provider_id,
        }
    }
}

impl ProviderCatalog {
    pub fn built_in() -> Self {
        let providers = vec![
            deepseek_definition(),
            ProviderDefinition::new(
                ProviderId::new("openai").expect("built-in provider id is valid"),
                "OpenAI / Codex",
                Some(ModelId::new("gpt-5.4").expect("built-in model id is valid")),
                vec![ModelId::new("gpt-5.4").expect("built-in model id is valid")],
                ProviderCapabilities::network_chat(),
            ),
            ProviderDefinition::new(
                ProviderId::new("openrouter").expect("built-in provider id is valid"),
                "OpenRouter",
                None,
                Vec::new(),
                ProviderCapabilities::network_chat(),
            ),
            ProviderDefinition::new(
                ProviderId::new("gemini").expect("built-in provider id is valid"),
                "Gemini",
                None,
                Vec::new(),
                ProviderCapabilities::network_chat(),
            ),
            ProviderDefinition::new(
                ProviderId::new("local").expect("built-in provider id is valid"),
                "Local Models",
                None,
                Vec::new(),
                ProviderCapabilities::local_model(),
            ),
        ];

        Self { providers }
    }

    pub fn empty() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    pub fn providers(&self) -> &[ProviderDefinition] {
        &self.providers
    }

    pub fn summaries(&self, configurations: &[ProviderConfiguration]) -> Vec<ProviderSummary> {
        let mut summaries = Vec::new();

        for provider in &self.providers {
            let configuration = configurations
                .iter()
                .find(|configuration| configuration.provider_id == provider.id);

            summaries.push(ProviderSummary {
                capabilities: provider.capabilities.clone(),
                configured: configuration.is_some(),
                default_model: provider.default_model.clone(),
                display_name: provider.display_name.clone(),
                enabled: configuration
                    .map(|configuration| configuration.enabled)
                    .unwrap_or(false),
                provider_id: provider.id.clone(),
                registered: true,
            });
        }

        for configuration in configurations {
            if self
                .providers
                .iter()
                .any(|provider| provider.id == configuration.provider_id)
            {
                continue;
            }

            summaries.push(ProviderSummary {
                capabilities: ProviderCapabilities::unavailable(),
                configured: true,
                default_model: None,
                display_name: configuration.provider_id.as_str().to_owned(),
                enabled: configuration.enabled,
                provider_id: configuration.provider_id.clone(),
                registered: false,
            });
        }

        summaries
    }
}

fn deepseek_definition() -> ProviderDefinition {
    ProviderDefinition::new(
        ProviderId::new("deepseek").expect("built-in provider id is valid"),
        "DeepSeek",
        Some(ModelId::new(DEEPSEEK_DEFAULT_MODEL).expect("built-in model id is valid")),
        vec![
            ModelId::new(DEEPSEEK_DEFAULT_MODEL).expect("built-in model id is valid"),
            ModelId::new("deepseek-v4-flash").expect("built-in model id is valid"),
        ],
        ProviderCapabilities::network_chat(),
    )
}

fn map_deepseek_error_status(status: u16, body: &str) -> ProviderError {
    let detail = response_error_detail(body);

    match status {
        401 | 403 => ProviderError::AuthenticationFailed(detail),
        429 => ProviderError::RateLimited(detail),
        _ => ProviderError::HttpStatus { detail, status },
    }
}

fn response_error_detail(body: &str) -> String {
    let trimmed = body.trim();

    if trimmed.is_empty() {
        return "empty response body".to_owned();
    }

    let parsed = serde_json::from_str::<serde_json::Value>(trimmed)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(|message| message.as_str())
                .or_else(|| value.get("message").and_then(|message| message.as_str()))
                .map(ToOwned::to_owned)
        });
    let detail = parsed.unwrap_or_else(|| trimmed.to_owned());

    let truncated = detail.chars().take(300).collect::<String>();

    if truncated.len() < detail.len() {
        format!("{truncated}...")
    } else {
        detail
    }
}

impl Display for ProviderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AuthenticationFailed(detail) => {
                write!(formatter, "DeepSeek authentication failed: {detail}")
            }
            Self::ExecutionDisabled(provider_id) => {
                write!(
                    formatter,
                    "provider execution is not implemented for {provider_id}"
                )
            }
            Self::HttpStatus { detail, status } => {
                write!(formatter, "DeepSeek API returned HTTP {status}: {detail}")
            }
            Self::HttpTransport(detail) => {
                write!(formatter, "DeepSeek API request failed: {detail}")
            }
            Self::InvalidModelId(model_id) => write!(formatter, "model id is not safe: {model_id}"),
            Self::InvalidProviderId(provider_id) => {
                write!(formatter, "provider id is not safe: {provider_id}")
            }
            Self::MalformedResponse(detail) => {
                write!(formatter, "DeepSeek API response was malformed: {detail}")
            }
            Self::MissingApiKey => {
                write!(
                    formatter,
                    "{DEEPSEEK_API_KEY_ENV} is required for DeepSeek execution. Get a DeepSeek API key at {DEEPSEEK_API_KEY_URL}"
                )
            }
            Self::RateLimited(detail) => {
                write!(formatter, "DeepSeek API rate limit reached: {detail}")
            }
        }
    }
}

impl Error for ProviderError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_registry_lists_deepseek_first_and_future_boundaries() {
        let catalog = ProviderCatalog::built_in();
        let ids = catalog
            .providers()
            .iter()
            .map(|provider| provider.id().as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids[0], "deepseek");
        assert!(ids.contains(&"openai"));
        assert!(ids.contains(&"openrouter"));
        assert!(ids.contains(&"gemini"));
        assert!(ids.contains(&"local"));
    }

    #[test]
    fn provider_id_validation_rejects_unsafe_values() {
        assert!(ProviderId::new("deepseek").is_ok());
        assert!(ProviderId::new("deepseek-v4-pro").is_ok());
        assert!(ProviderId::new("DeepSeek").is_err());
        assert!(ProviderId::new("../deepseek").is_err());
        assert!(ProviderId::new("-deepseek").is_err());
    }

    #[test]
    fn model_id_validation_supports_common_model_names() {
        assert!(ModelId::new("deepseek-v4-pro").is_ok());
        assert!(ModelId::new("gpt-5.4").is_ok());
        assert!(ModelId::new("local/qwen3-coder").is_ok());
        assert!(ModelId::new("../secret").is_err());
    }

    #[test]
    fn provider_summaries_apply_configuration_without_execution() {
        let catalog = ProviderCatalog::built_in();
        let configuration = ProviderConfiguration::new(ProviderId::new("deepseek").unwrap(), true);
        let summaries = catalog.summaries(&[configuration]);
        let deepseek = summaries
            .iter()
            .find(|summary| summary.provider_id.as_str() == "deepseek")
            .unwrap();
        let openai = summaries
            .iter()
            .find(|summary| summary.provider_id.as_str() == "openai")
            .unwrap();

        assert!(deepseek.registered);
        assert!(deepseek.configured);
        assert!(deepseek.enabled);
        assert_eq!(
            deepseek.default_model.as_ref().unwrap().as_str(),
            "deepseek-v4-pro"
        );
        assert!(deepseek.capabilities.labels().contains(&"network-required"));
        assert!(!openai.configured);
        assert!(!openai.enabled);
    }

    #[test]
    fn deepseek_provider_exposes_v0_1_defaults() {
        let provider = DeepSeekProvider::with_endpoint("test-key", "https://example.invalid")
            .expect("test key should build provider");

        assert_eq!(provider.id().as_str(), "deepseek");
        assert_eq!(provider.display_name(), "DeepSeek");
        assert_eq!(
            provider.default_model().unwrap().as_str(),
            DEEPSEEK_DEFAULT_MODEL
        );
        assert!(provider
            .models()
            .iter()
            .any(|model| model.as_str() == "deepseek-v4-pro"));
        assert!(provider.capabilities().network_required);
    }

    #[test]
    fn missing_deepseek_api_key_is_clear_and_does_not_leak_values() {
        let error = DeepSeekProvider::with_endpoint("   ", "https://example.invalid").unwrap_err();
        let message = error.to_string();

        assert!(matches!(error, ProviderError::MissingApiKey));
        assert!(message.contains(DEEPSEEK_API_KEY_ENV));
        assert!(message.contains(DEEPSEEK_API_KEY_URL));
        assert!(!message.contains("secret"));
    }

    #[test]
    fn deepseek_request_serialization_matches_chat_completion_shape() {
        let request = CompletionRequest::new(
            ProviderId::new("deepseek").unwrap(),
            ModelId::new("deepseek-v4-pro").unwrap(),
            vec![
                CompletionMessage::system("You are RaptorCode."),
                CompletionMessage::user("Hello"),
            ],
        );
        let deepseek_request = deepseek_chat_request_from_completion(&request).unwrap();
        let json = serde_json::to_value(deepseek_request).unwrap();

        assert_eq!(json["model"], "deepseek-v4-pro");
        assert_eq!(json["stream"], false);
        assert_eq!(json["messages"][0]["role"], "system");
        assert_eq!(json["messages"][0]["content"], "You are RaptorCode.");
        assert_eq!(json["messages"][1]["role"], "user");
        assert_eq!(json["messages"][1]["content"], "Hello");
    }

    #[test]
    fn deepseek_response_parsing_succeeds() {
        let provider_id = ProviderId::new("deepseek").unwrap();
        let model_id = ModelId::new("deepseek-v4-pro").unwrap();
        let body = r#"{
            "id": "abc",
            "object": "chat.completion",
            "model": "deepseek-v4-pro",
            "choices": [
                {
                    "index": 0,
                    "message": {"role": "assistant", "content": "Hello from DeepSeek"},
                    "finish_reason": "stop"
                }
            ],
            "usage": {
                "prompt_tokens": 3,
                "completion_tokens": 4,
                "total_tokens": 7
            }
        }"#;

        let response = parse_deepseek_chat_response(200, body, &provider_id, &model_id).unwrap();

        assert_eq!(response.provider_id.as_str(), "deepseek");
        assert_eq!(response.model_id.as_str(), "deepseek-v4-pro");
        assert_eq!(response.content, "Hello from DeepSeek");
        assert_eq!(response.finish_reason.as_deref(), Some("stop"));
        assert_eq!(response.usage.unwrap().total_tokens, Some(7));
    }

    #[test]
    fn deepseek_response_parsing_rejects_malformed_response() {
        let provider_id = ProviderId::new("deepseek").unwrap();
        let model_id = ModelId::new("deepseek-v4-pro").unwrap();
        let body = r#"{"choices":[{"message":{},"finish_reason":"stop"}]}"#;

        let error = parse_deepseek_chat_response(200, body, &provider_id, &model_id).unwrap_err();

        assert!(matches!(error, ProviderError::MalformedResponse(_)));
        assert!(error.to_string().contains("message.content"));
    }

    #[test]
    fn deepseek_non_200_responses_are_explicit() {
        let provider_id = ProviderId::new("deepseek").unwrap();
        let model_id = ModelId::new("deepseek-v4-pro").unwrap();

        let auth = parse_deepseek_chat_response(
            401,
            r#"{"error":{"message":"invalid auth"}}"#,
            &provider_id,
            &model_id,
        )
        .unwrap_err();
        let rate = parse_deepseek_chat_response(
            429,
            r#"{"error":{"message":"slow down"}}"#,
            &provider_id,
            &model_id,
        )
        .unwrap_err();
        let generic =
            parse_deepseek_chat_response(500, "server error", &provider_id, &model_id).unwrap_err();

        assert!(matches!(auth, ProviderError::AuthenticationFailed(_)));
        assert!(auth.to_string().contains("authentication failed"));
        assert!(matches!(rate, ProviderError::RateLimited(_)));
        assert!(rate.to_string().contains("rate limit"));
        assert!(matches!(
            generic,
            ProviderError::HttpStatus { status: 500, .. }
        ));
        assert!(generic.to_string().contains("HTTP 500"));
    }

    #[test]
    fn empty_provider_catalog_returns_empty_summaries() {
        let catalog = ProviderCatalog::empty();

        assert!(catalog.summaries(&[]).is_empty());
    }

    #[test]
    fn provider_trait_blocks_completion_execution() {
        let catalog = ProviderCatalog::built_in();
        let provider = catalog.providers().first().unwrap();
        let request = CompletionRequest::new(
            provider.id().clone(),
            provider.default_model().unwrap().clone(),
            vec![CompletionMessage::user("hello")],
        );

        let error = provider.complete(request).unwrap_err();

        assert!(error.to_string().contains("not implemented"));
    }
}
