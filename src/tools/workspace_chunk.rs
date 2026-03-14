//! Workspace Chunk Tool — file chunking for RAG pipelines.
//!
//! Reads workspace files and splits them into overlapping text chunks
//! suitable for embedding and retrieval. Supports common text-based
//! file formats: .txt, .rs, .py, .md, .json, .toml, .js, .ts, .html,
//! .css, .yaml, .yml, .cfg, .ini, .sh, .bat.

use anyhow::Result;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tracing::info;
use walkdir::WalkDir;

/// Default chunk size in characters.
const DEFAULT_CHUNK_SIZE: usize = 1000;

/// Overlap between consecutive chunks in characters.
const DEFAULT_OVERLAP: usize = 200;

/// Maximum file size to process (10 MB).
const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// Supported file extensions for chunking.
const SUPPORTED_EXTENSIONS: &[&str] = &[
    "txt", "rs", "py", "md", "json", "toml", "js", "ts", "html", "css",
    "yaml", "yml", "cfg", "ini", "sh", "bat", "c", "cpp", "h", "hpp",
    "java", "go", "rb", "php", "sql", "xml", "csv", "log",
];

/// Workspace file chunking tool.
pub struct WorkspaceChunkTool;

/// A single text chunk with metadata.
#[derive(Debug, Clone)]
struct Chunk {
    /// The text content of this chunk.
    text: String,
    /// Zero-based index of this chunk within the source file.
    index: usize,
    /// Starting character offset in the original file.
    start_offset: usize,
    /// Ending character offset in the original file.
    end_offset: usize,
}

impl WorkspaceChunkTool {
    /// Check if a file extension is supported for chunking.
    fn is_supported(ext: &str) -> bool {
        SUPPORTED_EXTENSIONS.contains(&ext.to_lowercase().as_str())
    }

    /// Read a file and split it into overlapping chunks.
    fn chunk_file(path: &str, chunk_size: usize, overlap: usize) -> String {
        let file_path = Path::new(path);

        if !file_path.exists() {
            return format!("File not found: {path}");
        }

        if !file_path.is_file() {
            return format!("Not a file: {path}");
        }

        // Check file size.
        match std::fs::metadata(file_path) {
            Ok(meta) => {
                if meta.len() > MAX_FILE_SIZE {
                    return format!(
                        "File too large ({:.1} MB, max {} MB): {path}",
                        meta.len() as f64 / (1024.0 * 1024.0),
                        MAX_FILE_SIZE / (1024 * 1024)
                    );
                }
            }
            Err(e) => return format!("Cannot read file metadata: {e}"),
        }

        // Check extension.
        let ext = file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if !Self::is_supported(ext) {
            return format!(
                "Unsupported file type: .{ext}. Use list_supported to see supported types."
            );
        }

        // Read file content.
        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(e) => return format!("Failed to read file: {e}"),
        };

        if content.is_empty() {
            return format!("File is empty: {path}");
        }

        let chunks = split_into_chunks(&content, chunk_size, overlap);

        let mut output = format!(
            "Chunked '{}': {} chunks (size={}, overlap={})\n\n",
            file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path),
            chunks.len(),
            chunk_size,
            overlap,
        );

        for chunk in &chunks {
            output.push_str(&format!(
                "[Chunk {} | offset {}-{} | {} chars]\n{}\n\n",
                chunk.index,
                chunk.start_offset,
                chunk.end_offset,
                chunk.text.len(),
                // Show first 100 chars as preview.
                if chunk.text.len() > 100 {
                    format!("{}...", &chunk.text[..100])
                } else {
                    chunk.text.clone()
                }
            ));
        }

        output.trim_end().to_string()
    }

    /// Chunk all matching files in a directory.
    fn chunk_directory(
        dir_path: &str,
        extensions: &[String],
        chunk_size: usize,
        overlap: usize,
    ) -> String {
        let dir = Path::new(dir_path);

        if !dir.exists() {
            return format!("Directory not found: {dir_path}");
        }
        if !dir.is_dir() {
            return format!("Not a directory: {dir_path}");
        }

        // Filter extensions (use all supported if none specified).
        let filter_exts: Vec<String> = if extensions.is_empty() {
            SUPPORTED_EXTENSIONS.iter().map(|s| s.to_string()).collect()
        } else {
            extensions
                .iter()
                .map(|e| e.trim_start_matches('.').to_lowercase())
                .collect()
        };

        let mut files_found: Vec<PathBuf> = Vec::new();
        for entry in WalkDir::new(dir)
            .max_depth(5)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                    if filter_exts.contains(&ext.to_lowercase()) {
                        files_found.push(entry.path().to_path_buf());
                    }
                }
            }
        }

        if files_found.is_empty() {
            return format!(
                "No matching files found in '{dir_path}' with extensions: {}",
                filter_exts.join(", ")
            );
        }

        files_found.sort();

        let mut total_chunks = 0usize;
        let mut output = format!(
            "Chunking {} files in '{}' (size={}, overlap={})\n\n",
            files_found.len(),
            dir_path,
            chunk_size,
            overlap
        );

        for file_path in &files_found {
            let content = match std::fs::read_to_string(file_path) {
                Ok(c) => c,
                Err(e) => {
                    output.push_str(&format!(
                        "  [SKIP] {}: {e}\n",
                        file_path.display()
                    ));
                    continue;
                }
            };

            if content.is_empty() {
                continue;
            }

            // Check file size.
            if let Ok(meta) = std::fs::metadata(file_path) {
                if meta.len() > MAX_FILE_SIZE {
                    output.push_str(&format!(
                        "  [SKIP] {}: too large ({:.1} MB)\n",
                        file_path.display(),
                        meta.len() as f64 / (1024.0 * 1024.0)
                    ));
                    continue;
                }
            }

            let chunks = split_into_chunks(&content, chunk_size, overlap);
            total_chunks += chunks.len();

            output.push_str(&format!(
                "  {} -> {} chunks\n",
                file_path.display(),
                chunks.len()
            ));
        }

        output.push_str(&format!(
            "\nTotal: {} files, {} chunks",
            files_found.len(),
            total_chunks
        ));

        output
    }

    /// List supported file extensions.
    fn list_supported() -> String {
        let mut exts: Vec<String> = SUPPORTED_EXTENSIONS
            .iter()
            .map(|e| format!(".{e}"))
            .collect();
        exts.sort();

        format!(
            "Supported file extensions for chunking ({}):\n{}",
            exts.len(),
            exts.join(", ")
        )
    }
}

/// Split text into overlapping chunks.
fn split_into_chunks(text: &str, chunk_size: usize, overlap: usize) -> Vec<Chunk> {
    let chunk_size = chunk_size.max(100); // Minimum 100 chars.
    let overlap = overlap.min(chunk_size / 2); // Overlap cannot exceed half the chunk size.

    let text_len = text.len();
    if text_len == 0 {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut start = 0usize;
    let mut index = 0usize;
    let step = chunk_size.saturating_sub(overlap).max(1);

    while start < text_len {
        let end = (start + chunk_size).min(text_len);
        let chunk_text = &text[start..end];

        // Try to break at a natural boundary (newline or space) near the end.
        let actual_end = if end < text_len {
            // Look for a newline near the end to avoid breaking mid-line.
            if let Some(nl_pos) = chunk_text.rfind('\n') {
                if nl_pos > chunk_size / 2 {
                    start + nl_pos + 1
                } else {
                    end
                }
            } else {
                end
            }
        } else {
            end
        };

        let final_text = &text[start..actual_end];
        if !final_text.trim().is_empty() {
            chunks.push(Chunk {
                text: final_text.to_string(),
                index,
                start_offset: start,
                end_offset: actual_end,
            });
            index += 1;
        }

        start += step;
        // Skip past the actual end if we adjusted for a natural boundary.
        if start < actual_end && actual_end > start {
            // Keep the overlap-based stepping.
        }
    }

    chunks
}

#[async_trait::async_trait]
impl super::Tool for WorkspaceChunkTool {
    fn name(&self) -> &'static str {
        "workspace_chunk"
    }

    fn description(&self) -> &'static str {
        "Chunk workspace files for RAG. Actions: \
         chunk_file (path, chunk_size?, overlap?), \
         chunk_directory (path, extensions?, chunk_size?, overlap?), \
         list_supported. \
         Input: {\"action\": \"chunk_file\", \"path\": \"src/main.rs\", \"chunk_size\": 1000}."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("chunk_file");

        info!("workspace_chunk: action={action}");

        let input_clone = input.clone();
        let action_owned = action.to_string();

        let result = tokio::task::spawn_blocking(move || {
            match action_owned.as_str() {
                "chunk_file" | "chunk" => {
                    let path = input_clone
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if path.is_empty() {
                        return "chunk_file requires a \"path\" field.".to_string();
                    }
                    let chunk_size = input_clone
                        .get("chunk_size")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(DEFAULT_CHUNK_SIZE as u64)
                        as usize;
                    let overlap = input_clone
                        .get("overlap")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(DEFAULT_OVERLAP as u64)
                        as usize;
                    Self::chunk_file(path, chunk_size, overlap)
                }
                "chunk_directory" | "chunk_dir" => {
                    let path = input_clone
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if path.is_empty() {
                        return "chunk_directory requires a \"path\" field.".to_string();
                    }
                    let extensions: Vec<String> = input_clone
                        .get("extensions")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    let chunk_size = input_clone
                        .get("chunk_size")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(DEFAULT_CHUNK_SIZE as u64)
                        as usize;
                    let overlap = input_clone
                        .get("overlap")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(DEFAULT_OVERLAP as u64)
                        as usize;
                    Self::chunk_directory(path, &extensions, chunk_size, overlap)
                }
                "list_supported" | "supported" => Self::list_supported(),
                other => format!(
                    "Unknown action: '{other}'. Use: chunk_file, chunk_directory, list_supported."
                ),
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!("spawn_blocking failed: {e}"))?;

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;
    use serde_json::json;

    #[test]
    fn test_list_supported() {
        let result = WorkspaceChunkTool::list_supported();
        assert!(result.contains(".rs"));
        assert!(result.contains(".py"));
        assert!(result.contains(".txt"));
        assert!(result.contains(".md"));
        assert!(result.contains(".json"));
        assert!(result.contains(".toml"));
    }

    #[test]
    fn test_chunk_simple_text() {
        let text = "Hello world. This is a test document for chunking. \
                    It contains multiple sentences that should be split \
                    into chunks based on the configured chunk size.";
        let chunks = split_into_chunks(text, 50, 10);
        assert!(!chunks.is_empty(), "should produce at least one chunk");

        // Verify all text is covered.
        assert_eq!(chunks[0].start_offset, 0);
        assert_eq!(chunks[0].index, 0);

        // Verify chunks have content.
        for chunk in &chunks {
            assert!(!chunk.text.trim().is_empty());
        }
    }

    #[test]
    fn test_chunk_empty_text() {
        let chunks = split_into_chunks("", 100, 20);
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_chunk_small_text() {
        let text = "Short text.";
        let chunks = split_into_chunks(text, 1000, 200);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, "Short text.");
    }

    #[test]
    fn test_is_supported() {
        assert!(WorkspaceChunkTool::is_supported("rs"));
        assert!(WorkspaceChunkTool::is_supported("py"));
        assert!(WorkspaceChunkTool::is_supported("txt"));
        assert!(WorkspaceChunkTool::is_supported("md"));
        assert!(!WorkspaceChunkTool::is_supported("exe"));
        assert!(!WorkspaceChunkTool::is_supported("png"));
    }

    #[tokio::test]
    async fn test_list_supported_action() {
        let tool = WorkspaceChunkTool;
        let result = tool
            .execute(json!({"action": "list_supported"}))
            .await
            .unwrap();
        assert!(result.contains(".rs"));
        assert!(result.contains("Supported"));
    }

    #[tokio::test]
    async fn test_chunk_file_missing_path() {
        let tool = WorkspaceChunkTool;
        let result = tool
            .execute(json!({"action": "chunk_file"}))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_chunk_file_nonexistent() {
        let tool = WorkspaceChunkTool;
        let result = tool
            .execute(json!({"action": "chunk_file", "path": "/nonexistent/file.txt"}))
            .await
            .unwrap();
        assert!(result.contains("not found") || result.contains("Not"));
    }

    #[tokio::test]
    async fn test_unknown_action() {
        let tool = WorkspaceChunkTool;
        let result = tool.execute(json!({"action": "fly"})).await.unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[tokio::test]
    async fn test_chunk_real_file() {
        // Create a temp file and chunk it.
        let dir = std::env::temp_dir().join("titan_chunk_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test_chunk.txt");
        let content = "Line one of the test file.\n\
                       Line two with more content here.\n\
                       Line three is the final line.\n";
        std::fs::write(&path, content).unwrap();

        let tool = WorkspaceChunkTool;
        let result = tool
            .execute(json!({
                "action": "chunk_file",
                "path": path.to_str().unwrap(),
                "chunk_size": 50,
                "overlap": 10
            }))
            .await
            .unwrap();
        assert!(result.contains("Chunked"));
        assert!(result.contains("chunk"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
