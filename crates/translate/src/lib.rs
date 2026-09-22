//! `lumen-translate`: translation pipeline stage, token protection, translation memory,
//! offline pack management, glossaries, context history and engine fallback.
//!
//! This crate is portable across Windows, Linux and macOS, containing no platform-specific code.

pub mod context;
pub mod engine;
pub mod fallback;
pub mod glossary;
pub mod memory;
pub mod packs;
pub mod token;

pub use context::{ContextTurn, DialogueContext};
pub use engine::{
    EngineKind, StubTranslationEngine, TranslateItem, TranslatedItem, TranslationEngine, TranslationError,
    TranslationRequest, TranslationResponse,
};
pub use fallback::{CircuitBreaker, CircuitState, FallbackEngine};
pub use glossary::{Glossary, GlossaryEntry};
pub use memory::{MemoryKey, MemoryRecord, MemoryStats, TranslationMemory};
pub use packs::{ModelPackInfo, PackManager, PackStatus};
pub use token::{normalize, protect, restore, ProtectedToken, TokenKind};
