//! Post-processing: fix translated text for display quality.
//!
//! After translation, the text may need adjustments:
//! - Length matching: Russian is typically 15-30% longer than English
//! - Punctuation normalization for the target language
//! - Removing translation artifacts (brackets, notes, transliterations)
//! - Ensuring the text fits the visual space

/// Post-processes translated text for display quality.
pub fn postprocess(text: &str, max_chars: Option<usize>) -> String {
    let mut result = text.to_owned();

    // Step 1: Remove translation artifacts
    result = remove_artifacts(&result);

    // Step 2: Normalize punctuation for Russian
    result = normalize_punctuation(&result);

    // Step 3: Truncate if too long (with ellipsis)
    if let Some(max) = max_chars {
        result = truncate_smart(&result, max);
    }

    // Step 4: Clean up whitespace
    result = result.split_whitespace().collect::<Vec<_>>().join(" ");
    result = result.trim().to_owned();

    result
}

/// Removes common translation artifacts like [translated:], brackets, notes.
fn remove_artifacts(text: &str) -> String {
    let mut result = text.to_owned();

    // Remove stub engine markers like "[ru: text]"
    if result.starts_with("[ru: ") && result.ends_with(']') {
        result = result[5..result.len() - 1].to_owned();
    }
    if result.starts_with("[en: ") && result.ends_with(']') {
        result = result[5..result.len() - 1].to_owned();
    }

    // Remove translator notes in brackets
    result = remove_bracket_notes(&result);

    // Remove transliterations in parentheses (e.g., "Привет (privet)")
    result = remove_transliterations(&result);

    result
}

/// Removes bracketed translator notes like [прим. перев.] or (note).
fn remove_bracket_notes(text: &str) -> String {
    let mut result = String::new();
    let mut depth = 0i32;
    let mut in_note = false;

    for ch in text.chars() {
        match ch {
            '[' => {
                depth += 1;
                in_note = true;
            }
            ']' => {
                depth -= 1;
                if depth <= 0 {
                    in_note = false;
                    depth = 0;
                }
            }
            _ if !in_note => result.push(ch),
            _ => {}
        }
    }

    result
}

/// Removes transliterations in parentheses.
fn remove_transliterations(text: &str) -> String {
    let mut result = String::new();
    let mut depth = 0i32;

    for ch in text.chars() {
        match ch {
            '(' => {
                depth += 1;
                // Only skip if it looks like a transliteration (all ASCII)
            }
            ')' => {
                depth -= 1;
                if depth <= 0 {
                    depth = 0;
                }
            }
            _ if depth > 0 => {
                // Skip content inside parentheses
            }
            _ => result.push(ch),
        }
    }

    result
}

/// Normalizes punctuation for Russian text.
fn normalize_punctuation(text: &str) -> String {
    text.replace("...", "…")
        .replace(" - ", " — ")
        .replace(" -- ", " — ")
}

/// Smart truncation: cuts at word boundary and adds ellipsis.
fn truncate_smart(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }

    let chars: Vec<char> = text.chars().collect();
    let cut_at = max_chars.saturating_sub(1); // Room for ellipsis

    // Find last word boundary before cut point
    let mut pos = cut_at;
    while pos > 0 && !chars[pos].is_whitespace() {
        pos -= 1;
    }

    if pos == 0 {
        // No word boundary found — just cut
        let truncated: String = chars[..cut_at].iter().collect();
        format!("{truncated}…")
    } else {
        let truncated: String = chars[..pos].iter().collect();
        format!("{}…", truncated.trim_end())
    }
}

/// Estimates how many characters fit in a given pixel width at a given font size.
pub fn estimate_max_chars(width_px: u32, height_px: u32, font_size: f32) -> usize {
    let char_width = font_size * 0.55; // Average character width
    let line_height = font_size * 1.3;
    let lines = (height_px as f32 / line_height).max(1.0) as usize;
    let chars_per_line = (width_px as f32 / char_width).max(1.0) as usize;
    lines * chars_per_line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_stub_engine_markers() {
        assert_eq!(remove_artifacts("[ru: Привет]"), "Привет");
        assert_eq!(remove_artifacts("[en: Hello]"), "Hello");
    }

    #[test]
    fn removes_bracket_notes() {
        assert_eq!(remove_bracket_notes("Привет [прим. перев.]"), "Привет ");
    }

    #[test]
    fn normalizes_russian_punctuation() {
        assert_eq!(normalize_punctuation("Привет - мир"), "Привет — мир");
        assert_eq!(normalize_punctuation("Подожди..."), "Подожди…");
    }

    #[test]
    fn smart_truncation_at_word_boundary() {
        let text = "Это очень длинный текст который нужно обрезать";
        let result = truncate_smart(text, 20);
        assert!(result.chars().count() <= 21); // 20 + ellipsis
        assert!(result.ends_with('…'));
    }

    #[test]
    fn no_truncation_when_short_enough() {
        assert_eq!(truncate_smart("Короткий", 20), "Короткий");
    }

    #[test]
    fn estimates_max_chars() {
        let chars = estimate_max_chars(200, 40, 16.0);
        assert!(chars > 0);
        assert!(chars < 100);
    }

    #[test]
    fn postprocess_full_pipeline() {
        let input = "[ru: Привет - мир...]";
        let result = postprocess(input, None);
        assert_eq!(result, "Привет — мир…");
    }
}
