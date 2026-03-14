//! Email and Calendar Data Types — core structs for messages and events.
//!
//! Ported from `sovereign_titan/email_calendar/types.py`. Provides serializable
//! data structures for email messages, attachments, and calendar events with
//! auto-generated IDs.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// An email message with metadata, body, and attachments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailMessage {
    /// Unique message identifier (format: `msg_{timestamp}_{rand}`).
    pub id: String,
    /// Email subject line.
    pub subject: String,
    /// Sender address.
    pub sender: String,
    /// List of recipient addresses.
    pub recipients: Vec<String>,
    /// Message body (plain text or HTML).
    pub body: String,
    /// Unix timestamp of when the message was created/received.
    pub timestamp: f64,
    /// File attachments.
    pub attachments: Vec<Attachment>,
    /// Whether the message has been read.
    pub read: bool,
    /// Labels or tags applied to the message.
    pub labels: Vec<String>,
}

impl EmailMessage {
    /// Create a new email message with an auto-generated ID and current timestamp.
    pub fn new(
        subject: &str,
        sender: &str,
        recipients: Vec<String>,
        body: &str,
    ) -> Self {
        Self {
            id: generate_message_id(),
            subject: subject.to_string(),
            sender: sender.to_string(),
            recipients,
            body: body.to_string(),
            timestamp: current_epoch_secs(),
            attachments: Vec::new(),
            read: false,
            labels: Vec::new(),
        }
    }

    /// Add an attachment to this message.
    pub fn add_attachment(&mut self, attachment: Attachment) {
        self.attachments.push(attachment);
    }

    /// Mark this message as read.
    pub fn mark_read(&mut self) {
        self.read = true;
    }

    /// Add a label to this message.
    pub fn add_label(&mut self, label: &str) {
        if !self.labels.contains(&label.to_string()) {
            self.labels.push(label.to_string());
        }
    }
}

/// A file attachment on an email message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    /// Original filename.
    pub filename: String,
    /// MIME content type (e.g. `"application/pdf"`).
    pub content_type: String,
    /// File size in bytes.
    pub size: u64,
}

impl Attachment {
    /// Create a new attachment descriptor.
    pub fn new(filename: &str, content_type: &str, size: u64) -> Self {
        Self {
            filename: filename.to_string(),
            content_type: content_type.to_string(),
            size,
        }
    }
}

/// A calendar event with time range, attendees, and reminders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarEvent {
    /// Unique event identifier (format: `evt_{timestamp}_{rand}`).
    pub id: String,
    /// Event title.
    pub title: String,
    /// Start time as Unix timestamp.
    pub start_time: f64,
    /// End time as Unix timestamp.
    pub end_time: f64,
    /// Event description or notes.
    pub description: String,
    /// Location (physical address or virtual meeting URL).
    pub location: String,
    /// List of attendee email addresses.
    pub attendees: Vec<String>,
    /// Reminder offsets in minutes before the event.
    pub reminders: Vec<u32>,
}

impl CalendarEvent {
    /// Create a new calendar event with an auto-generated ID.
    pub fn new(
        title: &str,
        start_time: f64,
        end_time: f64,
        description: &str,
    ) -> Self {
        Self {
            id: generate_event_id(),
            title: title.to_string(),
            start_time,
            end_time,
            description: description.to_string(),
            location: String::new(),
            attendees: Vec::new(),
            reminders: vec![15], // Default 15-minute reminder.
        }
    }

    /// Duration of the event in minutes.
    pub fn duration_minutes(&self) -> f64 {
        (self.end_time - self.start_time) / 60.0
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ID Generation Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Get current time as epoch seconds (f64).
fn current_epoch_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// Generate a unique message ID: `msg_{timestamp_millis}_{random_hex}`.
fn generate_message_id() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let rand: u32 = rand::random();
    format!("msg_{}_{:08x}", ts, rand)
}

/// Generate a unique event ID: `evt_{timestamp_millis}_{random_hex}`.
fn generate_event_id() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let rand: u32 = rand::random();
    format!("evt_{}_{:08x}", ts, rand)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_email_message_creation() {
        let msg = EmailMessage::new(
            "Hello",
            "alice@example.com",
            vec!["bob@example.com".to_string()],
            "Hi Bob!",
        );
        assert!(msg.id.starts_with("msg_"));
        assert_eq!(msg.subject, "Hello");
        assert_eq!(msg.sender, "alice@example.com");
        assert_eq!(msg.recipients.len(), 1);
        assert!(!msg.read);
        assert!(msg.timestamp > 0.0);
    }

    #[test]
    fn test_email_mark_read() {
        let mut msg = EmailMessage::new("Test", "a@b.com", vec![], "body");
        assert!(!msg.read);
        msg.mark_read();
        assert!(msg.read);
    }

    #[test]
    fn test_email_add_label() {
        let mut msg = EmailMessage::new("Test", "a@b.com", vec![], "body");
        msg.add_label("important");
        msg.add_label("important"); // duplicate
        assert_eq!(msg.labels.len(), 1);
        assert_eq!(msg.labels[0], "important");
    }

    #[test]
    fn test_email_add_attachment() {
        let mut msg = EmailMessage::new("Test", "a@b.com", vec![], "body");
        msg.add_attachment(Attachment::new("report.pdf", "application/pdf", 1024));
        assert_eq!(msg.attachments.len(), 1);
        assert_eq!(msg.attachments[0].filename, "report.pdf");
        assert_eq!(msg.attachments[0].size, 1024);
    }

    #[test]
    fn test_calendar_event_creation() {
        let evt = CalendarEvent::new("Meeting", 1000.0, 4600.0, "Team sync");
        assert!(evt.id.starts_with("evt_"));
        assert_eq!(evt.title, "Meeting");
        assert_eq!(evt.start_time, 1000.0);
        assert_eq!(evt.end_time, 4600.0);
        assert_eq!(evt.reminders, vec![15]);
    }

    #[test]
    fn test_calendar_event_duration() {
        let evt = CalendarEvent::new("Meeting", 0.0, 3600.0, "");
        assert!((evt.duration_minutes() - 60.0).abs() < 0.001);
    }

    #[test]
    fn test_email_serialization() {
        let msg = EmailMessage::new("Ser Test", "a@b.com", vec!["c@d.com".to_string()], "body");
        let json = serde_json::to_string(&msg).unwrap();
        let restored: EmailMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.subject, "Ser Test");
        assert_eq!(restored.sender, "a@b.com");
    }

    #[test]
    fn test_calendar_serialization() {
        let evt = CalendarEvent::new("Serialized", 100.0, 200.0, "desc");
        let json = serde_json::to_string(&evt).unwrap();
        let restored: CalendarEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.title, "Serialized");
        assert_eq!(restored.start_time, 100.0);
    }

    #[test]
    fn test_unique_message_ids() {
        let id1 = generate_message_id();
        let id2 = generate_message_id();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_attachment_new() {
        let att = Attachment::new("photo.jpg", "image/jpeg", 500_000);
        assert_eq!(att.filename, "photo.jpg");
        assert_eq!(att.content_type, "image/jpeg");
        assert_eq!(att.size, 500_000);
    }
}
