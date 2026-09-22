//! The machine-readable result of a corpus run.
//!
//! Two documents come out of the harness: a manifest of what was generated — every scene with its
//! text, boxes and the image that holds them — and a report of what was scored. Both are plain
//! JSON so a regression is a diff, and both are written whether the gate passes or fails, because
//! a failing gate without its numbers is not actionable.

use lumen_language::Language;
use serde::{Deserialize, Serialize};

use crate::background::BackgroundKind;
use crate::font::FontStyle;
use crate::gate::{GateScore, IdentificationScore};
use crate::plan::{language_tag, CorpusPlan};
use crate::scene::GroundLine;

/// How many scenes one language contributed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LanguageCount {
    pub language: Language,
    pub tag: String,
    pub scenes: usize,
}

/// What was generated, before any scoring.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerationSummary {
    pub scenes: usize,
    pub width: u32,
    pub height: u32,
    pub seed: u64,
    pub limit: usize,
    pub languages: Vec<LanguageCount>,
}

impl GenerationSummary {
    /// Summarizes the plan and the languages the scenes actually used.
    pub fn new(plan: &CorpusPlan, languages: &[Language]) -> Self {
        let counts = Language::all()
            .iter()
            .map(|language| (*language, languages.iter().filter(|used| *used == language).count()))
            .filter(|(_, scenes)| *scenes > 0)
            .map(|(language, scenes)| LanguageCount {
                language,
                tag: language_tag(language).to_owned(),
                scenes,
            })
            .collect();
        Self {
            scenes: languages.len(),
            width: plan.width,
            height: plan.height,
            seed: plan.seed,
            limit: plan.limit,
            languages: counts,
        }
    }
}

/// One generated scene as the manifest records it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneRecord {
    pub name: String,
    pub language: Language,
    pub style: FontStyle,
    pub background: BackgroundKind,
    pub lines: Vec<GroundLine>,
    /// The PNG holding the scene, when images were written.
    pub image: Option<String>,
}

/// The ground truth of the whole corpus: what was drawn, where, and in which language.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorpusManifest {
    pub width: u32,
    pub height: u32,
    pub seed: u64,
    pub scenes: Vec<SceneRecord>,
}

/// Everything a corpus run has to say.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorpusReport {
    pub generation: GenerationSummary,
    pub recognition: GateScore,
    pub identification: IdentificationScore,
    /// The gate verdict in one field, which is also the exit status of the harness.
    pub gate_passed: bool,
}

impl CorpusReport {
    /// The console summary: what ran, the two scores against their ceilings, the verdict.
    pub fn summarize(&self) -> String {
        let generation = &self.generation;
        let recognition = &self.recognition;
        let identification = &self.identification;
        format!(
            "corpus: {} scenes · {} languages · {}x{} · seed {}\n\
             recognition ({}): clean {:.3} ≤ {:.2} · stylised {:.3} ≤ {:.2} · {}\n\
             identification: {}/{} correct, {} wrong, {} unknown · accuracy {:.3}",
            generation.scenes,
            generation.languages.len(),
            generation.width,
            generation.height,
            generation.seed,
            recognition.engine,
            recognition.clean.mean_character_error_rate,
            recognition.clean.threshold,
            recognition.stylised.mean_character_error_rate,
            recognition.stylised.threshold,
            if recognition.passed { "pass" } else { "FAIL" },
            identification.correct,
            identification.scenes,
            identification.wrong,
            identification.unknown,
            identification.accuracy,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::{self, Thresholds};
    use crate::scene::{self, SceneSpec};
    use lumen_core::Rect;

    fn spec(seed: u64, language: Language) -> SceneSpec {
        SceneSpec {
            name: format!("scene-{seed}"),
            seed,
            language,
            style: FontStyle::Regular,
            background: BackgroundKind::Solid,
            width: 320,
            height: 200,
            lines: 3,
        }
    }

    fn material() -> gate::CorpusMaterial {
        let scenes = vec![
            scene::compose(&spec(1, Language::English)).unwrap(),
            scene::compose(&spec(2, Language::Russian)).unwrap(),
        ];
        gate::prepare(scenes)
    }

    fn report() -> CorpusReport {
        let plan = CorpusPlan {
            width: 320,
            height: 200,
            limit: 1,
            seed: 7,
            lines_per_scene: 3,
        };
        let prepared = material();
        let languages: Vec<Language> = prepared.languages.clone();
        let mut engine = prepared.perfect_engine();
        let recognition = gate::score_recognition(&prepared, &mut engine, &Thresholds::plan_defaults()).unwrap();
        let identification = gate::score_identification(&prepared.transcripts);
        CorpusReport {
            generation: GenerationSummary::new(&plan, &languages),
            gate_passed: recognition.passed,
            recognition,
            identification,
        }
    }

    #[test]
    fn the_generation_summary_counts_what_ran() {
        let summary = report().generation;
        assert_eq!(summary.scenes, 2);
        assert_eq!(summary.languages.len(), 2);
        assert_eq!(summary.languages[0].language, Language::English);
        assert_eq!(summary.languages[0].tag, "english");
        assert_eq!(summary.languages[0].scenes, 1);
        assert_eq!(summary.languages[1].language, Language::Russian);
        assert_eq!(summary.languages[1].scenes, 1);
    }

    #[test]
    fn the_summary_carries_the_verdict_and_both_scores() {
        let text = report().summarize();
        assert!(text.contains("corpus: 2 scenes"));
        assert!(text.contains("clean"));
        assert!(text.contains("stylised"));
        assert!(text.contains("pass"));
        assert!(text.contains("identification"));
    }

    #[test]
    fn the_report_round_trips_through_json() {
        let original = report();
        let text = serde_json::to_string_pretty(&original).unwrap();
        let restored: CorpusReport = serde_json::from_str(&text).unwrap();
        assert_eq!(restored, original);
        assert!(restored.gate_passed);
    }

    #[test]
    fn the_manifest_round_trips_through_json() {
        let line = GroundLine {
            text: "Начать игру".to_owned(),
            bounds: Rect::new(24, 40, 180, 22),
            language: Language::Russian,
        };
        let manifest = CorpusManifest {
            width: 960,
            height: 540,
            seed: 7,
            scenes: vec![SceneRecord {
                name: "russian-regular-solid".to_owned(),
                language: Language::Russian,
                style: FontStyle::Regular,
                background: BackgroundKind::Solid,
                lines: vec![line],
                image: Some("scene-000.png".to_owned()),
            }],
        };
        let text = serde_json::to_string(&manifest).unwrap();
        let restored: CorpusManifest = serde_json::from_str(&text).unwrap();
        assert_eq!(restored, manifest);
        // The enums serialize as the lowercase names the reports are read by.
        assert!(text.contains("\"regular\""));
        assert!(text.contains("\"solid\""));
    }
}
