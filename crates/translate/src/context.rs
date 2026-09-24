//! Dialogue and LLM context caching for contextual translation.
//!
//! Dialogue in games and narrative applications spans multiple consecutive lines or turns.
//! Translating each sentence in complete isolation causes lost gender agreements, fluctuating
//! character pronouns, and unnatural tone shifts.
//!
//! This module maintains a sliding window of recent conversational turns and scene lines, providing
//! contextual memory to translation engines and LLMs while invalidating automatically on scene
//! or window switches.

use serde::{Deserialize, Serialize};

/// One historical dialogue or UI utterance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextTurn {
    /// Optional speaker name or role identifier.
    pub speaker: Option<String>,
    /// Original source line.
    pub source: String,
    /// Translated line, if completed.
    pub translated: Option<String>,
}

/// A sliding context window for narrative continuity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DialogueContext {
    /// Identifier for the current scene, window or application.
    pub scope: String,
    /// Maximum turns retained in memory.
    pub max_turns: usize,
    /// History of recent dialogue turns (newest at the back).
    pub turns: Vec<ContextTurn>,
}

impl DialogueContext {
    /// Creates a new dialogue context for the specified application or scene scope.
    pub fn new(scope: impl Into<String>, max_turns: usize) -> Self {
        Self {
            scope: scope.into(),
            max_turns: max_turns.max(2),
            turns: Vec::new(),
        }
    }

    /// Default context holding up to 8 turns.
    pub fn standard(scope: impl Into<String>) -> Self {
        Self::new(scope, 8)
    }

    /// Adds a turn to context history, discarding oldest if over capacity.
    pub fn push(&mut self, speaker: Option<&str>, source: &str, translated: Option<&str>) {
        let src_norm = crate::token::normalize(source);
        if src_norm.is_empty() {
            return;
        }

        if self.turns.len() >= self.max_turns {
            self.turns.remove(0);
        }

        self.turns.push(ContextTurn {
            speaker: speaker.map(|s| s.trim().to_owned()),
            source: src_norm,
            translated: translated.map(crate::token::normalize),
        });
    }

    /// Updates the translation for the most recent matching source turn.
    pub fn complete_last(&mut self, source: &str, translated: &str) {
        let src_norm = crate::token::normalize(source);
        if let Some(turn) = self.turns.iter_mut().rev().find(|t| t.source == src_norm) {
            turn.translated = Some(crate::token::normalize(translated));
        }
    }

    /// Formats recent turns into a context block suitable for prompt injection or neural conditioning.
    pub fn format_prompt_context(&self) -> String {
        if self.turns.is_empty() {
            return String::new();
        }

        let mut lines = Vec::with_capacity(self.turns.len());
        for turn in &self.turns {
            let spk = match &turn.speaker {
                Some(name) => format!("{name}: "),
                None => String::new(),
            };
            if let Some(trans) = &turn.translated {
                lines.push(format!("{}{}", spk, trans));
            } else {
                lines.push(format!("{}{}", spk, turn.source));
            }
        }
        lines.join("\n")
    }

    /// Clears context history when switching scenes or applications.
    pub fn reset(&mut self, new_scope: impl Into<String>) {
        self.scope = new_scope.into();
        self.turns.clear();
    }

    /// Total active turns currently in memory.
    pub fn len(&self) -> usize {
        self.turns.len()
    }

    /// Whether context is empty.
    pub fn is_empty(&self) -> bool {
        self.turns.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_turns_and_formats_prompt() {
        let mut ctx = DialogueContext::standard("game-rpg");
        let intro = "You enter the dark dungeon.";
        let intro_ru = "Вы входите в тёмное подземелье.";
        ctx.push(Some("Narrator"), intro, Some(intro_ru));
        ctx.push(Some("Guard"), "Halt! Who goes there?", None);

        assert_eq!(ctx.len(), 2);
        let prompt = ctx.format_prompt_context();
        assert!(prompt.contains("Narrator: Вы входите в тёмное подземелье."));
        assert!(prompt.contains("Guard: Halt! Who goes there?"));

        ctx.complete_last("Halt! Who goes there?", "Стой! Кто идёт?");
        let updated = ctx.format_prompt_context();
        assert!(updated.contains("Guard: Стой! Кто идёт?"));
    }

    #[test]
    fn discards_oldest_when_exceeding_max_turns() {
        let mut ctx = DialogueContext::new("chat", 3);
        ctx.push(None, "Line 1", None);
        ctx.push(None, "Line 2", None);
        ctx.push(None, "Line 3", None);
        ctx.push(None, "Line 4", None);

        assert_eq!(ctx.len(), 3);
        assert_eq!(ctx.turns[0].source, "Line 2");
        assert_eq!(ctx.turns[2].source, "Line 4");
    }

    #[test]
    fn a_blank_turn_is_not_recorded() {
        let mut ctx = DialogueContext::new("scene", 4);
        ctx.push(Some("   "), "   ", Some("  "));
        assert!(ctx.is_empty());
    }

    #[test]
    fn resets_on_scope_change() {
        let mut ctx = DialogueContext::standard("app-1");
        ctx.push(None, "Hello", None);
        assert_eq!(ctx.len(), 1);

        ctx.reset("app-2");
        assert_eq!(ctx.len(), 0);
        assert_eq!(ctx.scope, "app-2");
    }
}
