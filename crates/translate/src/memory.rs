//! Translation memory and bounded cache.
//!
//! Repeatedly translating static UI elements (buttons, menus, recurring dialog) adds latency,
//! drains battery and wastes tokens. The translation memory caches previously translated blocks
//! indexed by normalized source text, language pair, and glossary version. When a glossary term
//! changes, its version bump automatically isolates subsequent lookups from stale translations.

use std::collections::HashMap;

use lumen_language::Language;
use serde::{Deserialize, Serialize};

/// The lookup key identifying a distinct translation unit.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MemoryKey {
    /// Normalized source string.
    pub source: String,
    /// Detected or declared source language.
    pub source_language: Language,
    /// Desired target language.
    pub target_language: Language,
    /// Active glossary version (isolates entries across term modifications).
    pub glossary_version: u64,
}

impl MemoryKey {
    /// Constructs a new lookup key.
    pub fn new(source: &str, source_lang: Language, target_lang: Language, version: u64) -> Self {
        Self {
            source: crate::token::normalize(source),
            source_language: source_lang,
            target_language: target_lang,
            glossary_version: version,
        }
    }
}

/// A stored translation entry with usage metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryRecord {
    /// The translated text.
    pub translation: String,
    /// The engine that produced the translation (e.g. "offline", "online", "manual").
    pub engine: String,
    /// Estimated confidence score (0.0 to 1.0).
    pub confidence: f32,
    /// How many times this entry has been hit in cache.
    pub hit_count: u64,
}

/// Statistics for translation memory performance monitoring.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryStats {
    /// Total successful cache hits.
    pub hits: u64,
    /// Total cache misses requiring an engine call.
    pub misses: u64,
    /// Total new records inserted.
    pub inserts: u64,
    /// Total entries evicted due to capacity bounds.
    pub evictions: u64,
}

/// In-memory bounded cache and persistent translation store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationMemory {
    capacity: usize,
    entries: HashMap<MemoryKey, MemoryRecord>,
    access_order: Vec<MemoryKey>,
    stats: MemoryStats,
}

impl TranslationMemory {
    /// Creates a new translation memory with the given maximum entry capacity.
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.max(16);
        Self {
            capacity: cap,
            entries: HashMap::with_capacity(cap),
            access_order: Vec::with_capacity(cap),
            stats: MemoryStats::default(),
        }
    }

    /// Default configuration suitable for standard sessions (4096 entries).
    pub fn standard() -> Self {
        Self::new(4096)
    }

    /// Looks up an entry. Updates access order and hit count on success.
    pub fn get(&mut self, key: &MemoryKey) -> Option<&MemoryRecord> {
        if let Some(record) = self.entries.get_mut(key) {
            record.hit_count = record.hit_count.saturating_add(1);
            self.stats.hits += 1;

            // Touch access order (move key to back)
            if let Some(idx) = self.access_order.iter().position(|k| k == key) {
                let touched = self.access_order.remove(idx);
                self.access_order.push(touched);
            }
            // Return immutable reference
            self.entries.get(key)
        } else {
            self.stats.misses += 1;
            None
        }
    }

    /// Inserts or updates an entry, evicting the least recently used entry if over capacity.
    pub fn insert(&mut self, key: MemoryKey, text: impl Into<String>, engine: &str, score: f32) {
        let trans = text.into();
        if let Some(existing) = self.entries.get_mut(&key) {
            existing.translation = trans;
            existing.engine = engine.to_owned();
            existing.confidence = score;
            return;
        }

        if self.entries.len() >= self.capacity {
            if !self.access_order.is_empty() {
                let oldest = self.access_order.remove(0);
                self.entries.remove(&oldest);
                self.stats.evictions += 1;
            }
        }

        self.access_order.push(key.clone());
        self.entries.insert(
            key,
            MemoryRecord {
                translation: trans,
                engine: engine.to_owned(),
                confidence: score,
                hit_count: 0,
            },
        );
        self.stats.inserts += 1;
    }

    /// Returns current cache performance metrics.
    pub fn stats(&self) -> MemoryStats {
        self.stats
    }

    /// Total active entries currently in memory.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the memory contains zero entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clears all stored entries and resets order.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.access_order.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_retrieves_and_records_hits() {
        let mut memory = TranslationMemory::new(100);
        let key = MemoryKey::new("Attack", Language::English, Language::Russian, 1);

        assert!(memory.get(&key).is_none());
        assert_eq!(memory.stats().misses, 1);

        memory.insert(key.clone(), "Атаковать", "offline", 0.99);
        assert_eq!(memory.len(), 1);
        assert_eq!(memory.stats().inserts, 1);

        let hit = memory.get(&key).unwrap();
        assert_eq!(hit.translation, "Атаковать");
        assert_eq!(hit.hit_count, 1);
        assert_eq!(memory.stats().hits, 1);
    }

    #[test]
    fn glossary_version_change_isolates_stale_cache() {
        let mut memory = TranslationMemory::new(100);
        let key_v1 = MemoryKey::new("Potion", Language::English, Language::Russian, 1);
        let key_v2 = MemoryKey::new("Potion", Language::English, Language::Russian, 2);

        memory.insert(key_v1.clone(), "Зелье", "offline", 0.95);
        assert!(memory.get(&key_v1).is_some());
        assert!(memory.get(&key_v2).is_none());
    }

    #[test]
    fn evicts_least_recently_used_when_capacity_reached() {
        let mut memory = TranslationMemory::new(16); // minimum clamp is 16
        for i in 0..16 {
            let key = MemoryKey::new(&format!("Word {i}"), Language::English, Language::Russian, 1);
            memory.insert(key, format!("Слово {i}"), "stub", 1.0);
        }
        assert_eq!(memory.len(), 16);

        // Touch Word 0
        let key_0 = MemoryKey::new("Word 0", Language::English, Language::Russian, 1);
        assert!(memory.get(&key_0).is_some());

        // Insert 17th item -> oldest untouched item (Word 1) must be evicted
        let key_16 = MemoryKey::new("Word 16", Language::English, Language::Russian, 1);
        memory.insert(key_16, "Слово 16", "stub", 1.0);

        assert_eq!(memory.len(), 16);
        assert_eq!(memory.stats().evictions, 1);

        let key_1 = MemoryKey::new("Word 1", Language::English, Language::Russian, 1);
        assert!(memory.get(&key_1).is_none());
        assert!(memory.get(&key_0).is_some());
    }
}
