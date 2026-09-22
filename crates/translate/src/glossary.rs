//! Per-application and global glossaries.
//!
//! Games and applications have specialized terminology, character names, places, and UI jargon
//! that generic translation models mistranslate or render inconsistently. Glossaries enforce
//! exact substitutions before or after translation, and track a version number so translation
//! memory cache entries are invalidated when glossary terms change.

use serde::{Deserialize, Serialize};

/// One glossary mapping from a source phrase to a target phrase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlossaryEntry {
    /// The source term or phrase to match.
    pub source: String,
    /// The replacement target term. If empty or equal to source, term is kept as-is.
    pub target: String,
    /// Whether matching should be case-sensitive.
    pub case_sensitive: bool,
}

/// A collection of term mappings with version tracking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Glossary {
    /// Unique identifier (e.g. application bundle ID or "global").
    pub id: String,
    /// Monotonically increasing version counter, used in cache keys.
    pub version: u64,
    /// Registered term entries.
    pub entries: Vec<GlossaryEntry>,
}

impl Glossary {
    /// Creates an empty glossary with version 1.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            version: 1,
            entries: Vec::new(),
        }
    }

    /// Adds or updates a glossary term, incrementing the version.
    pub fn add(&mut self, source: &str, target: &str, case_sensitive: bool) {
        let src_norm = source.trim();
        if src_norm.is_empty() {
            return;
        }
        if let Some(existing) = self.entries.iter_mut().find(|e| {
            if case_sensitive || e.case_sensitive {
                e.source == src_norm
            } else {
                e.source.eq_ignore_ascii_case(src_norm)
            }
        }) {
            existing.target = target.trim().to_owned();
            existing.case_sensitive = case_sensitive;
        } else {
            self.entries.push(GlossaryEntry {
                source: src_norm.to_owned(),
                target: target.trim().to_owned(),
                case_sensitive,
            });
            // Keep entries sorted by length descending so longer terms match first
            self.entries.sort_by_key(|e| std::cmp::Reverse(e.source.len()));
        }
        self.version = self.version.wrapping_add(1);
    }

    /// Removes a term by source phrase, incrementing the version if removed.
    pub fn remove(&mut self, source: &str) -> bool {
        let src_norm = source.trim();
        let initial_len = self.entries.len();
        self.entries.retain(|e| !e.source.eq_ignore_ascii_case(src_norm));
        if self.entries.len() < initial_len {
            self.version = self.version.wrapping_add(1);
            true
        } else {
            false
        }
    }

    /// Returns list of source terms that should not be translated.
    pub fn do_not_translate(&self) -> Vec<&str> {
        let mut terms = Vec::new();
        for entry in &self.entries {
            if entry.target.is_empty() || entry.target == entry.source {
                terms.push(entry.source.as_str());
            }
        }
        terms
    }

    /// Applies glossary replacements directly to text.
    pub fn apply(&self, text: &str) -> String {
        let mut result = text.to_owned();
        for entry in &self.entries {
            if entry.target.is_empty() || entry.target == entry.source {
                continue;
            }
            result = replace_word_boundary(&result, &entry.source, &entry.target, entry.case_sensitive);
        }
        result
    }

    /// Total entries in the glossary.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the glossary contains zero entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Replaces whole words/terms respecting boundaries.
fn replace_word_boundary(text: &str, from: &str, to: &str, case_sensitive: bool) -> String {
    if text.is_empty() || from.is_empty() {
        return text.to_owned();
    }

    let mut result = String::with_capacity(text.len());
    let mut i = 0;
    let bytes = text.as_bytes();
    let text_len = bytes.len();
    let from_len = from.len();

    while i < text_len {
        let matches = if i + from_len <= text_len {
            let slice = &text[i..i + from_len];
            if case_sensitive {
                slice == from
            } else {
                slice.eq_ignore_ascii_case(from)
            }
        } else {
            false
        };

        if matches {
            // Check left boundary
            let left_ok = if i == 0 {
                true
            } else {
                !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_'
            };
            // Check right boundary
            let right_ok = if i + from_len == text_len {
                true
            } else {
                !bytes[i + from_len].is_ascii_alphanumeric() && bytes[i + from_len] != b'_'
            };

            if left_ok && right_ok {
                result.push_str(to);
                i += from_len;
                continue;
            }
        }

        // Copy character at i
        let ch = text[i..].chars().next().unwrap();
        result.push(ch);
        i += ch.len_utf8();
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_and_replaces_terms_at_word_boundaries() {
        let mut glossary = Glossary::new("game-test");
        glossary.add("Stamina", "Выносливость", false);
        glossary.add("HP", "Очки здоровья", true);

        let input = "Restore Stamina and HP! StaminaPotion does not match.";
        let output = glossary.apply(input);
        assert_eq!(output, "Restore Выносливость and Очки здоровья! StaminaPotion does not match.");
    }

    #[test]
    fn longer_phrases_take_precedence() {
        let mut glossary = Glossary::new("rpg");
        glossary.add("Health", "Здоровье", false);
        glossary.add("Health Potion", "Зелье здоровья", false);

        let input = "Drink a Health Potion, not raw Health.";
        let output = glossary.apply(input);
        assert_eq!(output, "Drink a Зелье здоровья, not raw Здоровье.");
    }

    #[test]
    fn do_not_translate_terms_extracted() {
        let mut glossary = Glossary::new("sci-fi");
        glossary.add("Cyberdeck", "", false);
        glossary.add("Militech", "Militech", false);
        glossary.add("Credits", "Кредиты", false);

        let dnt = glossary.do_not_translate();
        assert_eq!(dnt, vec!["Cyberdeck", "Militech"]);
    }

    #[test]
    fn version_increments_on_mutation() {
        let mut glossary = Glossary::new("app");
        assert_eq!(glossary.version, 1);
        glossary.add("Save", "Сохранить", false);
        assert_eq!(glossary.version, 2);
        glossary.remove("Save");
        assert_eq!(glossary.version, 3);
    }
}
