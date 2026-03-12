//! Text Transform Tool — string manipulation and encoding utilities.
//!
//! Actions: uppercase, lowercase, title_case, reverse, base64_encode, base64_decode,
//! url_encode, url_decode, md5, sha256, word_count, char_count, line_count.

use anyhow::Result;
use serde_json::Value;
use tracing::info;

pub struct TextTransformTool;

impl TextTransformTool {
    fn uppercase(text: &str) -> String {
        text.to_uppercase()
    }

    fn lowercase(text: &str) -> String {
        text.to_lowercase()
    }

    fn title_case(text: &str) -> String {
        text.split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(c) => {
                        let upper: String = c.to_uppercase().collect();
                        format!("{upper}{}", chars.as_str().to_lowercase())
                    }
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn reverse(text: &str) -> String {
        text.chars().rev().collect()
    }

    fn base64_encode(text: &str) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(text.as_bytes())
    }

    fn base64_decode(text: &str) -> Result<String> {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(text.trim())
            .map_err(|e| anyhow::anyhow!("Invalid base64: {e}"))?;
        String::from_utf8(bytes).map_err(|e| anyhow::anyhow!("Invalid UTF-8 in decoded data: {e}"))
    }

    fn url_encode(text: &str) -> String {
        let mut encoded = String::new();
        for byte in text.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    encoded.push(byte as char);
                }
                _ => {
                    encoded.push_str(&format!("%{:02X}", byte));
                }
            }
        }
        encoded
    }

    fn url_decode(text: &str) -> Result<String> {
        let mut decoded = Vec::new();
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                let hex = &text[i + 1..i + 3];
                let byte = u8::from_str_radix(hex, 16)
                    .map_err(|_| anyhow::anyhow!("Invalid URL encoding at position {i}"))?;
                decoded.push(byte);
                i += 3;
            } else if bytes[i] == b'+' {
                decoded.push(b' ');
                i += 1;
            } else {
                decoded.push(bytes[i]);
                i += 1;
            }
        }
        String::from_utf8(decoded)
            .map_err(|e| anyhow::anyhow!("Invalid UTF-8 in decoded URL: {e}"))
    }

    fn md5_hash(text: &str) -> String {
        use md5::Digest;
        let mut hasher = md5::Md5::new();
        hasher.update(text.as_bytes());
        let result = hasher.finalize();
        format!("{:x}", result)
    }

    fn sha256_hash(text: &str) -> String {
        use sha2::Digest;
        let mut hasher = sha2::Sha256::new();
        hasher.update(text.as_bytes());
        let result = hasher.finalize();
        format!("{:x}", result)
    }

    fn word_count(text: &str) -> String {
        let words = text.split_whitespace().count();
        format!("{words} words")
    }

    fn char_count(text: &str) -> String {
        let chars = text.chars().count();
        let bytes = text.len();
        format!("{chars} characters ({bytes} bytes)")
    }

    fn line_count(text: &str) -> String {
        let lines = if text.is_empty() { 0 } else { text.lines().count() };
        format!("{lines} lines")
    }

    fn count_all(text: &str) -> String {
        let words = text.split_whitespace().count();
        let chars = text.chars().count();
        let lines = if text.is_empty() { 0 } else { text.lines().count() };
        let bytes = text.len();
        format!("{words} words, {chars} characters, {lines} lines ({bytes} bytes)")
    }
}

#[async_trait::async_trait]
impl super::Tool for TextTransformTool {
    fn name(&self) -> &'static str {
        "text_transform"
    }

    fn description(&self) -> &'static str {
        "Transform and analyze text. Input: {\"action\": \"<action>\", \"text\": \"...\"}. \
         Actions: uppercase, lowercase, title_case, reverse, \
         base64_encode, base64_decode, url_encode, url_decode, \
         md5, sha256, word_count, char_count, line_count, count_all."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("count_all");

        let text = input
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if text.is_empty() && !["count_all", "word_count", "char_count", "line_count"].contains(&action) {
            return Ok("text_transform requires a \"text\" field.".to_string());
        }

        info!("text_transform: action={action}");

        Ok(match action {
            "uppercase" | "upper" => Self::uppercase(text),
            "lowercase" | "lower" => Self::lowercase(text),
            "title_case" | "title" => Self::title_case(text),
            "reverse" => Self::reverse(text),
            "base64_encode" | "b64encode" => Self::base64_encode(text),
            "base64_decode" | "b64decode" => {
                Self::base64_decode(text).unwrap_or_else(|e| format!("Error: {e}"))
            }
            "url_encode" => Self::url_encode(text),
            "url_decode" => {
                Self::url_decode(text).unwrap_or_else(|e| format!("Error: {e}"))
            }
            "md5" => Self::md5_hash(text),
            "sha256" => Self::sha256_hash(text),
            "word_count" | "words" => Self::word_count(text),
            "char_count" | "chars" => Self::char_count(text),
            "line_count" | "lines" => Self::line_count(text),
            "count_all" | "count" | "stats" => Self::count_all(text),
            other => format!(
                "Unknown action: '{other}'. Use: uppercase, lowercase, title_case, reverse, \
                 base64_encode, base64_decode, url_encode, url_decode, md5, sha256, \
                 word_count, char_count, line_count, count_all."
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;
    use serde_json::json;

    #[test]
    fn test_uppercase() {
        assert_eq!(TextTransformTool::uppercase("hello world"), "HELLO WORLD");
    }

    #[test]
    fn test_lowercase() {
        assert_eq!(TextTransformTool::lowercase("HELLO WORLD"), "hello world");
    }

    #[test]
    fn test_title_case() {
        assert_eq!(TextTransformTool::title_case("hello world"), "Hello World");
    }

    #[test]
    fn test_title_case_mixed() {
        assert_eq!(TextTransformTool::title_case("hELLO wORLD"), "Hello World");
    }

    #[test]
    fn test_reverse() {
        assert_eq!(TextTransformTool::reverse("hello"), "olleh");
    }

    #[test]
    fn test_base64_roundtrip() {
        let original = "Hello, World!";
        let encoded = TextTransformTool::base64_encode(original);
        let decoded = TextTransformTool::base64_decode(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_base64_encode_known() {
        assert_eq!(TextTransformTool::base64_encode("Hello"), "SGVsbG8=");
    }

    #[test]
    fn test_base64_decode_invalid() {
        assert!(TextTransformTool::base64_decode("!!!not-base64!!!").is_err());
    }

    #[test]
    fn test_url_encode() {
        assert_eq!(TextTransformTool::url_encode("hello world"), "hello%20world");
        assert_eq!(TextTransformTool::url_encode("a=1&b=2"), "a%3D1%26b%3D2");
    }

    #[test]
    fn test_url_decode() {
        assert_eq!(TextTransformTool::url_decode("hello%20world").unwrap(), "hello world");
        assert_eq!(TextTransformTool::url_decode("hello+world").unwrap(), "hello world");
    }

    #[test]
    fn test_md5() {
        // md5("hello") = 5d41402abc4b2a76b9719d911017c592
        assert_eq!(TextTransformTool::md5_hash("hello"), "5d41402abc4b2a76b9719d911017c592");
    }

    #[test]
    fn test_sha256() {
        // sha256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        assert_eq!(
            TextTransformTool::sha256_hash("hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_word_count() {
        assert_eq!(TextTransformTool::word_count("hello beautiful world"), "3 words");
    }

    #[test]
    fn test_char_count() {
        let result = TextTransformTool::char_count("hello");
        assert!(result.contains("5 characters"));
    }

    #[test]
    fn test_line_count() {
        assert_eq!(TextTransformTool::line_count("a\nb\nc"), "3 lines");
    }

    #[test]
    fn test_line_count_empty() {
        assert_eq!(TextTransformTool::line_count(""), "0 lines");
    }

    #[test]
    fn test_count_all() {
        let result = TextTransformTool::count_all("hello world");
        assert!(result.contains("2 words"));
        assert!(result.contains("11 characters"));
    }

    #[tokio::test]
    async fn test_tool_uppercase() {
        let tool = TextTransformTool;
        let result = tool
            .execute(json!({"action": "uppercase", "text": "hello"}))
            .await
            .unwrap();
        assert_eq!(result, "HELLO");
    }

    #[tokio::test]
    async fn test_tool_missing_text() {
        let tool = TextTransformTool;
        let result = tool
            .execute(json!({"action": "uppercase"}))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_tool_unknown_action() {
        let tool = TextTransformTool;
        let result = tool
            .execute(json!({"action": "fly", "text": "hello"}))
            .await
            .unwrap();
        assert!(result.contains("Unknown action"));
    }
}
