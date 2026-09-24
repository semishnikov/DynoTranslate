//! Deduplication and fuzzy matching for OCR output.
//!
//! OCR is noisy: the same text can arrive as "HELLO WORLD", "HELL0 W0RLD", or "HELO WORLD"
//! across consecutive frames. Treating each variant as a distinct translation unit wastes
//! engine calls and produces flickering overlays. This module provides:
//!
//! - **Canonical normalization**: case folding, whitespace collapse, punctuation normalization
//! - **Edit-distance fuzzy matching**: Levenshtein-based similarity with configurable threshold
//! - **Cluster deduplication**: groups similar texts and picks the best reading as canonical
//! - **OCR error correction**: common OCR substitution patterns (O↔0, l↔1, rn↔m)

use std::collections::HashMap;

/// Threshold for fuzzy matching (0.0 = exact only, 1.0 = anything matches).
const DEFAULT_SIMILARITY_THRESHOLD: f32 = 0.85;

/// Common OCR character substitutions.
const OCR_SUBSTITUTIONS: &[(char, char)] = &[
    ('0', 'O'),
    ('1', 'l'),
    ('5', 'S'),
    ('8', 'B'),
    ('|', 'I'),
    ('`', '\''),
    (''', '\''),
    (''', '\''),
    ('"', '"'),
    ('"', '"'),
    ('…', '.'),
    ('—', '-'),
    ('–', '-'),
];

/// Normalizes text for comparison: lowercases, collapses whitespace, strips control characters,
/// and applies common OCR corrections.
pub fn canonicalize(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut prev_space = false;

    for ch in text.chars() {
        if ch.is_control() {
            continue;
        }

        // Apply OCR corrections
        let corrected = OCR_SUBSTITUTIONS
            .iter()
            .find(|(from, _)| *from == ch)
            .map(|(_, to)| *to)
            .unwrap_or(ch);

        // Collapse whitespace
        if corrected.is_whitespace() {
            if !prev_space && !result.is_empty() {
                result.push(' ');
                prev_space = true;
            }
            continue;
        }

        prev_space = false;
        // Lowercase for comparison (but preserve for display)
        for lower in corrected.to_lowercase() {
            result.push(lower);
        }
    }

    // Trim trailing space
    if result.ends_with(' ') {
        result.pop();
    }

    result
}

/// Computes the Levenshtein edit distance between two strings.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a_len = a.chars().count();
    let b_len = b.chars().count();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0; b_len + 1];

    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.chars().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j] + cost)
                .min(curr[j] + 1)
                .min(prev[j + 1] + 1);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

/// Computes similarity as a value in [0.0, 1.0] based on edit distance.
pub fn similarity(a: &str, b: &str) -> f32 {
    let max_len = a.chars().count().max(b.chars().count());
    if max_len == 0 {
        return 1.0;
    }
    let dist = edit_distance(a, b);
    1.0 - (dist as f32 / max_len as f32)
}

/// A cluster of similar texts with a chosen canonical reading.
#[derive(Debug, Clone)]
pub struct TextCluster {
    /// The canonical (best) reading for this cluster.
    pub canonical: String,
    /// All variant readings observed.
    pub variants: Vec<String>,
    /// Highest confidence among the variants.
    pub confidence: f32,
    /// Number of times this cluster has been observed.
    pub observations: u64,
}

/// Deduplicates similar OCR readings within a single frame.
#[derive(Debug)]
pub struct FrameDedup {
    threshold: f32,
}

impl FrameDedup {
    pub fn new() -> Self {
        Self {
            threshold: DEFAULT_SIMILARITY_THRESHOLD,
        }
    }

    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Deduplicates a list of (text, confidence) pairs, returning clusters.
    pub fn deduplicate(&self, readings: &[(String, f32)]) -> Vec<TextCluster> {
        let mut clusters: Vec<TextCluster> = Vec::new();
        let mut assigned = vec![false; readings.len()];

        for (i, (text_a, conf_a)) in readings.iter().enumerate() {
            if assigned[i] {
                continue;
            }
            assigned[i] = true;

            let canon_a = canonicalize(text_a);
            let mut cluster = TextCluster {
                canonical: text_a.clone(),
                variants: vec![text_a.clone()],
                confidence: *conf_a,
                observations: 1,
            };

            for (j, (text_b, conf_b)) in readings.iter().enumerate() {
                if assigned[j] || i == j {
                    continue;
                }
                let canon_b = canonicalize(text_b);
                let sim = similarity(&canon_a, &canon_b);
                if sim >= self.threshold {
                    assigned[j] = true;
                    cluster.variants.push(text_b.clone());
                    cluster.confidence = cluster.confidence.max(*conf_b);
                    cluster.observations += 1;

                    // Prefer the longer reading as canonical (more characters = less truncation)
                    if text_b.len() > cluster.canonical.len() {
                        cluster.canonical = text_b.clone();
                    }
                }
            }

            clusters.push(cluster);
        }

        clusters
    }
}

impl Default for FrameDedup {
    fn default() -> Self {
        Self::new()
    }
}

/// Cross-frame fuzzy cache: remembers translations for texts that are similar but not identical,
/// so a single OCR character flip doesn't trigger a full re-translation.
#[derive(Debug)]
pub struct FuzzyCache {
    threshold: f32,
    entries: Vec<(String, String)>,
    capacity: usize,
}

impl FuzzyCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            threshold: DEFAULT_SIMILARITY_THRESHOLD,
            entries: Vec::with_capacity(capacity),
            capacity: capacity.max(16),
        }
    }

    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Looks up a text, returning the cached translation if a similar enough key exists.
    pub fn get(&self, text: &str) -> Option<&str> {
        let canon = canonicalize(text);
        let mut best: Option<(f32, &str)> = None;

        for (key, value) in &self.entries {
            let key_canon = canonicalize(key);
            let sim = similarity(&canon, &key_canon);
            if sim >= self.threshold {
                match best {
                    None => best = Some((sim, value)),
                    Some((best_sim, _)) if sim > best_sim => best = Some((sim, value)),
                    _ => {}
                }
            }
        }

        best.map(|(_, v)| v)
    }

    /// Stores a translation, evicting the oldest entry if at capacity.
    pub fn insert(&mut self, source: String, translation: String) {
        // Check if we already have a similar entry
        let canon = canonicalize(&source);
        for (key, value) in &mut self.entries {
            let key_canon = canonicalize(key);
            if similarity(&canon, &key_canon) >= self.threshold {
                *key = source;
                *value = translation;
                return;
            }
        }

        // Evict oldest if at capacity
        if self.entries.len() >= self.capacity {
            self.entries.remove(0);
        }

        self.entries.push((source, translation));
    }

    /// Number of entries currently cached.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clears all cached entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalize_normalizes_whitespace_and_case() {
        assert_eq!(canonicalize("Hello   World"), "hello world");
        assert_eq!(canonicalize("  HELLO  "), "hello");
        assert_eq!(canonicalize("HELLO\tWORLD"), "hello world");
    }

    #[test]
    fn canonicalize_applies_ocr_corrections() {
        assert_eq!(canonicalize("HELL0"), "hello");
        assert_eq!(canonicalize("W0RLD"), "world");
    }

    #[test]
    fn edit_distance_computes_correctly() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("abc", "abc"), 0);
        assert_eq!(edit_distance("abc", "axc"), 1);
    }

    #[test]
    fn similarity_is_one_for_identical_strings() {
        assert_eq!(similarity("hello", "hello"), 1.0);
        assert_eq!(similarity("", ""), 1.0);
    }

    #[test]
    fn similarity_decreases_with_edits() {
        let s = similarity("hello world", "hello worlx");
        assert!(s > 0.8, "{}", s);
        assert!(s < 1.0);
    }

    #[test]
    fn frame_dedup_merges_similar_texts() {
        let dedup = FrameDedup::new();
        let readings = vec![
            ("Hello World".to_owned(), 0.9),
            ("Hello Worlx".to_owned(), 0.7),
            ("Completely Different".to_owned(), 0.8),
        ];
        let clusters = dedup.deduplicate(&readings);
        assert_eq!(clusters.len(), 2);
        // "Hello World" and "Hello Worlx" should be merged
        let hello_cluster = clusters.iter().find(|c| c.canonical.contains("Hello")).unwrap();
        assert_eq!(hello_cluster.variants.len(), 2);
    }

    #[test]
    fn frame_dedup_keeps_distinct_texts_separate() {
        let dedup = FrameDedup::new();
        let readings = vec![
            ("Hello".to_owned(), 0.9),
            ("Goodbye".to_owned(), 0.9),
        ];
        let clusters = dedup.deduplicate(&readings);
        assert_eq!(clusters.len(), 2);
    }

    #[test]
    fn fuzzy_cache_returns_similar_translation() {
        let mut cache = FuzzyCache::new(100);
        cache.insert("Hello World".to_owned(), "Привет мир".to_owned());

        // Exact match
        assert_eq!(cache.get("Hello World"), Some("Привет мир"));

        // Similar match (one char different)
        assert_eq!(cache.get("Hello Worlx"), Some("Привет мир"));
    }

    #[test]
    fn fuzzy_cache_evicts_oldest() {
        let mut cache = FuzzyCache::new(3);
        cache.insert("one".to_owned(), "один".to_owned());
        cache.insert("two".to_owned(), "два".to_owned());
        cache.insert("three".to_owned(), "три".to_owned());
        cache.insert("four".to_owned(), "четыре".to_owned());

        assert_eq!(cache.len(), 3);
        assert!(cache.get("one").is_none());
        assert!(cache.get("four").is_some());
    }

    #[test]
    fn fuzzy_cache_updates_existing_similar_entry() {
        let mut cache = FuzzyCache::new(100);
        cache.insert("Hello World".to_owned(), "Привет мир".to_owned());
        cache.insert("Hello Worlx".to_owned(), "Обновлённый перевод".to_owned());

        // Should update the existing entry, not add a new one
        assert_eq!(cache.len(), 1);
    }
}
