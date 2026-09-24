//! Live translation orchestrator — the production pipeline that turns raw observations
//! into stable, contextually-translated overlays.
//!
//! This is the module that ties everything together:
//!
//! ```text
//!   Observations → Dedup → Memory lookup → Batch translate → Cache store → Display
//! ```
//!
//! Key design decisions:
//! - **Batch everything**: one HTTP call per frame, not per bubble.
//! - **Cache aggressively**: identical text is never translated twice.
//! - **Context flows forward**: recent dialogue is always in the prompt.
//! - **Translations stick**: once placed, they don't move or re-translate.
//! - **Auto-cleanup**: bounded memory, LRU eviction, no disk bloat.

use std::time::{Duration, Instant};

use lumen_core::Rect;
use lumen_language::Language;
use lumen_layout::BlockKind;
use lumen_stability::{Observation, StabilityConfig, StabilityTracker, StableBlock};

use crate::context::DialogueContext;
use crate::dedup::FuzzyCache;
use crate::engine::{
    EngineKind, TranslateItem, TranslatedItem, TranslationEngine, TranslationError,
    TranslationRequest, TranslationResponse,
};
use crate::memory::{MemoryKey, TranslationMemory};

/// Configuration for the live translation pipeline.
#[derive(Debug, Clone)]
pub struct LiveConfig {
    /// Source language (auto-detected or forced).
    pub source_language: Language,
    /// Target language for translation.
    pub target_language: Language,
    /// Stability thresholds for temporal tracking.
    pub stability: StabilityConfig,
    /// Maximum items per translation batch.
    pub max_batch_size: usize,
    /// Minimum time between engine calls (debounce).
    pub min_batch_interval: Duration,
    /// Maximum entries in translation memory.
    pub memory_capacity: usize,
    /// Whether to include dialogue context in translation prompts.
    pub use_context: bool,
    /// Application/window identifier for glossary lookup.
    pub app_id: Option<String>,
}

impl Default for LiveConfig {
    fn default() -> Self {
        Self {
            source_language: Language::English,
            target_language: Language::Russian,
            stability: StabilityConfig {
                match_iou: 0.3,
                agree_frames: 2,
                forget_misses: 5,
            },
            max_batch_size: 50,
            min_batch_interval: Duration::from_millis(100),
            memory_capacity: 10_000,
            use_context: true,
            app_id: None,
        }
    }
}

/// Statistics for the live pipeline.
#[derive(Debug, Default, Clone)]
pub struct LiveStats {
    /// Total frames processed.
    pub frames: u64,
    /// Total blocks tracked.
    pub blocks: u64,
    /// Total translation engine calls.
    pub engine_calls: u64,
    /// Total items translated.
    pub items_translated: u64,
    /// Total cache hits (memory + stability reuse).
    pub cache_hits: u64,
    /// Total cache misses requiring engine calls.
    pub cache_misses: u64,
    /// Cumulative engine time in milliseconds.
    pub engine_time_ms: u64,
    /// Last frame processing time in milliseconds.
    pub last_frame_ms: u64,
}

/// One translated block ready for rendering.
#[derive(Debug, Clone)]
pub struct TranslatedBlock {
    /// Screen rectangle of the original text.
    pub rect: Rect,
    /// The original source text (agreed reading).
    pub source_text: String,
    /// The translated text to display.
    pub display_text: String,
    /// Whether this is a fresh translation this frame.
    pub is_new: bool,
    /// Confidence of the source reading.
    pub confidence: f32,
    /// Block classification from layout.
    pub kind: Option<BlockKind>,
    /// Whether this block is numeric-only (never translated).
    pub numeric_only: bool,
}

/// The live translation pipeline.
pub struct LivePipeline {
    config: LiveConfig,
    tracker: StabilityTracker,
    memory: TranslationMemory,
    context: DialogueContext,
    fuzzy_cache: FuzzyCache,
    stats: LiveStats,
    /// Last time we called the engine.
    last_engine_call: Option<Instant>,
    /// Glossary version for memory keying.
    glossary_version: u64,
}

impl LivePipeline {
    pub fn new(config: LiveConfig) -> Self {
        let memory = TranslationMemory::new(config.memory_capacity);
        let context = DialogueContext::standard(
            config.app_id.as_deref().unwrap_or("default"),
        );
        let fuzzy_cache = FuzzyCache::new(config.memory_capacity / 2);
        Self {
            config,
            tracker: StabilityTracker::new(),
            memory,
            context,
            fuzzy_cache,
            stats: LiveStats::default(),
            last_engine_call: None,
            glossary_version: 0,
        }
    }

    /// Current pipeline statistics.
    pub fn stats(&self) -> &LiveStats {
        &self.stats
    }

    /// Current dialogue context.
    pub fn context(&self) -> &DialogueContext {
        &self.context
    }

    /// Translation memory for inspection or export.
    pub fn memory(&self) -> &TranslationMemory {
        &self.memory
    }

    /// Updates the glossary version, invalidating stale memory entries.
    pub fn set_glossary_version(&mut self, version: u64) {
        self.glossary_version = version;
    }

    /// Processes a frame's observations and returns blocks ready for rendering.
    ///
    /// This is the main entry point called once per frame:
    /// 1. Feed observations through the stability tracker
    /// 2. Identify blocks needing translation
    /// 3. Look up translation memory
    /// 4. Batch-translate remaining items
    /// 5. Store results and update context
    pub fn process_frame(
        &mut self,
        observations: &[Observation],
        engine: &mut dyn TranslationEngine,
    ) -> Vec<TranslatedBlock> {
        let frame_start = Instant::now();
        self.stats.frames += 1;

        // Step 1: Temporal stability — match observations to tracks
        let stable_blocks = self.tracker.observe(observations, &self.config.stability);
        self.stats.blocks = stable_blocks.len() as u64;

        // Step 2: Identify which blocks need translation
        let needs_translation: Vec<(usize, &StableBlock)> = stable_blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| b.needs_translation())
            .collect();

        if needs_translation.is_empty() {
            // Everything is cached — just return display blocks
            self.stats.last_frame_ms = frame_start.elapsed().as_millis() as u64;
            return self.build_display_blocks(&stable_blocks);
        }

        // Step 3: Check memory for cached translations
        let mut memory_hits = 0u64;
        let mut uncached: Vec<(usize, &StableBlock)> = Vec::new();

        for (idx, block) in &needs_translation {
            let key = MemoryKey::new(
                &block.source_text,
                self.config.source_language,
                self.config.target_language,
                self.glossary_version,
            );
            if let Some(record) = self.memory.get(&key) {
                self.tracker
                    .provide_translation(&block.source_text, record.translation.clone());
                memory_hits += 1;
            } else {
                uncached.push((*idx, *block));
            }
        }
        self.stats.cache_hits += memory_hits;

        // Step 4: Batch-translate uncached items (debounced)
        if !uncached.is_empty() {
            let should_translate = match self.last_engine_call {
                None => true,
                Some(last) => last.elapsed() >= self.config.min_batch_interval,
            };

            if should_translate {
                self.batch_translate(&uncached, engine);
                self.last_engine_call = Some(Instant::now());
            }
        }

        // Re-read stable blocks after translation updates
        let updated_blocks = self.tracker.tracks().to_vec();
        self.stats.last_frame_ms = frame_start.elapsed().as_millis() as u64;
        self.build_display_blocks(&updated_blocks)
    }

    /// Sends all uncached items to the engine in one batch and stores results.
    fn batch_translate(
        &mut self,
        uncached: &[(usize, &StableBlock)],
        engine: &mut dyn TranslationEngine,
    ) {
        let items: Vec<TranslateItem> = uncached
            .iter()
            .enumerate()
            .map(|(i, (_, block))| TranslateItem {
                id: i,
                text: block.source_text.clone(),
                kind: None, // Layout classification would be attached here
            })
            .collect();

        let context_text = if self.config.use_context {
            let ctx = self.context.format_prompt_context();
            if ctx.is_empty() {
                None
            } else {
                Some(ctx)
            }
        } else {
            None
        };

        let request = TranslationRequest {
            items,
            source_language: self.config.source_language,
            target_language: self.config.target_language,
            context: context_text,
            app_id: self.config.app_id.clone(),
        };

        match engine.translate(&request) {
            Ok(response) => {
                self.stats.engine_calls += 1;
                self.stats.engine_time_ms += response.duration_ms;
                self.stats.items_translated += response.items.len() as u64;
                self.stats.cache_misses += response.items.len() as u64;

                for translated in &response.items {
                    let source = &uncached[translated.id].1.source_text;

                    // Store in memory for future lookups
                    self.memory.insert(
                        MemoryKey::new(
                            source,
                            self.config.source_language,
                            self.config.target_language,
                            self.glossary_version,
                        ),
                        translated.translated.clone(),
                        &format!("{:?}", response.engine),
                        translated.confidence,
                    );

                    // Store in fuzzy cache for near-match lookups (OCR variants)
                    self.fuzzy_cache.insert(source.clone(), translated.translated.clone());

                    // Attach to the stability tracker
                    self.tracker
                        .provide_translation(source, translated.translated.clone());

                    // Update dialogue context for narrative continuity
                    if source.len() > 3 {
                        self.context.push(
                            None,
                            source,
                            Some(&translated.translated),
                        );
                    }
                }
            }
            Err(_err) => {
                // Engine failure: blocks will retry next frame via stability tracker.
                // No crash, no overlay corruption — source text is shown as fallback.
            }
        }
    }

    /// Builds the final display blocks from the current stable state.
    fn build_display_blocks(&self, blocks: &[StableBlock]) -> Vec<TranslatedBlock> {
        blocks
            .iter()
            .map(|block| {
                let display_text = block.display_text().to_owned();
                let is_new = block.translation.is_some()
                    && block.translated_source.as_deref() == Some(block.source_text.as_str());
                TranslatedBlock {
                    rect: block.display_rect,  // Use smoothed position
                    source_text: block.source_text.clone(),
                    display_text,
                    is_new,
                    confidence: block.confidence,
                    kind: None,
                    numeric_only: block.numeric_only,
                }
            })
            .collect()
    }

    /// Clears all cached translations (e.g. on language switch).
    pub fn clear_cache(&mut self) {
        self.memory = TranslationMemory::new(self.config.memory_capacity);
        self.context = DialogueContext::standard(
            self.config.app_id.as_deref().unwrap_or("default"),
        );
    }

    /// Resets temporal tracking (e.g. on window switch).
    pub fn reset_tracking(&mut self) {
        self.tracker = StabilityTracker::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::StubTranslationEngine;

    fn obs(text: &str, x: i32, y: i32) -> Observation {
        Observation {
            rect: Rect::new(x, y, 120, 24),
            text: text.to_owned(),
            confidence: 0.9,
        }
    }

    #[test]
    fn first_frame_translates_all_blocks() {
        let config = LiveConfig::default();
        let mut pipeline = LivePipeline::new(config);
        let mut engine = StubTranslationEngine::new();

        let observations = vec![
            obs("New Game", 10, 10),
            obs("Options", 10, 40),
        ];

        let blocks = pipeline.process_frame(&observations, &mut engine);
        assert_eq!(blocks.len(), 2);
        // Stub engine wraps unknown text in [ru: ...], known terms get dictionary translations
        assert_eq!(blocks[0].display_text, "Новая игра");
        assert_eq!(blocks[1].display_text, "Настройки");
    }

    #[test]
    fn second_frame_uses_cache_no_engine_call() {
        let config = LiveConfig::default();
        let mut pipeline = LivePipeline::new(config);
        let mut engine = StubTranslationEngine::new();

        let observations = vec![obs("New Game", 10, 10)];
        let _ = pipeline.process_frame(&observations, &mut engine);
        let calls_after_first = engine.call_count();

        let _ = pipeline.process_frame(&observations, &mut engine);
        // Engine should not be called again for the same text
        assert_eq!(engine.call_count(), calls_after_first);
        assert!(pipeline.stats().cache_hits > 0);
    }

    #[test]
    fn numeric_only_blocks_are_never_translated() {
        let config = LiveConfig::default();
        let mut pipeline = LivePipeline::new(config);
        let mut engine = StubTranslationEngine::new();

        let observations = vec![obs("87 / 100", 10, 10)];
        let blocks = pipeline.process_frame(&observations, &mut engine);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].numeric_only);
        assert_eq!(blocks[0].display_text, "87 / 100");
        assert_eq!(engine.call_count(), 0);
    }

    #[test]
    fn clear_cache_forces_retranslation() {
        let config = LiveConfig::default();
        let mut pipeline = LivePipeline::new(config);
        let mut engine = StubTranslationEngine::new();

        let observations = vec![obs("New Game", 10, 10)];
        let _ = pipeline.process_frame(&observations, &mut engine);
        let calls = engine.call_count();

        pipeline.clear_cache();
        pipeline.reset_tracking();

        let _ = pipeline.process_frame(&observations, &mut engine);
        assert!(engine.call_count() > calls);
    }

    #[test]
    fn stats_track_pipeline_metrics() {
        let config = LiveConfig::default();
        let mut pipeline = LivePipeline::new(config);
        let mut engine = StubTranslationEngine::new();

        let observations = vec![obs("New Game", 10, 10), obs("Quit", 10, 40)];
        let _ = pipeline.process_frame(&observations, &mut engine);

        assert_eq!(pipeline.stats().frames, 1);
        assert!(pipeline.stats().items_translated > 0);
        assert!(pipeline.stats().last_frame_ms < 1000);
    }
}
