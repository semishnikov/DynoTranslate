//! The translation engine abstraction and scripted test double.
//!
//! All translation backends — offline INT8 models, cloud LLMs/APIs, and test doubles —
//! implement the unified [`TranslationEngine`] trait. Engines receive batch translation requests
//! along with optional conversational context, and return translated items with confidence scores.

use std::collections::HashMap;

use lumen_language::Language;
use lumen_layout::BlockKind;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::token::{protect, restore};

/// The category of translation engine that fulfilled a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineKind {
    /// Offline local INT8 model (e.g. CTranslate2 / OPUS-MT).
    Offline,
    /// Remote cloud engine or LLM.
    Online,
    /// Retrieved directly from the local translation memory.
    Memory,
    /// Scripted deterministic double for tests and benchmarks.
    Stub,
}

/// A specific text unit submitted for translation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslateItem {
    /// Unique identifier preserving correlation with the layout block.
    pub id: usize,
    /// Raw or normalized text to translate.
    pub text: String,
    /// Optional UI layout block classification (e.g. Button, Dialogue, Menu).
    pub kind: Option<BlockKind>,
}

/// A batch request submitted to a translation engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslationRequest {
    /// Text items to translate.
    pub items: Vec<TranslateItem>,
    /// Source language of the text.
    pub source_language: Language,
    /// Target language for output.
    pub target_language: Language,
    /// Optional dialogue or screen context.
    pub context: Option<String>,
    /// Optional application identifier for glossary lookup.
    pub app_id: Option<String>,
}

/// The result for one translated item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranslatedItem {
    /// Identifier matching the request item.
    pub id: usize,
    /// Original source text.
    pub source: String,
    /// Translated text with protected tokens restored.
    pub translated: String,
    /// Confidence score (0.0 to 1.0).
    pub confidence: f32,
    /// Whether this result came from cache.
    pub from_cache: bool,
}

/// The complete response from a translation engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranslationResponse {
    /// Translated items in original request order.
    pub items: Vec<TranslatedItem>,
    /// Which engine type produced the result.
    pub engine: EngineKind,
    /// Elapsed translation duration in milliseconds.
    pub duration_ms: u64,
}

/// Errors that can arise during translation.
#[derive(Debug, Error, PartialEq, Eq, Serialize, Deserialize)]
pub enum TranslationError {
    #[error("the language pair {0:?} -> {1:?} is not supported")]
    UnsupportedLanguagePair(Language, Language),
    #[error("model pack is missing for {0}")]
    ModelMissing(String),
    #[error("network request failed: {0}")]
    Network(String),
    #[error("translation timed out after {0} ms")]
    Timeout(u64),
    #[error("engine failure: {0}")]
    Engine(String),
}

/// Unified contract implemented by all translation engines.
pub trait TranslationEngine: Send + Sync {
    /// Translates a batch of items.
    fn translate(&mut self, req: &TranslationRequest) -> Result<TranslationResponse, TranslationError>;

    /// Whether this engine can translate between the given languages.
    fn is_available(&self, source: Language, target: Language) -> bool;

    /// The kind of engine.
    fn engine_kind(&self) -> EngineKind;
}

/// A deterministic scripted translation engine for CI tests, the headless pipeline, and benchmarks.
#[derive(Debug, Clone)]
pub struct StubTranslationEngine {
    dictionary: HashMap<String, String>,
    call_count: usize,
}

impl StubTranslationEngine {
    /// Creates a stub engine populated with standard game and interface UI terms.
    pub fn new() -> Self {
        let mut dict = HashMap::new();
        // Common interface dictionary
        dict.insert("New Game".to_owned(), "Новая игра".to_owned());
        dict.insert("Continue".to_owned(), "Продолжить".to_owned());
        dict.insert("Load Game".to_owned(), "Загрузить игру".to_owned());
        dict.insert("Save Game".to_owned(), "Сохранить игру".to_owned());
        dict.insert("Options".to_owned(), "Настройки".to_owned());
        dict.insert("Settings".to_owned(), "Параметры".to_owned());
        dict.insert("Quit".to_owned(), "Выход".to_owned());
        dict.insert("Exit".to_owned(), "Выход".to_owned());
        dict.insert("Back".to_owned(), "Назад".to_owned());
        dict.insert("Apply".to_owned(), "Применить".to_owned());
        dict.insert("Cancel".to_owned(), "Отмена".to_owned());
        dict.insert("Inventory".to_owned(), "Инвентарь".to_owned());
        dict.insert("Map".to_owned(), "Карта".to_owned());
        dict.insert("Quests".to_owned(), "Задания".to_owned());
        dict.insert("Level".to_owned(), "Уровень".to_owned());
        dict.insert("Health".to_owned(), "Здоровье".to_owned());
        dict.insert("Stamina".to_owned(), "Выносливость".to_owned());
        dict.insert("Mana".to_owned(), "Мана".to_owned());
        dict.insert("Attack".to_owned(), "Атака".to_owned());
        dict.insert("Defense".to_owned(), "Защита".to_owned());
        dict.insert("Score".to_owned(), "Счёт".to_owned());
        dict.insert("Press [E] to interact".to_owned(), "Нажмите [E] для взаимодействия".to_owned());

        Self {
            dictionary: dict,
            call_count: 0,
        }
    }

    /// Registers a custom dictionary translation.
    pub fn add_mapping(&mut self, source: impl Into<String>, target: impl Into<String>) {
        self.dictionary.insert(source.into(), target.into());
    }

    /// Total number of batch translations performed.
    pub fn call_count(&self) -> usize {
        self.call_count
    }
}

impl Default for StubTranslationEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TranslationEngine for StubTranslationEngine {
    fn translate(&mut self, req: &TranslationRequest) -> Result<TranslationResponse, TranslationError> {
        self.call_count += 1;
        let mut translated_items = Vec::with_capacity(req.items.len());

        for item in &req.items {
            let (masked, tokens) = protect(&item.text, &[]);

            // Check dictionary for whole masked string or unmasked text
            let translated_raw = if let Some(target) = self.dictionary.get(&item.text) {
                target.clone()
            } else if let Some(target) = self.dictionary.get(&masked) {
                target.clone()
            } else {
                // Algorithmic mock translation: preserve words and wrap in target tag
                format!("[{}: {}]", req.target_language.alpha2(), masked)
            };

            let final_text = restore(&translated_raw, &tokens);
            translated_items.push(TranslatedItem {
                id: item.id,
                source: item.text.clone(),
                translated: final_text,
                confidence: 0.98,
                from_cache: false,
            });
        }

        Ok(TranslationResponse {
            items: translated_items,
            engine: EngineKind::Stub,
            duration_ms: 2,
        })
    }

    fn is_available(&self, _source: Language, _target: Language) -> bool {
        true
    }

    fn engine_kind(&self) -> EngineKind {
        EngineKind::Stub
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_known_dictionary_terms_and_protects_tokens() {
        let mut engine = StubTranslationEngine::new();
        let request = TranslationRequest {
            items: vec![
                TranslateItem {
                    id: 1,
                    text: "New Game".to_owned(),
                    kind: Some(BlockKind::Button),
                },
                TranslateItem {
                    id: 2,
                    text: "Press [E] to interact".to_owned(),
                    kind: Some(BlockKind::Tooltip),
                },
                TranslateItem {
                    id: 3,
                    text: "Unknown Line 42".to_owned(),
                    kind: None,
                },
            ],
            source_language: Language::English,
            target_language: Language::Russian,
            context: None,
            app_id: None,
        };

        let response = engine.translate(&request).unwrap();
        assert_eq!(response.items.len(), 3);
        assert_eq!(response.items[0].translated, "Новая игра");
        assert_eq!(response.items[1].translated, "Нажмите [E] для взаимодействия");
        assert_eq!(response.items[2].translated, "[ru: Unknown Line 42]");
        assert_eq!(engine.call_count(), 1);
    }
}
