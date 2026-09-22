//! Which writing system a character belongs to, and which one a passage is written in.
//!
//! Script is the part of language identification that can be done without a model and without
//! training data: the code point says so. It narrows a passage to a handful of candidates, and it
//! is what makes the rest of the problem small.

use serde::{Deserialize, Serialize};

/// A writing system, or [`Script::Neutral`] for everything that carries no language signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Script {
    Latin,
    Cyrillic,
    Greek,
    Han,
    Hiragana,
    Katakana,
    Hangul,
    Arabic,
    Hebrew,
    Thai,
    Devanagari,
    /// Digits, punctuation and symbols. They appear in every language and identify none.
    Neutral,
}

impl Script {
    /// Whether a passage in this script still has to be narrowed down to a language.
    ///
    /// False for the scripts that carry a single supported language, where the script is the
    /// answer and no orthographic evidence can improve on it.
    pub fn is_ambiguous(self) -> bool {
        matches!(self, Script::Latin | Script::Cyrillic | Script::Han)
    }
}

/// The script a character belongs to.
///
/// The ranges are the blocks that matter for screen text rather than the whole of Unicode; a
/// character outside them is neutral, which is the honest answer and costs nothing downstream.
pub fn script_of(character: char) -> Script {
    match character as u32 {
        0x0041..=0x005A | 0x0061..=0x007A | 0x00C0..=0x00D6 | 0x00D8..=0x00F6 | 0x00F8..=0x024F => Script::Latin,
        0x0370..=0x03FF | 0x1F00..=0x1FFF => Script::Greek,
        0x0400..=0x052F => Script::Cyrillic,
        0x0590..=0x05FF => Script::Hebrew,
        0x0600..=0x06FF | 0x0750..=0x077F => Script::Arabic,
        0x0900..=0x097F => Script::Devanagari,
        0x0E00..=0x0E7F => Script::Thai,
        0x1100..=0x11FF | 0xAC00..=0xD7AF => Script::Hangul,
        0x3040..=0x309F => Script::Hiragana,
        0x30A0..=0x30FF => Script::Katakana,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF => Script::Han,
        _ => Script::Neutral,
    }
}

/// The script most of the letters in a passage are written in, ignoring digits and punctuation.
///
/// Returns nothing when the passage has no letters at all. Ties go to the script that appeared
/// first, so the same input always gives the same answer.
pub fn dominant_script(text: &str) -> Option<Script> {
    let mut seen: Vec<(Script, usize)> = Vec::new();
    for character in text.chars() {
        let script = script_of(character);
        if script == Script::Neutral {
            continue;
        }
        match seen.iter_mut().find(|(known, _)| *known == script) {
            Some((_, count)) => *count += 1,
            None => seen.push((script, 1)),
        }
    }

    let mut strongest: Option<(Script, usize)> = None;
    for entry in seen {
        let dominated = strongest.is_some_and(|(_, best)| entry.1 <= best);
        if !dominated {
            strongest = Some(entry);
        }
    }
    strongest.map(|(script, _)| script)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_are_placed_by_their_block() {
        assert_eq!(script_of('a'), Script::Latin);
        assert_eq!(script_of('ö'), Script::Latin);
        assert_eq!(script_of('щ'), Script::Cyrillic);
        assert_eq!(script_of('あ'), Script::Hiragana);
        assert_eq!(script_of('ク'), Script::Katakana);
        assert_eq!(script_of('한'), Script::Hangul);
        assert_eq!(script_of('設'), Script::Han);
        assert_eq!(script_of('م'), Script::Arabic);
        assert_eq!(script_of('ש'), Script::Hebrew);
        assert_eq!(script_of('ก'), Script::Thai);
        assert_eq!(script_of('क'), Script::Devanagari);
        assert_eq!(script_of('Ω'), Script::Greek);
    }

    #[test]
    fn digits_and_punctuation_carry_no_signal() {
        assert_eq!(script_of('7'), Script::Neutral);
        assert_eq!(script_of('!'), Script::Neutral);
        assert_eq!(script_of(' '), Script::Neutral);
        assert_eq!(script_of('×'), Script::Neutral, "a symbol, not a letter");
    }

    #[test]
    fn the_dominant_script_is_the_one_with_most_letters() {
        assert_eq!(dominant_script("Настройки 4K"), Some(Script::Cyrillic));
        assert_eq!(dominant_script("Continue (2 players)"), Some(Script::Latin));
        assert_eq!(dominant_script("ゲームをはじめます"), Some(Script::Hiragana));
    }

    #[test]
    fn a_tie_goes_to_the_script_that_came_first() {
        assert_eq!(dominant_script("abАБ"), Some(Script::Latin));
    }

    #[test]
    fn text_without_letters_has_no_script() {
        assert_eq!(dominant_script("12:45 — 100%"), None);
        assert_eq!(dominant_script(""), None);
    }

    #[test]
    fn only_the_shared_scripts_are_ambiguous() {
        assert!(Script::Latin.is_ambiguous());
        assert!(Script::Cyrillic.is_ambiguous());
        assert!(Script::Han.is_ambiguous());
        assert!(!Script::Hiragana.is_ambiguous());
        assert!(!Script::Hangul.is_ambiguous());
    }
}
