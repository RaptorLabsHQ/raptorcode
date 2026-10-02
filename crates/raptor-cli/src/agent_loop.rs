//! The policy-gated tool-calling loop shared by the interactive REPL and the
//! live swarm workers.
//!
//! One implementation of: ask the model for a step, run every returned tool
//! call through [`ToolPolicy`], execute what is allowed, feed the results back
//! as `tool` messages, compact history against the context budget, and repeat
//! until the model answers with text or [`MAX_STEPS`] rounds have run.
//! Everything that differs between callers (how a confirmation is asked, how
//! progress is shown, whether to stream, whether a cancel flag exists) goes
//! through the [`AgentLoopUi`] hooks, so the most privileged path in the
//! product has exactly one copy.

use std::path::PathBuf;

use raptor_providers::{
    AgentMessage, AgentStep, AgentStepResult, DeepSeekRequestOptions, ModelId, Provider,
    ProviderError, TokenUsage, ToolCall, ToolDefinition,
};

use crate::agent_tools::{describe_tool_call, execute_tool_call};
use crate::context_budget::{self, ContextBudgetNotice, ContextBudgetState};
use crate::tool_policy::{ConfirmationKind, ConfirmationRequest, PolicyDecision, ToolPolicy};

/// Model requests per run before the loop pauses with [`AgentLoopEnd::StepLimit`].
pub(crate) const MAX_STEPS: u32 = 100;

/// Everything the loop needs from its caller besides the message history.
pub(crate) struct AgentLoopContext<'a> {
    pub budget: &'a mut ContextBudgetState,
    pub model_id: &'a ModelId,
    pub provider: &'a dyn Provider,
    pub request_options: &'a DeepSeekRequestOptions,
    pub streaming: bool,
    pub tool_policy: &'a mut ToolPolicy,
    pub tools: &'a [ToolDefinition],
    pub working_dir: &'a mut PathBuf,
}

/// Why the loop returned. History already holds every assistant tool-call
/// message and tool result; a final text answer is handed back for the caller
/// to render and append.
#[derive(Debug)]
pub(crate) enum AgentLoopEnd {
    Answered {
        assistant_message: AgentMessage,
        content: String,
        streamed: bool,
        usage: Option<TokenUsage>,
    },
    StepLimit,
    Cancelled,
}

/// How one tool call ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToolOutcome {
    Succeeded,
    Failed,
    DeniedByPolicy { reason: String },
    DeclinedByDeveloper { message: String },
}

/// Progress notifications, in the order the loop emits them.
#[derive(Debug)]
pub(crate) enum AgentLoopEvent {
    /// A model request is about to be sent.
    StepStarted { step: u32 },
    /// A streamed content fragment (streaming mode only).
    ContentDelta(String),
    /// The model replied (or the request failed); the step's request is over.
    StepFinished,
    /// Text the model attached to a tool-call round, when it was not streamed.
    Narration(String),
    /// Token usage reported for a tool-call round.
    Usage(TokenUsage),
    /// A tool call is about to be policy-checked.
    ToolStarted { action: String, call: ToolCall },
    /// Policy allowed (or the developer approved) the call; it is executing.
    ToolExecuting { action: String },
    /// The call finished, was denied, or was declined.
    ToolFinished { outcome: ToolOutcome },
    /// Context-budget warning or compaction after a tool round.
    BudgetNotice(ContextBudgetNotice),
    /// Every tool call of one round was processed.
    RoundFinished,
}

/// Caller hooks. The REPL prints to the terminal and asks on stdin; a swarm
/// worker sends dashboard events and waits on a reply channel.
pub(crate) trait AgentLoopUi {
    fn on_event(&mut self, event: AgentLoopEvent);
    /// Asks the developer about a gray-zone action; `true` allows it.
    fn confirm(&mut self, request: &ConfirmationRequest) -> bool;
    /// Checked before every model request and every tool call.
    fn cancelled(&self) -> bool {
        false
    }
}

pub(crate) fn run(
    context: &mut AgentLoopContext<'_>,
    history: &mut Vec<AgentMessage>,
    ui: &mut dyn AgentLoopUi,
) -> Result<AgentLoopEnd, ProviderError> {
    for step in 1..=MAX_STEPS {
        if ui.cancelled() {
            return Ok(AgentLoopEnd::Cancelled);
        }
        ui.on_event(AgentLoopEvent::StepStarted { step });

        let mut streamed = false;
        let step_result = if context.streaming {
            context.provider.complete_agent_step_streaming(
                context.model_id,
                history,
                context.tools,
                context.request_options,
                &mut |delta| {
                    if let Some(text) = delta.content {
                        streamed = true;
                        ui.on_event(AgentLoopEvent::ContentDelta(text));
                    }
                },
            )
        } else {
            context.provider.complete_agent_step(
                context.model_id,
                history,
                context.tools,
                context.request_options,
            )
        };
        ui.on_event(AgentLoopEvent::StepFinished);
        let AgentStep {
            assistant_message,
            result,
            usage,
        } = step_result?;

        let calls = match result {
            AgentStepResult::Text(content) => {
                return Ok(AgentLoopEnd::Answered {
                    assistant_message,
                    content,
                    streamed,
                    usage,
                });
            }
            AgentStepResult::ToolCalls(calls) => calls,
        };

        if !streamed {
            if let Some(text) = assistant_message.content.as_deref() {
                ui.on_event(AgentLoopEvent::Narration(text.to_owned()));
            }
        }
        if let Some(usage) = usage.as_ref() {
            ui.on_event(AgentLoopEvent::Usage(usage.clone()));
        }
        history.push(assistant_message);

        for call in &calls {
            if ui.cancelled() {
                return Ok(AgentLoopEnd::Cancelled);
            }
            let action = describe_tool_call(&call.name, &call.arguments);
            ui.on_event(AgentLoopEvent::ToolStarted {
                action: action.clone(),
                call: call.clone(),
            });

            let (outcome, result) = match context.tool_policy.evaluate(
                context.working_dir,
                &call.name,
                &call.arguments,
            ) {
                PolicyDecision::Deny { reason } => (
                    ToolOutcome::DeniedByPolicy {
                        reason: reason.clone(),
                    },
                    format!(
                        "error: blocked by RaptorCode policy ({reason}). Choose a safer approach and continue."
                    ),
                ),
                PolicyDecision::Confirm(request) => {
                    if ui.confirm(&request) {
                        if request.kind == ConfirmationKind::OutsideWorkspaceWrite {
                            context.tool_policy.allow_outside_writes_for_session();
                        }
                        execute(context.working_dir, &action, call, ui)
                    } else {
                        (
                            ToolOutcome::DeclinedByDeveloper {
                                message: request.message.clone(),
                            },
                            format!(
                                "error: the developer declined this action ({}). Ask for guidance or choose a different approach.",
                                request.message
                            ),
                        )
                    }
                }
                PolicyDecision::Allow => execute(context.working_dir, &action, call, ui),
            };

            ui.on_event(AgentLoopEvent::ToolFinished { outcome });
            history.push(AgentMessage::tool(call.id.clone(), result));
        }

        for notice in context_budget::agent_notices(history, usage.as_ref(), context.budget) {
            ui.on_event(AgentLoopEvent::BudgetNotice(notice));
        }
        ui.on_event(AgentLoopEvent::RoundFinished);
    }

    Ok(AgentLoopEnd::StepLimit)
}

fn execute(
    working_dir: &mut PathBuf,
    action: &str,
    call: &ToolCall,
    ui: &mut dyn AgentLoopUi,
) -> (ToolOutcome, String) {
    ui.on_event(AgentLoopEvent::ToolExecuting {
        action: action.to_owned(),
    });
    let result = execute_tool_call(working_dir, &call.name, &call.arguments);
    let outcome = if result.starts_with("error:") {
        ToolOutcome::Failed
    } else {
        ToolOutcome::Succeeded
    };
    (outcome, result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_tools::build_agent_tools;
    use raptor_providers::{
        AgentMessageRole, CompletionRequest, CompletionResponse, CompletionStreamDelta,
        ProviderCapabilities, ProviderId,
    };
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct ScriptedProvider {
        capabilities: ProviderCapabilities,
        id: ProviderId,
        model: ModelId,
        requests: RefCell<usize>,
        steps: RefCell<VecDeque<Result<AgentStep, ProviderError>>>,
        stream_text: Option<String>,
    }

    impl ScriptedProvider {
        fn new(steps: Vec<Result<AgentStep, ProviderError>>) -> Self {
            Self {
                capabilities: ProviderCapabilities::network_chat(),
                id: ProviderId::new("deepseek").unwrap(),
                model: ModelId::new("deepseek-v4-pro").unwrap(),
                requests: RefCell::new(0),
                steps: RefCell::new(steps.into_iter().collect()),
                stream_text: None,
            }
        }

        fn requests(&self) -> usize {
            *self.requests.borrow()
        }

        /// Pops the next scripted step; once the script is exhausted every
        /// further request gets the same tool round, so step-limit tests can
        /// script "tool calls forever" with one entry.
        fn next_step(&self) -> Result<AgentStep, ProviderError> {
            *self.requests.borrow_mut() += 1;
            let mut steps = self.steps.borrow_mut();
            if steps.len() == 1 {
                return clone_step(&steps[0]);
            }
            steps.pop_front().unwrap_or_else(|| Ok(text_step("done")))
        }
    }

    fn clone_step(step: &Result<AgentStep, ProviderError>) -> Result<AgentStep, ProviderError> {
        match step {
            Ok(step) => Ok(step.clone()),
            Err(error) => Err(error.clone()),
        }
    }

    impl Provider for ScriptedProvider {
        fn id(&self) -> &ProviderId {
            &self.id
        }

        fn display_name(&self) -> &str {
            "scripted"
        }

        fn capabilities(&self) -> &ProviderCapabilities {
            &self.capabilities
        }

        fn default_model(&self) -> Option<&ModelId> {
            Some(&self.model)
        }

        fn models(&self) -> &[ModelId] {
            std::slice::from_ref(&self.model)
        }

        fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            Err(ProviderError::ExecutionDisabled("scripted".to_owned()))
        }

        fn complete_streaming(
            &self,
            _request: CompletionRequest,
            _on_delta: &mut dyn FnMut(CompletionStreamDelta),
        ) -> Result<CompletionResponse, ProviderError> {
            Err(ProviderError::ExecutionDisabled("scripted".to_owned()))
        }

        fn complete_agent_step(
            &self,
            _model_id: &ModelId,
            _messages: &[AgentMessage],
            _tools: &[ToolDefinition],
            _options: &DeepSeekRequestOptions,
        ) -> Result<AgentStep, ProviderError> {
            self.next_step()
        }

        fn complete_agent_step_streaming(
            &self,
            _model_id: &ModelId,
            _messages: &[AgentMessage],
            _tools: &[ToolDefinition],
            _options: &DeepSeekRequestOptions,
            on_delta: &mut dyn FnMut(CompletionStreamDelta),
        ) -> Result<AgentStep, ProviderError> {
            if let Some(text) = self.stream_text.as_deref() {
                for word in text.split_inclusive(' ') {
                    on_delta(CompletionStreamDelta {
                        content: Some(word.to_owned()),
                        finish_reason: None,
                        reasoning_content: None,
                    });
                }
            }
            self.next_step()
        }

        fn list_models(&self) -> Result<Vec<String>, ProviderError> {
            Ok(vec![self.model.as_str().to_owned()])
        }
    }

    fn usage() -> Option<TokenUsage> {
        Some(TokenUsage {
            completion_tokens: Some(2),
            prompt_tokens: Some(8),
            prompt_cache_hit_tokens: None,
            prompt_cache_miss_tokens: None,
            total_tokens: Some(10),
        })
    }

    fn text_step(content: &str) -> AgentStep {
        AgentStep {
            assistant_message: AgentMessage {
                role: AgentMessageRole::Assistant,
                content: Some(content.to_owned()),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
            },
            result: AgentStepResult::Text(content.to_owned()),
            usage: usage(),
        }
    }

    fn tool_step(narration: Option<&str>, calls: &[(&str, serde_json::Value)]) -> AgentStep {
        let calls: Vec<ToolCall> = calls
            .iter()
            .enumerate()
            .map(|(index, (name, arguments))| ToolCall {
                id: format!("call_{index}"),
                name: (*name).to_owned(),
                arguments: arguments.to_string(),
            })
            .collect();
        AgentStep {
            assistant_message: AgentMessage {
                role: AgentMessageRole::Assistant,
                content: narration.map(str::to_owned),
                reasoning_content: None,
                tool_calls: Some(calls.clone()),
                tool_call_id: None,
            },
            result: AgentStepResult::ToolCalls(calls),
            usage: usage(),
        }
    }

    #[derive(Default)]
    struct RecordingUi {
        cancel: bool,
        confirm_answer: bool,
        confirmations: Vec<String>,
        events: Vec<AgentLoopEvent>,
    }

    impl AgentLoopUi for RecordingUi {
        fn on_event(&mut self, event: AgentLoopEvent) {
            self.events.push(event);
        }

        fn confirm(&mut self, request: &ConfirmationRequest) -> bool {
            self.confirmations.push(request.message.clone());
            self.confirm_answer
        }

        fn cancelled(&self) -> bool {
            self.cancel
        }
    }

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn create(name: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "raptor-agent-loop-{name}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            let path = path.canonicalize().unwrap_or(path);
            Self {
                path: raptor_core::simplify_path(&path),
            }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn history() -> Vec<AgentMessage> {
        vec![
            AgentMessage::system("system"),
            AgentMessage::user("build it"),
        ]
    }

    fn run_loop(
        provider: &ScriptedProvider,
        workspace: &Path,
        streaming: bool,
        ui: &mut RecordingUi,
        history: &mut Vec<AgentMessage>,
    ) -> Result<AgentLoopEnd, ProviderError> {
        let tools = build_agent_tools();
        let mut budget = ContextBudgetState::default();
        let mut policy = ToolPolicy::new(workspace.to_path_buf());
        let mut working_dir = workspace.to_path_buf();
        let options = DeepSeekRequestOptions::default();
        let mut context = AgentLoopContext {
            budget: &mut budget,
            model_id: &provider.model,
            provider,
            request_options: &options,
            streaming,
            tool_policy: &mut policy,
            tools: &tools,
            working_dir: &mut working_dir,
        };
        run(&mut context, history, ui)
    }

    fn tool_outcomes(ui: &RecordingUi) -> Vec<&ToolOutcome> {
        ui.events
            .iter()
            .filter_map(|event| match event {
                AgentLoopEvent::ToolFinished { outcome, .. } => Some(outcome),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn text_answer_is_returned_and_history_is_left_for_the_caller() {
        let workspace = TestDir::create("text");
        let provider = ScriptedProvider::new(vec![Ok(text_step("All done."))]);
        let mut ui = RecordingUi::default();
        let mut messages = history();

        let end = run_loop(&provider, &workspace.path, false, &mut ui, &mut messages).unwrap();

        match end {
            AgentLoopEnd::Answered {
                content, streamed, ..
            } => {
                assert_eq!(content, "All done.");
                assert!(!streamed);
            }
            _ => panic!("expected a text answer"),
        }
        assert_eq!(
            messages.len(),
            2,
            "the loop must not append the final answer"
        );
        assert_eq!(provider.requests(), 1);
        assert!(matches!(
            ui.events[0],
            AgentLoopEvent::StepStarted { step: 1 }
        ));
        assert!(matches!(ui.events[1], AgentLoopEvent::StepFinished));
    }

    #[test]
    fn allowed_tool_call_executes_and_feeds_its_result_back() {
        let workspace = TestDir::create("allow");
        let provider = ScriptedProvider::new(vec![
            Ok(tool_step(
                Some("Writing the note."),
                &[(
                    "write_file",
                    serde_json::json!({ "path": "note.txt", "content": "raptor" }),
                )],
            )),
            Ok(text_step("Wrote the note.")),
        ]);
        let mut ui = RecordingUi::default();
        let mut messages = history();

        let end = run_loop(&provider, &workspace.path, false, &mut ui, &mut messages).unwrap();

        assert!(matches!(end, AgentLoopEnd::Answered { .. }));
        assert_eq!(
            fs::read_to_string(workspace.path.join("note.txt")).unwrap(),
            "raptor"
        );
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[2].role, AgentMessageRole::Assistant);
        assert!(messages[2].tool_calls.is_some());
        assert_eq!(messages[3].role, AgentMessageRole::Tool);
        assert_eq!(messages[3].tool_call_id.as_deref(), Some("call_0"));
        assert!(messages[3]
            .content
            .as_deref()
            .unwrap()
            .starts_with("wrote 6 bytes"));
        assert_eq!(tool_outcomes(&ui), vec![&ToolOutcome::Succeeded]);
        assert!(ui.events.iter().any(|event| matches!(
            event,
            AgentLoopEvent::Narration(text) if text == "Writing the note."
        )));
        assert!(ui
            .events
            .iter()
            .any(|event| matches!(event, AgentLoopEvent::ToolExecuting { .. })));
        assert!(ui
            .events
            .iter()
            .any(|event| matches!(event, AgentLoopEvent::RoundFinished)));
        assert!(ui.confirmations.is_empty());
    }

    #[test]
    fn policy_denial_is_reported_to_the_model_and_never_executed() {
        let workspace = TestDir::create("deny");
        let provider = ScriptedProvider::new(vec![
            Ok(tool_step(
                None,
                &[("run_command", serde_json::json!({ "command": "rm -rf /" }))],
            )),
            Ok(text_step("Understood.")),
        ]);
        let mut ui = RecordingUi::default();
        let mut messages = history();

        run_loop(&provider, &workspace.path, false, &mut ui, &mut messages).unwrap();

        assert!(matches!(
            tool_outcomes(&ui)[0],
            ToolOutcome::DeniedByPolicy { .. }
        ));
        assert!(messages[3]
            .content
            .as_deref()
            .unwrap()
            .starts_with("error: blocked by RaptorCode policy"));
        assert!(ui.confirmations.is_empty());
        assert!(!ui
            .events
            .iter()
            .any(|event| matches!(event, AgentLoopEvent::ToolExecuting { .. })));
    }

    #[test]
    fn declined_confirmation_is_reported_and_nothing_is_written() {
        let workspace = TestDir::create("decline");
        let outside = TestDir::create("decline-outside");
        let target = outside.path.join("escaped.txt");
        let provider = ScriptedProvider::new(vec![
            Ok(tool_step(
                None,
                &[(
                    "write_file",
                    serde_json::json!({ "path": target, "content": "x" }),
                )],
            )),
            Ok(text_step("Okay.")),
        ]);
        let mut ui = RecordingUi::default();
        let mut messages = history();

        run_loop(&provider, &workspace.path, false, &mut ui, &mut messages).unwrap();

        assert_eq!(ui.confirmations.len(), 1);
        assert!(matches!(
            tool_outcomes(&ui)[0],
            ToolOutcome::DeclinedByDeveloper { .. }
        ));
        assert!(!target.exists());
        assert!(messages[3]
            .content
            .as_deref()
            .unwrap()
            .starts_with("error: the developer declined this action"));
    }

    #[test]
    fn approved_outside_write_unlocks_the_rest_of_the_session() {
        let workspace = TestDir::create("approve");
        let outside = TestDir::create("approve-outside");
        let first = outside.path.join("first.txt");
        let second = outside.path.join("second.txt");
        let provider = ScriptedProvider::new(vec![
            Ok(tool_step(
                None,
                &[
                    (
                        "write_file",
                        serde_json::json!({ "path": first, "content": "1" }),
                    ),
                    (
                        "write_file",
                        serde_json::json!({ "path": second, "content": "2" }),
                    ),
                ],
            )),
            Ok(text_step("Done.")),
        ]);
        let mut ui = RecordingUi {
            confirm_answer: true,
            ..RecordingUi::default()
        };
        let mut messages = history();

        run_loop(&provider, &workspace.path, false, &mut ui, &mut messages).unwrap();

        assert_eq!(
            ui.confirmations.len(),
            1,
            "the second outside write must not ask again"
        );
        assert_eq!(
            tool_outcomes(&ui),
            vec![&ToolOutcome::Succeeded, &ToolOutcome::Succeeded]
        );
        assert!(first.exists() && second.exists());
    }

    #[test]
    fn step_limit_pauses_without_a_final_answer() {
        let workspace = TestDir::create("limit");
        let provider = ScriptedProvider::new(vec![Ok(tool_step(
            None,
            &[("list_directory", serde_json::json!({ "path": "." }))],
        ))]);
        let mut ui = RecordingUi::default();
        let mut messages = history();

        let end = run_loop(&provider, &workspace.path, false, &mut ui, &mut messages).unwrap();

        assert!(matches!(end, AgentLoopEnd::StepLimit));
        assert_eq!(provider.requests() as u32, MAX_STEPS);
        assert_eq!(messages.len(), 2 + 2 * MAX_STEPS as usize);
    }

    #[test]
    fn provider_errors_propagate_after_the_step_is_marked_finished() {
        let workspace = TestDir::create("error");
        let provider = ScriptedProvider::new(vec![
            Err(ProviderError::AuthenticationFailed("bad key".to_owned())),
            Ok(text_step("unreachable")),
        ]);
        let mut ui = RecordingUi::default();
        let mut messages = history();

        let error =
            run_loop(&provider, &workspace.path, false, &mut ui, &mut messages).unwrap_err();

        assert!(matches!(error, ProviderError::AuthenticationFailed(_)));
        assert!(matches!(
            ui.events.last(),
            Some(AgentLoopEvent::StepFinished)
        ));
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn cancellation_is_honored_before_the_first_request() {
        let workspace = TestDir::create("cancel");
        let provider = ScriptedProvider::new(vec![Ok(text_step("never"))]);
        let mut ui = RecordingUi {
            cancel: true,
            ..RecordingUi::default()
        };
        let mut messages = history();

        let end = run_loop(&provider, &workspace.path, false, &mut ui, &mut messages).unwrap();

        assert!(matches!(end, AgentLoopEnd::Cancelled));
        assert_eq!(provider.requests(), 0);
        assert!(ui.events.is_empty());
    }

    #[test]
    fn streamed_content_is_forwarded_and_marks_the_answer_streamed() {
        let workspace = TestDir::create("stream");
        let mut provider = ScriptedProvider::new(vec![Ok(text_step("hello world"))]);
        provider.stream_text = Some("hello world".to_owned());
        let mut ui = RecordingUi::default();
        let mut messages = history();

        let end = run_loop(&provider, &workspace.path, true, &mut ui, &mut messages).unwrap();

        match end {
            AgentLoopEnd::Answered { streamed, .. } => assert!(streamed),
            _ => panic!("expected a streamed text answer"),
        }
        let deltas: Vec<&str> = ui
            .events
            .iter()
            .filter_map(|event| match event {
                AgentLoopEvent::ContentDelta(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(deltas, vec!["hello ", "world"]);
    }
}
