//! Token protection and text normalization for machine translation.
//!
//! Real application and game interfaces contain non-translatable tokens: numbers, key hints like `[E]`
//! or `Ctrl+S`, format placeholders like `{0}` or `%s`, markup tags like `<color=red>`, URLs, and
//! proper names. Passing them raw to an MT engine risks mangled punctuation, translated variable names,
//! or altered numbers.
//!
//! This module normalizes input text, masks non-translatable tokens with stable sentinel placeholders,
//! and restores them into the translated output.

use serde::{Deserialize, Serialize};

/// The category of a protected token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenKind {
    /// Numbers, currencies and percentages (e.g. `42`, `3.14`, `$9.99`, `100%`).
    Number,
    /// Keyboard and gamepad button hints (e.g. `[E]`, `[Space]`, `Ctrl+S`, `Alt+F4`).
    KeyHint,
    /// Interpolation placeholders (e.g. `{0}`, `{player}`, `%s`, `%d`, `%1$s`, `$VAR`).
    Placeholder,
    /// Formatting tags (e.g. `<b>`, `</b>`, `<color=#ff0000>`, `[b]`, `[/b]`).
    Tag,
    /// Web URLs and protocols (e.g. `https://example.com`, `http://...`).
    Url,
    /// Do-not-translate proper names or glossary terms.
    Literal,
}

/// A protected token extracted from source text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectedToken {
    /// 0-based index of this token in the sequence.
    pub id: usize,
    /// The original text of the token.
    pub original: String,
    /// The category of this token.
    pub kind: TokenKind,
    /// Byte offset start in the original normalized text.
    pub start: usize,
    /// Byte offset end in the original normalized text.
    pub end: usize,
}

/// Normalizes whitespace and trims edge spaces while preserving interior layout.
pub fn normalize(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut last_was_space = false;
    for ch in text.trim().chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                normalized.push(' ');
                last_was_space = true;
            }
        } else {
            normalized.push(ch);
            last_was_space = false;
        }
    }
    normalized
}

/// Identifies non-translatable tokens and returns a masked string along with extracted tokens.
///
/// Sentinels have the form `__T{id}__` (e.g. `__T0__`), which survive tokenization intact.
pub fn protect(text: &str, do_not_translate: &[&str]) -> (String, Vec<ProtectedToken>) {
    let norm = normalize(text);
    if norm.is_empty() {
        return (String::new(), Vec::new());
    }

    let mut spans: Vec<(usize, usize, TokenKind)> = Vec::new();

    // 1. Explicit do-not-translate literals
    for &literal in do_not_translate {
        let lit_trim = literal.trim();
        if lit_trim.is_empty() {
            continue;
        }
        let mut search_from = 0;
        while let Some(pos) = norm[search_from..].find(lit_trim) {
            let start = search_from + pos;
            let end = start + lit_trim.len();
            spans.push((start, end, TokenKind::Literal));
            search_from = end;
        }
    }

    // 2. URLs
    for prefix in &["https://", "http://"] {
        let mut search_from = 0;
        while let Some(pos) = norm[search_from..].find(prefix) {
            let start = search_from + pos;
            let end = norm[start..]
                .find(char::is_whitespace)
                .map(|p| start + p)
                .unwrap_or(norm.len());
            spans.push((start, end, TokenKind::Url));
            search_from = end;
        }
    }

    // 3. XML/HTML and BBCode tags: <...>
    for (start, _) in norm.match_indices('<') {
        if let Some(close) = norm[start..].find('>') {
            let end = start + close + 1;
            let inside = &norm[start + 1..end - 1];
            if is_tag_content(inside) {
                spans.push((start, end, TokenKind::Tag));
            }
        }
    }

    // 4. Placeholders: {0}, {name}, %s, %d, %1$s
    for (start, _) in norm.match_indices('{') {
        if let Some(close) = norm[start..].find('}') {
            let end = start + close + 1;
            let inside = &norm[start + 1..end - 1];
            if is_placeholder_identifier(inside) {
                spans.push((start, end, TokenKind::Placeholder));
            }
        }
    }

    for (start, _) in norm.match_indices('%') {
        let remainder = &norm[start + 1..];
        let mut j = 0;
        for ch in remainder.chars() {
            if ch.is_ascii_digit() || ch == '$' || ch == '.' {
                j += ch.len_utf8();
            } else if matches!(ch, 's' | 'd' | 'i' | 'f' | 'x' | 'X' | 'u') {
                j += ch.len_utf8();
                spans.push((start, start + 1 + j, TokenKind::Placeholder));
                break;
            } else {
                break;
            }
        }
    }

    // 5. Key hints: [E], [Space], [Ctrl+S]
    for (start, _) in norm.match_indices('[') {
        if let Some(close) = norm[start..].find(']') {
            let end = start + close + 1;
            let inside = &norm[start + 1..end - 1];
            if is_key_hint(inside) {
                spans.push((start, end, TokenKind::KeyHint));
            }
        }
    }

    // 6. Free-standing hotkey combos like "Ctrl+S" or "Alt+F4"
    for combo in &["ctrl+", "alt+", "shift+"] {
        let mut search_from = 0;
        let lower = norm.to_ascii_lowercase();
        while let Some(pos) = lower[search_from..].find(combo) {
            let start = search_from + pos;
            let combo_remainder = &norm[start + combo.len()..];
            let mut extra = 0;
            for ch in combo_remainder.chars() {
                if ch.is_ascii_alphanumeric() || ch == '+' {
                    extra += ch.len_utf8();
                } else {
                    break;
                }
            }
            if extra > 0 {
                let end = start + combo.len() + extra;
                spans.push((start, end, TokenKind::KeyHint));
                search_from = end;
            } else {
                search_from = start + combo.len();
            }
        }
    }

    // 7. Numbers: integers, decimals, percentages, currency prefixes
    let mut search_idx = 0;
    while search_idx < norm.len() {
        let remainder = &norm[search_idx..];
        let mut match_token = None;
        for (offset, ch) in remainder.char_indices() {
            if ch.is_ascii_digit() || ch == '$' || ch == '#' || ch == '€' || ch == '£' {
                match_token = Some((offset, ch));
                break;
            }
        }
        if let Some((offset, ch)) = match_token {
            let start = search_idx + offset;
            let num_remainder = &norm[start..];
            let mut num_len = 0;
            for c in num_remainder.chars() {
                if c.is_ascii_digit() || matches!(c, '.' | ',' | '%' | '$' | '#' | '€' | '£') {
                    num_len += c.len_utf8();
                } else {
                    break;
                }
            }
            let slice = &norm[start..start + num_len];
            if slice.chars().any(|c| c.is_ascii_digit()) {
                spans.push((start, start + num_len, TokenKind::Number));
                search_idx = start + num_len;
            } else {
                search_idx = start + ch.len_utf8();
            }
        } else {
            break;
        }
    }

    // Sort spans by start asc, length desc, and resolve overlaps (first wins)
    spans.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)));

    let mut non_overlapping: Vec<(usize, usize, TokenKind)> = Vec::new();
    let mut last_end = 0;
    for (start, end, kind) in spans {
        if start >= last_end && end <= norm.len() && start < end {
            non_overlapping.push((start, end, kind));
            last_end = end;
        }
    }

    let mut masked = String::with_capacity(norm.len());
    let mut tokens = Vec::with_capacity(non_overlapping.len());
    let mut cursor = 0;

    for (id, (start, end, kind)) in non_overlapping.into_iter().enumerate() {
        if start > cursor {
            masked.push_str(&norm[cursor..start]);
        }
        let original = norm[start..end].to_owned();
        let sentinel = format!("__T{id}__");
        masked.push_str(&sentinel);
        tokens.push(ProtectedToken {
            id,
            original,
            kind,
            start,
            end,
        });
        cursor = end;
    }

    if cursor < norm.len() {
        masked.push_str(&norm[cursor..]);
    }

    (masked, tokens)
}

/// Restores protected tokens into the translated string by replacing sentinel placeholders.
pub fn restore(translated: &str, tokens: &[ProtectedToken]) -> String {
    let mut result = translated.to_owned();
    for token in tokens {
        let sentinel = format!("__T{}__", token.id);
        result = result.replace(&sentinel, &token.original);
    }
    result
}

fn is_tag_content(inside: &str) -> bool {
    let t = inside.trim();
    if t.is_empty() {
        return false;
    }
    let trimmed = t.trim_start_matches('/');
    let tag_name = trimmed
        .split(|c: char| c.is_whitespace() || c == '=')
        .next()
        .unwrap_or("");
    matches!(
        tag_name.to_ascii_lowercase().as_str(),
        "b" | "i" | "u" | "s" | "color" | "size" | "font" | "align" | "alpha" | "link" | "pos"
    )
}

fn is_placeholder_identifier(inside: &str) -> bool {
    let t = inside.trim();
    if t.is_empty() {
        return false;
    }
    t.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == ':')
}

fn is_key_hint(inside: &str) -> bool {
    let t = inside.trim();
    if t.is_empty() || t.len() > 16 {
        return false;
    }
    if t.chars().count() == 1 {
        return true;
    }
    matches!(
        t.to_ascii_lowercase().as_str(),
        "space"
            | "tab"
            | "enter"
            | "esc"
            | "escape"
            | "shift"
            | "ctrl"
            | "control"
            | "alt"
            | "backspace"
            | "delete"
            | "up"
            | "down"
            | "left"
            | "right"
            | "f1"
            | "f2"
            | "f3"
            | "f4"
            | "f5"
            | "f6"
            | "f7"
            | "f8"
            | "f9"
            | "f10"
            | "f11"
            | "f12"
    ) || t.contains('+')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_whitespace_cleanly() {
        assert_eq!(normalize("  Hello \t world \r\n !  "), "Hello world !");
        assert_eq!(normalize(""), "");
    }

    #[test]
    fn protects_numbers_and_restores() {
        let (masked, tokens) = protect("Save 42 items for $19.99 (95%)", &[]);
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].original, "42");
        assert_eq!(tokens[0].kind, TokenKind::Number);
        assert_eq!(tokens[1].original, "$19.99");
        assert_eq!(tokens[2].original, "95%");
        assert_eq!(masked, "Save __T0__ items for __T1__ (__T2__)");

        let translated_mock = "Сохраните __T0__ предметов за __T1__ (__T2__)";
        let restored = restore(translated_mock, &tokens);
        assert_eq!(restored, "Сохраните 42 предметов за $19.99 (95%)");
    }

    #[test]
    fn protects_key_hints_and_hotkeys() {
        let (masked, tokens) = protect("Press [E] to interact or Ctrl+S to save", &[]);
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].original, "[E]");
        assert_eq!(tokens[0].kind, TokenKind::KeyHint);
        assert_eq!(tokens[1].original, "Ctrl+S");
        assert_eq!(tokens[1].kind, TokenKind::KeyHint);
        assert_eq!(masked, "Press __T0__ to interact or __T1__ to save");

        let translated = "Нажмите __T0__ для взаимодействия или __T1__ для сохранения";
        assert_eq!(
            restore(translated, &tokens),
            "Нажмите [E] для взаимодействия или Ctrl+S для сохранения"
        );
    }

    #[test]
    fn protects_placeholders_and_tags() {
        let (_masked, tokens) = protect("Hello <color=red>{player_name}</color>, level %d!", &[]);
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0].kind, TokenKind::Tag);
        assert_eq!(tokens[0].original, "<color=red>");
        assert_eq!(tokens[1].kind, TokenKind::Placeholder);
        assert_eq!(tokens[1].original, "{player_name}");
        assert_eq!(tokens[2].kind, TokenKind::Tag);
        assert_eq!(tokens[2].original, "</color>");
        assert_eq!(tokens[3].kind, TokenKind::Placeholder);
        assert_eq!(tokens[3].original, "%d");

        let translated = "Привет __T0____T1____T2__, уровень __T3__!";
        assert_eq!(
            restore(translated, &tokens),
            "Привет <color=red>{player_name}</color>, уровень %d!"
        );
    }

    #[test]
    fn protects_explicit_do_not_translate_literals() {
        let (_masked, tokens) = protect("Visit DynoTranslate at https://example.com", &["DynoTranslate"]);
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].original, "DynoTranslate");
        assert_eq!(tokens[0].kind, TokenKind::Literal);
        assert_eq!(tokens[1].original, "https://example.com");
        assert_eq!(tokens[1].kind, TokenKind::Url);

        let translated = "Посетите __T0__ на __T1__";
        assert_eq!(
            restore(translated, &tokens),
            "Посетите DynoTranslate на https://example.com"
        );
    }
}
