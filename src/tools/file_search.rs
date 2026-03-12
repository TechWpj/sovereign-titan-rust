//! File Search Tool — native recursive filesystem scanner.
//!
//! Uses the `walkdir` crate for blazing-fast recursive directory traversal
//! across `~/Desktop`, `~/Documents`, and `~/OneDrive`. Returns matching
//! file and folder paths as the observation.

use serde_json::Value;
use tracing::info;
use walkdir::WalkDir;

use super::Tool;

/// Default directories to scan (relative to the user's home directory).
const SEARCH_ROOTS: &[&str] = &["Desktop", "Documents", "OneDrive"];

/// Maximum results to return per search to avoid flooding the context.
const MAX_RESULTS: usize = 50;

/// Native file search tool using `walkdir` for recursive scanning.
pub struct FileSearchTool;

impl FileSearchTool {
    /// Resolve the user's home directory.
    fn home_dir() -> Option<std::path::PathBuf> {
        std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .ok()
            .map(std::path::PathBuf::from)
    }
}

#[async_trait::async_trait]
impl Tool for FileSearchTool {
    fn name(&self) -> &'static str {
        "file_search"
    }

    fn description(&self) -> &'static str {
        "Search for files and folders by name across Desktop, Documents, and OneDrive. \
         Input: {\"query\": \"<search term>\"}. Returns matching paths."
    }

    async fn execute(&self, input: Value) -> Result<String, anyhow::Error> {
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("file_search requires a \"query\" string field"))?
            .to_lowercase();

        let home = Self::home_dir()
            .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;

        info!("file_search: scanning for \"{query}\"");

        let query_clone = query.clone();
        // Run the filesystem scan in a blocking task to avoid blocking the runtime.
        let results = tokio::task::spawn_blocking(move || {
            let mut matches = Vec::new();

            for root_name in SEARCH_ROOTS {
                let root = home.join(root_name);
                if !root.exists() {
                    continue;
                }

                for entry in WalkDir::new(&root)
                    .follow_links(true)
                    .max_depth(8)
                    .into_iter()
                    .filter_map(|e| e.ok())
                {
                    let name = entry.file_name().to_string_lossy().to_lowercase();
                    if name.contains(&query_clone) {
                        matches.push(entry.path().to_string_lossy().to_string());
                        if matches.len() >= MAX_RESULTS {
                            return matches;
                        }
                    }
                }
            }

            matches
        })
        .await?;

        if results.is_empty() {
            Ok(format!("No files or folders matching \"{query}\" found."))
        } else {
            let count = results.len();
            let listing = results.join("\n");
            Ok(format!(
                "Found {count} result(s) for \"{query}\":\n{listing}"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_missing_query_returns_error() {
        let tool = FileSearchTool;
        let result = tool.execute(json!({})).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("requires a \"query\""));
    }

    #[tokio::test]
    async fn test_search_nonexistent_returns_no_results() {
        let tool = FileSearchTool;
        let result = tool
            .execute(json!({"query": "zzzz_nonexistent_file_xyzzy_42"}))
            .await;
        assert!(result.is_ok());
        let text = result.unwrap();
        assert!(text.contains("No files or folders matching"));
    }
}
