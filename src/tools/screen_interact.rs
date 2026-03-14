//! Screen Interact Tool — mouse and keyboard UI interaction via enigo.
//!
//! Provides low-level mouse movement, clicking, scrolling, key presses,
//! and key combinations. Wraps the `enigo` crate and runs all operations
//! inside `spawn_blocking` to avoid blocking the async runtime.

use anyhow::Result;
use serde_json::Value;
use tracing::{info, warn};

/// Mouse/keyboard interaction tool using enigo.
pub struct ScreenInteractTool;

impl ScreenInteractTool {
    /// Move the mouse cursor to absolute screen coordinates.
    fn mouse_move(x: i32, y: i32) -> String {
        match enigo::Enigo::new(&enigo::Settings::default()) {
            Ok(mut enigo) => {
                use enigo::Mouse;
                match enigo.move_mouse(x, y, enigo::Coordinate::Abs) {
                    Ok(()) => format!("Mouse moved to ({x}, {y})"),
                    Err(e) => format!("Failed to move mouse: {e}"),
                }
            }
            Err(e) => format!("Failed to initialize enigo: {e}"),
        }
    }

    /// Click a mouse button at the given coordinates.
    fn mouse_click(x: i32, y: i32, button: &str) -> String {
        let btn = match button {
            "left" | "" => enigo::Button::Left,
            "right" => enigo::Button::Right,
            "middle" => enigo::Button::Middle,
            other => return format!("Unknown button: '{other}'. Use: left, right, middle."),
        };

        match enigo::Enigo::new(&enigo::Settings::default()) {
            Ok(mut enigo) => {
                use enigo::Mouse;
                if let Err(e) = enigo.move_mouse(x, y, enigo::Coordinate::Abs) {
                    return format!("Failed to move mouse to ({x}, {y}): {e}");
                }
                match enigo.button(btn, enigo::Direction::Click) {
                    Ok(()) => format!("Clicked {button} at ({x}, {y})"),
                    Err(e) => format!("Failed to click: {e}"),
                }
            }
            Err(e) => format!("Failed to initialize enigo: {e}"),
        }
    }

    /// Double-click at the given coordinates.
    fn double_click(x: i32, y: i32) -> String {
        match enigo::Enigo::new(&enigo::Settings::default()) {
            Ok(mut enigo) => {
                use enigo::Mouse;
                if let Err(e) = enigo.move_mouse(x, y, enigo::Coordinate::Abs) {
                    return format!("Failed to move mouse to ({x}, {y}): {e}");
                }
                // Two rapid clicks.
                for _ in 0..2 {
                    if let Err(e) = enigo.button(enigo::Button::Left, enigo::Direction::Click) {
                        return format!("Failed to double-click: {e}");
                    }
                }
                format!("Double-clicked at ({x}, {y})")
            }
            Err(e) => format!("Failed to initialize enigo: {e}"),
        }
    }

    /// Right-click at the given coordinates.
    fn right_click(x: i32, y: i32) -> String {
        Self::mouse_click(x, y, "right")
    }

    /// Press and release a single key.
    fn key_press(key: &str) -> String {
        if key.is_empty() {
            return "key_press requires a non-empty \"key\" field.".to_string();
        }

        match enigo::Enigo::new(&enigo::Settings::default()) {
            Ok(mut enigo) => {
                use enigo::Keyboard;
                match parse_key(key) {
                    Some(k) => match enigo.key(k, enigo::Direction::Click) {
                        Ok(()) => format!("Pressed key: {key}"),
                        Err(e) => format!("Failed to press '{key}': {e}"),
                    },
                    None => {
                        // Try typing it as text if it is a single character.
                        if key.len() == 1 {
                            match enigo.text(key) {
                                Ok(()) => format!("Typed character: {key}"),
                                Err(e) => format!("Failed to type '{key}': {e}"),
                            }
                        } else {
                            format!(
                                "Unknown key: '{key}'. Use: enter, tab, escape, space, \
                                 backspace, delete, up, down, left, right, home, end, \
                                 pageup, pagedown, f1-f12, or a single character."
                            )
                        }
                    }
                }
            }
            Err(e) => format!("Failed to initialize enigo: {e}"),
        }
    }

    /// Press a key combination (e.g., ctrl+c, alt+tab).
    fn key_combo(keys: &[String]) -> String {
        if keys.is_empty() {
            return "key_combo requires a non-empty \"keys\" array.".to_string();
        }

        match enigo::Enigo::new(&enigo::Settings::default()) {
            Ok(mut enigo) => {
                use enigo::Keyboard;
                let mut pressed: Vec<enigo::Key> = Vec::new();

                // Press all keys in order.
                for key_str in keys {
                    match parse_key(key_str) {
                        Some(k) => {
                            if let Err(e) = enigo.key(k, enigo::Direction::Press) {
                                // Release already-pressed keys before returning.
                                for pk in pressed.iter().rev() {
                                    let _ = enigo.key(*pk, enigo::Direction::Release);
                                }
                                return format!("Failed to press '{key_str}': {e}");
                            }
                            pressed.push(k);
                        }
                        None => {
                            // Release already-pressed keys before returning.
                            for pk in pressed.iter().rev() {
                                let _ = enigo.key(*pk, enigo::Direction::Release);
                            }
                            return format!("Unknown key in combo: '{key_str}'");
                        }
                    }
                }

                // Release all keys in reverse order.
                for k in pressed.iter().rev() {
                    if let Err(e) = enigo.key(*k, enigo::Direction::Release) {
                        warn!("Failed to release key: {e}");
                    }
                }

                let combo_str = keys.join("+");
                format!("Key combo: {combo_str}")
            }
            Err(e) => format!("Failed to initialize enigo: {e}"),
        }
    }

    /// Scroll the mouse wheel.
    fn scroll_wheel(direction: &str, amount: i32) -> String {
        match enigo::Enigo::new(&enigo::Settings::default()) {
            Ok(mut enigo) => {
                use enigo::Mouse;
                let result = match direction {
                    "up" => enigo.scroll(amount, enigo::Axis::Vertical),
                    "down" => enigo.scroll(-amount, enigo::Axis::Vertical),
                    "left" => enigo.scroll(-amount, enigo::Axis::Horizontal),
                    "right" => enigo.scroll(amount, enigo::Axis::Horizontal),
                    other => {
                        return format!(
                            "Unknown scroll direction: '{other}'. Use: up, down, left, right."
                        )
                    }
                };
                match result {
                    Ok(()) => format!("Scrolled {direction} by {amount}"),
                    Err(e) => format!("Failed to scroll: {e}"),
                }
            }
            Err(e) => format!("Failed to initialize enigo: {e}"),
        }
    }
}

/// Parse a key name string into an enigo `Key`.
fn parse_key(name: &str) -> Option<enigo::Key> {
    match name.to_lowercase().as_str() {
        "enter" | "return" => Some(enigo::Key::Return),
        "tab" => Some(enigo::Key::Tab),
        "escape" | "esc" => Some(enigo::Key::Escape),
        "space" => Some(enigo::Key::Space),
        "backspace" => Some(enigo::Key::Backspace),
        "delete" | "del" => Some(enigo::Key::Delete),
        "up" | "uparrow" => Some(enigo::Key::UpArrow),
        "down" | "downarrow" => Some(enigo::Key::DownArrow),
        "left" | "leftarrow" => Some(enigo::Key::LeftArrow),
        "right" | "rightarrow" => Some(enigo::Key::RightArrow),
        "home" => Some(enigo::Key::Home),
        "end" => Some(enigo::Key::End),
        "pageup" => Some(enigo::Key::PageUp),
        "pagedown" => Some(enigo::Key::PageDown),
        "f1" => Some(enigo::Key::F1),
        "f2" => Some(enigo::Key::F2),
        "f3" => Some(enigo::Key::F3),
        "f4" => Some(enigo::Key::F4),
        "f5" => Some(enigo::Key::F5),
        "f6" => Some(enigo::Key::F6),
        "f7" => Some(enigo::Key::F7),
        "f8" => Some(enigo::Key::F8),
        "f9" => Some(enigo::Key::F9),
        "f10" => Some(enigo::Key::F10),
        "f11" => Some(enigo::Key::F11),
        "f12" => Some(enigo::Key::F12),
        "ctrl" | "control" => Some(enigo::Key::Control),
        "alt" => Some(enigo::Key::Alt),
        "shift" => Some(enigo::Key::Shift),
        "meta" | "win" | "super" | "command" => Some(enigo::Key::Meta),
        "capslock" => Some(enigo::Key::CapsLock),
        // Single character keys.
        s if s.len() == 1 => {
            let c = s.chars().next().unwrap();
            Some(enigo::Key::Unicode(c))
        }
        _ => None,
    }
}

/// Parse a key combo string like "ctrl+c" into a vector of key name strings.
pub fn parse_combo_string(combo: &str) -> Vec<String> {
    combo
        .split('+')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[async_trait::async_trait]
impl super::Tool for ScreenInteractTool {
    fn name(&self) -> &'static str {
        "screen_interact"
    }

    fn description(&self) -> &'static str {
        "Mouse and keyboard interaction. Actions: \
         mouse_move (x, y), mouse_click (x, y, button?), \
         double_click (x, y), right_click (x, y), \
         key_press (key), key_combo (keys: [\"ctrl\", \"c\"]), \
         scroll_wheel (direction: up/down/left/right, amount). \
         Input: {\"action\": \"mouse_click\", \"x\": 500, \"y\": 300}."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("mouse_click");

        info!("screen_interact: action={action}");

        let input_clone = input.clone();
        let action_owned = action.to_string();

        let result = tokio::task::spawn_blocking(move || {
            match action_owned.as_str() {
                "mouse_move" | "move" => {
                    let x = input_clone.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let y = input_clone.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    Self::mouse_move(x, y)
                }
                "mouse_click" | "click" => {
                    let x = input_clone.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let y = input_clone.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let button = input_clone
                        .get("button")
                        .and_then(|v| v.as_str())
                        .unwrap_or("left");
                    Self::mouse_click(x, y, button)
                }
                "double_click" => {
                    let x = input_clone.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let y = input_clone.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    Self::double_click(x, y)
                }
                "right_click" => {
                    let x = input_clone.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let y = input_clone.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    Self::right_click(x, y)
                }
                "key_press" | "key" => {
                    let key = input_clone
                        .get("key")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    Self::key_press(key)
                }
                "key_combo" | "combo" | "hotkey" => {
                    // Accept either an array of keys or a combo string like "ctrl+c".
                    let keys: Vec<String> = if let Some(arr) = input_clone.get("keys").and_then(|v| v.as_array()) {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    } else if let Some(combo_str) = input_clone.get("keys").and_then(|v| v.as_str()) {
                        parse_combo_string(combo_str)
                    } else {
                        Vec::new()
                    };
                    Self::key_combo(&keys)
                }
                "scroll_wheel" | "scroll" => {
                    let direction = input_clone
                        .get("direction")
                        .and_then(|v| v.as_str())
                        .unwrap_or("down");
                    let amount = input_clone
                        .get("amount")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(3) as i32;
                    Self::scroll_wheel(direction, amount)
                }
                other => format!(
                    "Unknown action: '{other}'. Use: mouse_move, mouse_click, \
                     double_click, right_click, key_press, key_combo, scroll_wheel."
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
    async fn test_unknown_action() {
        let tool = ScreenInteractTool;
        let result = tool.execute(json!({"action": "fly"})).await.unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[test]
    fn test_key_combo_parse() {
        let keys = parse_combo_string("ctrl+shift+s");
        assert_eq!(keys, vec!["ctrl", "shift", "s"]);
    }

    #[test]
    fn test_key_combo_parse_spaces() {
        let keys = parse_combo_string("alt + tab");
        assert_eq!(keys, vec!["alt", "tab"]);
    }

    #[test]
    fn test_key_combo_parse_empty() {
        let keys = parse_combo_string("");
        assert!(keys.is_empty());
    }

    #[test]
    fn test_parse_key_known() {
        assert!(parse_key("enter").is_some());
        assert!(parse_key("ctrl").is_some());
        assert!(parse_key("f1").is_some());
        assert!(parse_key("a").is_some());
    }

    #[test]
    fn test_parse_key_unknown() {
        assert!(parse_key("nonexistent_key_name").is_none());
    }

    #[test]
    fn test_parse_key_case_insensitive() {
        assert!(parse_key("ENTER").is_some());
        assert!(parse_key("Ctrl").is_some());
        assert!(parse_key("Tab").is_some());
    }

    #[test]
    fn test_tool_name() {
        let tool = ScreenInteractTool;
        assert_eq!(tool.name(), "screen_interact");
    }

    #[tokio::test]
    async fn test_key_press_empty() {
        let tool = ScreenInteractTool;
        let result = tool
            .execute(json!({"action": "key_press", "key": ""}))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_key_combo_empty_keys() {
        let tool = ScreenInteractTool;
        let result = tool
            .execute(json!({"action": "key_combo", "keys": []}))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }
}
