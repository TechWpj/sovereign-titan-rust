//! Pro Document Tool — professional document creation.
//!
//! Generates formatted professional documents including business letters,
//! invoices, and meeting/project notes. Outputs are saved as Markdown or
//! plain text files in the workspace documents directory.

use anyhow::Result;
use serde_json::Value;
use std::path::PathBuf;
use tracing::info;

/// Professional document creation tool.
pub struct ProDocumentTool;

impl ProDocumentTool {
    /// Get the workspace documents directory.
    fn workspace_dir() -> PathBuf {
        let base = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        let dir = base.join("Sovereign Titan").join("pro_documents");
        std::fs::create_dir_all(&dir).ok();
        dir
    }

    /// Sanitize a filename — no path traversal, no special chars.
    fn sanitize_filename(name: &str) -> String {
        let no_path: String = name.replace('\\', "").replace('/', "");
        let cleaned: String = no_path
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
            .collect();
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
        let result = result.trim_start_matches('.').to_string();
        if result.is_empty() {
            "document.md".to_string()
        } else {
            result
        }
    }

    /// Create a professional business letter.
    fn create_letter(recipient: &str, subject: &str, body: &str, sender: &str) -> String {
        if body.is_empty() {
            return "create_letter requires a non-empty \"body\" field.".to_string();
        }

        let date = chrono::Local::now().format("%B %d, %Y").to_string();
        let sender_name = if sender.is_empty() { "Sovereign Titan" } else { sender };
        let recipient_name = if recipient.is_empty() {
            "To Whom It May Concern"
        } else {
            recipient
        };
        let subject_line = if subject.is_empty() {
            String::new()
        } else {
            format!("\n**Re: {subject}**\n")
        };

        let content = format!(
            "# Business Letter\n\n\
             **Date:** {date}\n\n\
             **To:** {recipient_name}\n\
             **From:** {sender_name}\n\
             {subject_line}\n\
             ---\n\n\
             Dear {recipient_name},\n\n\
             {body}\n\n\
             Sincerely,\n\n\
             {sender_name}\n"
        );

        let slug = Self::sanitize_filename(
            &format!("letter_{}", subject.replace(' ', "_"))
                .chars()
                .take(40)
                .collect::<String>(),
        );
        let filename = format!("{slug}.md");
        let path = Self::workspace_dir().join(&filename);

        match std::fs::write(&path, &content) {
            Ok(()) => format!(
                "Created business letter: **{}** ({} bytes)",
                path.display(),
                content.len()
            ),
            Err(e) => format!("Failed to create letter: {e}"),
        }
    }

    /// Create an invoice document.
    fn create_invoice(items: &[InvoiceItem], total: &str, recipient: &str) -> String {
        if items.is_empty() {
            return "create_invoice requires at least one item in the \"items\" array.".to_string();
        }

        let date = chrono::Local::now().format("%B %d, %Y").to_string();
        let invoice_num = chrono::Local::now().format("INV-%Y%m%d-%H%M").to_string();
        let recipient_name = if recipient.is_empty() { "Client" } else { recipient };

        let mut content = format!(
            "# Invoice {invoice_num}\n\n\
             **Date:** {date}\n\
             **Bill To:** {recipient_name}\n\n\
             ---\n\n\
             | # | Description | Qty | Unit Price | Amount |\n\
             |---|-------------|-----|------------|--------|\n"
        );

        let mut calculated_total = 0.0f64;
        for (i, item) in items.iter().enumerate() {
            let amount = item.quantity as f64 * item.unit_price;
            calculated_total += amount;
            content.push_str(&format!(
                "| {} | {} | {} | ${:.2} | ${:.2} |\n",
                i + 1,
                item.description,
                item.quantity,
                item.unit_price,
                amount
            ));
        }

        // Use provided total or calculated total.
        let total_str = if total.is_empty() {
            format!("${calculated_total:.2}")
        } else {
            total.to_string()
        };

        content.push_str(&format!(
            "\n**Total: {total_str}**\n\n\
             ---\n\n\
             *Thank you for your business.*\n"
        ));

        let filename = format!("{invoice_num}.md");
        let path = Self::workspace_dir().join(Self::sanitize_filename(&filename));

        match std::fs::write(&path, &content) {
            Ok(()) => format!(
                "Created invoice: **{}** ({} items, total: {total_str})",
                path.display(),
                items.len()
            ),
            Err(e) => format!("Failed to create invoice: {e}"),
        }
    }

    /// Create structured meeting or project notes.
    fn create_notes(title: &str, sections: &[NoteSection]) -> String {
        if title.is_empty() {
            return "create_notes requires a non-empty \"title\" field.".to_string();
        }

        let date = chrono::Local::now().format("%B %d, %Y at %H:%M").to_string();

        let mut content = format!("# {title}\n\n**Date:** {date}\n\n---\n\n");

        if sections.is_empty() {
            content.push_str("*(No sections provided)*\n");
        } else {
            for section in sections {
                content.push_str(&format!("## {}\n\n", section.heading));
                for point in &section.points {
                    content.push_str(&format!("- {point}\n"));
                }
                content.push('\n');
            }
        }

        let slug = Self::sanitize_filename(
            &format!("notes_{}", title.replace(' ', "_"))
                .chars()
                .take(40)
                .collect::<String>(),
        );
        let filename = format!("{slug}.md");
        let path = Self::workspace_dir().join(&filename);

        match std::fs::write(&path, &content) {
            Ok(()) => format!(
                "Created notes: **{}** ({} sections, {} bytes)",
                path.display(),
                sections.len(),
                content.len()
            ),
            Err(e) => format!("Failed to create notes: {e}"),
        }
    }
}

/// A single invoice line item.
struct InvoiceItem {
    description: String,
    quantity: u32,
    unit_price: f64,
}

/// A section within notes, containing a heading and bullet points.
struct NoteSection {
    heading: String,
    points: Vec<String>,
}

/// Parse invoice items from JSON array.
fn parse_invoice_items(items_val: &Value) -> Vec<InvoiceItem> {
    items_val
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|item| InvoiceItem {
                    description: item
                        .get("description")
                        .or_else(|| item.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("Item")
                        .to_string(),
                    quantity: item
                        .get("quantity")
                        .or_else(|| item.get("qty"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(1) as u32,
                    unit_price: item
                        .get("unit_price")
                        .or_else(|| item.get("price"))
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parse note sections from JSON array.
fn parse_note_sections(sections_val: &Value) -> Vec<NoteSection> {
    sections_val
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|sec| {
                    let heading = sec
                        .get("heading")
                        .or_else(|| sec.get("title"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("Section")
                        .to_string();
                    let points: Vec<String> = sec
                        .get("points")
                        .or_else(|| sec.get("items"))
                        .and_then(|v| v.as_array())
                        .map(|pts| {
                            pts.iter()
                                .filter_map(|p| p.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    NoteSection { heading, points }
                })
                .collect()
        })
        .unwrap_or_default()
}

#[async_trait::async_trait]
impl super::Tool for ProDocumentTool {
    fn name(&self) -> &'static str {
        "pro_document"
    }

    fn description(&self) -> &'static str {
        "Create professional documents. Actions: \
         create_letter (recipient, subject, body, sender?), \
         create_invoice (items: [{description, quantity, unit_price}], total?, recipient?), \
         create_notes (title, sections: [{heading, points: []}]). \
         Input: {\"action\": \"create_notes\", \"title\": \"Sprint 5\", \
         \"sections\": [{\"heading\": \"Done\", \"points\": [\"Feature X\"]}]}."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("create_notes");

        info!("pro_document: action={action}");

        match action {
            "create_letter" | "letter" => {
                let recipient = input
                    .get("recipient")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let subject = input
                    .get("subject")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let body = input
                    .get("body")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let sender = input
                    .get("sender")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                Ok(Self::create_letter(recipient, subject, body, sender))
            }
            "create_invoice" | "invoice" => {
                let items_val = input.get("items").cloned().unwrap_or(Value::Null);
                let items = parse_invoice_items(&items_val);
                let total = input
                    .get("total")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let recipient = input
                    .get("recipient")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                Ok(Self::create_invoice(&items, total, recipient))
            }
            "create_notes" | "notes" => {
                let title = input
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let sections_val = input
                    .get("sections")
                    .cloned()
                    .unwrap_or(Value::Null);
                let sections = parse_note_sections(&sections_val);
                Ok(Self::create_notes(title, &sections))
            }
            other => Ok(format!(
                "Unknown action: '{other}'. Use: create_letter, create_invoice, create_notes."
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;
    use serde_json::json;

    #[tokio::test]
    async fn test_create_notes() {
        let tool = ProDocumentTool;
        let result = tool
            .execute(json!({
                "action": "create_notes",
                "title": "Test Meeting Notes",
                "sections": [
                    {
                        "heading": "Discussion",
                        "points": ["Point A", "Point B"]
                    },
                    {
                        "heading": "Action Items",
                        "points": ["Do X", "Do Y"]
                    }
                ]
            }))
            .await
            .unwrap();
        assert!(result.contains("Created notes"));
        assert!(result.contains("2 sections"));
    }

    #[tokio::test]
    async fn test_unknown_action() {
        let tool = ProDocumentTool;
        let result = tool.execute(json!({"action": "fly"})).await.unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[tokio::test]
    async fn test_create_notes_empty_title() {
        let tool = ProDocumentTool;
        let result = tool
            .execute(json!({"action": "create_notes", "title": ""}))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_create_letter_empty_body() {
        let tool = ProDocumentTool;
        let result = tool
            .execute(json!({
                "action": "create_letter",
                "recipient": "Alice",
                "subject": "Test",
                "body": ""
            }))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_create_letter() {
        let tool = ProDocumentTool;
        let result = tool
            .execute(json!({
                "action": "create_letter",
                "recipient": "Alice Smith",
                "subject": "Project Update",
                "body": "The project is on track for Q2 delivery.",
                "sender": "Bob Jones"
            }))
            .await
            .unwrap();
        assert!(result.contains("Created business letter"));
    }

    #[tokio::test]
    async fn test_create_invoice() {
        let tool = ProDocumentTool;
        let result = tool
            .execute(json!({
                "action": "create_invoice",
                "recipient": "Acme Corp",
                "items": [
                    {"description": "Consulting", "quantity": 10, "unit_price": 150.0},
                    {"description": "Development", "quantity": 5, "unit_price": 200.0}
                ]
            }))
            .await
            .unwrap();
        assert!(result.contains("Created invoice"));
        assert!(result.contains("2 items"));
    }

    #[tokio::test]
    async fn test_create_invoice_no_items() {
        let tool = ProDocumentTool;
        let result = tool
            .execute(json!({
                "action": "create_invoice",
                "recipient": "Test"
            }))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(ProDocumentTool::sanitize_filename("report.md"), "report.md");
        assert_eq!(ProDocumentTool::sanitize_filename(""), "document.md");
        let result = ProDocumentTool::sanitize_filename("../../etc/passwd");
        assert!(!result.contains(".."));
    }

    #[test]
    fn test_parse_invoice_items() {
        let val = json!([
            {"description": "Widget", "quantity": 3, "unit_price": 9.99},
            {"name": "Service", "qty": 1, "price": 50.0}
        ]);
        let items = parse_invoice_items(&val);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].description, "Widget");
        assert_eq!(items[0].quantity, 3);
        assert!((items[0].unit_price - 9.99).abs() < f64::EPSILON);
        assert_eq!(items[1].description, "Service");
    }

    #[test]
    fn test_parse_note_sections() {
        let val = json!([
            {"heading": "Intro", "points": ["A", "B"]},
            {"title": "Summary", "items": ["C"]}
        ]);
        let sections = parse_note_sections(&val);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].heading, "Intro");
        assert_eq!(sections[0].points.len(), 2);
        assert_eq!(sections[1].heading, "Summary");
    }

    #[test]
    fn test_tool_name() {
        let tool = ProDocumentTool;
        assert_eq!(tool.name(), "pro_document");
    }
}
