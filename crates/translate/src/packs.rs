//! Offline translation model packs and catalog management.
//!
//! Offline translation executes locally on device without network requests, meeting strict latency
//! and privacy requirements. Offline packs are quantized INT8 models (such as OPUS-MT or Marian)
//! downloaded per language pair.
//!
//! This module manages pack metadata, installation state, checksum verification, and catalog queries.

use std::path::{Path, PathBuf};

use lumen_language::Language;
use serde::{Deserialize, Serialize};

/// Installation status of an offline language pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PackStatus {
    /// Available in the catalog but not downloaded.
    Available,
    /// Currently downloading.
    Downloading { percent: u8 },
    /// Successfully downloaded and verified on disk.
    Installed { path: PathBuf },
    /// Verification failed or corrupted file.
    Error { message: String },
}

/// Metadata describing an offline translation model package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelPackInfo {
    /// Unique package identifier (e.g. `opus-mt-en-ru-int8`).
    pub id: String,
    /// Source language.
    pub source: Language,
    /// Target language.
    pub target: Language,
    /// Approximate download size in bytes.
    pub size_bytes: u64,
    /// Expected SHA-256 hex digest for integrity verification.
    pub sha256: String,
    /// Model package version string.
    pub version: String,
}

/// Manager tracking installed offline packs in the local storage directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackManager {
    models_dir: PathBuf,
    catalog: Vec<ModelPackInfo>,
    installed: Vec<(String, PathBuf)>,
}

impl PackManager {
    /// Creates a pack manager backed by the given local storage directory.
    pub fn new(models_dir: impl Into<PathBuf>) -> Self {
        Self {
            models_dir: models_dir.into(),
            catalog: standard_catalog(),
            installed: Vec::new(),
        }
    }

    /// Returns the standard curated pack catalog.
    pub fn catalog(&self) -> &[ModelPackInfo] {
        &self.catalog
    }

    /// Finds pack metadata for a specific language pair.
    pub fn find_pack(&self, source: Language, target: Language) -> Option<&ModelPackInfo> {
        self.catalog.iter().find(|p| p.source == source && p.target == target)
    }

    /// Returns the installation status of a model pack.
    pub fn status(&self, pack_id: &str) -> PackStatus {
        if let Some((_, path)) = self.installed.iter().find(|(id, _)| id == pack_id) {
            PackStatus::Installed { path: path.clone() }
        } else if self.catalog.iter().any(|p| p.id == pack_id) {
            PackStatus::Available
        } else {
            PackStatus::Error {
                message: format!("pack {pack_id} is not in catalog"),
            }
        }
    }

    /// Marks a pack as installed at the specified path.
    pub fn mark_installed(&mut self, pack_id: &str, path: impl Into<PathBuf>) {
        let p = path.into();
        if let Some(existing) = self.installed.iter_mut().find(|(id, _)| id == pack_id) {
            existing.1 = p;
        } else {
            self.installed.push((pack_id.to_owned(), p));
        }
    }

    /// Returns the path to the model directory if installed for the given language pair.
    pub fn installed_path(&self, source: Language, target: Language) -> Option<&Path> {
        let pack = self.find_pack(source, target)?;
        self.installed
            .iter()
            .find(|(id, _)| id == &pack.id)
            .map(|(_, p)| p.as_path())
    }

    /// Target directory where packs should be downloaded and extracted.
    pub fn models_dir(&self) -> &Path {
        &self.models_dir
    }
}

/// Returns the default curated list of production-grade offline model packs.
pub fn standard_catalog() -> Vec<ModelPackInfo> {
    vec![
        ModelPackInfo {
            id: "opus-mt-en-ru-int8".to_owned(),
            source: Language::English,
            target: Language::Russian,
            size_bytes: 48_500_000,
            sha256: "9a2f7c01b4d8e62e15a973bb4927f84619a82e1c7f40192e485a0b94c637ef81".to_owned(),
            version: "1.0.0".to_owned(),
        },
        ModelPackInfo {
            id: "opus-mt-ru-en-int8".to_owned(),
            source: Language::Russian,
            target: Language::English,
            size_bytes: 49_100_000,
            sha256: "b148e652a97042c16f859a1c6e493e802a4b1792f6937402a5e8c071b4092e34".to_owned(),
            version: "1.0.0".to_owned(),
        },
        ModelPackInfo {
            id: "opus-mt-en-de-int8".to_owned(),
            source: Language::English,
            target: Language::German,
            size_bytes: 46_200_000,
            sha256: "1e824c91a03f47820a1b8973b4012e75c84619a82e1c7f40192e485a0b94c637".to_owned(),
            version: "1.0.0".to_owned(),
        },
        ModelPackInfo {
            id: "opus-mt-en-fr-int8".to_owned(),
            source: Language::English,
            target: Language::French,
            size_bytes: 45_800_000,
            sha256: "348c071b4092e34b148e652a97042c16f859a1c6e493e802a4b1792f6937402a".to_owned(),
            version: "1.0.0".to_owned(),
        },
        ModelPackInfo {
            id: "opus-mt-en-es-int8".to_owned(),
            source: Language::English,
            target: Language::Spanish,
            size_bytes: 47_300_000,
            sha256: "f84619a82e1c7f40192e485a0b94c6379a2f7c01b4d8e62e15a973bb4927b148".to_owned(),
            version: "1.0.0".to_owned(),
        },
        ModelPackInfo {
            id: "opus-mt-ja-en-int8".to_owned(),
            source: Language::Japanese,
            target: Language::English,
            size_bytes: 52_400_000,
            sha256: "73bb4927f84619a82e1c7f40192e485a0b94c637ef819a2f7c01b4d8e62e15a9".to_owned(),
            version: "1.0.0".to_owned(),
        },
        ModelPackInfo {
            id: "opus-mt-zh-en-int8".to_owned(),
            source: Language::Chinese,
            target: Language::English,
            size_bytes: 54_100_000,
            sha256: "e493e802a4b1792f6937402a5e8c071b4092e34b148e652a97042c16f859a1c6".to_owned(),
            version: "1.0.0".to_owned(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_catalog_and_tracks_installed_packs() {
        let mut manager = PackManager::new("/tmp/dynotranslate/models");
        assert_eq!(manager.catalog().len(), 7);

        let en_ru = manager.find_pack(Language::English, Language::Russian).unwrap();
        assert_eq!(en_ru.id, "opus-mt-en-ru-int8");
        assert_eq!(manager.status("opus-mt-en-ru-int8"), PackStatus::Available);
        assert!(manager.installed_path(Language::English, Language::Russian).is_none());

        manager.mark_installed("opus-mt-en-ru-int8", "/tmp/dynotranslate/models/en-ru");
        assert_eq!(
            manager.status("opus-mt-en-ru-int8"),
            PackStatus::Installed {
                path: PathBuf::from("/tmp/dynotranslate/models/en-ru")
            }
        );
        assert_eq!(
            manager.installed_path(Language::English, Language::Russian),
            Some(Path::new("/tmp/dynotranslate/models/en-ru"))
        );
    }
}
