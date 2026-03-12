//! Document Create — generate text files, CSV, and simple documents.
//!
//! Creates files in a workspace directory. Supports text, CSV, JSON,
//! and Markdown formats. Uses pure Rust (no external office deps).

use std::path::PathBuf;

use anyhow::Result;
use serde_json::Value;
use tracing::info;

pub struct DocumentCreateTool;

impl DocumentCreateTool {
    /// Get the workspace documents directory.
    fn workspace_dir() -> PathBuf {
        let base = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        let dir = base.join("Sovereign Titan").join("documents");
        std::fs::create_dir_all(&dir).ok();
        dir
    }

    /// Sanitize a filename — no path traversal, no special chars.
    fn sanitize_filename(name: &str) -> String {
        // Remove path separators and traversal
        let no_path: String = name
            .replace('\\', "")
            .replace('/', "");
        // Keep only safe characters
        let cleaned: String = no_path
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
            .collect();
        // Collapse consecutive dots to prevent ".." traversal
        let mut result = String::new();
        let mut last_was_dot = false;
        for ch in cleaned.chars() {
            if ch == '.' {
                if !last_was_dot {
                    result.push(ch);
                }
                last_was_dot = true;
            } else {
                last_was_dot = false;
                result.push(ch);
            }
        }
        // Strip leading dots
        let result = result.trim_start_matches('.').to_string();
        if result.is_empty() {
            "document.txt".to_string()
        } else {
            result
        }
    }

    fn create_text_file(filename: &str, content: &str) -> String {
        let path = Self::workspace_dir().join(Self::sanitize_filename(filename));
        match std::fs::write(&path, content) {
            Ok(()) => format!(
                "Created text file: **{}** ({} bytes)",
                path.display(),
                content.len()
            ),
            Err(e) => format!("Failed to create file: {e}"),
        }
    }

    fn create_csv(filename: &str, headers: &[String], rows: &[Vec<String>]) -> String {
        let sanitized = Self::sanitize_filename(filename);
        let name = if sanitized.ends_with(".csv") {
            sanitized
        } else {
            format!("{sanitized}.csv")
        };
        let path = Self::workspace_dir().join(&name);

        let mut csv_content = String::new();

        // Headers
        csv_content.push_str(
            &headers
                .iter()
                .map(|h| Self::csv_escape(h))
                .collect::<Vec<_>>()
                .join(","),
        );
        csv_content.push('\n');

        // Rows
        for row in rows {
            csv_content.push_str(
                &row.iter()
                    .map(|c| Self::csv_escape(c))
                    .collect::<Vec<_>>()
                    .join(","),
            );
            csv_content.push('\n');
        }

        match std::fs::write(&path, &csv_content) {
            Ok(()) => format!(
                "Created CSV file: **{}** ({} rows, {} columns)",
                path.display(),
                rows.len(),
                headers.len()
            ),
            Err(e) => format!("Failed to create CSV: {e}"),
        }
    }

    fn csv_escape(value: &str) -> String {
        if value.contains(',') || value.contains('"') || value.contains('\n') {
            format!("\"{}\"", value.replace('"', "\"\""))
        } else {
            value.to_string()
        }
    }

    fn create_json_file(filename: &str, data: &Value) -> String {
        let sanitized = Self::sanitize_filename(filename);
        let name = if sanitized.ends_with(".json") {
            sanitized
        } else {
            format!("{sanitized}.json")
        };
        let path = Self::workspace_dir().join(&name);

        match serde_json::to_string_pretty(data) {
            Ok(json) => match std::fs::write(&path, &json) {
                Ok(()) => format!("Created JSON file: **{}** ({} bytes)", path.display(), json.len()),
                Err(e) => format!("Failed to create JSON file: {e}"),
            },
            Err(e) => format!("Failed to serialize JSON: {e}"),
        }
    }

    fn create_markdown(filename: &str, content: &str) -> String {
        let sanitized = Self::sanitize_filename(filename);
        let name = if sanitized.ends_with(".md") {
            sanitized
        } else {
            format!("{sanitized}.md")
        };
        let path = Self::workspace_dir().join(&name);

        match std::fs::write(&path, content) {
            Ok(()) => format!(
                "Created Markdown file: **{}** ({} bytes)",
                path.display(),
                content.len()
            ),
            Err(e) => format!("Failed to create Markdown file: {e}"),
        }
    }

    fn list_documents() -> String {
        let dir = Self::workspace_dir();
        match std::fs::read_dir(&dir) {
            Ok(entries) => {
                let mut files: Vec<String> = entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                    .map(|e| {
                        let name = e.file_name().to_string_lossy().to_string();
                        let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                        format!("  - {} ({} bytes)", name, size)
                    })
                    .collect();

                if files.is_empty() {
                    format!("No documents found in {}", dir.display())
                } else {
                    files.sort();
                    format!(
                        "**Documents** ({}/):\n{}",
                        dir.display(),
                        files.join("\n")
                    )
                }
            }
            Err(e) => format!("Failed to list documents: {e}"),
        }
    }
}

#[async_trait::async_trait]
impl super::Tool for DocumentCreateTool {
    fn name(&self) -> &'static str {
        "document_create"
    }

    fn description(&self) -> &'static str {
        "Create documents and files. Input: {\"action\": \"<action>\", ...}. \
         Actions: text (filename, content), csv (filename, headers: [], rows: [[]]), \
         json (filename, data: {}), markdown (filename, content), list."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("text");

        info!("document_create: action={action}");

        match action {
            "text" | "txt" => {
                let filename = input.get("filename").and_then(|v| v.as_str()).unwrap_or("document.txt");
                let content = input.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if content.is_empty() {
                    return Ok("text requires a \"content\" field.".to_string());
                }
                Ok(Self::create_text_file(filename, content))
            }
            "csv" => {
                let filename = input.get("filename").and_then(|v| v.as_str()).unwrap_or("data.csv");
                let headers: Vec<String> = input
                    .get("headers")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let rows: Vec<Vec<String>> = input
                    .get("rows")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|row| {
                                row.as_array().map(|cells| {
                                    cells
                                        .iter()
                                        .map(|c| c.as_str().unwrap_or("").to_string())
                                        .collect()
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                if headers.is_empty() {
                    return Ok("csv requires a \"headers\" array.".to_string());
                }
                Ok(Self::create_csv(filename, &headers, &rows))
            }
            "json" => {
                let filename = input.get("filename").and_then(|v| v.as_str()).unwrap_or("data.json");
                let data = input.get("data").cloned().unwrap_or(Value::Null);
                if data.is_null() {
                    return Ok("json requires a \"data\" field.".to_string());
                }
                Ok(Self::create_json_file(filename, &data))
            }
            "markdown" | "md" => {
                let filename = input.get("filename").and_then(|v| v.as_str()).unwrap_or("document.md");
                let content = input.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if content.is_empty() {
                    return Ok("markdown requires a \"content\" field.".to_string());
                }
                Ok(Self::create_markdown(filename, content))
            }
            "list" => Ok(Self::list_documents()),
            other => Ok(format!(
                "Unknown action: '{other}'. Use: text, csv, json, markdown, list."
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;
    use serde_json::json;

    #[test]
    fn test_sanitize_filename_normal() {
        assert_eq!(DocumentCreateTool::sanitize_filename("report.txt"), "report.txt");
    }

    #[test]
    fn test_sanitize_filename_path_traversal() {
        let result = DocumentCreateTool::sanitize_filename("../../etc/passwd");
        assert!(!result.contains(".."));
        assert!(!result.contains('/'));
    }

    #[test]
    fn test_sanitize_filename_empty() {
        assert_eq!(DocumentCreateTool::sanitize_filename(""), "document.txt");
    }

    #[test]
    fn test_sanitize_filename_special_chars() {
        let result = DocumentCreateTool::sanitize_filename("my file<>.txt");
        assert!(!result.contains('<'));
        assert!(!result.contains('>'));
        assert!(!result.contains(' '));
    }

    #[test]
    fn test_csv_escape_plain() {
        assert_eq!(DocumentCreateTool::csv_escape("hello"), "hello");
    }

    #[test]
    fn test_csv_escape_comma() {
        assert_eq!(DocumentCreateTool::csv_escape("hello,world"), "\"hello,world\"");
    }

    #[test]
    fn test_csv_escape_quotes() {
        assert_eq!(DocumentCreateTool::csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[tokio::test]
    async fn test_unknown_action() {
        let tool = DocumentCreateTool;
        let result = tool.execute(json!({"action": "docx"})).await.unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[tokio::test]
    async fn test_text_missing_content() {
        let tool = DocumentCreateTool;
        let result = tool
            .execute(json!({"action": "text", "filename": "test.txt"}))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_csv_missing_headers() {
        let tool = DocumentCreateTool;
        let result = tool
            .execute(json!({"action": "csv", "filename": "test.csv"}))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_json_missing_data() {
        let tool = DocumentCreateTool;
        let result = tool
            .execute(json!({"action": "json", "filename": "test.json"}))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_create_text_file() {
        let tool = DocumentCreateTool;
        let result = tool
            .execute(json!({
                "action": "text",
                "filename": "test_wave4.txt",
                "content": "Hello from Wave 4!"
            }))
            .await
            .unwrap();
        assert!(result.contains("Created text file"));
    }

    #[tokio::test]
    async fn test_list_documents() {
        let tool = DocumentCreateTool;
        let result = tool.execute(json!({"action": "list"})).await.unwrap();
        // Should succeed regardless of whether documents exist
        assert!(result.contains("Documents") || result.contains("No documents"));
    }
}
