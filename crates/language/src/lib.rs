//! Which language the text on screen is written in.
//!
//! Translation has to know the source language before it can do anything, and it has to know it
//! from whatever a frame happens to contain: a four-word menu, a subtitle, a number in a corner.
//! There is no model here and no training data, so the identification is built out of what text
//! gives up for free — the script its characters come from, the letters only one language uses,
//! and the words that grammar and menus make unavoidable.
//!
//! That is enough to be certain about the script and about the languages with letters of their
//! own, and enough to lean the right way elsewhere. Where two languages share an alphabet and a
//! vocabulary, this settles for the one with more evidence and reports how thin that evidence was
//! through [`LanguageGuess::confidence`]. Deciding firmly on thin evidence is what
//! [`Tracker`] is for: it holds the answer steady per window until the evidence for a rival
//! repeats.

pub mod evidence;
pub mod script;
pub mod tracker;

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

pub use evidence::{cues, Cues};
pub use script::{dominant_script, script_of, Script};
pub use tracker::{Switch, Tracker, TrackerConfig};

/// Below this many letters there is nothing to identify. Two characters can be any language.
pub const MIN_CHARACTERS: usize = 3;

/// A letter only one supported language uses is worth this much.
const DISTINCTIVE_WEIGHT: f32 = 3.0;
/// A known word is worth this much. Words are weaker than letters: they repeat, and short ones
/// collide across languages.
const WORD_WEIGHT: f32 = 1.5;
/// The score at which a lead stops gaining confidence. Without it a single lucky word would look
/// as sure as a paragraph.
const SATURATION_SCORE: f32 = 6.0;
/// The floor on a returned confidence, so a guess is never reported as exactly nothing.
const MIN_CONFIDENCE: f32 = 0.1;
/// A script that carries one supported language needs no further evidence.
const SCRIPT_ONLY_CONFIDENCE: f32 = 0.85;
/// The script is known and the language is not. Translation can still run; it will pick.
const FALLBACK_CONFIDENCE: f32 = 0.2;

/// A language the pipeline can translate from.
///
/// [`Language::Unknown`] is part of the answer set on purpose: short text and text with no letters
/// in it are normal, and the caller has to be able to tell "no idea" from a guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    English,
    German,
    French,
    Spanish,
    Italian,
    Portuguese,
    Dutch,
    Swedish,
    Polish,
    Czech,
    Romanian,
    Hungarian,
    Turkish,
    Russian,
    Ukrainian,
    Belarusian,
    Bulgarian,
    Serbian,
    Greek,
    Japanese,
    ChineseSimplified,
    ChineseTraditional,
    Korean,
    Arabic,
    Hebrew,
    Thai,
    Hindi,
    Unknown,
}

impl Language {
    /// Every language in a fixed order, so iteration and tie-breaking never depend on a hash.
    pub fn all() -> &'static [Language] {
        &[
            Language::English,
            Language::German,
            Language::French,
            Language::Spanish,
            Language::Italian,
            Language::Portuguese,
            Language::Dutch,
            Language::Swedish,
            Language::Polish,
            Language::Czech,
            Language::Romanian,
            Language::Hungarian,
            Language::Turkish,
            Language::Russian,
            Language::Ukrainian,
            Language::Belarusian,
            Language::Bulgarian,
            Language::Serbian,
            Language::Greek,
            Language::Japanese,
            Language::ChineseSimplified,
            Language::ChineseTraditional,
            Language::Korean,
            Language::Arabic,
            Language::Hebrew,
            Language::Thai,
            Language::Hindi,
            Language::Unknown,
        ]
    }

    /// The languages a passage in this script could be written in.
    ///
    /// Han is empty: the two Chinese variants are told apart by their own characters in
    /// [`han_variant`], not by scoring, because a score over shared vocabulary cannot separate
    /// them.
    pub fn for_script(script: Script) -> &'static [Language] {
        match script {
            Script::Latin => &[
                Language::English,
                Language::German,
                Language::French,
                Language::Spanish,
                Language::Italian,
                Language::Portuguese,
                Language::Dutch,
                Language::Swedish,
                Language::Polish,
                Language::Czech,
                Language::Romanian,
                Language::Hungarian,
                Language::Turkish,
            ],
            Script::Cyrillic => &[
                Language::Russian,
                Language::Ukrainian,
                Language::Belarusian,
                Language::Bulgarian,
                Language::Serbian,
            ],
            Script::Greek => &[Language::Greek],
            Script::Hiragana | Script::Katakana => &[Language::Japanese],
            Script::Hangul => &[Language::Korean],
            Script::Arabic => &[Language::Arabic],
            Script::Hebrew => &[Language::Hebrew],
            Script::Thai => &[Language::Thai],
            Script::Devanagari => &[Language::Hindi],
            Script::Han => &[],
            Script::Neutral => &[],
        }
    }

    /// The BCP 47 tag and the English name, kept together so the two never drift apart.
    fn identity(self) -> (&'static str, &'static str) {
        match self {
            Language::English => ("en", "English"),
            Language::German => ("de", "German"),
            Language::French => ("fr", "French"),
            Language::Spanish => ("es", "Spanish"),
            Language::Italian => ("it", "Italian"),
            Language::Portuguese => ("pt", "Portuguese"),
            Language::Dutch => ("nl", "Dutch"),
            Language::Swedish => ("sv", "Swedish"),
            Language::Polish => ("pl", "Polish"),
            Language::Czech => ("cs", "Czech"),
            Language::Romanian => ("ro", "Romanian"),
            Language::Hungarian => ("hu", "Hungarian"),
            Language::Turkish => ("tr", "Turkish"),
            Language::Russian => ("ru", "Russian"),
            Language::Ukrainian => ("uk", "Ukrainian"),
            Language::Belarusian => ("be", "Belarusian"),
            Language::Bulgarian => ("bg", "Bulgarian"),
            Language::Serbian => ("sr", "Serbian"),
            Language::Greek => ("el", "Greek"),
            Language::Japanese => ("ja", "Japanese"),
            Language::ChineseSimplified => ("zh-Hans", "Chinese (simplified)"),
            Language::ChineseTraditional => ("zh-Hant", "Chinese (traditional)"),
            Language::Korean => ("ko", "Korean"),
            Language::Arabic => ("ar", "Arabic"),
            Language::Hebrew => ("he", "Hebrew"),
            Language::Thai => ("th", "Thai"),
            Language::Hindi => ("hi", "Hindi"),
            Language::Unknown => ("und", "Unknown"),
        }
    }

    /// The BCP 47 tag, which is what a translation engine wants.
    pub fn code(self) -> &'static str {
        self.identity().0
    }

    /// The English name, for interfaces and reports.
    pub fn name(self) -> &'static str {
        self.identity().1
    }

    /// The script this language is written in.
    pub fn script(self) -> Script {
        match self {
            Language::English => Script::Latin,
            Language::German => Script::Latin,
            Language::French => Script::Latin,
            Language::Spanish => Script::Latin,
            Language::Italian => Script::Latin,
            Language::Portuguese => Script::Latin,
            Language::Dutch => Script::Latin,
            Language::Swedish => Script::Latin,
            Language::Polish => Script::Latin,
            Language::Czech => Script::Latin,
            Language::Romanian => Script::Latin,
            Language::Hungarian => Script::Latin,
            Language::Turkish => Script::Latin,
            Language::Russian => Script::Cyrillic,
            Language::Ukrainian => Script::Cyrillic,
            Language::Belarusian => Script::Cyrillic,
            Language::Bulgarian => Script::Cyrillic,
            Language::Serbian => Script::Cyrillic,
            Language::Greek => Script::Greek,
            Language::Japanese => Script::Hiragana,
            Language::ChineseSimplified => Script::Han,
            Language::ChineseTraditional => Script::Han,
            Language::Korean => Script::Hangul,
            Language::Arabic => Script::Arabic,
            Language::Hebrew => Script::Hebrew,
            Language::Thai => Script::Thai,
            Language::Hindi => Script::Devanagari,
            Language::Unknown => Script::Neutral,
        }
    }
}

/// What one pass of identification concluded, and how much of the passage backs it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LanguageGuess {
    pub language: Language,
    /// Evidence behind the answer, 0..=1. A caller deciding whether to switch a session should
    /// treat anything below [`TrackerConfig::min_confidence`] as no answer at all.
    pub confidence: f32,
}

impl LanguageGuess {
    pub fn new(language: Language, confidence: f32) -> Self {
        Self { language, confidence }
    }

    pub fn unknown() -> Self {
        Self {
            language: Language::Unknown,
            confidence: 0.0,
        }
    }
}

/// The parts of a passage that identification looks at.
struct Sample {
    /// Letters only, so a screen full of digits and rules cannot look like a language.
    characters: Vec<char>,
    /// Lowercased words, split on anything that is not a letter or a digit.
    tokens: Vec<String>,
}

impl Sample {
    fn new(text: &str) -> Self {
        let characters: Vec<char> = text
            .chars()
            .filter(|character| script_of(*character) != Script::Neutral)
            .collect();
        let tokens: Vec<String> = text
            .split(|character: char| !character.is_alphanumeric())
            .filter(|token| !token.is_empty())
            .map(|token| token.to_lowercase())
            .collect();
        Self { characters, tokens }
    }
}

/// Identifies the language a passage is written in.
///
/// The answer is never an error: text that is too short, or has no letters in it, comes back as
/// [`Language::Unknown`], which is a normal outcome on a screen full of numbers.
pub fn identify(text: &str) -> LanguageGuess {
    let sample = Sample::new(text);
    if sample.characters.len() < MIN_CHARACTERS {
        return LanguageGuess::unknown();
    }
    let Some(script) = dominant_script(text) else {
        return LanguageGuess::unknown();
    };
    if script == Script::Han {
        return han_variant(&sample.characters);
    }

    let candidates = Language::for_script(script);
    if candidates.is_empty() {
        return LanguageGuess::unknown();
    }
    if candidates.len() == 1 {
        return LanguageGuess::new(candidates[0], SCRIPT_ONLY_CONFIDENCE);
    }

    let mut scores: Vec<(Language, f32)> = candidates.iter().map(|each| (*each, 0.0)).collect();
    for character in text.chars() {
        if let Some(owner) = owner_of_character(candidates, character) {
            add(&mut scores, owner, DISTINCTIVE_WEIGHT);
        }
    }
    for token in &sample.tokens {
        if let Some(owner) = owner_of_token(candidates, token) {
            add(&mut scores, owner, WORD_WEIGHT);
        }
    }
    scores.sort_by(|left, right| right.1.partial_cmp(&left.1).unwrap_or(Ordering::Equal));

    let (best, best_score) = scores[0];
    let runner_up = scores.get(1).map(|(_, score)| *score).unwrap_or(0.0);
    if best_score <= 0.0 {
        return LanguageGuess::new(fallback(script), FALLBACK_CONFIDENCE);
    }
    LanguageGuess::new(best, confidence_of(best_score, runner_up))
}

/// Places a Han passage between the two Chinese variants by the characters only one of them uses.
///
/// Japanese written entirely in kanji cannot be told from Chinese without a model, so an even
/// count returns the more common variant at [`FALLBACK_CONFIDENCE`] instead of a confident guess.
pub fn han_variant(characters: &[char]) -> LanguageGuess {
    let traditional = count_distinctive(characters, cues(Language::ChineseTraditional).distinctive);
    let simplified = count_distinctive(characters, cues(Language::ChineseSimplified).distinctive);
    if traditional == simplified {
        return LanguageGuess::new(Language::ChineseSimplified, FALLBACK_CONFIDENCE);
    }
    let variant = if traditional > simplified {
        Language::ChineseTraditional
    } else {
        Language::ChineseSimplified
    };
    LanguageGuess::new(variant, SCRIPT_ONLY_CONFIDENCE)
}

/// Turns a lead into a confidence: how far ahead the winner is, scaled by how much text says so.
fn confidence_of(best: f32, runner_up: f32) -> f32 {
    let margin = best / (best + runner_up);
    let saturation = (best / SATURATION_SCORE).min(1.0);
    (margin * saturation).max(MIN_CONFIDENCE)
}

fn owner_of_character(candidates: &[Language], character: char) -> Option<Language> {
    candidates
        .iter()
        .copied()
        .find(|language| cues(*language).distinctive.contains(character))
}

fn owner_of_token(candidates: &[Language], token: &str) -> Option<Language> {
    candidates
        .iter()
        .copied()
        .find(|language| has_word(cues(*language).words, token) || has_word(cues(*language).menu, token))
}

fn has_word(vocabulary: &str, token: &str) -> bool {
    vocabulary.split(' ').any(|word| word == token)
}

fn count_distinctive(characters: &[char], distinctive: &str) -> usize {
    characters
        .iter()
        .filter(|character| distinctive.contains(**character))
        .count()
}

fn add(scores: &mut [(Language, f32)], language: Language, weight: f32) {
    for (candidate, score) in scores.iter_mut() {
        if *candidate == language {
            *score += weight;
        }
    }
}

/// The language to name when the script is known and nothing else is.
fn fallback(script: Script) -> Language {
    match script {
        Script::Latin => Language::English,
        Script::Cyrillic => Language::Russian,
        Script::Greek => Language::Greek,
        Script::Hiragana | Script::Katakana => Language::Japanese,
        Script::Hangul => Language::Korean,
        Script::Arabic => Language::Arabic,
        Script::Hebrew => Language::Hebrew,
        Script::Thai => Language::Thai,
        Script::Devanagari => Language::Hindi,
        Script::Han => Language::ChineseSimplified,
        Script::Neutral => Language::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_agrees_with_its_own_script() {
        for language in Language::all().iter().copied() {
            if language == Language::Unknown {
                continue;
            }
            let candidates = Language::for_script(language.script());
            let known = candidates.contains(&language) || language.script() == Script::Han;
            assert!(known, "{language:?} is missing from its own script");
        }
    }

    #[test]
    fn codes_and_names_are_filled_in() {
        for language in Language::all().iter().copied() {
            assert!(!language.code().is_empty(), "{language:?}");
            assert!(!language.name().is_empty(), "{language:?}");
        }
        assert_eq!(Language::ChineseSimplified.code(), "zh-Hans");
        assert_eq!(Language::Unknown.code(), "und");
    }

    #[test]
    fn an_english_screen_is_english() {
        let guess = identify("The game will not start with this settings file");
        assert_eq!(guess.language, Language::English);
        assert!((guess.confidence - 1.0).abs() < f32::EPSILON, "{}", guess.confidence);
    }

    #[test]
    fn a_letter_only_one_language_uses_decides_it() {
        let guess = identify("Straße fortsetzen");
        assert_eq!(guess.language, Language::German);
        assert!((guess.confidence - 0.75).abs() < f32::EPSILON, "{}", guess.confidence);
    }

    #[test]
    fn punctuation_can_carry_the_signal_too() {
        let guess = identify("¿Dónde está el jugador?");
        assert_eq!(guess.language, Language::Spanish);
        assert!((guess.confidence - 0.75).abs() < f32::EPSILON, "{}", guess.confidence);
    }

    #[test]
    fn cyrillic_languages_are_told_apart_by_their_own_letters_and_menus() {
        assert_eq!(identify("Подешавања").language, Language::Serbian);
        assert_eq!(identify("Налаштування мови").language, Language::Ukrainian);
        assert_eq!(identify("Начать игру").language, Language::Russian);
    }

    #[test]
    fn a_script_with_one_language_needs_no_further_evidence() {
        let guess = identify("ゲームをはじめる");
        assert_eq!(guess.language, Language::Japanese);
        assert!((guess.confidence - 0.85).abs() < f32::EPSILON);

        assert_eq!(identify("게임 설정").language, Language::Korean);
        assert_eq!(identify("إعدادات اللعبة").language, Language::Arabic);
    }

    #[test]
    fn chinese_variants_are_split_by_their_characters() {
        assert_eq!(identify("开始新游戏").language, Language::ChineseSimplified);
        assert_eq!(identify("開始新遊戲").language, Language::ChineseTraditional);
    }

    #[test]
    fn han_with_nothing_to_go_on_is_a_weak_answer() {
        let guess = identify("新遊戲");
        assert_eq!(guess.language, Language::ChineseSimplified);
        assert!((guess.confidence - 0.2).abs() < f32::EPSILON);
    }

    #[test]
    fn latin_text_with_no_cues_falls_back_rather_than_guessing() {
        let guess = identify("Xyzzy plugh frobozz");
        assert_eq!(guess.language, Language::English);
        assert!((guess.confidence - 0.2).abs() < f32::EPSILON);
    }

    #[test]
    fn text_too_short_to_judge_says_so() {
        assert_eq!(identify("OK"), LanguageGuess::unknown());
        assert_eq!(identify(""), LanguageGuess::unknown());
    }

    #[test]
    fn a_screen_of_numbers_has_no_language() {
        let guess = identify("12:45  ·  100%  ·  3 / 7");
        assert_eq!(guess, LanguageGuess::unknown());
    }

    #[test]
    fn menu_words_are_evidence_too() {
        assert_eq!(identify("Ustawienia gry").language, Language::Polish);
        assert_eq!(identify("Načíst uložit").language, Language::Czech);
    }
}
