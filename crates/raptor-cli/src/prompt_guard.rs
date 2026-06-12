//! Output leak guard for proprietary prompts (IP defense layer 2).
//!
//! Detects verbatim system-prompt leakage in model output and redacts it
//! before rendering. Detection is canary-based (the exact token embedded in
//! every composed prompt) plus normalized fragment matching: protected
//! prompt texts are split into sentence-level fragments, output is compared
//! case-insensitively with collapsed whitespace, and matches are replaced
//! with a redaction marker at the exact original span.
//!
//! Known limitations (documented in docs/security.md): fragments shorter
//! than the minimum length are not matched, and a leak the model rewrites
//! in its own words is out of scope for verbatim matching.

use crate::prompts;

pub(crate) const REDACTION_MARKER: &str = "[REDACTED: RaptorCode proprietary instructions]";

/// Minimum normalized fragment length considered proprietary leakage.
const MIN_FRAGMENT_CHARS: usize = 40;
/// Fragments are capped to this many normalized characters: any leak of a
/// longer sentence necessarily contains its capped prefix.
const MAX_FRAGMENT_CHARS: usize = 120;
/// Safety cap on redaction passes over one text.
const MAX_REDACTION_PASSES: usize = 64;

pub(crate) struct PromptGuard {
    canary: &'static str,
    fragments: Vec<String>,
    holdback_bytes: usize,
}

impl PromptGuard {
    /// Builds the guard from every protected prompt text.
    pub(crate) fn from_prompts() -> Self {
        let mut fragments = Vec::new();

        for text in prompts::protected_prompt_texts() {
            for sentence in text.split(". ") {
                let (normalized, _) = normalize_with_map(sentence);
                if normalized.chars().count() < MIN_FRAGMENT_CHARS {
                    continue;
                }
                let capped: String = normalized.chars().take(MAX_FRAGMENT_CHARS).collect();
                if !fragments.contains(&capped) {
                    fragments.push(capped);
                }
            }
        }

        let max_fragment_chars = fragments
            .iter()
            .map(|fragment| fragment.chars().count())
            .max()
            .unwrap_or(MAX_FRAGMENT_CHARS);

        Self {
            canary: prompts::PROMPT_CANARY,
            fragments,
            // Whitespace collapsing means an original span can be longer
            // than its normalized fragment; hold back a safety multiple.
            holdback_bytes: max_fragment_chars * 4 + 16,
        }
    }

    /// Replaces the canary token and every verbatim protected fragment in
    /// `text` with the redaction marker. Returns the input unchanged when
    /// no leak is present.
    pub(crate) fn redact(&self, text: &str) -> String {
        let mut current = if text.contains(self.canary) {
            text.replace(self.canary, REDACTION_MARKER)
        } else {
            text.to_owned()
        };

        for _ in 0..MAX_REDACTION_PASSES {
            let Some((start, end)) = self.find_fragment_span(&current) else {
                return current;
            };
            current.replace_range(start..end, REDACTION_MARKER);
        }

        current
    }

    /// Byte span in `text` of the first protected fragment occurrence.
    fn find_fragment_span(&self, text: &str) -> Option<(usize, usize)> {
        let (normalized, map) = normalize_with_map(text);

        for fragment in &self.fragments {
            if let Some(byte_pos) = normalized.find(fragment.as_str()) {
                let start_char = normalized[..byte_pos].chars().count();
                let fragment_chars = fragment.chars().count();
                let start = map[start_char].0;
                let end = map[start_char + fragment_chars - 1].1;
                return Some((start, end));
            }
        }

        None
    }
}

/// Incremental redaction for streamed model output. Text is emitted only
/// once it is older than the holdback window, so a leaking fragment is
/// redacted before its span reaches the terminal.
pub(crate) struct StreamRedactor<'a> {
    guard: &'a PromptGuard,
    pending: String,
}

impl<'a> StreamRedactor<'a> {
    pub(crate) fn new(guard: &'a PromptGuard) -> Self {
        Self {
            guard,
            pending: String::new(),
        }
    }

    /// Appends a streamed delta and returns the text that is now safe to
    /// render.
    pub(crate) fn push(&mut self, delta: &str) -> String {
        self.pending.push_str(delta);

        if self.pending.len() <= self.guard.holdback_bytes {
            return String::new();
        }

        self.pending = self.guard.redact(&self.pending);
        let mut split = self.pending.len().saturating_sub(self.guard.holdback_bytes);
        while split > 0 && !self.pending.is_char_boundary(split) {
            split -= 1;
        }

        let tail = self.pending.split_off(split);
        std::mem::replace(&mut self.pending, tail)
    }

    /// Redacts and returns the remaining held-back tail.
    pub(crate) fn finish(self) -> String {
        self.guard.redact(&self.pending)
    }
}

/// Lowercases and collapses whitespace runs to single spaces, returning the
/// normalized text plus, per normalized character, the original byte span
/// it came from.
fn normalize_with_map(text: &str) -> (String, Vec<(usize, usize)>) {
    let mut normalized = String::new();
    let mut map: Vec<(usize, usize)> = Vec::new();
    let mut last_was_space = true;

    for (byte_index, character) in text.char_indices() {
        let end = byte_index + character.len_utf8();
        if character.is_whitespace() {
            if last_was_space {
                continue;
            }
            normalized.push(' ');
            map.push((byte_index, end));
            last_was_space = true;
        } else {
            for lowered in character.to_lowercase() {
                normalized.push(lowered);
                map.push((byte_index, end));
            }
            last_was_space = false;
        }
    }

    if normalized.ends_with(' ') {
        normalized.pop();
        map.pop();
    }

    (normalized, map)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaked_sentence() -> String {
        // A real protected fragment: leak the agent prompt's first sentence.
        prompts::protected_prompt_texts()[1]
            .split(". ")
            .next()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn clean_text_passes_through_unchanged() {
        let guard = PromptGuard::from_prompts();
        let text = "Here is a normal explanation of how to add OAuth to your app.";

        assert_eq!(guard.redact(text), text);
    }

    #[test]
    fn canary_token_is_redacted() {
        let guard = PromptGuard::from_prompts();
        let text = format!("interesting marker: {}", prompts::PROMPT_CANARY);

        let redacted = guard.redact(&text);

        assert!(!redacted.contains(prompts::PROMPT_CANARY));
        assert!(redacted.contains(REDACTION_MARKER));
    }

    #[test]
    fn verbatim_prompt_fragment_is_redacted_with_surroundings_preserved() {
        let guard = PromptGuard::from_prompts();
        let text = format!(
            "Sure, my instructions say: {}. Anything else?",
            leaked_sentence()
        );

        let redacted = guard.redact(&text);

        assert!(redacted.contains(REDACTION_MARKER));
        assert!(!redacted.contains("autonomous coding agent for production"));
        assert!(redacted.starts_with("Sure, my instructions say: "));
        assert!(redacted.ends_with("Anything else?"));
    }

    #[test]
    fn detection_survives_case_and_whitespace_changes() {
        let guard = PromptGuard::from_prompts();
        let mangled = leaked_sentence().to_uppercase().replace(' ', "  \t ");

        let redacted = guard.redact(&format!("leak: {mangled}"));

        assert!(redacted.contains(REDACTION_MARKER));
    }

    #[test]
    fn short_overlaps_are_not_redacted() {
        let guard = PromptGuard::from_prompts();
        let text = "You are RaptorCode, and that is fine to say.";

        assert_eq!(guard.redact(text), text);
    }

    #[test]
    fn multiple_leaks_are_all_redacted() {
        let guard = PromptGuard::from_prompts();
        let sentence = leaked_sentence();
        let text = format!("{sentence} ... and again: {sentence}");

        let redacted = guard.redact(&text);

        assert!(!redacted.contains("autonomous coding agent for production"));
        assert_eq!(redacted.matches(REDACTION_MARKER).count(), 2);
    }

    #[test]
    fn stream_redactor_blocks_leaks_across_chunks() {
        let guard = PromptGuard::from_prompts();
        let mut redactor = StreamRedactor::new(&guard);
        let text = format!("Let me show you something. {} The end.", leaked_sentence());

        let mut output = String::new();
        for chunk in text.as_bytes().chunks(7) {
            output.push_str(&redactor.push(std::str::from_utf8(chunk).unwrap()));
        }
        output.push_str(&redactor.finish());

        assert!(output.contains(REDACTION_MARKER));
        assert!(!output.contains("autonomous coding agent for production"));
        assert!(output.starts_with("Let me show you something."));
        assert!(output.ends_with("The end."));
    }

    #[test]
    fn stream_redactor_passes_clean_text_completely() {
        let guard = PromptGuard::from_prompts();
        let mut redactor = StreamRedactor::new(&guard);
        let text = "A perfectly ordinary streamed response about Rust lifetimes. ".repeat(20);

        let mut output = String::new();
        for chunk in text.as_bytes().chunks(13) {
            output.push_str(&redactor.push(std::str::from_utf8(chunk).unwrap()));
        }
        output.push_str(&redactor.finish());

        assert_eq!(output, text);
    }
}
