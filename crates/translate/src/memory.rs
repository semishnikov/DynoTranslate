//! Translation memory and bounded cache with persistence and auto-cleanup.
//!
//! Repeatedly translating static UI elements (buttons, menus, recurring dialog) adds latency,
//! drains battery and wastes tokens. The translation memory caches previously translated blocks
//! indexed by normalized source text, language pair, and glossary version. When a glossary term
//! changes, its version bump automatically isolates subsequent lookups from stale translations.
//!
//! # Persistence
//!
//! Memory can be persisted to a JSON file so translations survive application restarts.
//! The file is written atomically (write to temp, rename) and loaded on startup.
//!
//! # Auto-cleanup
//!
//! Entries are evicted by two policies:
//! - **Capacity (LRU)**: when the entry count exceeds `capacity`, the least recently used entry
//!   is removed.
//! - **TTL**: entries not accessed within `ttl_seconds` are pruned during `compact()`.
//!
//! The caller controls cleanup frequency: calling `compact()` periodically (e.g. every N frames
//! or on application idle) keeps the store bounded without background threads.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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
    /// Unix timestamp (seconds) of last access.
    pub last_access: u64,
    /// Unix timestamp (seconds) of creation.
    pub created: u64,
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
    /// Total entries pruned by TTL during compaction.
    pub pruned: u64,
}

/// Configuration for translation memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Maximum number of entries in memory.
    pub capacity: usize,
    /// Time-to-live in seconds. Entries not accessed within this period are pruned
    /// during `compact()`. Zero means no TTL.
    pub ttl_seconds: u64,
    /// Optional file path for persistence.
    pub persist_path: Option<PathBuf>,
    /// Whether to write to disk after every insert (slower but crash-safe).
    pub write_through: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            capacity: 4096,
            ttl_seconds: 86_400 * 7, // 7 days
            persist_path: None,
            write_through: false,
        }
    }
}

/// Persistent data format for serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistentStore {
    version: u32,
    entries: Vec<(MemoryKey, MemoryRecord)>,
}

/// In-memory bounded cache with optional persistence and TTL-based auto-cleanup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationMemory {
    capacity: usize,
    ttl_seconds: u64,
    entries: HashMap<MemoryKey, MemoryRecord>,
    access_order: Vec<MemoryKey>,
    stats: MemoryStats,
    #[serde(skip)]
    persist_path: Option<PathBuf>,
    #[serde(skip)]
    write_through: bool,
    /// Dirty flag: set when entries change since last save.
    #[serde(skip)]
    dirty: bool,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl TranslationMemory {
    /// Creates a new translation memory with the given maximum entry capacity.
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.max(16);
        Self {
            capacity: cap,
            ttl_seconds: 0,
            entries: HashMap::with_capacity(cap),
            access_order: Vec::with_capacity(cap),
            stats: MemoryStats::default(),
            persist_path: None,
            write_through: false,
            dirty: false,
        }
    }

    /// Creates a memory from a full configuration.
    pub fn with_config(config: &MemoryConfig) -> Self {
        let cap = config.capacity.max(16);
        let mut mem = Self {
            capacity: cap,
            ttl_seconds: config.ttl_seconds,
            entries: HashMap::with_capacity(cap),
            access_order: Vec::with_capacity(cap),
            stats: MemoryStats::default(),
            persist_path: config.persist_path.clone(),
            write_through: config.write_through,
            dirty: false,
        };

        // Try to load from disk
        if let Some(path) = &config.persist_path {
            if let Ok(()) = mem.load(path) {
                // Loaded successfully
            }
        }

        mem
    }

    /// Default configuration suitable for standard sessions (4096 entries).
    pub fn standard() -> Self {
        Self::new(4096)
    }

    /// Looks up an entry. Updates access order and hit count on success.
    pub fn get(&mut self, key: &MemoryKey) -> Option<&MemoryRecord> {
        if let Some(record) = self.entries.get_mut(key) {
            record.hit_count = record.hit_count.saturating_add(1);
            record.last_access = now_unix();
            self.stats.hits += 1;

            // Touch access order (move key to back)
            if let Some(idx) = self.access_order.iter().position(|k| k == key) {
                let touched = self.access_order.remove(idx);
                self.access_order.push(touched);
            }
            self.entries.get(key)
        } else {
            self.stats.misses += 1;
            None
        }
    }

    /// Inserts or updates an entry, evicting the least recently used entry if over capacity.
    pub fn insert(&mut self, key: MemoryKey, text: impl Into<String>, engine: &str, score: f32) {
        let trans = text.into();
        let now = now_unix();

        if let Some(existing) = self.entries.get_mut(&key) {
            existing.translation = trans;
            existing.engine = engine.to_owned();
            existing.confidence = score;
            existing.last_access = now;
            self.dirty = true;
            if self.write_through {
                let _ = self.save_to_disk();
            }
            return;
        }

        if self.entries.len() >= self.capacity && !self.access_order.is_empty() {
            let oldest = self.access_order.remove(0);
            self.entries.remove(&oldest);
            self.stats.evictions += 1;
        }

        self.access_order.push(key.clone());
        self.entries.insert(
            key,
            MemoryRecord {
                translation: trans,
                engine: engine.to_owned(),
                confidence: score,
                hit_count: 0,
                last_access: now,
                created: now,
            },
        );
        self.stats.inserts += 1;
        self.dirty = true;

        if self.write_through {
            let _ = self.save_to_disk();
        }
    }

    /// Prunes entries that have not been accessed within the TTL window.
    /// Call this periodically (e.g. every 100 frames or on idle) to keep the store bounded.
    pub fn compact(&mut self) {
        if self.ttl_seconds == 0 {
            return;
        }

        let cutoff = now_unix().saturating_sub(self.ttl_seconds);
        let before = self.entries.len();

        self.access_order.retain(|key| {
            if let Some(record) = self.entries.get(key) {
                record.last_access >= cutoff
            } else {
                false
            }
        });

        self.entries.retain(|_, record| record.last_access >= cutoff);

        let pruned = before - self.entries.len();
        self.stats.pruned += pruned as u64;
        if pruned > 0 {
            self.dirty = true;
        }
    }

    /// Saves the current state to disk atomically (write to temp, rename).
    pub fn save_to_disk(&mut self) -> Result<(), String> {
        let path = match &self.persist_path {
            Some(p) => p.clone(),
            None => return Ok(()),
        };

        let store = PersistentStore {
            version: 1,
            entries: self.entries.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        };

        let json = serde_json::to_string(&store).map_err(|e| e.to_string())?;
        let tmp_path = path.with_extension("tmp");

        std::fs::write(&tmp_path, json.as_bytes()).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp_path, &path).map_err(|e| e.to_string())?;

        self.dirty = false;
        Ok(())
    }

    /// Loads state from a JSON file.
    fn load(&mut self, path: &Path) -> Result<(), String> {
        let data = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let store: PersistentStore = serde_json::from_str(&data).map_err(|e| e.to_string())?;

        if store.version != 1 {
            return Err(format!("unsupported store version: {}", store.version));
        }

        self.entries.clear();
        self.access_order.clear();

        for (key, record) in store.entries {
            self.access_order.push(key.clone());
            self.entries.insert(key, record);
        }

        // Trim to capacity if the file had more entries
        while self.entries.len() > self.capacity && !self.access_order.is_empty() {
            let oldest = self.access_order.remove(0);
            self.entries.remove(&oldest);
        }

        self.dirty = false;
        Ok(())
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
        self.dirty = true;
    }

    /// Whether there are unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Sets the persistence path.
    pub fn set_persist_path(&mut self, path: PathBuf) {
        self.persist_path = Some(path);
    }

    /// Estimated memory usage in bytes.
    pub fn memory_usage(&self) -> usize {
        self.entries
            .iter()
            .map(|(k, v)| {
                k.source.len() + v.translation.len() + v.engine.len() + 128 // overhead
            })
            .sum()
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
        let mut memory = TranslationMemory::new(16);
        for i in 0..16 {
            let key = MemoryKey::new(&format!("Word {i}"), Language::English, Language::Russian, 1);
            memory.insert(key, format!("Слово {i}"), "stub", 1.0);
        }
        assert_eq!(memory.len(), 16);

        let key_0 = MemoryKey::new("Word 0", Language::English, Language::Russian, 1);
        assert!(memory.get(&key_0).is_some());

        let key_16 = MemoryKey::new("Word 16", Language::English, Language::Russian, 1);
        memory.insert(key_16, "Слово 16", "stub", 1.0);

        assert_eq!(memory.len(), 16);
        assert_eq!(memory.stats().evictions, 1);

        let key_1 = MemoryKey::new("Word 1", Language::English, Language::Russian, 1);
        assert!(memory.get(&key_1).is_none());
        assert!(memory.get(&key_0).is_some());
    }

    #[test]
    fn memory_usage_is_positive_for_nonempty_memory() {
        let mut memory = TranslationMemory::new(100);
        let key = MemoryKey::new("Hello", Language::English, Language::Russian, 1);
        memory.insert(key, "Привет", "test", 1.0);
        assert!(memory.memory_usage() > 0);
    }

    #[test]
    fn records_have_timestamps() {
        let mut memory = TranslationMemory::new(100);
        let key = MemoryKey::new("Test", Language::English, Language::Russian, 1);
        memory.insert(key.clone(), "Тест", "test", 1.0);
        let record = memory.get(&key).unwrap();
        assert!(record.created > 0);
        assert!(record.last_access > 0);
    }
}
