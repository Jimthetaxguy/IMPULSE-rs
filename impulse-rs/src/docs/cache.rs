// Cache module - store and retrieve fetched documentation
// Persists model info and docs to local files

use super::{ModelInfo, Provider};
use crate::storage::Storage;
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Cache directory for docs
pub struct DocsCache {
    base_path: std::path::PathBuf,
}

impl DocsCache {
    pub fn new(base_path: std::path::PathBuf) -> Self {
        Self { base_path }
    }

    fn models_path(&self) -> std::path::PathBuf {
        self.base_path.join("models.json")
    }

    fn providers_path(&self) -> std::path::PathBuf {
        self.base_path.join("providers.json")
    }

    fn metadata_path(&self) -> std::path::PathBuf {
        self.base_path.join("cache_metadata.json")
    }

    /// Save models to cache
    pub fn save_models(&self, models: &[ModelInfo]) -> Result<()> {
        let json =
            serde_json::to_string_pretty(models).context("failed to serialize cached models")?;
        Storage::atomic_write_path(&self.models_path(), json.as_bytes())
            .context("failed to write models cache file")?;
        Ok(())
    }

    /// Load models from cache
    pub fn load_models(&self) -> Result<Vec<ModelInfo>> {
        let path = self.models_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let json = fs::read_to_string(&path).context("failed to read models cache file")?;
        let models: Vec<ModelInfo> =
            serde_json::from_str(&json).context("failed to parse models cache JSON")?;
        Ok(models)
    }

    /// Save providers to cache
    pub fn save_providers(&self, providers: &[Provider]) -> Result<()> {
        let json = serde_json::to_string_pretty(providers)
            .context("failed to serialize cached providers")?;
        Storage::atomic_write_path(&self.providers_path(), json.as_bytes())
            .context("failed to write providers cache file")?;
        Ok(())
    }

    /// Load providers from cache
    pub fn load_providers(&self) -> Result<Vec<Provider>> {
        let path = self.providers_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let json = fs::read_to_string(&path).context("failed to read providers cache file")?;
        let providers: Vec<Provider> =
            serde_json::from_str(&json).context("failed to parse providers cache JSON")?;
        Ok(providers)
    }

    /// Save cache metadata (timestamps, etc.)
    pub fn save_metadata(&self, metadata: &CacheMetadata) -> Result<()> {
        let json =
            serde_json::to_string_pretty(metadata).context("failed to serialize cache metadata")?;
        Storage::atomic_write_path(&self.metadata_path(), json.as_bytes())
            .context("failed to write cache metadata file")?;
        Ok(())
    }

    /// Load cache metadata
    pub fn load_metadata(&self) -> Result<CacheMetadata> {
        let path = self.metadata_path();
        if !path.exists() {
            return Ok(CacheMetadata::default());
        }
        let json = fs::read_to_string(&path).context("failed to read cache metadata file")?;
        let metadata: CacheMetadata =
            serde_json::from_str(&json).context("failed to parse cache metadata JSON")?;
        Ok(metadata)
    }

    /// Check if cache is stale (older than specified duration)
    pub fn is_stale(&self, max_age: std::time::Duration) -> bool {
        if let Ok(metadata) = self.load_metadata() {
            let age = std::time::SystemTime::now()
                .duration_since(metadata.last_updated)
                .unwrap_or_default();
            age > max_age
        } else {
            true
        }
    }

    /// Get cache age in seconds
    pub fn age_seconds(&self) -> Option<u64> {
        self.load_metadata().ok().and_then(|m| {
            std::time::SystemTime::now()
                .duration_since(m.last_updated)
                .ok()
                .map(|d| d.as_secs())
        })
    }

    /// Clear the cache
    pub fn clear(&self) -> Result<()> {
        let _ = fs::remove_file(self.models_path());
        let _ = fs::remove_file(self.providers_path());
        let _ = fs::remove_file(self.metadata_path());
        Ok(())
    }
}

/// Metadata about the cached data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CacheMetadata {
    pub last_updated: std::time::SystemTime,
    pub model_count: usize,
    pub provider_count: usize,
    pub source: String,
}

impl Default for CacheMetadata {
    fn default() -> Self {
        Self {
            last_updated: std::time::SystemTime::UNIX_EPOCH,
            model_count: 0,
            provider_count: 0,
            source: "unknown".to_string(),
        }
    }
}

/// Create a new cache at the specified path
pub fn create_cache(base_path: &Path) -> Result<DocsCache> {
    let cache_dir = base_path.join("docs_cache");
    fs::create_dir_all(&cache_dir).context("failed to create docs cache directory")?;
    Ok(DocsCache::new(cache_dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    #[test]
    fn test_cache_metadata_default() {
        let meta = CacheMetadata::default();
        // Should have epoch as default time
        assert_eq!(meta.last_updated, SystemTime::UNIX_EPOCH);
    }

    #[test]
    fn test_is_stale() {
        let temp_dir = std::env::temp_dir().join("impulse_test_cache");
        let _ = std::fs::create_dir_all(&temp_dir);
        let cache = DocsCache::new(temp_dir.clone());

        // New cache should be stale
        assert!(cache.is_stale(Duration::from_secs(1)));

        // Save metadata with current time
        let meta = CacheMetadata {
            last_updated: SystemTime::now(),
            model_count: 10,
            provider_count: 5,
            source: "test".to_string(),
        };
        cache.save_metadata(&meta).unwrap();

        // Small delay to ensure filesystem timestamp is different
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Now shouldn't be stale for a day
        assert!(!cache.is_stale(Duration::from_secs(86400)));

        // But should be stale for 0 seconds
        assert!(cache.is_stale(Duration::from_secs(0)));

        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
