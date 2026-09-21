//! Text spans and the sources that produce them.
//!
//! Two kinds of source describe the text a screen is showing: UI Automation knows the exact
//! characters, recognition knows where the glyphs are. Both arrive as [`TextSpan`]s and are folded
//! into one view of the screen by [`merge`]. The Windows UI Automation source itself lands with
//! the Windows integration; the contract and the merge rules live here so the pipeline can be
//! built and tested against deterministic doubles.

use lumen_core::{Frame, Rect};

use crate::language::{identify, LanguageId};
use crate::{OcrEngine, OcrError, Recognition};

/// Where a span's text came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextOrigin {
    /// Recognition over the pixels.
    Ocr,
    /// The UI Automation accessibility tree.
    Automation,
}

/// One piece of on-screen text: the characters, the language they are written in, which source
/// knew them, where the glyphs sit in frame pixels, and how sure the producing source is (1.0 for
/// accessibility text, which is exact by construction).
#[derive(Debug, Clone, PartialEq)]
pub struct TextSpan {
    pub text: String,
    pub language: LanguageId,
    pub origin: TextOrigin,
    pub bounds: Rect,
    pub confidence: f32,
}

impl TextSpan {
    /// Builds a span out of a recognised line: the language is identified from the text, the
    /// origin is [`TextOrigin::Ocr`].
    pub fn from_recognition(recognition: Recognition) -> Self {
        let language = identify(&recognition.text);
        Self {
            text: recognition.text,
            language,
            origin: TextOrigin::Ocr,
            bounds: recognition.bounds,
            confidence: recognition.confidence,
        }
    }

    /// Whether the span carries no text or no area, the case [`merge`] drops.
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty() || self.bounds.is_empty()
    }
}

/// Why a source could not describe the screen.
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    /// The recognition engine behind an OCR source failed.
    #[error(transparent)]
    Ocr(#[from] OcrError),
    /// A source failed; `name` identifies it in the report.
    #[error("text source '{name}' failed: {detail}")]
    Failed { name: String, detail: String },
}

/// A snapshot of the text the screen is showing.
///
/// The frame is the screen state the snapshot must describe: recognition sources read its pixels,
/// while a UI Automation source may ignore them and read the live element tree. Keeping the frame
/// in the contract makes every source interchangeable and every run reproducible.
pub trait TextSource {
    /// Stable identifier used in reports (`"windows-ocr"`, `"stub"`, `"ui-automation"`).
    fn name(&self) -> &str;

    /// The text the screen is showing, one span per line or accessibility element.
    fn snapshot(&mut self, frame: &Frame) -> Result<Vec<TextSpan>, SourceError>;
}

/// Recognition behind the [`TextSource`] contract.
pub struct OcrSource<E: OcrEngine> {
    engine: E,
}

impl<E: OcrEngine> OcrSource<E> {
    pub fn new(engine: E) -> Self {
        Self { engine }
    }

    /// The engine recognition runs on.
    pub fn engine(&self) -> &E {
        &self.engine
    }

    /// Unwraps the engine again.
    pub fn into_engine(self) -> E {
        self.engine
    }
}

impl<E: OcrEngine> TextSource for OcrSource<E> {
    fn name(&self) -> &str {
        self.engine.name()
    }

    fn snapshot(&mut self, frame: &Frame) -> Result<Vec<TextSpan>, SourceError> {
        let spans = self
            .engine
            .recognize(frame, &[])?
            .into_iter()
            .map(TextSpan::from_recognition)
            .collect();
        Ok(spans)
    }
}

/// The overlap above which two spans are the same text seen by two sources.
pub const MATCH_IOU: f32 = 0.5;

/// Merges the recognition and UI Automation views of the same screen into one span list.
///
/// Empty spans are dropped from both sides. When a recognition span and an automation span overlap
/// with an IoU of at least [`MATCH_IOU`] they describe the same text: the automation text and
/// language win, because the accessibility tree holds the exact characters, while the recognition
/// bounds and confidence stay, because they describe the pixels; the merged span is marked
/// [`TextOrigin::Automation`]. Everything matched by nothing passes through unchanged. The result
/// is in reading order, top to bottom and left to right.
///
/// Each automation span matches the recognition span with the highest IoU below it, so a crowded
/// screen still folds into pairs instead of chains.
pub fn merge(recognition: Vec<TextSpan>, automation: Vec<TextSpan>) -> Vec<TextSpan> {
    let mut recognition: Vec<TextSpan> = recognition.into_iter().filter(|span| !span.is_empty()).collect();
    let mut automation: Vec<TextSpan> = automation.into_iter().filter(|span| !span.is_empty()).collect();
    recognition.sort_by(reading_order);
    automation.sort_by(reading_order);

    let mut taken = vec![false; automation.len()];
    let mut merged = Vec::with_capacity(recognition.len() + automation.len());
    for mut span in recognition {
        let best = automation
            .iter()
            .enumerate()
            .filter(|(index, _)| !taken[*index])
            .map(|(index, other)| (index, span.bounds.iou(&other.bounds)))
            .filter(|(_, overlap)| *overlap >= MATCH_IOU)
            .max_by(|left, right| left.1.total_cmp(&right.1).then(right.0.cmp(&left.0)));
        if let Some((index, _)) = best {
            taken[index] = true;
            let winner = &automation[index];
            span.text.clone_from(&winner.text);
            span.language = winner.language;
            span.origin = TextOrigin::Automation;
        }
        merged.push(span);
    }
    for (index, span) in automation.into_iter().enumerate() {
        if !taken[index] {
            merged.push(span);
        }
    }
    merged.sort_by(reading_order);
    merged
}

fn reading_order(left: &TextSpan, right: &TextSpan) -> std::cmp::Ordering {
    left.bounds
        .y
        .cmp(&right.bounds.y)
        .then(left.bounds.x.cmp(&right.bounds.x))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stub::StubEngine;

    fn span(text: &str, bounds: Rect, origin: TextOrigin, language: LanguageId) -> TextSpan {
        TextSpan {
            text: text.to_owned(),
            language,
            origin,
            bounds,
            confidence: 0.9,
        }
    }

    fn ocr(text: &str, bounds: Rect) -> TextSpan {
        span(text, bounds, TextOrigin::Ocr, LanguageId::En)
    }

    fn automation(text: &str, bounds: Rect, language: LanguageId) -> TextSpan {
        span(text, bounds, TextOrigin::Automation, language)
    }

    #[test]
    fn recognition_becomes_an_ocr_span_with_the_language_of_its_text() {
        let span = TextSpan::from_recognition(Recognition {
            text: "Привет".to_owned(),
            bounds: Rect::new(1, 2, 30, 10),
            confidence: 0.75,
        });
        assert_eq!(span.text, "Привет");
        assert_eq!(span.language, LanguageId::Ru);
        assert_eq!(span.origin, TextOrigin::Ocr);
        assert_eq!(span.bounds, Rect::new(1, 2, 30, 10));
        assert!((span.confidence - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn merge_drops_spans_without_text_or_area() {
        let merged = merge(
            vec![
                ocr("", Rect::new(0, 0, 10, 10)),
                ocr("   ", Rect::new(0, 20, 10, 10)),
                ocr("kept", Rect::new(0, 40, 10, 10)),
            ],
            vec![automation("gone", Rect::new(0, 0, 0, 0), LanguageId::En)],
        );
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].text, "kept");
    }

    #[test]
    fn merge_takes_the_automation_text_for_matching_spans() {
        let merged = merge(
            vec![ocr("menus", Rect::new(10, 10, 60, 14))],
            vec![automation("Меню", Rect::new(10, 10, 60, 14), LanguageId::Ru)],
        );
        assert_eq!(merged.len(), 1);
        let merged = &merged[0];
        assert_eq!(merged.text, "Меню");
        assert_eq!(merged.language, LanguageId::Ru);
        assert_eq!(merged.origin, TextOrigin::Automation);
        assert_eq!(merged.bounds, Rect::new(10, 10, 60, 14));
        assert!((merged.confidence - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn merge_keeps_both_spans_below_the_match_threshold() {
        let merged = merge(
            vec![ocr("left", Rect::new(0, 0, 10, 10))],
            vec![automation("right", Rect::new(5, 0, 10, 10), LanguageId::En)],
        );
        assert_eq!(merged.len(), 2);
        assert!(merged
            .iter()
            .any(|span| span.text == "left" && span.origin == TextOrigin::Ocr));
        assert!(merged
            .iter()
            .any(|span| span.text == "right" && span.origin == TextOrigin::Automation));
    }

    #[test]
    fn merge_pairs_each_span_with_the_best_overlap() {
        let merged = merge(
            vec![ocr("guess", Rect::new(0, 0, 10, 10))],
            vec![
                automation("far", Rect::new(2, 0, 10, 10), LanguageId::En),
                automation("near", Rect::new(1, 0, 10, 10), LanguageId::En),
            ],
        );
        assert_eq!(merged.len(), 2);
        let matched = merged
            .iter()
            .find(|span| span.origin == TextOrigin::Automation)
            .unwrap();
        assert_eq!(matched.text, "near");
        assert_eq!(matched.bounds, Rect::new(0, 0, 10, 10));
        assert!(merged.iter().any(|span| span.text == "far"));
    }

    #[test]
    fn merge_returns_spans_in_reading_order() {
        let merged = merge(
            vec![
                ocr("second", Rect::new(0, 40, 80, 10)),
                ocr("first", Rect::new(0, 10, 80, 10)),
            ],
            vec![automation("third", Rect::new(0, 70, 80, 10), LanguageId::En)],
        );
        let texts: Vec<&str> = merged.iter().map(|span| span.text.as_str()).collect();
        assert_eq!(texts, vec!["first", "second", "third"]);
    }

    #[test]
    fn an_ocr_source_snapshots_recognition_as_spans() {
        let mut source = OcrSource::new(StubEngine::new(vec![vec![Recognition {
            text: "Menu".to_owned(),
            bounds: Rect::new(4, 4, 40, 12),
            confidence: 0.8,
        }]]));
        assert_eq!(source.name(), "stub");
        let frame = Frame::filled(8, 8, [0, 0, 0, 255]).unwrap();
        let spans = source.snapshot(&frame).unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "Menu");
        assert_eq!(spans[0].origin, TextOrigin::Ocr);
        assert_eq!(spans[0].language, LanguageId::En);
    }
}
