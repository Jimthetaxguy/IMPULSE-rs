use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// PageIndex - A lightweight file indexing system for code search
/// Uses heading-based scoring to help agents find relevant files

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageDoc {
    pub path: String,
    pub name: String,
    pub headings: Vec<String>,
    pub body: String,
    pub file_type: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageIndex {
    pub docs: HashMap<String, PageDoc>,
    pub indexed_at: chrono::DateTime<chrono::Utc>,
    pub root_path: String,
}

impl Default for PageIndex {
    fn default() -> Self {
        Self {
            docs: HashMap::new(),
            indexed_at: chrono::Utc::now(),
            root_path: String::new(),
        }
    }
}

impl PageIndex {
    pub fn new(root_path: &Path) -> Self {
        Self {
            docs: HashMap::new(),
            indexed_at: chrono::Utc::now(),
            root_path: root_path.to_string_lossy().to_string(),
        }
    }

    /// Build index from a directory
    pub fn build(&mut self, root_path: &Path, extensions: &[&str]) -> std::io::Result<usize> {
        self.docs.clear();
        self.root_path = root_path.to_string_lossy().to_string();
        self.indexed_at = chrono::Utc::now();

        let mut count = 0;
        self.walk_dir(root_path, extensions, &mut count)?;
        Ok(count)
    }

    fn walk_dir(
        &mut self,
        dir: &Path,
        extensions: &[&str],
        count: &mut usize,
    ) -> std::io::Result<()> {
        if !dir.is_dir() {
            return Ok(());
        }

        // Skip common directories that shouldn't be indexed
        let skip_dirs = [
            ".git",
            "node_modules",
            "target",
            "dist",
            "build",
            ".impulse",
            ".venv",
            "venv",
        ];
        if let Some(name) = dir.file_name() {
            if skip_dirs.contains(&name.to_string_lossy().as_ref()) {
                return Ok(());
            }
        }

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                self.walk_dir(&path, extensions, count)?;
            } else if path.is_file() {
                if let Some(ext) = path.extension() {
                    let ext_raw = ext.to_string_lossy();
                    let ext_str = format!(".{}", ext_raw);
                    if extensions
                        .iter()
                        .any(|e| *e == ext_str || *e == ext_raw.as_ref())
                    {
                        if let Ok(doc) = self.index_file(&path) {
                            let key = path.to_string_lossy().to_string();
                            self.docs.insert(key, doc);
                            *count += 1;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn index_file(&self, path: &Path) -> std::io::Result<PageDoc> {
        let content = fs::read_to_string(path)?;
        let metadata = fs::metadata(path)?;

        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let file_type = path
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default();

        let headings = extract_headings(&content, &file_type);

        Ok(PageDoc {
            path: path.to_string_lossy().to_string(),
            name,
            headings,
            body: content,
            file_type,
            size_bytes: metadata.len(),
        })
    }

    /// Search the index with PageIndex scoring algorithm
    pub fn search(&self, query: &str, limit: usize) -> Vec<PageIndexResult> {
        if query.trim().is_empty() {
            return Vec::new();
        }

        let terms: Vec<String> = query
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();

        if terms.is_empty() {
            return Vec::new();
        }

        let mut scored: Vec<(String, f64)> = self
            .docs
            .iter()
            .map(|(path, doc)| {
                let score = score_document(query, &terms, doc);
                (path.clone(), score)
            })
            .filter(|(_, score)| *score > 0.0)
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        scored
            .into_iter()
            .take(limit)
            .filter_map(|(path, score)| {
                self.docs.get(&path).map(|doc| PageIndexResult {
                    path: doc.path.clone(),
                    name: doc.name.clone(),
                    score,
                    matched_headings: find_matched_headings(&terms, &doc.headings),
                    file_type: doc.file_type.clone(),
                })
            })
            .collect()
    }

    /// Save index to file using atomic temp + rename
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let tmp_path = path.with_extension(format!(
            "tmp.{}.{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::write(&tmp_path, json)?;
        fs::rename(tmp_path, path)?;
        Ok(())
    }

    /// Load index from file
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let content = fs::read_to_string(path)?;
        let index: PageIndex = serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(index)
    }

    /// Get index statistics
    pub fn stats(&self) -> PageIndexStats {
        let mut by_type: HashMap<String, usize> = HashMap::new();
        let mut total_size = 0u64;

        for doc in self.docs.values() {
            *by_type.entry(doc.file_type.clone()).or_insert(0) += 1;
            total_size += doc.size_bytes;
        }

        PageIndexStats {
            total_docs: self.docs.len(),
            total_size_bytes: total_size,
            by_type,
            indexed_at: self.indexed_at,
        }
    }
}

/// PageIndex search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageIndexResult {
    pub path: String,
    pub name: String,
    pub score: f64,
    pub matched_headings: Vec<String>,
    pub file_type: String,
}

/// PageIndex statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageIndexStats {
    pub total_docs: usize,
    pub total_size_bytes: u64,
    pub by_type: HashMap<String, usize>,
    pub indexed_at: chrono::DateTime<chrono::Utc>,
}

/// Extract headings from document content based on file type
fn extract_headings(content: &str, file_type: &str) -> Vec<String> {
    let mut headings = Vec::new();

    match file_type {
        "md" | "markdown" => {
            // Extract markdown headings
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('#') {
                    let heading = trimmed.trim_start_matches('#').trim().to_string();
                    if !heading.is_empty() {
                        headings.push(heading);
                    }
                }
            }
        }
        "rs" => {
            // Extract Rust items: fn, struct, enum, impl, mod, trait, const, type
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("pub fn ")
                    || trimmed.starts_with("fn ")
                    || trimmed.starts_with("pub struct ")
                    || trimmed.starts_with("struct ")
                    || trimmed.starts_with("pub enum ")
                    || trimmed.starts_with("enum ")
                    || trimmed.starts_with("pub trait ")
                    || trimmed.starts_with("trait ")
                    || trimmed.starts_with("impl ")
                    || trimmed.starts_with("pub mod ")
                    || trimmed.starts_with("mod ")
                {
                    let item = trimmed.split('(').next().unwrap_or(trimmed).to_string();
                    headings.push(item);
                }
            }
        }
        "ts" | "tsx" | "js" | "jsx" => {
            // Extract TypeScript/JavaScript: function, class, interface, type, const (with export)
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("export function ")
                    || trimmed.starts_with("function ")
                    || trimmed.starts_with("export class ")
                    || trimmed.starts_with("class ")
                    || trimmed.starts_with("export interface ")
                    || trimmed.starts_with("interface ")
                    || trimmed.starts_with("export type ")
                    || trimmed.starts_with("type ")
                {
                    let item = trimmed.split('{').next().unwrap_or(trimmed).to_string();
                    headings.push(item);
                }
            }
        }
        "json" => {
            // Extract top-level keys
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(content) {
                if let Some(obj) = value.as_object() {
                    for key in obj.keys() {
                        headings.push(key.clone());
                    }
                }
            }
        }
        "toml" => {
            // Extract section headers [section]
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('[') && trimmed.ends_with(']') {
                    let section = trimmed.trim_matches('[').trim_matches(']').to_string();
                    headings.push(section);
                }
            }
        }
        _ => {
            // For other files, use first line as title if it's short
            if let Some(first) = content.lines().next() {
                let first = first.trim();
                if !first.is_empty() && first.len() < 100 {
                    headings.push(first.to_string());
                }
            }
        }
    }

    headings
}

/// Score a document based on query terms
/// Algorithm:
/// - Heading matches: 2.5x weight
/// - Title matches: 2.0x weight
/// - Body matches: 1.0x weight
fn score_document(_query: &str, terms: &[String], doc: &PageDoc) -> f64 {
    let mut score = 0.0;

    // Normalize for comparison
    let title_lower = doc.name.to_lowercase();
    let body_lower = doc.body.to_lowercase();
    let headings_lower: Vec<String> = doc.headings.iter().map(|h| h.to_lowercase()).collect();

    for term in terms {
        let term_lower = term.to_lowercase();

        // Title match (2.0x)
        if title_lower.contains(&term_lower) {
            score += 2.0;
        }

        // Heading matches (2.5x per heading)
        for heading in &headings_lower {
            if heading.contains(&term_lower) {
                score += 2.5;
            }
        }

        // Body matches (1.0x per occurrence, with diminishing returns)
        let body_count = body_lower.matches(&term_lower).count();
        if body_count > 0 {
            score += 1.0 + (body_count as f64).min(10.0).log2();
        }
    }

    // Bonus for docs files (they tend to be more searchable)
    if doc.file_type == "md" {
        score *= 1.05;
    }

    score
}

/// Find which headings matched the query terms
fn find_matched_headings(terms: &[String], headings: &[String]) -> Vec<String> {
    headings
        .iter()
        .filter(|h| {
            let h_lower = h.to_lowercase();
            terms.iter().any(|t| h_lower.contains(t))
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_extract_markdown_headings() {
        let content = "# Title\n\n## Section 1\n\n### Subsection\n\n## Section 2";
        let headings = extract_headings(content, "md");
        assert!(headings.contains(&"Title".to_string()));
        assert!(headings.contains(&"Section 1".to_string()));
        assert!(headings.contains(&"Section 2".to_string()));
    }

    #[test]
    fn test_extract_rust_items() {
        let content = "pub fn main() {}\n\nstruct User {\n    name: String,\n}\n\nimpl User {\n    pub fn new() -> Self {}\n}";
        let headings = extract_headings(content, "rs");
        assert!(headings.iter().any(|h| h.contains("fn main")));
        assert!(headings.iter().any(|h| h.contains("struct User")));
    }

    #[test]
    fn test_score_document() {
        let doc = PageDoc {
            path: "/test/README.md".to_string(),
            name: "README".to_string(),
            headings: vec!["Installation".to_string(), "Usage".to_string()],
            body: "This is a test document about installation and usage.".to_string(),
            file_type: "md".to_string(),
            size_bytes: 100,
        };

        let score = score_document("installation", &["installation".to_string()], &doc);
        assert!(score > 0.0);

        // Case-insensitive - both should match (terms need to be lowercase)
        let score2 = score_document("Installation", &["installation".to_string()], &doc);
        assert!(score2 > 0.0);
    }

    #[test]
    fn test_search() {
        let mut index = PageIndex::default();
        index.docs.insert(
            "/test/README.md".to_string(),
            PageDoc {
                path: "/test/README.md".to_string(),
                name: "README.md".to_string(),
                headings: vec!["Getting Started".to_string()],
                body: "This is a guide to getting started.".to_string(),
                file_type: "md".to_string(),
                size_bytes: 100,
            },
        );

        let results = index.search("getting started", 5);
        assert!(!results.is_empty());
        assert!(results[0]
            .matched_headings
            .contains(&"Getting Started".to_string()));
    }

    #[test]
    fn test_build_index() {
        let tmp = TempDir::new().unwrap();

        // Create test files
        fs::write(tmp.path().join("README.md"), "# Hello\n\nContent").unwrap();
        fs::write(tmp.path().join("main.rs"), "fn main() {}").unwrap();

        let mut index = PageIndex::new(tmp.path());
        let count = index.build(tmp.path(), &["md", "rs"]).unwrap();

        assert_eq!(count, 2);
        assert!(index.stats().total_docs > 0);
    }
}
