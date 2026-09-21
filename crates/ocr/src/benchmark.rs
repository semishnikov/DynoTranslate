//! Character error rate and the engine benchmark runner.
//!
//! CER is the edit distance between the recognised text and the reference transcript, divided by
//! the reference length in characters. It is the single number every engine is compared by, so the
//! runner scores all engines through the same [`OcrEngine`] calls the pipeline makes.

use std::time::Instant;

use lumen_core::{Frame, Rect};
use serde::{Deserialize, Serialize};

use crate::{OcrEngine, OcrError};

/// One scored sample: the pixels, the regions to recognise (empty means the whole frame), and the
/// transcript the engine's answer is measured against.
#[derive(Debug, Clone)]
pub struct BenchmarkCase {
    pub name: String,
    pub frame: Frame,
    pub regions: Vec<Rect>,
    pub reference: String,
}

/// The score of a single case: the engine's answer, its character error rate, and how long
/// recognition took.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseReport {
    pub name: String,
    pub hypothesis: String,
    pub character_error_rate: f64,
    pub elapsed_ms: f64,
}

/// Whole-run score: per-case reports plus the mean character error rate across cases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub engine: String,
    pub cases: Vec<CaseReport>,
    pub mean_character_error_rate: f64,
}

/// Scores `engine` over `cases` in order. A recognition failure aborts the run with that error; an
/// empty case list scores a mean of zero.
pub fn run<E: OcrEngine>(engine: &mut E, cases: &[BenchmarkCase]) -> Result<BenchmarkReport, OcrError> {
    let mut reports = Vec::with_capacity(cases.len());
    for case in cases {
        let started = Instant::now();
        let lines = engine.recognize(&case.frame, &case.regions)?;
        let mut hypothesis = String::new();
        for (index, line) in lines.iter().enumerate() {
            if index > 0 {
                hypothesis.push('\n');
            }
            hypothesis.push_str(&line.text);
        }
        reports.push(CaseReport {
            name: case.name.clone(),
            character_error_rate: character_error_rate(&case.reference, &hypothesis),
            hypothesis,
            elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
        });
    }
    let mean_character_error_rate = if reports.is_empty() {
        0.0
    } else {
        reports.iter().map(|report| report.character_error_rate).sum::<f64>() / reports.len() as f64
    };
    Ok(BenchmarkReport {
        engine: engine.name().to_owned(),
        cases: reports,
        mean_character_error_rate,
    })
}

/// Character error rate of `hypothesis` against `reference`. Both empty scores zero; an empty
/// reference against a non-empty hypothesis scores one.
pub fn character_error_rate(reference: &str, hypothesis: &str) -> f64 {
    let reference: Vec<char> = reference.chars().collect();
    if reference.is_empty() {
        return if hypothesis.is_empty() { 0.0 } else { 1.0 };
    }
    let hypothesis: Vec<char> = hypothesis.chars().collect();
    edit_distance(&reference, &hypothesis) as f64 / reference.len() as f64
}

/// Levenshtein distance over two character slices, computed with two rolling rows.
fn edit_distance(reference: &[char], hypothesis: &[char]) -> usize {
    let mut previous: Vec<usize> = (0..=hypothesis.len()).collect();
    let mut current = vec![0; hypothesis.len() + 1];
    for (index, reference_char) in reference.iter().enumerate() {
        current[0] = index + 1;
        for (other, hypothesis_char) in hypothesis.iter().enumerate() {
            let substitution = previous[other] + usize::from(reference_char != hypothesis_char);
            current[other + 1] = substitution.min(previous[other + 1] + 1).min(current[other] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[hypothesis.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_cer(reference: &str, hypothesis: &str, expected: f64) {
        let actual = character_error_rate(reference, hypothesis);
        assert!(
            (actual - expected).abs() < 1e-9,
            "cer({reference:?}, {hypothesis:?}) = {actual}, expected {expected}"
        );
    }

    fn line(frame: &Frame, text: &str) -> crate::Recognition {
        crate::Recognition {
            text: text.to_owned(),
            bounds: frame.bounds(),
            confidence: 0.9,
        }
    }

    #[test]
    fn identical_text_scores_zero() {
        assert_cer("Hello, world!", "Hello, world!", 0.0);
        assert_cer("", "", 0.0);
    }

    #[test]
    fn single_substitution_over_five_characters_scores_point_two() {
        assert_cer("hello", "hallo", 0.2);
    }

    #[test]
    fn kitten_to_sitting_scores_one_half() {
        assert_cer("kitten", "sitting", 0.5);
    }

    #[test]
    fn empty_reference_scores_one_unless_both_are_empty() {
        assert_cer("", "abc", 1.0);
        assert_cer("abc", "", 1.0);
    }

    #[test]
    fn distance_counts_characters_not_bytes() {
        assert_cer("привет", "превет", 1.0 / 6.0);
    }

    #[test]
    fn benchmark_joins_lines_and_scores_each_case() {
        let frame = Frame::filled(8, 8, [0, 0, 0, 255]).unwrap();
        let mut engine = crate::stub::StubEngine::new(vec![
            vec![line(&frame, "Hello"), line(&frame, "world")],
            vec![line(&frame, "hallo")],
        ]);
        let cases = vec![
            BenchmarkCase {
                name: "two lines".to_owned(),
                frame: frame.clone(),
                regions: Vec::new(),
                reference: "Hello\nworld".to_owned(),
            },
            BenchmarkCase {
                name: "one substitution".to_owned(),
                frame,
                regions: Vec::new(),
                reference: "hello".to_owned(),
            },
        ];
        let report = run(&mut engine, &cases).unwrap();
        assert_eq!(report.engine, "stub");
        assert_eq!(report.cases.len(), 2);
        assert_eq!(report.cases[0].hypothesis, "Hello\nworld");
        assert_cer(&cases[0].reference, &report.cases[0].hypothesis, 0.0);
        assert_cer(&cases[1].reference, &report.cases[1].hypothesis, 0.2);
        assert!((report.mean_character_error_rate - 0.1).abs() < 1e-9);
    }

    #[test]
    fn empty_run_scores_zero() {
        let mut engine = crate::stub::StubEngine::new(Vec::new());
        let report = run(&mut engine, &[]).unwrap();
        assert!(report.cases.is_empty());
        assert!(report.mean_character_error_rate < f64::EPSILON);
    }
}
