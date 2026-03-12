//! Computer Control Tool — mouse/keyboard UI automation via enigo.
//!
//! Ported from `sovereign_titan/computer_use/ui_automation.py`.
//! Provides the AI with direct mouse/keyboard control for UI automation tasks.

use anyhow::Result;
use enigo::{
    Button, Coordinate, Direction, Enigo, Keyboard, Mouse, Settings,
};
use serde_json::Value;
use std::thread;
use std::time::Duration;

/// Tool for controlling the mouse and keyboard on the host machine.
pub struct ComputerControlTool;

#[async_trait::async_trait]
impl super::Tool for ComputerControlTool {
    fn name(&self) -> &'static str {
        "computer_control"
    }

    fn description(&self) -> &'static str {
        "Control the mouse and keyboard. Actions: move_mouse {x, y}, click {x, y, button?}, \
         double_click {x, y}, type_text {text}, key_press {key}, scroll {direction, amount?}"
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();

        if action.is_empty() {
            return Ok("Error: missing 'action'. Use: move_mouse, click, double_click, type_text, key_press, scroll".into());
        }

        // Run enigo operations on a blocking thread (enigo is not async-safe).
        let input_clone = input.clone();
        let result = tokio::task::spawn_blocking(move || execute_action(&action, &input_clone))
            .await
            .map_err(|e| anyhow::anyhow!("spawn_blocking failed: {e}"))??;

        Ok(result)
    }
}

/// Execute a computer control action synchronously.
fn execute_action(action: &str, input: &Value) -> Result<String> {
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| anyhow::anyhow!("Failed to initialize enigo: {e}"))?;

    match action {
        "move_mouse" => {
            let x = input.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let y = input.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            enigo
                .move_mouse(x, y, Coordinate::Abs)
                .map_err(|e| anyhow::anyhow!("move_mouse failed: {e}"))?;
            Ok(format!("Moved mouse to ({x}, {y})"))
        }

        "click" => {
            let x = input.get("x").and_then(|v| v.as_i64());
            let y = input.get("y").and_then(|v| v.as_i64());
            let button = input
                .get("button")
                .and_then(|v| v.as_str())
                .unwrap_or("left");

            // Move to coordinates if provided.
            if let (Some(x), Some(y)) = (x, y) {
                enigo
                    .move_mouse(x as i32, y as i32, Coordinate::Abs)
                    .map_err(|e| anyhow::anyhow!("move_mouse failed: {e}"))?;
                thread::sleep(Duration::from_millis(50));
            }

            let btn = match button {
                "right" => Button::Right,
                "middle" => Button::Middle,
                _ => Button::Left,
            };
            enigo
                .button(btn, Direction::Click)
                .map_err(|e| anyhow::anyhow!("click failed: {e}"))?;

            match (x, y) {
                (Some(x), Some(y)) => Ok(format!("Clicked {button} at ({x}, {y})")),
                _ => Ok(format!("Clicked {button} at current position")),
            }
        }

        "double_click" => {
            let x = input.get("x").and_then(|v| v.as_i64());
            let y = input.get("y").and_then(|v| v.as_i64());

            if let (Some(x), Some(y)) = (x, y) {
                enigo
                    .move_mouse(x as i32, y as i32, Coordinate::Abs)
                    .map_err(|e| anyhow::anyhow!("move_mouse failed: {e}"))?;
                thread::sleep(Duration::from_millis(50));
            }

            enigo
                .button(Button::Left, Direction::Click)
                .map_err(|e| anyhow::anyhow!("click failed: {e}"))?;
            thread::sleep(Duration::from_millis(50));
            enigo
                .button(Button::Left, Direction::Click)
                .map_err(|e| anyhow::anyhow!("click failed: {e}"))?;

            match (x, y) {
                (Some(x), Some(y)) => Ok(format!("Double-clicked at ({x}, {y})")),
                _ => Ok("Double-clicked at current position".into()),
            }
        }

        "type_text" => {
            let text = input
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if text.is_empty() {
                return Ok("Error: missing 'text' field".into());
            }
            // Safety: cap at 2000 characters to prevent runaway input.
            let safe_text = if text.len() > 2000 { &text[..2000] } else { text };
            enigo
                .text(safe_text)
                .map_err(|e| anyhow::anyhow!("type_text failed: {e}"))?;
            Ok(format!("Typed {} characters", safe_text.len()))
        }

        "key_press" => {
            let key = input
                .get("key")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if key.is_empty() {
                return Ok("Error: missing 'key' field".into());
            }

            use enigo::Key;
            let enigo_key = match key.to_lowercase().as_str() {
                "enter" | "return" => Key::Return,
                "tab" => Key::Tab,
                "escape" | "esc" => Key::Escape,
                "backspace" => Key::Backspace,
                "delete" => Key::Delete,
                "space" => Key::Space,
                "up" => Key::UpArrow,
                "down" => Key::DownArrow,
                "left" => Key::LeftArrow,
                "right" => Key::RightArrow,
                "home" => Key::Home,
                "end" => Key::End,
                "pageup" => Key::PageUp,
                "pagedown" => Key::PageDown,
                "f1" => Key::F1,
                "f2" => Key::F2,
                "f3" => Key::F3,
                "f4" => Key::F4,
                "f5" => Key::F5,
                "f11" => Key::F11,
                "f12" => Key::F12,
                other => {
                    // Try single character keys.
                    if let Some(ch) = other.chars().next() {
                        if other.len() == ch.len_utf8() {
                            Key::Unicode(ch)
                        } else {
                            return Ok(format!("Unknown key: '{other}'"));
                        }
                    } else {
                        return Ok(format!("Unknown key: '{other}'"));
                    }
                }
            };
            enigo
                .key(enigo_key, Direction::Click)
                .map_err(|e| anyhow::anyhow!("key_press failed: {e}"))?;
            Ok(format!("Pressed key: {key}"))
        }

        "scroll" => {
            let direction = input
                .get("direction")
                .and_then(|v| v.as_str())
                .unwrap_or("down");
            let amount = input
                .get("amount")
                .and_then(|v| v.as_i64())
                .unwrap_or(3) as i32;

            let scroll_amount = match direction {
                "up" => amount,
                "down" => -amount,
                _ => -amount,
            };

            enigo
                .scroll(scroll_amount, enigo::Axis::Vertical)
                .map_err(|e| anyhow::anyhow!("scroll failed: {e}"))?;
            Ok(format!("Scrolled {direction} by {amount}"))
        }

        _ => Ok(format!(
            "Unknown action: '{action}'. Use: move_mouse, click, double_click, type_text, key_press, scroll"
        )),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_unknown_action() {
        let _tool = super::ComputerControlTool;
    }
}
