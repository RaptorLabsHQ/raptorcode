use std::error::Error;
use std::fmt::{Display, Formatter};

pub const CRATE_PURPOSE: &str = "Provider integration boundaries for RaptorCode.";

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
    ExecutionDisabled(String),
    InvalidModelId(String),
    InvalidProviderId(String),
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
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: CompletionMessageRole::User,
            content: content.into(),
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
            ProviderDefinition::new(
                ProviderId::new("deepseek").expect("built-in provider id is valid"),
                "DeepSeek",
                Some(ModelId::new("deepseek-chat").expect("built-in model id is valid")),
                vec![
                    ModelId::new("deepseek-chat").expect("built-in model id is valid"),
                    ModelId::new("deepseek-reasoner").expect("built-in model id is valid"),
                ],
                ProviderCapabilities::network_chat(),
            ),
            ProviderDefinition::new(
                ProviderId::new("openai").expect("built-in provider id is valid"),
                "OpenAI / Codex",
                Some(ModelId::new("gpt-5.4").expect("built-in model id is valid")),
                vec![ModelId::new("gpt-5.4").expect("built-in model id is valid")],
                ProviderCapabilities::network_chat(),
            ),
            ProviderDefinition::new(
                ProviderId::new("claude").expect("built-in provider id is valid"),
                "Claude",
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

impl Display for ProviderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExecutionDisabled(provider_id) => {
                write!(
                    formatter,
                    "provider execution is not implemented for {provider_id}"
                )
            }
            Self::InvalidModelId(model_id) => write!(formatter, "model id is not safe: {model_id}"),
            Self::InvalidProviderId(provider_id) => {
                write!(formatter, "provider id is not safe: {provider_id}")
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
        assert!(ids.contains(&"claude"));
        assert!(ids.contains(&"gemini"));
        assert!(ids.contains(&"local"));
    }

    #[test]
    fn provider_id_validation_rejects_unsafe_values() {
        assert!(ProviderId::new("deepseek").is_ok());
        assert!(ProviderId::new("deepseek-chat").is_ok());
        assert!(ProviderId::new("DeepSeek").is_err());
        assert!(ProviderId::new("../deepseek").is_err());
        assert!(ProviderId::new("-deepseek").is_err());
    }

    #[test]
    fn model_id_validation_supports_common_model_names() {
        assert!(ModelId::new("deepseek-chat").is_ok());
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
            "deepseek-chat"
        );
        assert!(deepseek.capabilities.labels().contains(&"network-required"));
        assert!(!openai.configured);
        assert!(!openai.enabled);
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
