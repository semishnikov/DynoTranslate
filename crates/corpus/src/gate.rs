//! The quality gate: what the corpus measures, and what has to hold.
//!
//! Two scores come out of a generated corpus. The recognition score runs every scene through
//! [`lumen_ocr::benchmark`] — the same runner the engines are compared by — and holds the mean
//! character error rate of each category under the threshold `docs/PLAN.md` sets: 3 % for clean UI
//! text, 8 % for stylised text. The identification score runs every transcript through
//! [`lumen_language::identify`] and reports how often the language of the scene is the language
//! the identification names.
//!
//! The engine scored here is the corpus's own double: a [`StubEngine`] scripted with the ground
//! truth, because no engine reads pixels yet. That is honest about what it proves — the scenes,
//! the transcripts, the thresholds and the whole scoring path are exercised and pinned, and the
//! number a real engine has to beat is written down. The day a pixel-reading engine exists, it is
//! passed to [`score_recognition`] in the double's place and nothing else changes.

use lumen_language::{identify, Language};
use lumen_ocr::benchmark::{self, BenchmarkCase};
use lumen_ocr::{OcrEngine, OcrError, Recognition, StubEngine};
use serde::{Deserialize, Serialize};

use crate::background::BackgroundKind;
use crate::font::FontStyle;
use crate::scene::CorpusScene;

/// The character error rate ceilings from the plan's quality table.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Thresholds {
    /// Clean UI text: regular weight on a solid background.
    pub clean: f64,
    /// Everything harder: bold, italic, gradients and noise.
    pub stylised: f64,
}

impl Thresholds {
    /// The values `docs/PLAN.md` holds every engine to: 3 % clean, 8 % stylised.
    pub fn plan_defaults() -> Self {
        Self {
            clean: 0.03,
            stylised: 0.08,
        }
    }
}

impl Default for Thresholds {
    fn default() -> Self {
        Self::plan_defaults()
    }
}

/// Which ceiling a scene is scored under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Clean,
    Stylised,
}

impl Category {
    pub fn name(self) -> &'static str {
        match self {
            Category::Clean => "clean",
            Category::Stylised => "stylised",
        }
    }
}

/// Regular type on a solid panel is what the plan means by clean UI text; anything the corpus
/// makes harder — weight, slant, gradients, grain — is scored as stylised.
pub fn categorise(style: FontStyle, background: BackgroundKind) -> Category {
    match (style, background) {
        (FontStyle::Regular, BackgroundKind::Solid) => Category::Clean,
        _ => Category::Stylised,
    }
}

/// One scene's text as language identification sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneTranscript {
    pub name: String,
    pub language: Language,
    pub text: String,
}

/// Everything scoring needs, with the frames moved out of the scenes rather than copied.
pub struct CorpusMaterial {
    pub cases: Vec<BenchmarkCase>,
    pub categories: Vec<Category>,
    pub languages: Vec<Language>,
    /// The ground-truth answers, one scripted call per case.
    pub script: Vec<Vec<Recognition>>,
    pub transcripts: Vec<SceneTranscript>,
}

impl CorpusMaterial {
    /// The engine that replays the ground truth: recognition as good as the corpus can measure
    /// until a real engine exists.
    pub fn perfect_engine(&self) -> StubEngine {
        StubEngine::new(self.script.clone())
    }
}

/// Turns composed scenes into benchmark cases and transcripts. Consumes the scenes so the frames
/// are moved, not cloned.
pub fn prepare(scenes: Vec<CorpusScene>) -> CorpusMaterial {
    let mut material = CorpusMaterial {
        cases: Vec::with_capacity(scenes.len()),
        categories: Vec::with_capacity(scenes.len()),
        languages: Vec::with_capacity(scenes.len()),
        script: Vec::with_capacity(scenes.len()),
        transcripts: Vec::with_capacity(scenes.len()),
    };
    for scene in scenes {
        let reference = scene
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<&str>>()
            .join("\n");
        let answers = scene
            .lines
            .iter()
            .map(|line| Recognition {
                text: line.text.clone(),
                bounds: line.bounds,
                confidence: 1.0,
            })
            .collect();
        material.transcripts.push(SceneTranscript {
            name: scene.name.clone(),
            language: scene.language,
            text: scene.transcript(),
        });
        material
            .categories
            .push(categorise(scene.style, scene.background.kind()));
        material.languages.push(scene.language);
        material.cases.push(BenchmarkCase {
            name: scene.name,
            frame: scene.frame,
            regions: Vec::new(),
            reference,
        });
        material.script.push(answers);
    }
    material
}

/// The score of one scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseScore {
    pub name: String,
    pub category: Category,
    pub language: Language,
    pub character_error_rate: f64,
}

/// The score of one category against its ceiling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CategoryScore {
    pub cases: usize,
    pub mean_character_error_rate: f64,
    pub threshold: f64,
    pub passed: bool,
}

/// The whole recognition verdict.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateScore {
    pub engine: String,
    pub cases: Vec<CaseScore>,
    pub clean: CategoryScore,
    pub stylised: CategoryScore,
    pub passed: bool,
}

/// Scores `engine` over the corpus material and holds each category under its threshold. A
/// category with no cases reports a mean of zero and passes; the standard plan always fills both.
pub fn score_recognition<E: OcrEngine>(
    material: &CorpusMaterial,
    engine: &mut E,
    thresholds: &Thresholds,
) -> Result<GateScore, OcrError> {
    let report = benchmark::run(engine, &material.cases)?;
    let cases: Vec<CaseScore> = report
        .cases
        .iter()
        .zip(material.categories.iter())
        .zip(material.languages.iter())
        .map(|((case, category), language)| CaseScore {
            name: case.name.clone(),
            category: *category,
            language: *language,
            character_error_rate: case.character_error_rate,
        })
        .collect();
    let clean = category_score(&cases, Category::Clean, thresholds.clean);
    let stylised = category_score(&cases, Category::Stylised, thresholds.stylised);
    let passed = clean.passed && stylised.passed;
    Ok(GateScore {
        engine: report.engine,
        cases,
        clean,
        stylised,
        passed,
    })
}

fn category_score(cases: &[CaseScore], category: Category, threshold: f64) -> CategoryScore {
    let rates: Vec<f64> = cases
        .iter()
        .filter(|case| case.category == category)
        .map(|case| case.character_error_rate)
        .collect();
    // An empty category sums to zero over max(1) cases: a mean of zero, which passes.
    let mean = rates.iter().sum::<f64>() / rates.len().max(1) as f64;
    CategoryScore {
        cases: rates.len(),
        mean_character_error_rate: mean,
        threshold,
        passed: mean <= threshold,
    }
}

/// One language's identification tally.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LanguageScore {
    pub language: Language,
    pub scenes: usize,
    pub correct: usize,
}

/// How often identification names the language the scenes were generated in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdentificationScore {
    pub scenes: usize,
    pub correct: usize,
    /// A confident answer that is the wrong language.
    pub wrong: usize,
    /// No answer at all, which identification is allowed to give.
    pub unknown: usize,
    pub accuracy: f64,
    pub per_language: Vec<LanguageScore>,
}

/// Scores identification over the corpus transcripts. Every scene counts, including the ones
/// identification answers `Unknown` to: on a real screen that is a miss too, and the report keeps
/// the two kinds of miss apart.
pub fn score_identification(transcripts: &[SceneTranscript]) -> IdentificationScore {
    let mut correct = 0;
    let mut wrong = 0;
    let mut unknown = 0;
    let mut tallies: Vec<(Language, usize, usize)> = Vec::new();

    for transcript in transcripts {
        let guess = identify(&transcript.text);
        let hit = guess.language == transcript.language;
        match tallies
            .iter_mut()
            .find(|(language, _, _)| *language == transcript.language)
        {
            Some((_, scenes, hits)) => {
                *scenes += 1;
                *hits += usize::from(hit);
            }
            None => tallies.push((transcript.language, 1, usize::from(hit))),
        }
        if hit {
            correct += 1;
        } else if guess.language == Language::Unknown {
            unknown += 1;
        } else {
            wrong += 1;
        }
    }

    let per_language = Language::all()
        .iter()
        .filter_map(|language| {
            let (found, scenes, hits) = tallies.iter().find(|(known, _, _)| known == language)?;
            Some(LanguageScore {
                language: *found,
                scenes: *scenes,
                correct: *hits,
            })
        })
        .collect();

    let scenes = transcripts.len();
    // Zero scenes divide by max(1) and the numerator is zero anyway: a vacuous run reports zero.
    let accuracy = correct as f64 / scenes.max(1) as f64;

    IdentificationScore {
        scenes,
        correct,
        wrong,
        unknown,
        accuracy,
        per_language,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::CorpusPlan;
    use crate::scene::{self, SceneSpec};

    fn composed(spec: &SceneSpec) -> CorpusScene {
        scene::compose(spec).expect("the corpus specs are all drawable")
    }

    fn sample_specs() -> Vec<SceneSpec> {
        vec![
            SceneSpec {
                name: "english-regular-solid".to_owned(),
                seed: 1,
                language: Language::English,
                style: FontStyle::Regular,
                background: BackgroundKind::Solid,
                width: 320,
                height: 200,
                lines: 3,
            },
            SceneSpec {
                name: "russian-bold-noise".to_owned(),
                seed: 2,
                language: Language::Russian,
                style: FontStyle::Bold,
                background: BackgroundKind::Noise,
                width: 320,
                height: 200,
                lines: 3,
            },
            SceneSpec {
                name: "german-italic-gradient".to_owned(),
                seed: 3,
                language: Language::German,
                style: FontStyle::Italic,
                background: BackgroundKind::Gradient,
                width: 320,
                height: 200,
                lines: 3,
            },
        ]
    }

    fn sample_material() -> CorpusMaterial {
        prepare(sample_specs().iter().map(composed).collect())
    }

    fn transcript(name: &str, language: Language, text: &str) -> SceneTranscript {
        SceneTranscript {
            name: name.to_owned(),
            language,
            text: text.to_owned(),
        }
    }

    #[test]
    fn the_thresholds_are_the_ones_the_plan_states() {
        let thresholds = Thresholds::plan_defaults();
        assert!((thresholds.clean - 0.03).abs() < f64::EPSILON);
        assert!((thresholds.stylised - 0.08).abs() < f64::EPSILON);
        assert_eq!(Thresholds::default(), thresholds);
    }

    #[test]
    fn the_categories_follow_the_plan_definition() {
        let regular_solid = categorise(FontStyle::Regular, BackgroundKind::Solid);
        let bold_solid = categorise(FontStyle::Bold, BackgroundKind::Solid);
        let regular_noise = categorise(FontStyle::Regular, BackgroundKind::Noise);
        let italic_gradient = categorise(FontStyle::Italic, BackgroundKind::Gradient);
        assert_eq!(regular_solid, Category::Clean);
        assert_eq!(bold_solid, Category::Stylised);
        assert_eq!(regular_noise, Category::Stylised);
        assert_eq!(italic_gradient, Category::Stylised);
    }

    #[test]
    fn preparation_keeps_the_ground_truth_intact() {
        let scenes: Vec<CorpusScene> = sample_specs().iter().map(composed).collect();
        let mut expected = Vec::with_capacity(scenes.len());
        for scene in &scenes {
            let mut lines = Vec::new();
            for line in &scene.lines {
                lines.push(line.text.clone());
            }
            expected.push((scene.name.clone(), lines));
        }
        let material = prepare(scenes);

        assert_eq!(material.cases.len(), 3);
        let categories = [Category::Clean, Category::Stylised, Category::Stylised];
        assert_eq!(material.categories, categories);
        assert_eq!(material.languages, vec![Language::English, Language::Russian, Language::German]);
        for (case, (name, lines)) in material.cases.iter().zip(expected.iter()) {
            assert_eq!(case.name, *name);
            assert_eq!(case.reference, lines.join("\n"));
            assert!(case.regions.is_empty());
        }
        for (answers, (_, lines)) in material.script.iter().zip(expected.iter()) {
            let texts: Vec<&str> = answers.iter().map(|answer| answer.text.as_str()).collect();
            assert_eq!(texts, lines.iter().map(|line| line.as_str()).collect::<Vec<&str>>());
            assert!(answers.iter().all(|answer| answer.confidence == 1.0));
        }
        for entry in &material.transcripts {
            for line in &expected.iter().find(|(name, _)| name == &entry.name).unwrap().1 {
                assert!(entry.text.contains(line.as_str()));
            }
        }
    }

    #[test]
    fn the_ground_truth_engine_scores_exactly_zero_and_passes() {
        let material = sample_material();
        let mut engine = material.perfect_engine();
        let gate = score_recognition(&material, &mut engine, &Thresholds::plan_defaults()).unwrap();

        assert_eq!(gate.engine, "stub");
        assert_eq!(gate.cases.len(), 3);
        for case in &gate.cases {
            assert!(case.character_error_rate < f64::EPSILON, "{case:?}");
        }
        assert_eq!(gate.clean.cases, 1);
        assert_eq!(gate.stylised.cases, 2);
        assert!(gate.clean.passed && gate.stylised.passed && gate.passed);
    }

    #[test]
    fn a_wrong_answer_trips_the_gate_of_its_own_category() {
        let material = sample_material();
        let mut script = material.script.clone();
        // The clean case is first in the sample; replace its answers with something else entirely.
        script[0] = vec![Recognition {
            text: "nothing like the truth".to_owned(),
            bounds: material.cases[0].frame.bounds(),
            confidence: 1.0,
        }];
        let mut engine = StubEngine::new(script);
        let gate = score_recognition(&material, &mut engine, &Thresholds::plan_defaults()).unwrap();

        assert!(gate.cases[0].character_error_rate > 0.03);
        assert!(!gate.clean.passed);
        assert!(gate.stylised.passed);
        assert!(!gate.passed);
    }

    #[test]
    fn a_slightly_wrong_answer_stays_under_the_stylised_ceiling() {
        let material = sample_material();
        let mut script = material.script.clone();
        // One character in one line of one stylised scene: well under 8 %, over zero.
        let mut damaged: Vec<char> = script[1][0].text.chars().collect();
        damaged[0] = if damaged[0] == 'X' { 'Y' } else { 'X' };
        script[1][0].text = damaged.into_iter().collect();
        let mut engine = StubEngine::new(script);
        let gate = score_recognition(&material, &mut engine, &Thresholds::plan_defaults()).unwrap();

        assert!(gate.cases[1].character_error_rate > 0.0);
        assert!(gate.cases[1].character_error_rate <= 0.08, "{:?}", gate.cases[1]);
        assert!(gate.stylised.passed && gate.passed);
    }

    #[test]
    fn identification_finds_the_languages_with_letters_of_their_own() {
        let transcripts = vec![
            transcript("es", Language::Spanish, "Mañana otra vez"),
            transcript("de", Language::German, "Straße der Ehre"),
            transcript("fr", Language::French, "Le cœur du héros"),
            transcript("pl", Language::Polish, "Życie jest piękne"),
            transcript("cs", Language::Czech, "Příběh právě začíná"),
            transcript("pt", Language::Portuguese, "Não quero sair"),
            transcript("uk", Language::Ukrainian, "Їжа та напої"),
            transcript("ru", Language::Russian, "Вы действительно хотите выйти?"),
            transcript("nl", Language::Dutch, "Het avontuur begint"),
            transcript("it", Language::Italian, "Salva e continua la partita"),
            transcript("en", Language::English, "The journey starts here"),
        ];
        let score = score_identification(&transcripts);
        assert_eq!(score.scenes, transcripts.len());
        assert_eq!(score.correct, transcripts.len());
        assert_eq!(score.wrong, 0);
        assert_eq!(score.unknown, 0);
        assert!((score.accuracy - 1.0).abs() < f64::EPSILON);
        assert_eq!(score.per_language.len(), transcripts.len());
        assert!(score.per_language.iter().all(|entry| entry.correct == entry.scenes));
    }

    #[test]
    fn identification_counts_every_outcome_exactly_once() {
        let transcripts = vec![
            transcript("ru", Language::Russian, "Продолжить игру"),
            // Text with nothing to identify: identification answers Unknown and the score says so.
            transcript("en", Language::English, "12345 678"),
            transcript("de", Language::German, "Jouer Continuer Options"),
        ];
        let score = score_identification(&transcripts);
        assert_eq!(score.scenes, 3);
        assert_eq!(score.correct + score.wrong + score.unknown, 3);
        assert_eq!(score.unknown, 1);
        assert!((score.accuracy - score.correct as f64 / 3.0).abs() < f64::EPSILON);
        let total: usize = score.per_language.iter().map(|entry| entry.scenes).sum();
        assert_eq!(total, 3);
    }

    #[test]
    fn the_full_standard_plan_passes_the_gate_and_scores_consistently() {
        let plan = CorpusPlan::standard(320, 200);
        let scenes: Vec<CorpusScene> = plan.scenes().iter().map(composed).collect();
        assert_eq!(scenes.len(), plan.scene_count());
        let material = prepare(scenes);

        let mut engine = material.perfect_engine();
        let gate = score_recognition(&material, &mut engine, &Thresholds::plan_defaults()).unwrap();
        assert!(gate.passed);
        assert_eq!(gate.cases.len(), material.cases.len());
        assert_eq!(gate.clean.cases + gate.stylised.cases, material.cases.len());
        assert!(gate.clean.cases > 0 && gate.stylised.cases > 0);

        let identification = score_identification(&material.transcripts);
        assert_eq!(identification.scenes, plan.scene_count());
        assert_eq!(
            identification.correct + identification.wrong + identification.unknown,
            identification.scenes
        );
        let tally: usize = identification.per_language.iter().map(|entry| entry.scenes).sum();
        assert_eq!(tally, identification.scenes);
    }

    #[test]
    fn an_empty_corpus_passes_vacuously_and_says_how_many_it_scored() {
        let material = prepare(Vec::new());
        let mut engine = material.perfect_engine();
        let gate = score_recognition(&material, &mut engine, &Thresholds::plan_defaults()).unwrap();
        assert!(gate.passed);
        assert_eq!(gate.clean.cases, 0);
        let identification = score_identification(&material.transcripts);
        assert_eq!(identification.scenes, 0);
        assert!(identification.accuracy < f64::EPSILON);
    }
}
