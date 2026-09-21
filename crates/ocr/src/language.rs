//! Script-level language identification from Unicode blocks.
//!
//! Translation has to know which language a block of text is in before it can pick a model, but
//! the recognisers only report characters. [`identify`] answers with the language of the script
//! the text is written in — Latin is reported as English, Cyrillic as Russian, and so on — using
//! nothing but Unicode ranges, so the crate stays free of new dependencies on purpose. Telling
//! apart languages that share a script (English from German, Russian from Ukrainian) needs a real
//! identifier and is not attempted here.

/// The languages the identifier can name, plus [`LanguageId::Und`] for text it cannot classify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LanguageId {
    En,
    Ru,
    Ja,
    Zh,
    Ko,
    Ar,
    He,
    El,
    Und,
}

impl LanguageId {
    /// The BCP 47 primary language subtag, `"und"` for the unclassified case.
    pub const fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ru => "ru",
            Self::Ja => "ja",
            Self::Zh => "zh",
            Self::Ko => "ko",
            Self::Ar => "ar",
            Self::He => "he",
            Self::El => "el",
            Self::Und => "und",
        }
    }
}

/// The writing systems [`identify`] tells apart, in the order ties are broken in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Script {
    Latin,
    Cyrillic,
    Greek,
    Hebrew,
    Arabic,
    Kana,
    Han,
    Hangul,
}

impl Script {
    const fn index(self) -> usize {
        match self {
            Self::Latin => 0,
            Self::Cyrillic => 1,
            Self::Greek => 2,
            Self::Hebrew => 3,
            Self::Arabic => 4,
            Self::Kana => 5,
            Self::Han => 6,
            Self::Hangul => 7,
        }
    }

    const fn language(self) -> LanguageId {
        match self {
            Self::Latin => LanguageId::En,
            Self::Cyrillic => LanguageId::Ru,
            Self::Greek => LanguageId::El,
            Self::Hebrew => LanguageId::He,
            Self::Arabic => LanguageId::Ar,
            Self::Kana => LanguageId::Ja,
            Self::Han => LanguageId::Zh,
            Self::Hangul => LanguageId::Ko,
        }
    }
}

/// Names the language of `text` by the Unicode blocks its letters fall into.
///
/// Punctuation, digits and whitespace name nothing. Mixed text resolves by decisive scripts first —
/// any Hangul means Korean and any kana means Japanese, since both languages write with Han
/// characters as well — and otherwise by the largest letter count, with ties going to the earlier
/// script in [`Script`] declaration order. Text without a single letter is [`LanguageId::Und`].
pub fn identify(text: &str) -> LanguageId {
    let mut counts = [0usize; 8];
    for character in text.chars() {
        if let Some(script) = script_of(character) {
            counts[script.index()] += 1;
        }
    }
    if counts.iter().all(|count| *count == 0) {
        return LanguageId::Und;
    }
    let kana = counts[Script::Kana.index()];
    let hangul = counts[Script::Hangul.index()];
    if hangul > 0 && hangul >= kana {
        return LanguageId::Ko;
    }
    if kana > 0 {
        return LanguageId::Ja;
    }
    let mut best = Script::Latin;
    for script in [
        Script::Cyrillic,
        Script::Greek,
        Script::Hebrew,
        Script::Arabic,
        Script::Han,
    ] {
        if counts[script.index()] > counts[best.index()] {
            best = script;
        }
    }
    best.language()
}

/// The Unicode block a character is written in, or `None` for characters that name no language.
fn script_of(character: char) -> Option<Script> {
    let code = u32::from(character);
    Some(match code {
        0x0041..=0x005A | 0x0061..=0x007A | 0x00C0..=0x024F => Script::Latin,
        0x0400..=0x052F | 0x2DE0..=0x2DFF | 0xA640..=0xA69F => Script::Cyrillic,
        0x0370..=0x03FF | 0x1F00..=0x1FFF => Script::Greek,
        0x0590..=0x05FF | 0xFB1D..=0xFB4F => Script::Hebrew,
        0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => Script::Arabic,
        0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F => Script::Kana,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F => Script::Han,
        0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7FF => Script::Hangul,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_is_reported_as_english() {
        assert_eq!(identify("Hello, world!"), LanguageId::En);
    }

    #[test]
    fn cyrillic_is_reported_as_russian() {
        assert_eq!(identify("Привет, мир!"), LanguageId::Ru);
    }

    #[test]
    fn kana_marks_japanese_even_alongside_han() {
        assert_eq!(identify("こんにちは"), LanguageId::Ja);
        assert_eq!(identify("日本語のテキスト"), LanguageId::Ja);
        assert_eq!(identify("カタカナ"), LanguageId::Ja);
    }

    #[test]
    fn han_on_its_own_is_chinese() {
        assert_eq!(identify("漢字"), LanguageId::Zh);
    }

    #[test]
    fn hangul_marks_korean_even_alongside_han() {
        assert_eq!(identify("안녕하세요"), LanguageId::Ko);
        assert_eq!(identify("한국어 漢字"), LanguageId::Ko);
    }

    #[test]
    fn greek_hebrew_and_arabic_are_named() {
        assert_eq!(identify("Γεια σου"), LanguageId::El);
        assert_eq!(identify("שלום"), LanguageId::He);
        assert_eq!(identify("مرحبا"), LanguageId::Ar);
    }

    #[test]
    fn the_largest_script_wins_mixed_text() {
        assert_eq!(identify("OK Игра"), LanguageId::Ru);
        assert_eq!(identify("Игра OK"), LanguageId::Ru);
    }

    #[test]
    fn text_without_letters_is_unclassified() {
        assert_eq!(identify(""), LanguageId::Und);
        assert_eq!(identify("123 — ..."), LanguageId::Und);
    }

    #[test]
    fn codes_are_bcp_47_primary_subtags() {
        let expected = [
            (LanguageId::En, "en"),
            (LanguageId::Ru, "ru"),
            (LanguageId::Ja, "ja"),
            (LanguageId::Zh, "zh"),
            (LanguageId::Ko, "ko"),
            (LanguageId::Ar, "ar"),
            (LanguageId::He, "he"),
            (LanguageId::El, "el"),
            (LanguageId::Und, "und"),
        ];
        for (language, code) in expected {
            assert_eq!(language.code(), code);
        }
    }
}
