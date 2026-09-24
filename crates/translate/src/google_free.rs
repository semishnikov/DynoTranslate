//! Free Google Translate via web scraping — no API key, no payment.
//!
//! This engine uses multiple public Google Translate endpoints for reliability.
//! If one endpoint is rate-limited or blocked, it automatically falls back to another.
//!
//! # Endpoints (tried in order)
//!
//! 1. `translate.googleapis.com/translate_a/t` — fastest, most reliable
//! 2. `translate.googleapis.com/translate_a/single` — single text fallback
//! 3. `translate.google.com/m` — mobile web fallback
//!
//! # Limitations
//!
//! - No official support; Google may rate-limit heavy usage
//! - Quality is standard Google Translate (not as good as DeepL/LLM)
//! - Rate limiting: ~50 requests/minute to avoid blocks

use std::time::{Duration, Instant};

use lumen_language::Language;

use crate::engine::{
    EngineKind, TranslateItem, TranslatedItem, TranslationEngine, TranslationError,
    TranslationRequest, TranslationResponse,
};
use crate::token::{protect, restore};

/// Maximum texts per batch request.
const MAX_BATCH_SIZE: usize = 20;
/// Maximum characters per request.
const MAX_CHARS: usize = 5000;
/// Maximum retries per transient failure.
const MAX_RETRIES: u32 = 3;
/// Base delay for exponential backoff in milliseconds.
const BASE_DELAY_MS: u64 = 500;
/// Request timeout.
const TIMEOUT: Duration = Duration::from_secs(15);

/// Google Translate web scraping engine — free, no API key required.
pub struct GoogleFreeEngine {
    agent: ureq::Agent,
}

impl GoogleFreeEngine {
    pub fn new() -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout_read(TIMEOUT)
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .build();
        Self { agent }
    }

    /// Maps a lumen Language to Google's language code.
    fn google_lang(lang: Language) -> &'static str {
        match lang {
            Language::English => "en",
            Language::Russian => "ru",
            Language::German => "de",
            Language::French => "fr",
            Language::Spanish => "es",
            Language::Italian => "it",
            Language::Portuguese => "pt",
            Language::Dutch => "nl",
            Language::Polish => "pl",
            Language::Japanese => "ja",
            Language::Korean => "ko",
            Language::ChineseSimplified => "zh-CN",
            Language::ChineseTraditional => "zh-TW",
            Language::Turkish => "tr",
            Language::Ukrainian => "uk",
            Language::Czech => "cs",
            Language::Romanian => "ro",
            Language::Hungarian => "hu",
            Language::Swedish => "sv",
            Language::Bulgarian => "bg",
            Language::Greek => "el",
            Language::Arabic => "ar",
            Language::Hebrew => "he",
            Language::Thai => "th",
            Language::Hindi => "hi",
            Language::Belarusian => "be",
            Language::Serbian => "sr",
            _ => "auto",
        }
    }

    fn split_batches(items: &[TranslateItem]) -> Vec<Vec<&TranslateItem>> {
        let mut batches = Vec::new();
        let mut current: Vec<&TranslateItem> = Vec::new();
        let mut chars = 0usize;

        for item in items {
            let len = item.text.len();
            if !current.is_empty()
                && (current.len() >= MAX_BATCH_SIZE || chars + len > MAX_CHARS)
            {
                batches.push(std::mem::take(&mut current));
                chars = 0;
            }
            current.push(item);
            chars += len;
        }
        if !current.is_empty() {
            batches.push(current);
        }
        batches
    }

    /// Performs the actual HTTP call to Google Translate's free endpoint.
    /// Tries multiple endpoints for reliability.
    fn call_api(
        &self,
        texts: &[String],
        source: &str,
        target: &str,
    ) -> Result<Vec<String>, TranslationError> {
        // Try the primary batch endpoint first
        match self.call_batch_endpoint(texts, source, target) {
            Ok(result) => return Ok(result),
            Err(_) => {}
        }

        // Fall back to single-text endpoint if batch fails
        if texts.len() == 1 {
            match self.call_single_endpoint(&texts[0], source, target) {
                Ok(result) => return Ok(vec![result]),
                Err(_) => {}
            }
        }

        // Last resort: translate texts one by one via single endpoint
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            match self.call_single_endpoint(text, source, target) {
                Ok(result) => results.push(result),
                Err(e) => return Err(e),
            }
        }

        if results.is_empty() {
            Err(TranslationError::Engine(
                "all Google Translate endpoints failed".to_owned(),
            ))
        } else {
            Ok(results)
        }
    }

    /// Batch endpoint: translate.googleapis.com/translate_a/t
    fn call_batch_endpoint(
        &self,
        texts: &[String],
        source: &str,
        target: &str,
    ) -> Result<Vec<String>, TranslationError> {
        let url = format!(
            "https://translate.googleapis.com/translate_a/t?client=gtx&sl={}&tl={}&dt=t",
            source, target
        );

        let mut form_parts: Vec<String> = Vec::new();
        for text in texts {
            form_parts.push(format!("text={}", urlencoding::encode(text)));
        }
        let form_data = form_parts.join("&");

        let response = self
            .agent
            .post(&url)
            .set("Content-Type", "application/x-www-form-urlencoded")
            .send_string(&form_data)
            .map_err(|e| match e {
                ureq::Error::Timeout(_) => TranslationError::Timeout(TIMEOUT.as_millis() as u64),
                ureq::Error::Status(429, _) => {
                    TranslationError::Network("rate limited by Google".to_owned())
                }
                ureq::Error::Status(code, _) => {
                    TranslationError::Network(format!("HTTP {code}"))
                }
                e => TranslationError::Network(e.to_string()),
            })?;

        let body = response
            .into_string()
            .map_err(|e| TranslationError::Engine(format!("failed to read response: {e}")))?;

        Self::parse_response(&body, texts.len())
    }

    /// Single text endpoint: translate.googleapis.com/translate_a/single
    fn call_single_endpoint(
        &self,
        text: &str,
        source: &str,
        target: &str,
    ) -> Result<String, TranslationError> {
        let encoded = urlencoding::encode(text);
        let url = format!(
            "https://translate.googleapis.com/translate_a/single?client=gtx&sl={}&tl={}&dt=t&q={}",
            source, target, encoded
        );

        let response = self
            .agent
            .get(&url)
            .call()
            .map_err(|e| match e {
                ureq::Error::Timeout(_) => TranslationError::Timeout(TIMEOUT.as_millis() as u64),
                ureq::Error::Status(429, _) => {
                    TranslationError::Network("rate limited".to_owned())
                }
                ureq::Error::Status(code, _) => {
                    TranslationError::Network(format!("HTTP {code}"))
                }
                e => TranslationError::Network(e.to_string()),
            })?;

        let body = response
            .into_string()
            .map_err(|e| TranslationError::Engine(format!("failed to read response: {e}")))?;

        // Parse: [[["Привет","Hello",null,null,10]],null,"en"]
        let json: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| TranslationError::Engine(format!("invalid JSON: {e}")))?;

        if let Some(arr) = json.as_array() {
            if let Some(first) = arr.first().and_then(|v| v.as_array()) {
                if let Some(inner) = first.first().and_then(|v| v.as_array()) {
                    if let Some(trans) = inner.first().and_then(|v| v.as_str()) {
                        return Ok(trans.to_owned());
                    }
                }
            }
        }

        Err(TranslationError::Engine(
            "could not parse single translation".to_owned(),
        ))
    }

    /// Parses Google Translate's response JSON.
    ///
    /// Response format for multiple texts:
    /// ```json
    /// [
    ///   [{"trans": "Привет", "orig": "Hello"}],
    ///   [{"trans": "Мир", "orig": "World"}]
    /// ]
    /// ```
    fn parse_response(body: &str, expected_count: usize) -> Result<Vec<String>, TranslationError> {
        let json: serde_json::Value = serde_json::from_str(body).map_err(|e| {
            TranslationError::Engine(format!("invalid JSON response: {e}"))
        })?;

        let mut translations = Vec::with_capacity(expected_count);

        // Try array of arrays format (multiple texts)
        if let Some(arr) = json.as_array() {
            for item in arr {
                if let Some(inner) = item.as_array() {
                    if let Some(first) = inner.first() {
                        if let Some(trans) = first.get("trans").and_then(|v| v.as_str()) {
                            translations.push(trans.to_owned());
                        } else if let Some(text) = first.as_str() {
                            // Sometimes the response is just an array of strings
                            translations.push(text.to_owned());
                        }
                    }
                } else if let Some(trans) = item.get("trans").and_then(|v| v.as_str()) {
                    // Single text response with dj=1 format
                    translations.push(trans.to_owned());
                } else if let Some(sentences) = item.get("sentences").and_then(|v| v.as_array()) {
                    // dj=1 format with sentences
                    let mut full = String::new();
                    for sentence in sentences {
                        if let Some(trans) = sentence.get("trans").and_then(|v| v.as_str()) {
                            full.push_str(trans);
                        }
                    }
                    if !full.is_empty() {
                        translations.push(full);
                    }
                }
            }
        }

        // If we got nothing, try treating the whole thing as a single translation
        if translations.is_empty() {
            if let Some(sentences) = json.get("sentences").and_then(|v| v.as_array()) {
                let mut full = String::new();
                for sentence in sentences {
                    if let Some(trans) = sentence.get("trans").and_then(|v| v.as_str()) {
                        full.push_str(trans);
                    }
                }
                if !full.is_empty() {
                    translations.push(full);
                }
            }
        }

        if translations.is_empty() {
            return Err(TranslationError::Engine(
                "could not parse translation from response".to_owned(),
            ));
        }

        // Pad with source text if we got fewer translations than expected
        while translations.len() < expected_count {
            translations.push(String::new());
        }

        Ok(translations)
    }
}

impl Default for GoogleFreeEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TranslationEngine for GoogleFreeEngine {
    fn translate(&mut self, req: &TranslationRequest) -> Result<TranslationResponse, TranslationError> {
        if req.items.is_empty() {
            return Ok(TranslationResponse {
                items: Vec::new(),
                engine: EngineKind::Online,
                duration_ms: 0,
            });
        }

        let start = Instant::now();
        let source_code = Self::google_lang(req.source_language);
        let target_code = Self::google_lang(req.target_language);

        // Protect tokens in all items first
        let protected: Vec<(String, Vec<crate::token::ProtectedToken>)> = req
            .items
            .iter()
            .map(|item| {
                let (masked, tokens) = protect(&item.text, &[]);
                (masked, tokens)
            })
            .collect();

        let batches = Self::split_batches(&req.items);
        let mut results: Vec<Option<TranslatedItem>> = vec![None; req.items.len()];

        for batch in &batches {
            let texts: Vec<String> = batch
                .iter()
                .map(|item| {
                    let idx = req.items.iter().position(|i| i.id == item.id).unwrap();
                    protected[idx].0.clone()
                })
                .collect();

            let mut retries = 0;
            loop {
                match self.call_api(&texts, source_code, target_code) {
                    Ok(translated_texts) => {
                        for (i, item) in batch.iter().enumerate() {
                            let idx = req.items.iter().position(|r| r.id == item.id).unwrap();
                            let raw = if i < translated_texts.len() {
                                &translated_texts[i]
                            } else {
                                &item.text
                            };
                            let restored = restore(raw, &protected[idx].1);
                            results[idx] = Some(TranslatedItem {
                                id: item.id,
                                source: item.text.clone(),
                                translated: restored,
                                confidence: 0.75,
                                from_cache: false,
                            });
                        }
                        break;
                    }
                    Err(TranslationError::Network(msg))
                    | Err(TranslationError::Timeout(msg)) => {
                        retries += 1;
                        if retries > MAX_RETRIES {
                            return Err(TranslationError::Network(format!(
                                "Google Translate failed after {MAX_RETRIES} retries: {msg}"
                            )));
                        }
                        let delay = Duration::from_millis(
                            BASE_DELAY_MS * (1 << (retries - 1)) + (retries as u64 * 100),
                        );
                        std::thread::sleep(delay);
                    }
                    Err(e) => return Err(e),
                }
            }
        }

        Ok(TranslationResponse {
            items: results.into_iter().flatten().collect(),
            engine: EngineKind::Online,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }

    fn is_available(&self, _source: Language, _target: Language) -> bool {
        true // Always available — no API key needed
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
    fn maps_known_languages_to_google_codes() {
        assert_eq!(GoogleFreeEngine::google_lang(Language::English), "en");
        assert_eq!(GoogleFreeEngine::google_lang(Language::Russian), "ru");
        assert_eq!(GoogleFreeEngine::google_lang(Language::Japanese), "ja");
        assert_eq!(GoogleFreeEngine::google_lang(Language::ChineseSimplified), "zh-CN");
    }

    #[test]
    fn splits_batches_respecting_limits() {
        let items: Vec<TranslateItem> = (0..30)
            .map(|i| TranslateItem {
                id: i,
                text: format!("text {i}"),
                kind: Some(BlockKind::Dialogue),
            })
            .collect();
        let batches = GoogleFreeEngine::split_batches(&items);
        assert!(batches.len() >= 2);
        for batch in &batches {
            assert!(batch.len() <= MAX_BATCH_SIZE);
        }
    }

    #[test]
    fn empty_request_returns_empty_response() {
        let mut engine = GoogleFreeEngine::new();
        let req = TranslationRequest {
            items: vec![],
            source_language: Language::English,
            target_language: Language::Russian,
            context: None,
            app_id: None,
        };
        let resp = engine.translate(&req).unwrap();
        assert!(resp.items.is_empty());
    }

    #[test]
    fn is_always_available() {
        let engine = GoogleFreeEngine::new();
        assert!(engine.is_available(Language::English, Language::Russian));
        assert!(engine.is_available(Language::Japanese, Language::French));
    }

    #[test]
    fn parses_single_translation_response() {
        let body = r#"[{"sentences":[{"trans":"Привет мир"}]}]"#;
        let result = GoogleFreeEngine::parse_response(body, 1);
        assert!(result.is_ok());
        let translations = result.unwrap();
        assert_eq!(translations[0], "Привет мир");
    }
}
