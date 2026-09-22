//! Temporal stability for recognised blocks.
//!
//! Recognition is noisy: a menu entry jitters a pixel, an `и` arrives as an `n`, a subtitle
//! blinks for one frame. Drawing every observation would flicker and re-translate constantly.
//! This stage sits between layout and translation and holds each block steady:
//!
//! - blocks are matched across frames by intersection-over-union;
//! - a new reading only replaces the agreed one after [`StabilityConfig::agree_frames`] frames
//!   of the same answer, so a single bad OCR frame cannot rewrite what is on screen;
//! - a translation attached to an agreed reading is reused for as long as that reading stands,
//!   which is what stops the engine from being called on every jitter;
//! - numeric-only text is marked untranslatable, because a health bar reading `87 / 100` must
//!   never be rewritten.

use lumen_core::Rect;
use serde::{Deserialize, Serialize};

/// Thresholds that decide when a reading is steady enough to show.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StabilityConfig {
    /// Intersection-over-union at which two boxes count as the same block.
    pub match_iou: f32,
    /// Consecutive frames a candidate reading must repeat before it replaces the agreed one.
    /// The first reading of a brand-new block is accepted immediately: there is nothing to
    /// protect yet, and a fresh tooltip should appear at once.
    pub agree_frames: usize,
    /// Misses tolerated before a track is forgotten.
    pub forget_misses: usize,
}

impl Default for StabilityConfig {
    fn default() -> Self {
        Self {
            match_iou: 0.3,
            agree_frames: 2,
            forget_misses: 3,
        }
    }
}

/// One block as layout reported it this frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub rect: Rect,
    pub text: String,
    pub confidence: f32,
}

/// A track after this frame's observations have been folded in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StableBlock {
    /// Latest matched box, so a tooltip that drifts a pixel follows the source.
    pub rect: Rect,
    /// The reading every agree_frames vote has settled on.
    pub source_text: String,
    /// Whether the source is digits and punctuation only; never translate those.
    pub numeric_only: bool,
    /// A candidate that has not yet reached the agreement threshold.
    pub candidate: Option<String>,
    /// Frames the current candidate has been seen running.
    pub candidate_votes: usize,
    /// Translation of `source_text`, filled by the caller once and reused until the source
    /// text changes.
    pub translation: Option<String>,
    /// Which `source_text` `translation` belongs to. A stale translation is never drawn.
    pub translated_source: Option<String>,
    /// Frames since this track last matched an observation.
    pub misses: usize,
    /// Lowest confidence among the recent observations, for the plate fallback.
    pub confidence: f32,
}

impl StableBlock {
    /// The text to draw: the translation when it is current, the source otherwise.
    pub fn display_text(&self) -> &str {
        match (&self.translation, &self.translated_source) {
            (Some(text), Some(source)) if *source == self.source_text => text,
            _ => &self.source_text,
        }
    }

    /// Whether an engine should be asked for a translation this frame.
    pub fn needs_translation(&self) -> bool {
        !self.numeric_only && self.translated_source.as_deref() != Some(self.source_text.as_str())
    }
}

/// Whether a reading is only numbers, separators and key hints that carry no prose.
pub fn is_numeric_only(text: &str) -> bool {
    let mut saw_digit = false;
    for ch in text.chars() {
        if ch.is_ascii_digit() || ch.is_numeric() {
            saw_digit = true;
            continue;
        }
        if ch.is_whitespace() || matches!(ch, '.' | ',' | ':' | '/' | '%' | '-' | '+' | '×' | 'x') {
            continue;
        }
        return false;
    }
    saw_digit
}

#[derive(Debug, Default)]
pub struct StabilityTracker {
    tracks: Vec<StableBlock>,
}

impl StabilityTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Folds this frame's observations into the tracks and returns the blocks to draw.
    pub fn observe(&mut self, observations: &[Observation], config: &StabilityConfig) -> Vec<StableBlock> {
        let mut matched = vec![false; self.tracks.len()];

        // Greedy best-IoU matching: observations are few (a screenful of UI), so the quadratic
        // pass is cheaper than the bookkeeping a finer algorithm would need.
        let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
        for (ti, track) in self.tracks.iter().enumerate() {
            for (oi, observation) in observations.iter().enumerate() {
                let iou = track.rect.iou(&observation.rect);
                if iou >= config.match_iou {
                    pairs.push((iou, ti, oi));
                }
            }
        }
        pairs.sort_by(|a, b| b.0.partial_cmp(&a.0).expect("iou is ordered"));
        let mut taken = vec![false; observations.len()];
        for (_, ti, oi) in pairs {
            if matched[ti] || taken[oi] {
                continue;
            }
            matched[ti] = true;
            taken[oi] = true;
            self.fold(ti, &observations[oi], config);
        }

        for (ti, is_matched) in matched.iter().enumerate() {
            if !is_matched {
                self.tracks[ti].misses += 1;
            }
        }
        self.tracks.retain(|track| track.misses <= config.forget_misses);

        for (oi, is_taken) in taken.iter().enumerate() {
            if !is_taken {
                let observation = &observations[oi];
                self.tracks.push(StableBlock {
                    rect: observation.rect,
                    source_text: observation.text.clone(),
                    numeric_only: is_numeric_only(&observation.text),
                    candidate: None,
                    candidate_votes: 0,
                    translation: None,
                    translated_source: None,
                    misses: 0,
                    confidence: observation.confidence,
                });
            }
        }

        self.tracks
            .iter()
            .map(|track| {
                let mut block = track.clone();
                block.confidence = track.confidence;
                block
            })
            .collect()
    }

    fn fold(&mut self, index: usize, observation: &Observation, config: &StabilityConfig) {
        let track = &mut self.tracks[index];
        track.misses = 0;
        track.rect = observation.rect;
        track.confidence = track.confidence.min(observation.confidence);

        if observation.text == track.source_text {
            track.candidate = None;
            track.candidate_votes = 0;
            return;
        }

        if track.candidate.as_deref() == Some(observation.text.as_str()) {
            track.candidate_votes += 1;
        } else {
            track.candidate = Some(observation.text.clone());
            track.candidate_votes = 1;
        }

        let needed = config.agree_frames.max(1);
        if track.candidate_votes >= needed {
            if let Some(candidate) = track.candidate.take() {
                track.source_text = candidate;
                track.numeric_only = is_numeric_only(&track.source_text);
                track.translated_source = None;
                track.translation = None;
                track.candidate_votes = 0;
            }
        }
    }

    /// Attaches `translation` to the track whose source is `source_text`.
    pub fn provide_translation(&mut self, source_text: &str, translation: String) {
        for track in &mut self.tracks {
            if track.source_text == source_text {
                track.translation = Some(translation.clone());
                track.translated_source = Some(source_text.to_owned());
            }
        }
    }

    pub fn tracks(&self) -> &[StableBlock] {
        &self.tracks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install_annotations() {
        use std::sync::Once;
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            std::panic::set_hook(Box::new(|info| {
                let msg = info.to_string().replace('\n', " | ");
                eprintln!("::error title=test-panic::{msg}");
            }));
        });
    }

    fn observation(text: &str, x: i32, y: i32) -> Observation {
        Observation {
            rect: Rect::new(x, y, 120, 24),
            text: text.to_owned(),
            confidence: 0.9,
        }
    }

    #[test]
    fn a_new_block_is_visible_at_once() {
        install_annotations();
        let mut tracker = StabilityTracker::new();
        let config = StabilityConfig::default();
        let blocks = tracker.observe(&[observation("Настройки", 10, 10)], &config);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].source_text, "Настройки");
        assert!(blocks[0].needs_translation());
    }

    #[test]
    fn a_jittering_reading_does_not_replace_the_agreed_one_until_it_agrees() {
        install_annotations();
        let mut tracker = StabilityTracker::new();
        let config = StabilityConfig {
            agree_frames: 2,
            ..StabilityConfig::default()
        };

        tracker.observe(&[observation("Настройки", 10, 10)], &config);
        // One noisy frame: candidate, not yet agreed.
        let blocks = tracker.observe(&[observation("Настройкн", 10, 10)], &config);
        assert_eq!(blocks[0].source_text, "Настройки");
        assert_eq!(blocks[0].candidate.as_deref(), Some("Настройкн"));
        assert_eq!(blocks[0].candidate_votes, 1);

        // A return to the agreed reading clears the candidate.
        let blocks = tracker.observe(&[observation("Настройки", 10, 10)], &config);
        assert_eq!(blocks[0].source_text, "Настройки");
        assert!(blocks[0].candidate.is_none());
    }

    #[test]
    fn two_frames_of_agreement_replace_the_source_and_drop_the_stale_translation() {
        install_annotations();
        let mut tracker = StabilityTracker::new();
        let config = StabilityConfig {
            agree_frames: 2,
            ..StabilityConfig::default()
        };

        tracker.observe(&[observation("Options", 10, 10)], &config);
        tracker.provide_translation("Options", "Настройки".to_owned());
        let blocks = tracker.observe(&[observation("Options", 10, 10)], &config);
        assert_eq!(blocks[0].display_text(), "Настройки");

        // A new reading appears twice: still the old source after the first vote.
        let blocks = tracker.observe(&[observation("Optionz", 10, 10)], &config);
        assert_eq!(blocks[0].source_text, "Options");
        assert_eq!(blocks[0].candidate_votes, 1);

        // Second agreeing frame promotes the candidate and drops the stale translation.
        let blocks = tracker.observe(&[observation("Optionz", 10, 10)], &config);
        assert_eq!(blocks[0].source_text, "Optionz");
        assert!(blocks[0].translation.is_none(), "stale translation is dropped");
        assert_eq!(blocks[0].display_text(), "Optionz");
        assert!(blocks[0].needs_translation());
    }

    #[test]
    fn a_translation_is_reused_while_the_source_stands() {
        install_annotations();
        let mut tracker = StabilityTracker::new();
        let config = StabilityConfig::default();
        tracker.observe(&[observation("New Game", 0, 0)], &config);
        tracker.provide_translation("New Game", "Новая игра".to_owned());
        let blocks = tracker.observe(&[observation("New Game", 0, 0)], &config);
        assert!(!blocks[0].needs_translation());
        assert_eq!(blocks[0].display_text(), "Новая игра");
    }

    #[test]
    fn numeric_only_text_is_never_marked_for_translation() {
        install_annotations();
        assert!(is_numeric_only("87 / 100"));
        assert!(is_numeric_only("12,5%"));
        assert!(!is_numeric_only("Level 42"));
        assert!(!is_numeric_only("Настройки"));

        let mut tracker = StabilityTracker::new();
        let config = StabilityConfig::default();
        let blocks = tracker.observe(
            &[Observation {
                rect: Rect::new(0, 0, 40, 20),
                text: "87 / 100".to_owned(),
                confidence: 1.0,
            }],
            &config,
        );
        assert!(blocks[0].numeric_only);
        assert!(!blocks[0].needs_translation());
    }

    #[test]
    fn a_block_that_leaves_the_screen_is_forgotten_after_the_miss_budget() {
        install_annotations();
        let mut tracker = StabilityTracker::new();
        let config = StabilityConfig {
            forget_misses: 2,
            ..StabilityConfig::default()
        };

        tracker.observe(&[observation("Tooltip", 10, 10)], &config);
        assert_eq!(tracker.observe(&[], &config).len(), 1);
        assert_eq!(tracker.observe(&[], &config).len(), 1);
        assert!(tracker.observe(&[], &config).is_empty());
    }

    #[test]
    fn two_boxes_that_do_not_overlap_stay_separate_tracks() {
        install_annotations();
        let mut tracker = StabilityTracker::new();
        let config = StabilityConfig::default();
        let blocks = tracker.observe(
            &[
                observation("One", 0, 0),
                Observation {
                    rect: Rect::new(0, 200, 120, 24),
                    text: "Two".to_owned(),
                    confidence: 0.9,
                },
            ],
            &config,
        );
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn a_box_that_drifts_a_little_stays_on_the_same_track() {
        install_annotations();
        let mut tracker = StabilityTracker::new();
        let config = StabilityConfig::default();
        tracker.observe(&[observation("Drift", 10, 10)], &config);
        let blocks = tracker.observe(
            &[Observation {
                rect: Rect::new(14, 12, 120, 24),
                text: "Drift".to_owned(),
                confidence: 0.9,
            }],
            &config,
        );
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].source_text, "Drift");
        assert_eq!(blocks[0].rect, Rect::new(14, 12, 120, 24));
    }
}
