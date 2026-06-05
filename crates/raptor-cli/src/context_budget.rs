use raptor_providers::{AgentMessage, CompletionMessage, TokenUsage};

pub(crate) const DEEPSEEK_CONTEXT_TOKEN_LIMIT: u32 = 1_000_000;
const DEEPSEEK_CONTEXT_WARNING_TOKENS: u32 = 850_000;
const DEEPSEEK_CONTEXT_COMPACTION_TOKENS: u32 = 900_000;
pub(crate) const CHAT_COMPACTION_KEEP_MESSAGES: usize = 32;
pub(crate) const AGENT_COMPACTION_KEEP_MESSAGES: usize = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextBudgetNotice {
    Warning { total_tokens: u32, token_limit: u32 },
    Compacted { omitted: usize, retained: usize },
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ContextBudgetState {
    compacted_turns: usize,
    warned: bool,
}

pub(crate) fn chat_notices(
    history: &mut Vec<CompletionMessage>,
    usage: Option<&TokenUsage>,
    state: &mut ContextBudgetState,
) -> Vec<ContextBudgetNotice> {
    let Some(total_tokens) = usage.and_then(context_total_tokens) else {
        return Vec::new();
    };

    let mut notices = warning_notices(total_tokens, state);

    if total_tokens >= DEEPSEEK_CONTEXT_COMPACTION_TOKENS {
        let omitted = compact_chat_history(history, state);
        if omitted > 0 {
            notices.push(ContextBudgetNotice::Compacted {
                omitted,
                retained: history.len(),
            });
        }
    }

    notices
}

pub(crate) fn agent_notices(
    history: &mut Vec<AgentMessage>,
    usage: Option<&TokenUsage>,
    state: &mut ContextBudgetState,
) -> Vec<ContextBudgetNotice> {
    let Some(total_tokens) = usage.and_then(context_total_tokens) else {
        return Vec::new();
    };

    let mut notices = warning_notices(total_tokens, state);

    if total_tokens >= DEEPSEEK_CONTEXT_COMPACTION_TOKENS {
        let omitted = compact_agent_history(history, state);
        if omitted > 0 {
            notices.push(ContextBudgetNotice::Compacted {
                omitted,
                retained: history.len(),
            });
        }
    }

    notices
}

fn warning_notices(total_tokens: u32, state: &mut ContextBudgetState) -> Vec<ContextBudgetNotice> {
    if total_tokens < DEEPSEEK_CONTEXT_WARNING_TOKENS || state.warned {
        return Vec::new();
    }

    state.warned = true;
    vec![ContextBudgetNotice::Warning {
        total_tokens,
        token_limit: DEEPSEEK_CONTEXT_TOKEN_LIMIT,
    }]
}

fn context_total_tokens(usage: &TokenUsage) -> Option<u32> {
    usage
        .total_tokens
        .or_else(|| match (usage.prompt_tokens, usage.completion_tokens) {
            (Some(prompt), Some(completion)) => prompt.checked_add(completion),
            _ => None,
        })
}

pub(crate) fn compact_chat_history(
    history: &mut Vec<CompletionMessage>,
    state: &mut ContextBudgetState,
) -> usize {
    if history.len() <= CHAT_COMPACTION_KEEP_MESSAGES + 1 {
        return 0;
    }

    let Some(system_message) = history.first().cloned() else {
        return 0;
    };
    let recent_start = history.len().saturating_sub(CHAT_COMPACTION_KEEP_MESSAGES);
    let omitted = recent_start.saturating_sub(1);
    if omitted == 0 {
        return 0;
    }

    state.compacted_turns += omitted;
    state.warned = false;
    let mut compacted = Vec::with_capacity(CHAT_COMPACTION_KEEP_MESSAGES + 2);
    compacted.push(system_message);
    compacted.push(CompletionMessage::system(format!(
        "RaptorCode locally compacted this chat to stay inside DeepSeek's 1M-token context budget. {} older messages were omitted. Continue with the retained recent messages as authoritative context.",
        state.compacted_turns
    )));
    compacted.extend(history.drain(recent_start..));
    *history = compacted;

    omitted
}

pub(crate) fn compact_agent_history(
    history: &mut Vec<AgentMessage>,
    state: &mut ContextBudgetState,
) -> usize {
    if history.len() <= AGENT_COMPACTION_KEEP_MESSAGES + 1 {
        return 0;
    }

    let Some(system_message) = history.first().cloned() else {
        return 0;
    };
    let minimum_start = history.len().saturating_sub(AGENT_COMPACTION_KEEP_MESSAGES);
    let recent_start = history
        .iter()
        .enumerate()
        .skip(minimum_start.max(1))
        .find(|(_, message)| message.role == "user")
        .map(|(index, _)| index)
        .unwrap_or(minimum_start.max(1));
    let omitted = recent_start.saturating_sub(1);
    if omitted == 0 {
        return 0;
    }

    state.compacted_turns += omitted;
    state.warned = false;
    let mut compacted = Vec::with_capacity(AGENT_COMPACTION_KEEP_MESSAGES + 2);
    compacted.push(system_message);
    compacted.push(AgentMessage {
        role: "system".to_owned(),
        content: Some(format!(
            "RaptorCode locally compacted this agent conversation to stay inside DeepSeek's 1M-token context budget. {} older messages were omitted. Continue with the retained recent user, assistant, and tool messages as authoritative context. If missing details are needed, inspect the repository again with tools.",
            state.compacted_turns
        )),
        reasoning_content: None,
        tool_calls: None,
        tool_call_id: None,
    });
    compacted.extend(history.drain(recent_start..));
    *history = compacted;

    omitted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_usage_prefers_explicit_total_tokens() {
        let usage = TokenUsage {
            completion_tokens: Some(9),
            prompt_tokens: Some(5),
            prompt_cache_hit_tokens: None,
            prompt_cache_miss_tokens: None,
            total_tokens: Some(42),
        };

        assert_eq!(context_total_tokens(&usage), Some(42));
    }

    #[test]
    fn token_usage_falls_back_to_prompt_plus_completion_tokens() {
        let usage = TokenUsage {
            completion_tokens: Some(9),
            prompt_tokens: Some(5),
            prompt_cache_hit_tokens: None,
            prompt_cache_miss_tokens: None,
            total_tokens: None,
        };

        assert_eq!(context_total_tokens(&usage), Some(14));
    }

    #[test]
    fn chat_notice_warns_and_compacts_when_usage_crosses_budget() {
        let mut history = vec![CompletionMessage::system("system")];
        for index in 0..40 {
            history.push(CompletionMessage::user(format!("message {index}")));
        }
        let usage = TokenUsage {
            completion_tokens: None,
            prompt_tokens: None,
            prompt_cache_hit_tokens: None,
            prompt_cache_miss_tokens: None,
            total_tokens: Some(DEEPSEEK_CONTEXT_COMPACTION_TOKENS),
        };
        let mut state = ContextBudgetState::default();

        let notices = chat_notices(&mut history, Some(&usage), &mut state);

        assert_eq!(
            notices,
            vec![
                ContextBudgetNotice::Warning {
                    total_tokens: DEEPSEEK_CONTEXT_COMPACTION_TOKENS,
                    token_limit: DEEPSEEK_CONTEXT_TOKEN_LIMIT
                },
                ContextBudgetNotice::Compacted {
                    omitted: 8,
                    retained: CHAT_COMPACTION_KEEP_MESSAGES + 2
                }
            ]
        );
    }
}
