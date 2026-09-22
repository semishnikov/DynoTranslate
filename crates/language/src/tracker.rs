//! Holding the answer steady for one window.
//!
//! Identification runs on whatever a frame happens to contain, so on its own it flickers: a
//! loading screen of numbers, one short button, a frame caught mid-animation, and suddenly the
//! window is a different language. Translation cannot follow that, and neither can a person
//! reading the overlay.
//!
//! The tracker keeps the language a window was last known to have, and only gives it up when a
//! rival turns up again and again and with more evidence behind it. Switching too slowly is a
//! visible delay once, at the moment a language really changes; switching too quickly is a broken
//! overlay for as long as the screen is busy.

use std::collections::hash_map::Entry;
use std::collections::HashMap;

use crate::{Language, LanguageGuess};

/// What one reading did to the language a window is being translated from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switch {
    /// The window already had this language, and it still does.
    Kept,
    /// The window had no language yet, so this one was taken on trust.
    Adopted,
    /// Enough evidence piled up for a different language, and the tracker moved.
    Changed,
}

/// How stubborn the tracker is.
#[derive(Debug, Clone, Copy)]
pub struct TrackerConfig {
    /// Consecutive readings of the same rival language before the tracker changes its mind.
    pub switch_after: usize,
    /// How much more confident the rival has to be. Without a margin, two languages trading
    /// readings back and forth would alternate forever.
    pub margin: f32,
    /// Guesses below this are not evidence and are ignored entirely.
    pub min_confidence: f32,
}

impl Default for TrackerConfig {
    fn default() -> Self {
        Self {
            switch_after: 3,
            margin: 0.15,
            min_confidence: 0.3,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct State {
    current: Language,
    /// How well the current language was last supported.
    confidence: f32,
    /// The language currently challenging it, if any.
    rival: Language,
    /// How many readings in a row backed the rival.
    streak: usize,
}

impl State {
    fn new(language: Language, confidence: f32) -> Self {
        Self {
            current: language,
            confidence,
            rival: language,
            streak: 0,
        }
    }
}

/// The language each window is being translated from, and how that decision is being held.
#[derive(Debug)]
pub struct Tracker {
    config: TrackerConfig,
    states: HashMap<u64, State>,
}

impl Tracker {
    pub fn new(config: TrackerConfig) -> Self {
        Self {
            config,
            states: HashMap::new(),
        }
    }

    pub fn with_default_config() -> Self {
        Self::new(TrackerConfig::default())
    }

    /// Feeds one reading for one window.
    ///
    /// The window is identified by the handle the capture layer already uses, so a window that
    /// closes and reopens starts again with no memory of what it said before.
    pub fn observe(&mut self, target: u64, guess: LanguageGuess) -> Switch {
        match self.states.entry(target) {
            Entry::Vacant(slot) => {
                slot.insert(State::new(guess.language, guess.confidence));
                Switch::Adopted
            }
            Entry::Occupied(slot) => settle(&self.config, slot.into_mut(), guess),
        }
    }

    /// The language the tracker has settled on, or nothing if the window has not been seen.
    pub fn language_of(&self, target: u64) -> Option<Language> {
        self.states.get(&target).map(|state| state.current)
    }

    /// Drops everything known about a window, for when it closes.
    pub fn forget(&mut self, target: u64) {
        self.states.remove(&target);
    }
}

fn settle(config: &TrackerConfig, state: &mut State, guess: LanguageGuess) -> Switch {
    if guess.language == Language::Unknown || guess.confidence < config.min_confidence {
        state.streak = 0;
        return Switch::Kept;
    }
    if state.current == Language::Unknown {
        // Hysteresis protects an answer that is already known. With none, waiting three frames
        // buys nothing and costs the first seconds of a session.
        state.current = guess.language;
        state.confidence = guess.confidence;
        state.rival = guess.language;
        state.streak = 0;
        return Switch::Changed;
    }
    if guess.language == state.current {
        state.confidence = guess.confidence;
        state.streak = 0;
        return Switch::Kept;
    }

    if guess.language == state.rival {
        state.streak += 1;
    } else {
        state.rival = guess.language;
        state.streak = 1;
    }

    let convinced = guess.confidence >= state.confidence + config.margin;
    if state.streak < config.switch_after || !convinced {
        return Switch::Kept;
    }

    state.current = guess.language;
    state.confidence = guess.confidence;
    state.streak = 0;
    Switch::Changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guess(language: Language, confidence: f32) -> LanguageGuess {
        LanguageGuess::new(language, confidence)
    }

    #[test]
    fn the_first_reading_is_taken_on_trust() {
        let mut tracker = Tracker::with_default_config();
        assert_eq!(tracker.observe(1, guess(Language::German, 0.4)), Switch::Adopted);
        assert_eq!(tracker.language_of(1), Some(Language::German));
    }

    #[test]
    fn one_odd_frame_does_not_move_the_session() {
        let mut tracker = Tracker::with_default_config();
        tracker.observe(1, guess(Language::German, 0.9));
        assert_eq!(tracker.observe(1, guess(Language::French, 0.95)), Switch::Kept);
        assert_eq!(tracker.language_of(1), Some(Language::German));
    }

    #[test]
    fn a_rival_has_to_repeat_itself_before_it_wins() {
        let mut tracker = Tracker::with_default_config();
        tracker.observe(1, guess(Language::German, 0.5));
        assert_eq!(tracker.observe(1, guess(Language::French, 0.9)), Switch::Kept);
        assert_eq!(tracker.observe(1, guess(Language::French, 0.9)), Switch::Kept);
        assert_eq!(tracker.observe(1, guess(Language::French, 0.9)), Switch::Changed);
        assert_eq!(tracker.language_of(1), Some(Language::French));
    }

    #[test]
    fn a_rival_that_is_not_stronger_never_wins() {
        let mut tracker = Tracker::with_default_config();
        tracker.observe(1, guess(Language::German, 0.9));
        for _ in 0..10 {
            assert_eq!(tracker.observe(1, guess(Language::French, 0.95)), Switch::Kept);
        }
        assert_eq!(tracker.language_of(1), Some(Language::German));
    }

    #[test]
    fn a_reading_of_the_current_language_resets_the_streak() {
        let mut tracker = Tracker::with_default_config();
        tracker.observe(1, guess(Language::German, 0.5));
        tracker.observe(1, guess(Language::French, 0.9));
        tracker.observe(1, guess(Language::French, 0.9));
        tracker.observe(1, guess(Language::German, 0.5));
        assert_eq!(tracker.observe(1, guess(Language::French, 0.9)), Switch::Kept);
        assert_eq!(tracker.language_of(1), Some(Language::German));
    }

    #[test]
    fn weak_readings_are_not_evidence() {
        let mut tracker = Tracker::with_default_config();
        tracker.observe(1, guess(Language::German, 0.9));
        for _ in 0..5 {
            assert_eq!(tracker.observe(1, guess(Language::French, 0.1)), Switch::Kept);
        }
        assert_eq!(tracker.language_of(1), Some(Language::German));
    }

    #[test]
    fn an_unknown_reading_is_ignored() {
        let mut tracker = Tracker::with_default_config();
        tracker.observe(1, guess(Language::German, 0.9));
        assert_eq!(tracker.observe(1, LanguageGuess::unknown()), Switch::Kept);
        assert_eq!(tracker.language_of(1), Some(Language::German));
    }

    #[test]
    fn a_window_with_no_language_takes_the_first_real_one_at_once() {
        let mut tracker = Tracker::with_default_config();
        tracker.observe(1, LanguageGuess::unknown());
        assert_eq!(tracker.observe(1, guess(Language::German, 0.5)), Switch::Changed);
        assert_eq!(tracker.language_of(1), Some(Language::German));
    }

    #[test]
    fn windows_are_tracked_separately() {
        let mut tracker = Tracker::with_default_config();
        tracker.observe(1, guess(Language::German, 0.9));
        tracker.observe(2, guess(Language::Japanese, 0.9));
        assert_eq!(tracker.language_of(1), Some(Language::German));
        assert_eq!(tracker.language_of(2), Some(Language::Japanese));
    }

    #[test]
    fn forgetting_a_window_starts_it_again() {
        let mut tracker = Tracker::with_default_config();
        tracker.observe(1, guess(Language::German, 0.9));
        tracker.forget(1);
        assert_eq!(tracker.language_of(1), None);
        assert_eq!(tracker.observe(1, guess(Language::French, 0.4)), Switch::Adopted);
    }
}
