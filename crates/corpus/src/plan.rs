//! Which scenes the corpus contains.
//!
//! The plan is a product: every language pack crossed with every font style and every background
//! kind, `limit` scenes per cell, in a fixed order with seeds derived from one base seed. Two runs
//! of the same plan produce the same corpus, and a report can always say which cell a number came
//! from.

use lumen_language::Language;

use crate::background::BackgroundKind;
use crate::font::FontStyle;
use crate::phrases::packs;
use crate::rng::Rng;
use crate::scene::SceneSpec;

/// The plan the CI gate runs: the full product, one scene per cell, four lines each.
pub const STANDARD_LINES_PER_SCENE: usize = 4;

/// A description of the corpus to generate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorpusPlan {
    pub width: u32,
    pub height: u32,
    /// Scenes per language/style/background cell.
    pub limit: usize,
    /// Every scene's seed is drawn from this, in plan order.
    pub seed: u64,
    pub lines_per_scene: usize,
}

impl CorpusPlan {
    /// The plan the quality gate is defined against: the full cross-product at `960x540`.
    pub fn standard(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            limit: 1,
            seed: 0x5EED_2026,
            lines_per_scene: STANDARD_LINES_PER_SCENE,
        }
    }

    /// Every scene the plan describes, in a fixed order.
    pub fn scenes(&self) -> Vec<SceneSpec> {
        let mut rng = Rng::new(self.seed);
        let mut specs = Vec::new();
        for pack in packs() {
            for style in FontStyle::all() {
                for background in BackgroundKind::all() {
                    for cell in 0..self.limit.max(1) {
                        let language = language_tag(pack.language);
                        let style_name = style.name();
                        let background_name = background.name();
                        let name = if self.limit > 1 {
                            format!("{language}-{style_name}-{background_name}-{cell}")
                        } else {
                            format!("{language}-{style_name}-{background_name}")
                        };
                        specs.push(SceneSpec {
                            name,
                            seed: rng.next_u64(),
                            language: pack.language,
                            style: *style,
                            background: *background,
                            width: self.width,
                            height: self.height,
                            lines: self.lines_per_scene,
                        });
                    }
                }
            }
        }
        specs
    }

    /// How many scenes the plan describes: languages times styles times backgrounds times limit.
    pub fn scene_count(&self) -> usize {
        packs().len() * FontStyle::all().len() * BackgroundKind::all().len() * self.limit.max(1)
    }
}

/// A short lowercase tag for a language, for scene names and report keys.
pub fn language_tag(language: Language) -> &'static str {
    match language {
        Language::English => "english",
        Language::German => "german",
        Language::French => "french",
        Language::Spanish => "spanish",
        Language::Italian => "italian",
        Language::Portuguese => "portuguese",
        Language::Dutch => "dutch",
        Language::Swedish => "swedish",
        Language::Polish => "polish",
        Language::Czech => "czech",
        Language::Romanian => "romanian",
        Language::Hungarian => "hungarian",
        Language::Turkish => "turkish",
        Language::Russian => "russian",
        Language::Ukrainian => "ukrainian",
        Language::Belarusian => "belarusian",
        Language::Bulgarian => "bulgarian",
        Language::Serbian => "serbian",
        Language::Greek => "greek",
        Language::Japanese => "japanese",
        Language::ChineseSimplified => "chinese-simplified",
        Language::ChineseTraditional => "chinese-traditional",
        Language::Korean => "korean",
        Language::Arabic => "arabic",
        Language::Hebrew => "hebrew",
        Language::Thai => "thai",
        Language::Hindi => "hindi",
        Language::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plan_is_deterministic() {
        let left = CorpusPlan::standard(320, 200).scenes();
        let right = CorpusPlan::standard(320, 200).scenes();
        assert_eq!(left.len(), right.len());
        for (one, other) in left.iter().zip(right.iter()) {
            assert_eq!(one, other);
        }
    }

    #[test]
    fn the_standard_plan_is_the_full_product() {
        let plan = CorpusPlan::standard(320, 200);
        let scenes = plan.scenes();
        assert_eq!(scenes.len(), plan.scene_count());
        assert_eq!(scenes.len(), packs().len() * 3 * 3);

        // Every language meets every style and every background.
        for pack in packs() {
            for style in FontStyle::all() {
                for background in BackgroundKind::all() {
                    let found = scenes.iter().any(|spec| {
                        spec.language == pack.language && spec.style == *style && spec.background == *background
                    });
                    assert!(found, "{:?} {:?} {:?} is missing", pack.language, style, background);
                }
            }
        }
    }

    #[test]
    fn scene_names_are_unique() {
        let scenes = CorpusPlan::standard(320, 200).scenes();
        let mut names: Vec<&str> = scenes.iter().map(|spec| spec.name.as_str()).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total);
    }

    #[test]
    fn the_limit_multiplies_cells_with_distinct_seeds() {
        let plan = CorpusPlan {
            limit: 2,
            ..CorpusPlan::standard(320, 200)
        };
        let scenes = plan.scenes();
        assert_eq!(scenes.len(), plan.scene_count());
        let seeds: std::collections::HashSet<u64> = scenes.iter().map(|spec| spec.seed).collect();
        assert_eq!(seeds.len(), scenes.len(), "every scene gets its own seed");
    }

    #[test]
    fn language_tags_are_short_and_unique() {
        let mut tags: Vec<&str> = Vec::new();
        for language in Language::all() {
            let tag = language_tag(*language);
            assert!(!tag.is_empty());
            assert!(tag.chars().all(|c| c.is_ascii_lowercase() || c == '-'), "{tag}");
            assert!(!tags.contains(&tag), "{tag} appears twice");
            tags.push(tag);
        }
    }

    #[test]
    fn a_bigger_seed_gives_a_different_corpus() {
        let left = CorpusPlan::standard(320, 200).scenes();
        let right = CorpusPlan {
            seed: 7,
            ..CorpusPlan::standard(320, 200)
        }
        .scenes();
        assert_ne!(left[0].seed, right[0].seed);
        assert_eq!(left[0].name, right[0].name);
    }
}
