//! Clock Tool — time, date, timezone queries.
//!
//! Pure Rust implementation using the `chrono` crate.

use anyhow::Result;
use serde_json::Value;
use tracing::info;

pub struct ClockTool;

impl ClockTool {
    fn get_time() -> String {
        let now = chrono::Local::now();
        format!("Current time: {}", now.format("%I:%M:%S %p"))
    }

    fn get_date() -> String {
        let now = chrono::Local::now();
        format!("Current date: {}", now.format("%A, %B %d, %Y"))
    }

    fn get_timezone() -> String {
        let now = chrono::Local::now();
        format!("Timezone: {} (UTC{})", now.format("%Z"), now.format("%:z"))
    }

    fn get_all() -> String {
        let now = chrono::Local::now();
        format!(
            "Date: {}\nTime: {}\nTimezone: {} (UTC{})\nUnix: {}\nISO 8601: {}",
            now.format("%A, %B %d, %Y"),
            now.format("%I:%M:%S %p"),
            now.format("%Z"),
            now.format("%:z"),
            now.timestamp(),
            now.format("%Y-%m-%dT%H:%M:%S%:z")
        )
    }

    fn get_unix() -> String {
        let now = chrono::Local::now();
        format!("Unix timestamp: {}", now.timestamp())
    }

    fn get_iso() -> String {
        let now = chrono::Local::now();
        format!("ISO 8601: {}", now.format("%Y-%m-%dT%H:%M:%S%:z"))
    }
}

#[async_trait::async_trait]
impl super::Tool for ClockTool {
    fn name(&self) -> &'static str {
        "clock"
    }

    fn description(&self) -> &'static str {
        "Get current time, date, and timezone information. \
         Input: {\"action\": \"<action>\"}. \
         Actions: get_time, get_date, get_timezone, get_all, get_unix, get_iso."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("get_all");

        info!("clock: action={action}");

        Ok(match action {
            "get_time" | "time" => Self::get_time(),
            "get_date" | "date" => Self::get_date(),
            "get_timezone" | "timezone" | "tz" => Self::get_timezone(),
            "get_unix" | "unix" | "timestamp" => Self::get_unix(),
            "get_iso" | "iso" => Self::get_iso(),
            "get_all" | "all" => Self::get_all(),
            other => format!(
                "Unknown action: '{other}'. Use: get_time, get_date, get_timezone, get_all, get_unix, get_iso."
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;
    use serde_json::json;

    #[tokio::test]
    async fn test_get_time() {
        let tool = ClockTool;
        let result = tool.execute(json!({"action": "get_time"})).await.unwrap();
        assert!(result.contains("Current time"));
    }

    #[tokio::test]
    async fn test_get_date() {
        let tool = ClockTool;
        let result = tool.execute(json!({"action": "get_date"})).await.unwrap();
        assert!(result.contains("Current date"));
    }

    #[tokio::test]
    async fn test_get_timezone() {
        let tool = ClockTool;
        let result = tool.execute(json!({"action": "get_timezone"})).await.unwrap();
        assert!(result.contains("Timezone"));
    }

    #[tokio::test]
    async fn test_get_all() {
        let tool = ClockTool;
        let result = tool.execute(json!({"action": "get_all"})).await.unwrap();
        assert!(result.contains("Date:"));
        assert!(result.contains("Time:"));
        assert!(result.contains("Unix:"));
    }

    #[tokio::test]
    async fn test_default_is_all() {
        let tool = ClockTool;
        let result = tool.execute(json!({})).await.unwrap();
        assert!(result.contains("Date:"));
    }

    #[tokio::test]
    async fn test_unix_timestamp() {
        let tool = ClockTool;
        let result = tool.execute(json!({"action": "get_unix"})).await.unwrap();
        assert!(result.contains("Unix timestamp"));
    }

    #[tokio::test]
    async fn test_iso_format() {
        let tool = ClockTool;
        let result = tool.execute(json!({"action": "get_iso"})).await.unwrap();
        assert!(result.contains("ISO 8601"));
        assert!(result.contains("T"));
    }

    #[tokio::test]
    async fn test_unknown_action() {
        let tool = ClockTool;
        let result = tool.execute(json!({"action": "fly"})).await.unwrap();
        assert!(result.contains("Unknown action"));
    }
}
