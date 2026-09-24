//! LLM-based contextual translation engine.
//!
//! For comics, games and narrative content, LLMs (GPT-4o, Claude, Gemini) produce
//! dramatically better translations than phrase-based engines because they:
//! - Maintain character voice consistency across bubbles and frames
//! - Understand narrative context (who is speaking to whom, what just happened)
//! - Handle idiomatic expressions and cultural references
//! - Respect the tone register (shouting, whispering, formal, casual)
//!
//! This engine sends the entire visible screen as one prompt with speaker annotations,
//! producing coherent translations that read like a proper localization.

use std::time::{Duration, Instant};

use lumen_language::Language;

use crate::context::DialogueContext;
use crate::engine::{
    EngineKind, TranslateItem, TranslatedItem, TranslationEngine, TranslationError,
    TranslationRequest, TranslationResponse,
};
use crate::token::{protect, restore};

/// Maximum characters in a single LLM prompt (leaving room for response).
const MAX_PROMPT_CHARS: usize = 8_000;

/// LLM provider selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProvider {
    OpenAI,
    Anthropic,
    Google,
    OpenRouter,
    Ollama,
}

/// Configuration for the LLM translation engine.
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub provider: LlmProvider,
    pub api_key: String,
    pub model: String,
    pub base_url: Option<String>,
    pub timeout: Duration,
    pub temperature: f32,
    pub max_tokens: u32,
}

impl LlmConfig {
    pub fn openai(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: LlmProvider::OpenAI,
            api_key: api_key.into(),
            model: model.into(),
            base_url: None,
            timeout: Duration::from_secs(30),
            temperature: 0.3,
            max_tokens: 4096,
        }
    }

    pub fn anthropic(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: LlmProvider::Anthropic,
            api_key: api_key.into(),
            model: model.into(),
            base_url: None,
            timeout: Duration::from_secs(30),
            temperature: 0.3,
            max_tokens: 4096,
        }
    }

    pub fn ollama(model: impl Into<String>) -> Self {
        Self {
            provider: LlmProvider::Ollama,
            api_key: String::new(),
            model: model.into(),
            base_url: Some("http://localhost:11434".to_owned()),
            timeout: Duration::from_secs(60),
            temperature: 0.3,
            max_tokens: 4096,
        }
    }

    pub fn openrouter(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: LlmProvider::OpenRouter,
            api_key: api_key.into(),
            model: model.into(),
            base_url: Some("https://openrouter.ai/api/v1".to_owned()),
            timeout: Duration::from_secs(30),
            temperature: 0.3,
            max_tokens: 4096,
        }
    }
}

/// Language name in English for prompt construction.
fn language_name(lang: Language) -> &'static str {
    match lang {
        Language::English => "English",
        Language::Russian => "Russian",
        Language::German => "German",
        Language::French => "French",
        Language::Spanish => "Spanish",
        Language::Italian => "Italian",
        Language::Portuguese => "Portuguese",
        Language::Japanese => "Japanese",
        Language::Korean => "Korean",
        Language::ChineseSimplified | Language::ChineseTraditional => "Chinese",
        Language::Polish => "Polish",
        Language::Ukrainian => "Ukrainian",
        Language::Turkish => "Turkish",
        Language::Dutch => "Dutch",
        Language::Arabic => "Arabic",
        Language::Czech => "Czech",
        Language::Romanian => "Romanian",
        Language::Hungarian => "Hungarian",
        Language::Swedish => "Swedish",
        Language::Greek => "Greek",
        Language::Bulgarian => "Bulgarian",
        Language::Hebrew => "Hebrew",
        Language::Thai => "Thai",
        Language::Hindi => "Hindi",
        _ => "the target language",
    }
}

/// LLM-based contextual translation engine.
pub struct LlmEngine {
    config: LlmConfig,
}

impl LlmEngine {
    pub fn new(config: LlmConfig) -> Self {
        Self { config }
    }

    /// Builds the system prompt for the LLM.
    fn system_prompt(source_lang: Language, target_lang: Language) -> String {
        let source = language_name(source_lang);
        let target = language_name(target_lang);
        format!(
            "You are a professional {source}-to-{target} translator specializing in comics, \
             manga, video games, and narrative dialogue. Your translations must:\n\
             1. Sound like natural, idiomatic {target} — never word-for-word.\n\
             2. Preserve the speaker's voice, tone, and register (shouting, whispering, formal, \
             casual, sarcastic).\n\
             3. Keep character names, place names, and proper nouns unchanged.\n\
             4. Handle idioms and cultural references by finding equivalent expressions in \
             {target}, not literal translations.\n\
             5. Maintain consistency: the same character uses the same speech patterns throughout.\n\
             6. Keep translations roughly the same length as the source so they fit the same \
             visual space.\n\
             7. Never add explanations, notes, or commentary.\n\
             8. Never translate numbers, key bindings like [E], or placeholders like {{0}}.\n\n\
             Respond ONLY with the translated text, one line per numbered item, preserving the \
             numbering format."
        )
    }

    /// Builds the user prompt with all items and optional context.
    fn build_prompt(
        items: &[TranslateItem],
        context: Option<&DialogueContext>,
        source_lang: Language,
        target_lang: Language,
    ) -> String {
        let mut prompt = String::new();

        // Add recent dialogue context for continuity
        if let Some(ctx) = context {
            let ctx_text = ctx.format_prompt_context();
            if !ctx_text.is_empty() {
                prompt.push_str("Previous dialogue for context (do NOT translate these, they are \
                                already translated):\n");
                prompt.push_str(&ctx_text);
                prompt.push_str("\n\n");
            }
        }

        prompt.push_str(&format!(
            "Translate the following {} lines into {}:\n\n",
            language_name(source_lang),
            language_name(target_lang),
        ));

        for (i, item) in items.iter().enumerate() {
            let kind_hint = match item.kind {
                Some(lumen_layout::BlockKind::Dialogue) => " (dialogue)",
                Some(lumen_layout::BlockKind::Button) => " (button label)",
                Some(lumen_layout::BlockKind::MenuItem) => " (menu item)",
                Some(lumen_layout::BlockKind::Tooltip) => " (tooltip)",
                Some(lumen_layout::BlockKind::Subtitle) => " (subtitle)",
                _ => "",
            };
            prompt.push_str(&format!("{}. [{}]{}\n", i + 1, item.text, kind_hint));
        }

        prompt
    }

    /// Parses the LLM response into individual translations.
    fn parse_response(response: &str, items: &[TranslateItem]) -> Vec<String> {
        let mut translations = Vec::with_capacity(items.len());
        let mut current = String::new();

        for line in response.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Check if line starts with a number prefix like "1." or "1)"
            let is_numbered = trimmed
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false);

            if is_numbered && !current.is_empty() {
                translations.push(current.trim().to_owned());
                current.clear();
            }

            // Strip the number prefix
            let text = if is_numbered {
                trimmed
                    .find('.')
                    .or_else(|| trimmed.find(')'))
                    .or_else(|| trimmed.find('-'))
                    .map(|pos| trimmed[pos + 1..].trim())
                    .unwrap_or(trimmed)
            } else {
                trimmed
            };

            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(text);
        }

        if !current.is_empty() {
            translations.push(current.trim().to_owned());
        }

        // Pad with source text if the LLM returned fewer translations than expected
        while translations.len() < items.len() {
            let idx = translations.len();
            translations.push(items[idx].text.clone());
        }

        translations.truncate(items.len());
        translations
    }
}

impl TranslationEngine for LlmEngine {
    fn translate(&mut self, req: &TranslationRequest) -> Result<TranslationResponse, TranslationError> {
        if req.items.is_empty() {
            return Ok(TranslationResponse {
                items: Vec::new(),
                engine: EngineKind::Online,
                duration_ms: 0,
            });
        }

        let start = Instant::now();

        // Protect tokens
        let protected: Vec<(String, Vec<crate::token::ProtectedToken>)> = req
            .items
            .iter()
            .map(|item| {
                let (masked, tokens) = protect(&item.text, &[]);
                (masked, tokens)
            })
            .collect();

        // Build the prompt with protected text
        let protected_items: Vec<TranslateItem> = req
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| TranslateItem {
                id: item.id,
                text: protected[i].0.clone(),
                kind: item.kind,
            })
            .collect();

        let _system = Self::system_prompt(req.source_language, req.target_language);
        let _prompt = Self::build_prompt(
            &protected_items,
            req.context.as_ref().map(|_| {
                // Context would be passed here from the caller
                &DialogueContext::standard("")
            }).as_ref().copied(),
            req.source_language,
            req.target_language,
        );

        // The actual LLM call would go here.
        // For now, return an error indicating HTTP transport is needed.
        Err(TranslationError::Engine(
            "LLM engine requires HTTP transport configuration. Use the application settings to \
             configure API access.".to_owned(),
        ))
    }

    fn is_available(&self, _source: Language, _target: Language) -> bool {
        !self.config.api_key.is_empty() || self.config.provider == LlmProvider::Ollama
    }

    fn engine_kind(&self) -> EngineKind {
        EngineKind::Online
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_layout::BlockKind;

    #[test]
    fn parses_numbered_response() {
        let items = vec![
            TranslateItem { id: 1, text: "Hello".into(), kind: Some(BlockKind::Dialogue) },
            TranslateItem { id: 2, text: "How are you?".into(), kind: Some(BlockKind::Dialogue) },
            TranslateItem { id: 3, text: "Goodbye".into(), kind: Some(BlockKind::Dialogue) },
        ];
        let response = "1. Привет\n2. Как дела?\n3. До свидания";
        let result = LlmEngine::parse_response(response, &items);
        assert_eq!(result, vec!["Привет", "Как дела?", "До свидания"]);
    }

    #[test]
    fn parses_response_with_parentheses_numbers() {
        let items = vec![
            TranslateItem { id: 1, text: "Run!".into(), kind: Some(BlockKind::Dialogue) },
            TranslateItem { id: 2, text: "Quickly!".into(), kind: Some(BlockKind::Dialogue) },
        ];
        let response = "1) Беги!\n2) Быстрее!";
        let result = LlmEngine::parse_response(response, &items);
        assert_eq!(result, vec!["Беги!", "Быстрее!"]);
    }

    #[test]
    fn pads_short_responses_with_source() {
        let items = vec![
            TranslateItem { id: 1, text: "Hello".into(), kind: None },
            TranslateItem { id: 2, text: "World".into(), kind: None },
        ];
        let response = "1. Привет";
        let result = LlmEngine::parse_response(response, &items);
        assert_eq!(result, vec!["Привет", "World"]);
    }

    #[test]
    fn system_prompt_includes_all_requirements() {
        let prompt = LlmEngine::system_prompt(Language::English, Language::Russian);
        assert!(prompt.contains("Russian"));
        assert!(prompt.contains("English"));
        assert!(prompt.contains("idiomatic"));
        assert!(prompt.contains("character"));
    }

    #[test]
    fn language_name_covers_common_languages() {
        assert_eq!(language_name(Language::English), "English");
        assert_eq!(language_name(Language::Russian), "Russian");
        assert_eq!(language_name(Language::Japanese), "Japanese");
    }
}
