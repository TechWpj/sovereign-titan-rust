//! Code Operations Tool — file read, create, and append for the AI.
//!
//! Ported from `sovereign_titan/tools/code.py`.
//! Allows the AI to read, create, and modify local files on the host machine.
//! Includes safety guards to prevent writing to critical system paths.

use anyhow::Result;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// Maximum file size to read (10 MB).
const MAX_READ_BYTES: u64 = 10 * 1024 * 1024;

/// Maximum content to write per operation (1 MB).
const MAX_WRITE_BYTES: usize = 1_000_000;

/// Blocked path prefixes — never write to system directories.
const BLOCKED_PREFIXES: &[&str] = &[
    "C:\\Windows",
    "C:\\Program Files",
    "C:\\Program Files (x86)",
    "C:\\ProgramData",
    "/windows",
    "/program files",
];

/// Tool for reading, creating, and appending to local files.
pub struct CodeOpsTool;

#[async_trait::async_trait]
impl super::Tool for CodeOpsTool {
    fn name(&self) -> &'static str {
        "code_ops"
    }

    fn description(&self) -> &'static str {
        "Read, create, or append to local files. Actions: \
         read {path} — read a file's contents; \
         create {path, content} — create or overwrite a file; \
         append {path, content} — append text to an existing file; \
         list {path} — list directory contents"
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();

        if action.is_empty() {
            return Ok("Error: missing 'action'. Use: read, create, append, list".into());
        }

        match action.as_str() {
            "read" => read_file(&input),
            "create" => create_file(&input),
            "append" => append_file(&input),
            "list" => list_dir(&input),
            _ => Ok(format!(
                "Unknown action: '{action}'. Use: read, create, append, list"
            )),
        }
    }
}

/// Resolve and validate a file path from the input.
fn resolve_path(input: &Value) -> Result<PathBuf, String> {
    let raw = input
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if raw.is_empty() {
        return Err("Error: missing 'path' field".into());
    }

    let path = PathBuf::from(raw);

    // Block system-critical paths.
    let path_str = path.to_string_lossy().to_lowercase();
    for prefix in BLOCKED_PREFIXES {
        if path_str.starts_with(&prefix.to_lowercase()) {
            return Err(format!("Blocked: cannot access system path '{raw}'"));
        }
    }

    // Block hidden system files.
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        let lower = name.to_lowercase();
        if lower == "ntuser.dat" || lower.starts_with("ntuser.dat{") {
            return Err(format!("Blocked: cannot access system file '{name}'"));
        }
    }

    Ok(path)
}

/// Read a file and return its contents.
fn read_file(input: &Value) -> Result<String> {
    let path = resolve_path(input).map_err(|e| anyhow::anyhow!("{e}"))?;

    if !path.exists() {
        return Ok(format!("Error: file not found: '{}'", path.display()));
    }

    if !path.is_file() {
        return Ok(format!("Error: '{}' is not a file", path.display()));
    }

    // Check file size.
    let metadata = fs::metadata(&path)?;
    if metadata.len() > MAX_READ_BYTES {
        return Ok(format!(
            "Error: file too large ({} bytes, max {} bytes)",
            metadata.len(),
            MAX_READ_BYTES
        ));
    }

    // Try reading as UTF-8 text.
    match fs::read_to_string(&path) {
        Ok(content) => {
            let display_path = path.display();
            let lines = content.lines().count();
            Ok(format!(
                "[{display_path}] ({lines} lines, {} bytes)\n\n{content}",
                content.len()
            ))
        }
        Err(_) => Ok(format!(
            "Error: '{}' is not valid UTF-8 text (binary file?)",
            path.display()
        )),
    }
}

/// Create a new file (or overwrite) with the given content.
fn create_file(input: &Value) -> Result<String> {
    let path = resolve_path(input).map_err(|e| anyhow::anyhow!("{e}"))?;

    let content = input
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if content.len() > MAX_WRITE_BYTES {
        return Ok(format!(
            "Error: content too large ({} bytes, max {MAX_WRITE_BYTES} bytes)",
            content.len()
        ));
    }

    // Create parent directories if needed.
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    fs::write(&path, content)?;
    Ok(format!(
        "Created '{}' ({} bytes written)",
        path.display(),
        content.len()
    ))
}

/// Append content to an existing file.
fn append_file(input: &Value) -> Result<String> {
    let path = resolve_path(input).map_err(|e| anyhow::anyhow!("{e}"))?;

    let content = input
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if content.is_empty() {
        return Ok("Error: missing or empty 'content' field".into());
    }

    if content.len() > MAX_WRITE_BYTES {
        return Ok(format!(
            "Error: content too large ({} bytes, max {MAX_WRITE_BYTES} bytes)",
            content.len()
        ));
    }

    if !path.exists() {
        return Ok(format!(
            "Error: file not found: '{}'. Use 'create' action to make a new file.",
            path.display()
        ));
    }

    // Check resulting size won't be too large.
    let existing_size = fs::metadata(&path)?.len();
    if existing_size + content.len() as u64 > MAX_READ_BYTES {
        return Ok("Error: appending would exceed maximum file size".into());
    }

    use std::io::Write;
    let mut file = fs::OpenOptions::new().append(true).open(&path)?;
    file.write_all(content.as_bytes())?;

    Ok(format!(
        "Appended {} bytes to '{}'",
        content.len(),
        path.display()
    ))
}

/// List contents of a directory.
fn list_dir(input: &Value) -> Result<String> {
    let path = resolve_path(input).map_err(|e| anyhow::anyhow!("{e}"))?;

    if !path.exists() {
        return Ok(format!("Error: directory not found: '{}'", path.display()));
    }

    if !path.is_dir() {
        return Ok(format!("Error: '{}' is not a directory", path.display()));
    }

    let mut entries = Vec::new();
    for entry in fs::read_dir(&path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        let ft = entry.file_type()?;
        let kind = if ft.is_dir() {
            "DIR"
        } else if ft.is_symlink() {
            "LINK"
        } else {
            "FILE"
        };
        let size = if ft.is_file() {
            let meta = entry.metadata()?;
            format!(" ({} bytes)", meta.len())
        } else {
            String::new()
        };
        entries.push(format!("  [{kind}] {name}{size}"));
    }

    entries.sort();

    if entries.is_empty() {
        Ok(format!("[{}] (empty directory)", path.display()))
    } else {
        Ok(format!(
            "[{}] ({} entries)\n{}",
            path.display(),
            entries.len(),
            entries.join("\n")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blocked_path() {
        let input = serde_json::json!({"path": "C:\\Windows\\System32\\cmd.exe"});
        let result = resolve_path(&input);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Blocked"));
    }

    #[test]
    fn test_missing_path() {
        let input = serde_json::json!({});
        let result = resolve_path(&input);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing"));
    }

    #[test]
    fn test_valid_path() {
        let input = serde_json::json!({"path": "C:\\Users\\test\\file.txt"});
        let result = resolve_path(&input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_read_nonexistent() {
        let input = serde_json::json!({"path": "C:\\nonexistent_titan_test_file_xyz.txt"});
        let result = read_file(&input).unwrap();
        assert!(result.contains("not found"));
    }

    #[test]
    fn test_create_and_read() {
        let dir = std::env::temp_dir().join(format!("titan_code_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("test.txt");
        let path_str = file.to_str().unwrap();

        // Create.
        let input = serde_json::json!({"path": path_str, "content": "Hello, Titan!"});
        let result = create_file(&input).unwrap();
        assert!(result.contains("Created"));

        // Read back.
        let input = serde_json::json!({"path": path_str});
        let result = read_file(&input).unwrap();
        assert!(result.contains("Hello, Titan!"));

        // Append.
        let input = serde_json::json!({"path": path_str, "content": "\nLine 2"});
        let result = append_file(&input).unwrap();
        assert!(result.contains("Appended"));

        // Read again.
        let input = serde_json::json!({"path": path_str});
        let result = read_file(&input).unwrap();
        assert!(result.contains("Line 2"));

        // Cleanup.
        let _ = fs::remove_dir_all(&dir);
    }
}
