//! Clipboard Tool — read, write, and clear the system clipboard.
//!
//! Uses the `clipboard-win` crate for native Windows clipboard access.
//! Safety: caps reads at 10K chars, never logs clipboard content.

use anyhow::Result;
use serde_json::Value;
use tracing::info;

/// Maximum clipboard text to return.
const MAX_CLIPBOARD_LEN: usize = 10_000;

pub struct ClipboardTool;

impl ClipboardTool {
    fn read_clipboard() -> Result<String> {
        use clipboard_win::{formats, get_clipboard};
        let text: String = get_clipboard(formats::Unicode)
            .map_err(|e| anyhow::anyhow!("Failed to read clipboard: {e}"))?;

        if text.is_empty() {
            Ok("Clipboard is empty.".to_string())
        } else if text.len() > MAX_CLIPBOARD_LEN {
            Ok(format!(
                "{}...\n[Truncated — {} total chars]",
                &text[..MAX_CLIPBOARD_LEN],
                text.len()
            ))
        } else {
            Ok(text)
        }
    }

    fn write_clipboard(text: &str) -> Result<String> {
        use clipboard_win::{formats, set_clipboard};
        set_clipboard(formats::Unicode, text)
            .map_err(|e| anyhow::anyhow!("Failed to write clipboard: {e}"))?;
        Ok(format!("Copied {} chars to clipboard.", text.len()))
    }

    fn clear_clipboard() -> Result<String> {
        use clipboard_win::empty;
        empty().map_err(|e| anyhow::anyhow!("Failed to clear clipboard: {e}"))?;
        Ok("Clipboard cleared.".to_string())
    }
}

#[async_trait::async_trait]
impl super::Tool for ClipboardTool {
    fn name(&self) -> &'static str {
        "clipboard"
    }

    fn description(&self) -> &'static str {
        "Manage the system clipboard. Input: {\"action\": \"<action>\", ...}. \
         Actions: read (returns clipboard contents, max 10K chars), \
         write (text — copies text to clipboard), clear (empties clipboard)."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("read");

        info!("clipboard: action={action}");

        let input_clone = input.clone();
        let action_owned = action.to_string();

        let result = tokio::task::spawn_blocking(move || {
            match action_owned.as_str() {
                "read" => Self::read_clipboard(),
                "write" => {
                    let text = input_clone
                        .get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if text.is_empty() {
                        Ok("write requires a \"text\" field.".to_string())
                    } else {
                        Self::write_clipboard(text)
                    }
                }
                "clear" => Self::clear_clipboard(),
                other => Ok(format!(
                    "Unknown action: '{other}'. Use: read, write, clear."
                )),
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!("spawn_blocking failed: {e}"))??;

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;
    use serde_json::json;

    #[tokio::test]
    async fn test_unknown_action() {
        let tool = ClipboardTool;
        let result = tool.execute(json!({"action": "fly"})).await.unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[tokio::test]
    async fn test_write_missing_text() {
        let tool = ClipboardTool;
        let result = tool.execute(json!({"action": "write"})).await.unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_read_default() {
        let tool = ClipboardTool;
        let result = tool.execute(json!({})).await.unwrap();
        // Should not error — either content or "empty"
        assert!(!result.is_empty());
    }
}
