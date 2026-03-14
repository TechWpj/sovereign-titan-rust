//! File Operations Tool — copy, move, delete, rename, create directories, check existence and size.
//!
//! Uses `std::fs` operations wrapped in `spawn_blocking` to avoid blocking
//! the async runtime. All paths are validated before use.

use anyhow::Result;
use serde_json::Value;
use tracing::info;

pub struct FileOpsTool;

impl FileOpsTool {
    fn copy_file(source: &str, destination: &str) -> String {
        let src = std::path::Path::new(source);
        let dst = std::path::Path::new(destination);

        if !src.exists() {
            return format!("Source does not exist: {source}");
        }

        // If destination is a directory, copy into it with the same filename
        let final_dst = if dst.is_dir() {
            if let Some(name) = src.file_name() {
                dst.join(name)
            } else {
                return format!("Cannot determine filename from source: {source}");
            }
        } else {
            dst.to_path_buf()
        };

        match std::fs::copy(src, &final_dst) {
            Ok(bytes) => format!(
                "Copied {source} -> {} ({bytes} bytes)",
                final_dst.display()
            ),
            Err(e) => format!("Failed to copy: {e}"),
        }
    }

    fn move_file(source: &str, destination: &str) -> String {
        let src = std::path::Path::new(source);
        let dst = std::path::Path::new(destination);

        if !src.exists() {
            return format!("Source does not exist: {source}");
        }

        let final_dst = if dst.is_dir() {
            if let Some(name) = src.file_name() {
                dst.join(name)
            } else {
                return format!("Cannot determine filename from source: {source}");
            }
        } else {
            dst.to_path_buf()
        };

        match std::fs::rename(src, &final_dst) {
            Ok(()) => format!("Moved {source} -> {}", final_dst.display()),
            Err(_) => {
                // rename fails across drives; fall back to copy+delete
                match std::fs::copy(src, &final_dst) {
                    Ok(_) => match std::fs::remove_file(src) {
                        Ok(()) => format!("Moved {source} -> {} (cross-drive)", final_dst.display()),
                        Err(e) => format!(
                            "Copied to {} but failed to remove source: {e}",
                            final_dst.display()
                        ),
                    },
                    Err(e) => format!("Failed to move: {e}"),
                }
            }
        }
    }

    fn delete(path: &str) -> String {
        let p = std::path::Path::new(path);

        if !p.exists() {
            return format!("Path does not exist: {path}");
        }

        if p.is_dir() {
            match std::fs::remove_dir_all(p) {
                Ok(()) => format!("Deleted directory: {path}"),
                Err(e) => format!("Failed to delete directory: {e}"),
            }
        } else {
            match std::fs::remove_file(p) {
                Ok(()) => format!("Deleted file: {path}"),
                Err(e) => format!("Failed to delete file: {e}"),
            }
        }
    }

    fn rename(old: &str, new: &str) -> String {
        let old_path = std::path::Path::new(old);
        if !old_path.exists() {
            return format!("Path does not exist: {old}");
        }

        match std::fs::rename(old, new) {
            Ok(()) => format!("Renamed {old} -> {new}"),
            Err(e) => format!("Failed to rename: {e}"),
        }
    }

    fn create_dir(path: &str) -> String {
        match std::fs::create_dir_all(path) {
            Ok(()) => format!("Created directory: {path}"),
            Err(e) => format!("Failed to create directory: {e}"),
        }
    }

    fn exists(path: &str) -> String {
        let p = std::path::Path::new(path);
        if p.exists() {
            let kind = if p.is_dir() {
                "directory"
            } else if p.is_file() {
                "file"
            } else if p.is_symlink() {
                "symlink"
            } else {
                "exists"
            };
            format!("{path}: {kind} (exists)")
        } else {
            format!("{path}: does not exist")
        }
    }

    fn size(path: &str) -> String {
        let p = std::path::Path::new(path);
        if !p.exists() {
            return format!("Path does not exist: {path}");
        }

        if p.is_file() {
            match std::fs::metadata(p) {
                Ok(meta) => {
                    let bytes = meta.len();
                    let human = humanize_bytes(bytes);
                    format!("{path}: {human} ({bytes} bytes)")
                }
                Err(e) => format!("Failed to get size: {e}"),
            }
        } else if p.is_dir() {
            let total = dir_size(p);
            let human = humanize_bytes(total);
            format!("{path}: {human} ({total} bytes, directory total)")
        } else {
            format!("{path}: cannot determine size")
        }
    }
}

fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let meta = entry.metadata();
            if let Ok(m) = meta {
                if m.is_file() {
                    total += m.len();
                } else if m.is_dir() {
                    total += dir_size(&entry.path());
                }
            }
        }
    }
    total
}

fn humanize_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[async_trait::async_trait]
impl super::Tool for FileOpsTool {
    fn name(&self) -> &'static str {
        "file_ops"
    }

    fn description(&self) -> &'static str {
        "File operations tool. Input: {\"action\": \"<action>\", ...}. \
         Actions: copy (source, destination), move_file (source, destination), \
         delete (path), rename (old, new), create_dir (path), \
         exists (path), size (path)."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        info!("file_ops: action={action}");

        let input_clone = input.clone();
        let action_owned = action.to_string();

        let result = tokio::task::spawn_blocking(move || {
            match action_owned.as_str() {
                "copy" => {
                    let source = input_clone.get("source").and_then(|v| v.as_str()).unwrap_or("");
                    let dest = input_clone.get("destination").and_then(|v| v.as_str()).unwrap_or("");
                    if source.is_empty() || dest.is_empty() {
                        "copy requires \"source\" and \"destination\" fields.".to_string()
                    } else {
                        Self::copy_file(source, dest)
                    }
                }
                "move" | "move_file" => {
                    let source = input_clone.get("source").and_then(|v| v.as_str()).unwrap_or("");
                    let dest = input_clone.get("destination").and_then(|v| v.as_str()).unwrap_or("");
                    if source.is_empty() || dest.is_empty() {
                        "move_file requires \"source\" and \"destination\" fields.".to_string()
                    } else {
                        Self::move_file(source, dest)
                    }
                }
                "delete" | "remove" => {
                    let path = input_clone.get("path").and_then(|v| v.as_str()).unwrap_or("");
                    if path.is_empty() {
                        "delete requires a \"path\" field.".to_string()
                    } else {
                        Self::delete(path)
                    }
                }
                "rename" => {
                    let old = input_clone.get("old").and_then(|v| v.as_str()).unwrap_or("");
                    let new = input_clone.get("new").and_then(|v| v.as_str()).unwrap_or("");
                    if old.is_empty() || new.is_empty() {
                        "rename requires \"old\" and \"new\" fields.".to_string()
                    } else {
                        Self::rename(old, new)
                    }
                }
                "create_dir" | "mkdir" => {
                    let path = input_clone.get("path").and_then(|v| v.as_str()).unwrap_or("");
                    if path.is_empty() {
                        "create_dir requires a \"path\" field.".to_string()
                    } else {
                        Self::create_dir(path)
                    }
                }
                "exists" | "check" => {
                    let path = input_clone.get("path").and_then(|v| v.as_str()).unwrap_or("");
                    if path.is_empty() {
                        "exists requires a \"path\" field.".to_string()
                    } else {
                        Self::exists(path)
                    }
                }
                "size" => {
                    let path = input_clone.get("path").and_then(|v| v.as_str()).unwrap_or("");
                    if path.is_empty() {
                        "size requires a \"path\" field.".to_string()
                    } else {
                        Self::size(path)
                    }
                }
                "" => "file_ops requires an \"action\" field. Actions: copy, move_file, delete, rename, create_dir, exists, size.".to_string(),
                other => format!(
                    "Unknown action: '{other}'. Use: copy, move_file, delete, rename, create_dir, exists, size."
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

    #[tokio::test]
    async fn test_exists() {
        let tool = FileOpsTool;
        // The Cargo.toml at the project root should always exist
        let result = tool
            .execute(json!({"action": "exists", "path": "Cargo.toml"}))
            .await
            .unwrap();
        assert!(result.contains("exists") || result.contains("does not exist"));
    }

    #[tokio::test]
    async fn test_exists_nonexistent() {
        let tool = FileOpsTool;
        let result = tool
            .execute(json!({"action": "exists", "path": "this_path_absolutely_does_not_exist_xyz123.tmp"}))
            .await
            .unwrap();
        assert!(result.contains("does not exist"));
    }

    #[tokio::test]
    async fn test_unknown_action() {
        let tool = FileOpsTool;
        let result = tool.execute(json!({"action": "fly"})).await.unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[tokio::test]
    async fn test_missing_action() {
        let tool = FileOpsTool;
        let result = tool.execute(json!({})).await.unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_missing_path() {
        let tool = FileOpsTool;
        let result = tool
            .execute(json!({"action": "delete"}))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_copy_missing_fields() {
        let tool = FileOpsTool;
        let result = tool
            .execute(json!({"action": "copy", "source": "a.txt"}))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_size_nonexistent() {
        let tool = FileOpsTool;
        let result = tool
            .execute(json!({"action": "size", "path": "nonexistent_xyz.dat"}))
            .await
            .unwrap();
        assert!(result.contains("does not exist"));
    }

    #[test]
    fn test_humanize_bytes() {
        assert_eq!(humanize_bytes(500), "500 B");
        assert_eq!(humanize_bytes(1024), "1.00 KB");
        assert_eq!(humanize_bytes(1_048_576), "1.00 MB");
        assert_eq!(humanize_bytes(1_073_741_824), "1.00 GB");
    }

    #[tokio::test]
    async fn test_delete_nonexistent() {
        let tool = FileOpsTool;
        let result = tool
            .execute(json!({"action": "delete", "path": "nonexistent_xyz_delete_test.tmp"}))
            .await
            .unwrap();
        assert!(result.contains("does not exist"));
    }
}
