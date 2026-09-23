//! Translation backends beyond the bundled local model.
//!
//! The local MarianMT model is fast and offline but translates each line in isolation, which
//! is exactly why the owner saw disconnected Russian. The network backends exist to fix the
//! meaning: Google gives decent sentences with zero setup, DeepL is the quality reference,
//! and OpenAI receives the whole tick of lines plus the recent context, so names and tone stay
//! coherent across a screen. The live loop always falls back to the local model when a network
//! backend errors, so the overlay never goes silent.

use std::sync::OnceLock;
use std::time::Duration;

use crate::settings::LiveSettings;

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::builder()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(15))
            .build()
    })
}

/// `lines` in, translations out, same order and length — or an error the loop journals.
pub fn translate_batch(
    backend: &str,
    lines: &[String],
    context: &[(String, String)],
    settings: &LiveSettings,
) -> Result<Vec<String>, String> {
    match backend {
        "google" => lines.iter().map(google_one).collect(),
        "deepl" => deepl(lines, settings),
        "openai" => openai(lines, context, settings),
        other => Err(format!("unknown translator backend {other:?}")),
    }
}

fn url_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The keyless endpoint the open-source screen translators use; fine for short game lines.
fn google_one(line: &str) -> Result<String, String> {
    let url = format!(
        "https://translate.googleapis.com/translate_a/single?client=gtx&sl=en&tl=ru&dt=t&q={}",
        url_encode(line)
    );
    let body = agent()
        .get(&url)
        .call()
        .map_err(|error| format!("google: {error}"))?
        .into_string()
        .map_err(|error| format!("google body: {error}"))?;
    let value: serde_json::Value = serde_json::from_str(&body).map_err(|error| format!("google json: {error}"))?;
    let mut text = String::new();
    if let Some(segments) = value.get(0).and_then(|first| first.as_array()) {
        for segment in segments {
            if let Some(piece) = segment.get(0).and_then(|piece| piece.as_str()) {
                text.push_str(piece);
            }
        }
    }
    if text.trim().is_empty() {
        return Err("google returned nothing".to_string());
    }
    Ok(text)
}

fn deepl(lines: &[String], settings: &LiveSettings) -> Result<Vec<String>, String> {
    if settings.deepl_key.trim().is_empty() {
        return Err("deepl key is empty".to_string());
    }
    let mut form: Vec<(String, String)> = lines.iter().map(|line| ("text".to_string(), line.clone())).collect();
    form.push(("source_lang".to_string(), "EN".to_string()));
    form.push(("target_lang".to_string(), "RU".to_string()));
    let form_refs: Vec<(&str, &str)> = form.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect();
    let auth = format!("DeepL-Auth-Key {}", settings.deepl_key.trim());
    let mut last_error = String::new();
    for host in ["https://api-free.deepl.com", "https://api.deepl.com"] {
        match agent()
            .post(&format!("{host}/v2/translate"))
            .set("Authorization", &auth)
            .send_form(&form_refs)
        {
            Ok(response) => {
                let body = response.into_string().map_err(|error| format!("deepl body: {error}"))?;
                let value: serde_json::Value =
                    serde_json::from_str(&body).map_err(|error| format!("deepl json: {error}"))?;
                let translations = value
                    .get("translations")
                    .and_then(|item| item.as_array())
                    .ok_or_else(|| "deepl: no translations field".to_string())?;
                let mut out = Vec::new();
                for item in translations {
                    out.push(
                        item.get("text")
                            .and_then(|text| text.as_str())
                            .unwrap_or_default()
                            .to_string(),
                    );
                }
                if out.len() == lines.len() {
                    return Ok(out);
                }
                return Err(format!("deepl returned {} of {} lines", out.len(), lines.len()));
            }
            Err(error) => last_error = format!("deepl: {error}"),
        }
    }
    Err(last_error)
}

fn openai(lines: &[String], context: &[(String, String)], settings: &LiveSettings) -> Result<Vec<String>, String> {
    if settings.openai_key.trim().is_empty() {
        return Err("openai key is empty".to_string());
    }
    let mut user = String::new();
    if !context.is_empty() {
        user.push_str("CONTEXT (earlier lines of this session, for continuity):\n");
        for (source, target) in context {
            user.push_str(&format!("EN: {source}\nRU: {target}\n"));
        }
        user.push_str("\n");
    }
    user.push_str("LINES (one screen, one moment):\n");
    for (index, line) in lines.iter().enumerate() {
        user.push_str(&format!("{}. {line}\n", index + 1));
    }
    let body = serde_json::json!({
        "model": settings.openai_model,
        "temperature": 0.2,
        "messages": [
            {
                "role": "system",
                "content": "You are a professional translator of games, comics and articles from English to Russian. The numbered lines were captured from one screen at one moment; translate them as one coherent scene: consistent character names, terms and tone, natural living Russian that matches the register of the original. Earlier CONTEXT lines keep continuity with what was already shown. Respond with ONLY a JSON array of exactly the same number of Russian strings, in order, without numbering or commentary."
            },
            { "role": "user", "content": user }
        ]
    });
    let response = agent()
        .post("https://api.openai.com/v1/chat/completions")
        .set("Authorization", &format!("Bearer {}", settings.openai_key.trim()))
        .set("Content-Type", "application/json")
        .send_string(body.to_string())
        .map_err(|error| format!("openai: {error}"))?;
    let body = response
        .into_string()
        .map_err(|error| format!("openai body: {error}"))?;
    let value: serde_json::Value = serde_json::from_str(&body).map_err(|error| format!("openai json: {error}"))?;
    let content = value["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| "openai: no content".to_string())?;
    let cleaned = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let parsed: serde_json::Value =
        serde_json::from_str(cleaned).map_err(|error| format!("openai answer json: {error}"))?;
    let array = match &parsed {
        serde_json::Value::Array(items) => Some(items.clone()),
        serde_json::Value::Object(map) => map.values().find_map(|item| item.as_array().cloned()),
        _ => None,
    }
    .ok_or_else(|| "openai answer is not an array".to_string())?;
    let mut out = Vec::new();
    for item in &array {
        out.push(item.as_str().unwrap_or_default().to_string());
    }
    if out.len() != lines.len() {
        return Err(format!("openai returned {} of {} lines", out.len(), lines.len()));
    }
    Ok(out)
}
