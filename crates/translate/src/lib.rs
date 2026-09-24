//! `lumen-translate`: translation pipeline stage, token protection, translation memory,
//! offline pack management, glossaries, context history, engine fallback, live orchestration,
//! deduplication and fuzzy matching.
//!
//! This crate is portable across Windows, Linux and macOS, containing no platform-specific code.
//!
//! # Architecture
//!
//! ```text
//!   Observations → Dedup → Memory lookup → Batch translate → Cache store → Display
//! ```
//!
//! The [`live::LivePipeline`] orchestrates the full production path: temporal stability,
//! translation memory, context-aware batching, and bounded caching with automatic cleanup.
//!
//! # Engines
//!
//! - [`StubTranslationEngine`] — deterministic double for tests and benchmarks
//! - [`GoogleEngine`] — Google Cloud Translation API v2 with batching
//! - [`DeepLEngine`] — DeepL API for highest-quality European/Asian translations
//! - [`LlmEngine`] — LLM-based contextual translation (GPT, Claude, Gemini, Ollama)
//! - [`FallbackEngine`] — composite engine with circuit breaker and automatic fallback

pub mod context;
pub mod dedup;
pub mod deepl;
pub mod engine;
pub mod fallback;
pub mod glossary;
pub mod google;
pub mod google_free;
pub mod live;
pub mod llm;
pub mod memory;
pub mod packs;
pub mod token;

pub use context::{ContextTurn, DialogueContext};
pub use dedup::{canonicalize, edit_distance, similarity, FuzzyCache, FrameDedup, TextCluster};
pub use deepl::{DeepLConfig, DeepLEngine, Formality};
pub use engine::{
    EngineKind, StubTranslationEngine, TranslateItem, TranslatedItem, TranslationEngine,
    TranslationError, TranslationRequest, TranslationResponse,
};
pub use fallback::{CircuitBreaker, CircuitState, FallbackEngine};
pub use glossary::{Glossary, GlossaryEntry};
pub use google::{GoogleConfig, GoogleEngine};
pub use google_free::GoogleFreeEngine;
pub use live::{LiveConfig, LivePipeline, LiveStats, TranslatedBlock};
pub use llm::{LlmConfig, LlmEngine, LlmProvider};
pub use memory::{MemoryKey, MemoryRecord, MemoryStats, TranslationMemory};
pub use packs::{ModelPackInfo, PackManager, PackStatus};
pub use token::{normalize, protect, restore, ProtectedToken, TokenKind};
