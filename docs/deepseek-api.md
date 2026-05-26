# DeepSeek API Integration

Date: 2026-05-26
Author: Bob Vasic
Company: RaptorLabs.dev

## Summary

RaptorCode uses the official DeepSeek API directly through the Rust provider boundary in `crates/raptor-providers`.

The current implemented OpenAI-format DeepSeek surface is:

- `POST https://api.deepseek.com/chat/completions`
- `GET https://api.deepseek.com/models`
- `GET https://api.deepseek.com/user/balance`

## Current Defaults

- Default model: `deepseek-v4-pro`
- Alternate model: `deepseek-v4-flash`
- Thinking mode: enabled by default
- Reasoning effort: `max` for interactive agentic mode, `high` for regular chat requests
- Streaming: enabled by default for chat and agent mode; streamed agent tool-call deltas are assembled before local tool execution
- Response format: text by default, optional JSON output mode through `response_format: {"type": "json_object"}`
- Context caching: automatic on DeepSeek's side; RaptorCode displays cache hit and cache miss token telemetry when returned

## Interactive Commands

The interactive `raptor` agent supports these DeepSeek runtime controls:

- `/thinking`: toggles `{"thinking": {"type": "enabled"}}` and `{"thinking": {"type": "disabled"}}`
- `/effort`: toggles `reasoning_effort` between `high` and `max`
- `/stream`: toggles streamed DeepSeek responses in chat and agent mode
- `/json`: toggles DeepSeek JSON output mode and adds an explicit JSON-only system instruction
- `/balance`: calls `/user/balance` and prints availability plus returned balance rows
- `/model`: fetches `/models` and lets the user select a model
- `/clear`: resets conversation state after API-affecting changes

Changing thinking mode, reasoning effort, JSON output mode, or model resets the in-memory conversation history so stale assistant/tool context is not mixed across materially different request settings.

## Multi-Round Conversation

DeepSeek `/chat/completions` is stateless. RaptorCode keeps local conversation history and sends accumulated messages on each request.

In chat mode, streamed SSE chunks are parsed from `data:` events. RaptorCode prints final-answer `delta.content` chunks as they arrive, ignores keep-alive comments, captures thinking-mode `delta.reasoning_content` internally, and records final usage telemetry when returned.

For agentic tool calls, RaptorCode supports streamed and non-streamed responses. Streamed agent mode assembles `delta.tool_calls` fragments by index, concatenates streamed function names and JSON argument fragments, preserves assistant `reasoning_content`, then executes only the completed local RaptorCode tool call. This matches the DeepSeek thinking-mode requirement that tool-call reasoning content must be passed back in later requests for that conversation.

## Tool Calls

RaptorCode sends tools using OpenAI-format function tool definitions. Tool output is returned as `role = "tool"` messages with the required `tool_call_id`.

Tool output is treated as untrusted. Model output is not executed directly; only locally defined RaptorCode tools can run, and their results are appended back into the model conversation.

## JSON Output

DeepSeek JSON Output is exposed through `/json` in the interactive CLI. When enabled, RaptorCode:

- serializes `response_format: {"type": "json_object"}`
- inserts a system instruction that explicitly contains the word `JSON`
- asks for valid JSON only, with no Markdown fences or prose
- resets conversation history when the mode changes

This follows the official requirement to both set `response_format` and instruct the model to produce JSON in the prompt. The CLI does not yet enforce a caller-supplied JSON schema; strict tool schema mode remains separate beta work.

## Context Caching

DeepSeek context caching is automatic and best-effort. RaptorCode does not attempt to manage the cache directly.

When DeepSeek returns usage telemetry, RaptorCode surfaces:

- `prompt_tokens`
- `completion_tokens`
- `prompt_cache_hit_tokens`
- `prompt_cache_miss_tokens`
- `total_tokens`

This lets BOB verify whether repeated prompt prefixes are hitting cache and whether requests are cost-effective.

## User Isolation

The provider supports `user_id` in request options for future session/user isolation. User IDs are validated against the documented `[a-zA-Z0-9\-_]+` shape and 512-character maximum before serialization.

No private user data should be placed in `user_id`.

## Not Yet Implemented

- Chat Prefix Completion beta endpoint behavior
- FIM Completion beta endpoint behavior
- Strict tool-call schema beta mode
- Token budget warnings and rolling conversation compaction
- Persisted session restore across CLI invocations
