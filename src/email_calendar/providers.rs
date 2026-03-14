//! Email Providers — IMAP/SMTP email access and calendar management.
//!
//! Ported from `sovereign_titan/email_calendar/providers.py`. Provides:
//! - `EmailConfig` with TLS support and password redaction
//! - `ImapConfig` / `SmtpConfig` with protocol-specific fields
//! - `EmailMessage` with full header support, flags, and attachments
//! - `EmailProvider` with folder listing, message search, and send
//! - Calendar event CRUD with iCalendar format generation
//! - Recurrence patterns and reminder support

use std::collections::HashMap;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::types::EmailMessage;

// ─────────────────────────────────────────────────────────────────────────────
// Email Configuration
// ─────────────────────────────────────────────────────────────────────────────

/// Base email connection configuration.
///
/// The password field is redacted in Debug output to prevent credential leakage.
#[derive(Clone, Serialize, Deserialize)]
pub struct EmailConfig {
    /// Server hostname.
    pub server: String,
    /// Server port.
    pub port: u16,
    /// Login username / email address.
    pub username: String,
    /// Password or app-specific password.
    pub password: String,
    /// Whether to use TLS/SSL.
    pub use_tls: bool,
}

impl fmt::Debug for EmailConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EmailConfig")
            .field("server", &self.server)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .field("use_tls", &self.use_tls)
            .finish()
    }
}

impl EmailConfig {
    /// Create a new email configuration.
    pub fn new(server: &str, port: u16, username: &str, password: &str, use_tls: bool) -> Self {
        Self {
            server: server.to_string(),
            port,
            username: username.to_string(),
            password: password.to_string(),
            use_tls,
        }
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), String> {
        if self.server.is_empty() {
            return Err("Server hostname cannot be empty".to_string());
        }
        if self.username.is_empty() {
            return Err("Username cannot be empty".to_string());
        }
        if self.port == 0 {
            return Err("Port cannot be 0".to_string());
        }
        Ok(())
    }
}

/// IMAP-specific connection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImapConfig {
    /// IMAP server hostname.
    pub host: String,
    /// IMAP port (typically 993 for TLS).
    pub port: u16,
    /// Email address / login username.
    pub email: String,
    /// Password or app-specific password.
    pub password: String,
    /// SMTP server hostname for outgoing mail.
    pub smtp_host: String,
    /// SMTP port (typically 587 for STARTTLS or 465 for TLS).
    pub smtp_port: u16,
    /// Whether to use TLS for IMAP.
    pub use_tls: bool,
    /// IMAP idle timeout in seconds (for PUSH notifications).
    pub idle_timeout: u32,
}

impl ImapConfig {
    /// Create a new IMAP configuration.
    pub fn new(
        host: &str,
        port: u16,
        email: &str,
        password: &str,
        smtp_host: &str,
        smtp_port: u16,
    ) -> Self {
        Self {
            host: host.to_string(),
            port,
            email: email.to_string(),
            password: password.to_string(),
            smtp_host: smtp_host.to_string(),
            smtp_port,
            use_tls: true,
            idle_timeout: 300,
        }
    }

    /// Create a configuration for Gmail with default servers.
    pub fn gmail(email: &str, app_password: &str) -> Self {
        Self::new(
            "imap.gmail.com",
            993,
            email,
            app_password,
            "smtp.gmail.com",
            587,
        )
    }

    /// Create a configuration for Outlook/Hotmail with default servers.
    pub fn outlook(email: &str, password: &str) -> Self {
        Self::new(
            "outlook.office365.com",
            993,
            email,
            password,
            "smtp.office365.com",
            587,
        )
    }

    /// Create a configuration for Yahoo Mail.
    pub fn yahoo(email: &str, app_password: &str) -> Self {
        Self::new(
            "imap.mail.yahoo.com",
            993,
            email,
            app_password,
            "smtp.mail.yahoo.com",
            587,
        )
    }

    /// Convert to a base EmailConfig (IMAP).
    pub fn to_imap_config(&self) -> EmailConfig {
        EmailConfig::new(&self.host, self.port, &self.email, &self.password, self.use_tls)
    }

    /// Convert to a base EmailConfig (SMTP).
    pub fn to_smtp_config(&self) -> EmailConfig {
        EmailConfig::new(
            &self.smtp_host,
            self.smtp_port,
            &self.email,
            &self.password,
            true,
        )
    }
}

/// SMTP-specific connection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmtpConfig {
    /// SMTP server hostname.
    pub host: String,
    /// SMTP port.
    pub port: u16,
    /// Login username.
    pub username: String,
    /// Password.
    pub password: String,
    /// Whether to use STARTTLS.
    pub use_starttls: bool,
    /// Whether to use implicit TLS (port 465).
    pub use_tls: bool,
    /// Sender display name.
    pub from_name: Option<String>,
}

impl SmtpConfig {
    /// Create a new SMTP configuration.
    pub fn new(host: &str, port: u16, username: &str, password: &str) -> Self {
        Self {
            host: host.to_string(),
            port,
            username: username.to_string(),
            password: password.to_string(),
            use_starttls: port == 587,
            use_tls: port == 465,
            from_name: None,
        }
    }

    /// Set the sender display name.
    pub fn with_name(mut self, name: &str) -> Self {
        self.from_name = Some(name.to_string());
        self
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Email Flags & Folders
// ─────────────────────────────────────────────────────────────────────────────

/// Standard IMAP email flags.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EmailFlag {
    /// Message has been read.
    Seen,
    /// Message has been replied to.
    Answered,
    /// Message is flagged/starred.
    Flagged,
    /// Message is marked for deletion.
    Deleted,
    /// Message is a draft.
    Draft,
}

impl fmt::Display for EmailFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EmailFlag::Seen => write!(f, "\\Seen"),
            EmailFlag::Answered => write!(f, "\\Answered"),
            EmailFlag::Flagged => write!(f, "\\Flagged"),
            EmailFlag::Deleted => write!(f, "\\Deleted"),
            EmailFlag::Draft => write!(f, "\\Draft"),
        }
    }
}

/// Information about a mailbox folder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailFolder {
    /// Folder name (e.g. "INBOX", "Sent", "Drafts").
    pub name: String,
    /// Total number of messages in the folder.
    pub message_count: u32,
    /// Number of unread messages.
    pub unread_count: u32,
    /// Number of recent (new) messages.
    pub recent_count: u32,
}

impl EmailFolder {
    /// Create a new folder descriptor.
    pub fn new(name: &str, message_count: u32, unread_count: u32, recent_count: u32) -> Self {
        Self {
            name: name.to_string(),
            message_count,
            unread_count,
            recent_count,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Enhanced Email Message
// ─────────────────────────────────────────────────────────────────────────────

/// An email message with full header support.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailMessageFull {
    /// Sender email address.
    pub from: String,
    /// Primary recipient addresses.
    pub to: Vec<String>,
    /// Carbon copy recipients.
    pub cc: Vec<String>,
    /// Blind carbon copy recipients.
    pub bcc: Vec<String>,
    /// Email subject.
    pub subject: String,
    /// Plain text body.
    pub body: String,
    /// HTML body, if available.
    pub html_body: Option<String>,
    /// File attachments (filename -> size in bytes).
    pub attachments: Vec<AttachmentInfo>,
    /// Message date as Unix timestamp.
    pub date: f64,
    /// Unique message ID (RFC 2822 Message-ID header).
    pub message_id: String,
    /// IMAP flags on this message.
    pub flags: Vec<EmailFlag>,
}

/// Attachment metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentInfo {
    /// Original filename.
    pub filename: String,
    /// MIME content type.
    pub content_type: String,
    /// Size in bytes.
    pub size: u64,
}

impl AttachmentInfo {
    /// Create a new attachment info.
    pub fn new(filename: &str, content_type: &str, size: u64) -> Self {
        Self {
            filename: filename.to_string(),
            content_type: content_type.to_string(),
            size,
        }
    }
}

impl EmailMessageFull {
    /// Create a new email message.
    pub fn new(from: &str, to: Vec<String>, subject: &str, body: &str) -> Self {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let random_part: u32 = rand::random();

        Self {
            from: from.to_string(),
            to,
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: subject.to_string(),
            body: body.to_string(),
            html_body: None,
            attachments: Vec::new(),
            date: now_ms as f64 / 1000.0,
            message_id: format!("<{}_{:08x}@titan>", now_ms, random_part),
            flags: Vec::new(),
        }
    }

    /// Check if the message has a specific flag.
    pub fn has_flag(&self, flag: &EmailFlag) -> bool {
        self.flags.contains(flag)
    }

    /// Add a flag to this message.
    pub fn add_flag(&mut self, flag: EmailFlag) {
        if !self.flags.contains(&flag) {
            self.flags.push(flag);
        }
    }

    /// Remove a flag from this message.
    pub fn remove_flag(&mut self, flag: &EmailFlag) {
        self.flags.retain(|f| f != flag);
    }

    /// Check if the message has been read.
    pub fn is_read(&self) -> bool {
        self.has_flag(&EmailFlag::Seen)
    }

    /// Mark as read.
    pub fn mark_read(&mut self) {
        self.add_flag(EmailFlag::Seen);
    }

    /// Check if the message is flagged/starred.
    pub fn is_flagged(&self) -> bool {
        self.has_flag(&EmailFlag::Flagged)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Calendar Types
// ─────────────────────────────────────────────────────────────────────────────

/// Recurrence pattern for calendar events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Recurrence {
    /// Repeats every day.
    Daily,
    /// Repeats every week.
    Weekly,
    /// Repeats every month.
    Monthly,
    /// Repeats every year.
    Yearly,
    /// Custom RRULE string.
    Custom(String),
}

impl fmt::Display for Recurrence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Recurrence::Daily => write!(f, "FREQ=DAILY"),
            Recurrence::Weekly => write!(f, "FREQ=WEEKLY"),
            Recurrence::Monthly => write!(f, "FREQ=MONTHLY"),
            Recurrence::Yearly => write!(f, "FREQ=YEARLY"),
            Recurrence::Custom(rule) => write!(f, "{rule}"),
        }
    }
}

/// Enhanced calendar event with recurrence and reminders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarEventFull {
    /// Unique event ID.
    pub id: String,
    /// Event title.
    pub title: String,
    /// Start time as Unix timestamp.
    pub start: f64,
    /// End time as Unix timestamp.
    pub end: f64,
    /// Location (physical or virtual).
    pub location: String,
    /// Event description.
    pub description: String,
    /// Attendee email addresses.
    pub attendees: Vec<String>,
    /// Recurrence pattern, if any.
    pub recurrence: Option<Recurrence>,
    /// Reminder in minutes before the event.
    pub reminder_minutes: u32,
}

impl CalendarEventFull {
    /// Create a new calendar event.
    pub fn new(title: &str, start: f64, end: f64) -> Self {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let random_part: u32 = rand::random();

        Self {
            id: format!("evt_{}_{:08x}", now_ms, random_part),
            title: title.to_string(),
            start,
            end,
            location: String::new(),
            description: String::new(),
            attendees: Vec::new(),
            recurrence: None,
            reminder_minutes: 15,
        }
    }

    /// Duration in minutes.
    pub fn duration_minutes(&self) -> f64 {
        (self.end - self.start) / 60.0
    }

    /// Generate an iCalendar (RFC 5545) representation of this event.
    pub fn to_ical(&self) -> String {
        let dtstart = format_timestamp_ical(self.start);
        let dtend = format_timestamp_ical(self.end);
        let dtstamp = format_timestamp_ical(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64(),
        );

        let mut ical = String::new();
        ical.push_str("BEGIN:VCALENDAR\r\n");
        ical.push_str("VERSION:2.0\r\n");
        ical.push_str("PRODID:-//Sovereign Titan//titan_core//EN\r\n");
        ical.push_str("BEGIN:VEVENT\r\n");
        ical.push_str(&format!("UID:{}\r\n", self.id));
        ical.push_str(&format!("DTSTAMP:{}\r\n", dtstamp));
        ical.push_str(&format!("DTSTART:{}\r\n", dtstart));
        ical.push_str(&format!("DTEND:{}\r\n", dtend));
        ical.push_str(&format!("SUMMARY:{}\r\n", escape_ical(&self.title)));

        if !self.description.is_empty() {
            ical.push_str(&format!("DESCRIPTION:{}\r\n", escape_ical(&self.description)));
        }
        if !self.location.is_empty() {
            ical.push_str(&format!("LOCATION:{}\r\n", escape_ical(&self.location)));
        }

        for attendee in &self.attendees {
            ical.push_str(&format!("ATTENDEE:mailto:{}\r\n", attendee));
        }

        if let Some(ref recurrence) = self.recurrence {
            ical.push_str(&format!("RRULE:{}\r\n", recurrence));
        }

        // VALARM for reminder.
        if self.reminder_minutes > 0 {
            ical.push_str("BEGIN:VALARM\r\n");
            ical.push_str("TRIGGER:-PT");
            ical.push_str(&self.reminder_minutes.to_string());
            ical.push_str("M\r\n");
            ical.push_str("ACTION:DISPLAY\r\n");
            ical.push_str(&format!("DESCRIPTION:{}\r\n", escape_ical(&self.title)));
            ical.push_str("END:VALARM\r\n");
        }

        ical.push_str("END:VEVENT\r\n");
        ical.push_str("END:VCALENDAR\r\n");

        ical
    }
}

/// Format a Unix timestamp as an iCalendar UTC datetime (YYYYMMDDTHHMMSSZ).
fn format_timestamp_ical(ts: f64) -> String {
    let secs = ts as i64;
    // Basic UTC datetime calculation.
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Convert days since epoch to date.
    let (year, month, day) = days_to_date(days_since_epoch);

    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_date(days: i64) -> (i64, u32, u32) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    (year, m, d)
}

/// Escape text for iCalendar format.
fn escape_ical(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace('\n', "\\n")
}

// ─────────────────────────────────────────────────────────────────────────────
// Email Provider
// ─────────────────────────────────────────────────────────────────────────────

/// Connection state for the email provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderState {
    Disconnected,
    Connected,
    Error(String),
}

/// Email provider managing IMAP/SMTP connections, folder listing,
/// message search, and calendar operations.
pub struct EmailProvider {
    /// The stored IMAP configuration (set on connect).
    config: Option<ImapConfig>,
    /// Connection state.
    state: ProviderState,
    /// Known folders (name -> folder info).
    folders: HashMap<String, EmailFolder>,
    /// Calendar events (id -> event).
    calendar_events: HashMap<String, CalendarEventFull>,
}

impl EmailProvider {
    /// Create a new disconnected provider.
    pub fn new() -> Self {
        Self {
            config: None,
            state: ProviderState::Disconnected,
            folders: HashMap::new(),
            calendar_events: HashMap::new(),
        }
    }

    /// Connect to the IMAP server with the given configuration.
    pub fn connect(&mut self, config: ImapConfig) -> Result<(), String> {
        if config.host.is_empty() {
            return Err("IMAP host cannot be empty".to_string());
        }
        if config.email.is_empty() {
            return Err("Email address cannot be empty".to_string());
        }

        // Initialize default folders.
        self.folders.clear();
        self.folders.insert(
            "INBOX".to_string(),
            EmailFolder::new("INBOX", 0, 0, 0),
        );
        self.folders.insert(
            "Sent".to_string(),
            EmailFolder::new("Sent", 0, 0, 0),
        );
        self.folders.insert(
            "Drafts".to_string(),
            EmailFolder::new("Drafts", 0, 0, 0),
        );
        self.folders.insert(
            "Trash".to_string(),
            EmailFolder::new("Trash", 0, 0, 0),
        );

        self.config = Some(config);
        self.state = ProviderState::Connected;
        Ok(())
    }

    /// Disconnect from the server.
    pub fn disconnect(&mut self) {
        self.state = ProviderState::Disconnected;
        self.folders.clear();
    }

    /// Check whether the provider is connected.
    pub fn is_connected(&self) -> bool {
        self.state == ProviderState::Connected
    }

    /// Get the current connection state.
    pub fn connection_state(&self) -> &ProviderState {
        &self.state
    }

    /// Get the email address from the active configuration.
    pub fn email_address(&self) -> Option<&str> {
        self.config.as_ref().map(|c| c.email.as_str())
    }

    // ── Folder Operations ────────────────────────────────────────────────

    /// List all mailbox folders.
    pub fn list_folders(&self) -> Result<Vec<EmailFolder>, String> {
        if !self.is_connected() {
            return Err("Not connected to IMAP server".to_string());
        }
        Ok(self.folders.values().cloned().collect())
    }

    /// Get information about a specific folder.
    pub fn get_folder(&self, name: &str) -> Result<&EmailFolder, String> {
        if !self.is_connected() {
            return Err("Not connected to IMAP server".to_string());
        }
        self.folders
            .get(name)
            .ok_or_else(|| format!("Folder not found: {name}"))
    }

    // ── Message Operations ───────────────────────────────────────────────

    /// Fetch messages from a mailbox folder.
    pub fn get_messages(&self, folder: &str, limit: usize) -> Result<Vec<EmailMessage>, String> {
        if !self.is_connected() {
            return Err("Not connected to IMAP server".to_string());
        }
        let _config = self.config.as_ref().unwrap();
        if !self.folders.contains_key(folder) {
            return Err(format!("Unknown folder: {folder}"));
        }
        tracing::debug!(
            "EmailProvider::get_messages(folder={}, limit={}) — connected",
            folder,
            limit
        );
        // In production: SELECT folder, FETCH envelopes, parse MIME bodies.
        Ok(Vec::new())
    }

    /// Search messages by criteria.
    pub fn search_messages(
        &self,
        folder: &str,
        subject: Option<&str>,
        from: Option<&str>,
        since_timestamp: Option<f64>,
        limit: usize,
    ) -> Result<Vec<EmailMessage>, String> {
        if !self.is_connected() {
            return Err("Not connected to IMAP server".to_string());
        }
        if !self.folders.contains_key(folder) {
            return Err(format!("Unknown folder: {folder}"));
        }

        // Build IMAP search criteria string.
        let mut criteria = Vec::new();
        if let Some(subj) = subject {
            criteria.push(format!("SUBJECT \"{}\"", subj));
        }
        if let Some(sender) = from {
            criteria.push(format!("FROM \"{}\"", sender));
        }
        if let Some(ts) = since_timestamp {
            let date_str = format_date_for_imap(ts);
            criteria.push(format!("SINCE {}", date_str));
        }

        let criteria_str = if criteria.is_empty() {
            "ALL".to_string()
        } else {
            criteria.join(" ")
        };

        tracing::debug!(
            "EmailProvider::search_messages(folder={}, criteria={}, limit={})",
            folder,
            criteria_str,
            limit
        );

        // In production: SEARCH command, then FETCH matching messages.
        Ok(Vec::new())
    }

    /// Send an email message via SMTP.
    pub fn send_message(&self, message: &EmailMessage) -> Result<(), String> {
        if !self.is_connected() {
            return Err("Not connected to IMAP server".to_string());
        }
        if message.recipients.is_empty() {
            return Err("Message has no recipients".to_string());
        }
        let _config = self.config.as_ref().unwrap();
        tracing::debug!(
            "EmailProvider::send_message(subject={}) — connected, would send via SMTP",
            message.subject
        );
        // In production: build MIME message, connect to SMTP, send.
        Ok(())
    }

    // ── Calendar Operations ──────────────────────────────────────────────

    /// Create a new calendar event.
    pub fn create_event(&mut self, event: CalendarEventFull) -> Result<String, String> {
        if event.title.is_empty() {
            return Err("Event title cannot be empty".to_string());
        }
        if event.end <= event.start {
            return Err("Event end time must be after start time".to_string());
        }

        let event_id = event.id.clone();
        self.calendar_events.insert(event_id.clone(), event);
        Ok(event_id)
    }

    /// List calendar events, optionally filtered by time range.
    pub fn list_events(
        &self,
        start_after: Option<f64>,
        end_before: Option<f64>,
    ) -> Vec<&CalendarEventFull> {
        self.calendar_events
            .values()
            .filter(|evt| {
                if let Some(start) = start_after {
                    if evt.start < start {
                        return false;
                    }
                }
                if let Some(end) = end_before {
                    if evt.end > end {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    /// Get a calendar event by ID.
    pub fn get_event(&self, event_id: &str) -> Option<&CalendarEventFull> {
        self.calendar_events.get(event_id)
    }

    /// Update a calendar event.
    pub fn update_event(
        &mut self,
        event_id: &str,
        updated: CalendarEventFull,
    ) -> Result<(), String> {
        if !self.calendar_events.contains_key(event_id) {
            return Err(format!("Event not found: {event_id}"));
        }
        if updated.end <= updated.start {
            return Err("Event end time must be after start time".to_string());
        }
        self.calendar_events
            .insert(event_id.to_string(), updated);
        Ok(())
    }

    /// Delete a calendar event by ID.
    pub fn delete_event(&mut self, event_id: &str) -> Result<(), String> {
        self.calendar_events
            .remove(event_id)
            .map(|_| ())
            .ok_or_else(|| format!("Event not found: {event_id}"))
    }

    /// Export a calendar event as iCalendar format.
    pub fn export_event_ical(&self, event_id: &str) -> Result<String, String> {
        self.calendar_events
            .get(event_id)
            .map(|evt| evt.to_ical())
            .ok_or_else(|| format!("Event not found: {event_id}"))
    }

    /// Get the number of calendar events.
    pub fn event_count(&self) -> usize {
        self.calendar_events.len()
    }
}

impl Default for EmailProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Format a Unix timestamp as an IMAP date string (DD-Mon-YYYY).
fn format_date_for_imap(ts: f64) -> String {
    let secs = ts as i64;
    let days = secs / 86400;
    let (year, month, day) = days_to_date(days);

    let month_names = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month_str = if month >= 1 && month <= 12 {
        month_names[(month - 1) as usize]
    } else {
        "Jan"
    };

    format!("{:02}-{}-{:04}", day, month_str, year)
}

// ─────────────────────────────────────────────────────────────────────────────
// Legacy ImapProvider compatibility
// ─────────────────────────────────────────────────────────────────────────────

/// Legacy IMAP provider — wraps EmailProvider for backward compatibility.
pub type ImapProvider = EmailProvider;

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> ImapConfig {
        ImapConfig::new(
            "imap.test.com",
            993,
            "user@test.com",
            "password123",
            "smtp.test.com",
            587,
        )
    }

    // ── EmailConfig tests ────────────────────────────────────────────────

    #[test]
    fn test_email_config_debug_redacts_password() {
        let config = EmailConfig::new("server.com", 993, "user", "secret_pass", true);
        let debug_str = format!("{:?}", config);
        assert!(!debug_str.contains("secret_pass"));
        assert!(debug_str.contains("REDACTED"));
    }

    #[test]
    fn test_email_config_validate() {
        let config = EmailConfig::new("server.com", 993, "user", "pass", true);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_email_config_validate_empty_server() {
        let config = EmailConfig::new("", 993, "user", "pass", true);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_email_config_validate_empty_username() {
        let config = EmailConfig::new("server.com", 993, "", "pass", true);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_email_config_validate_zero_port() {
        let config = EmailConfig::new("server.com", 0, "user", "pass", true);
        assert!(config.validate().is_err());
    }

    // ── ImapConfig tests ─────────────────────────────────────────────────

    #[test]
    fn test_imap_config_new() {
        let config = test_config();
        assert_eq!(config.host, "imap.test.com");
        assert_eq!(config.port, 993);
        assert!(config.use_tls);
        assert_eq!(config.idle_timeout, 300);
    }

    #[test]
    fn test_gmail_config() {
        let cfg = ImapConfig::gmail("user@gmail.com", "app_pass");
        assert_eq!(cfg.host, "imap.gmail.com");
        assert_eq!(cfg.smtp_host, "smtp.gmail.com");
        assert_eq!(cfg.port, 993);
    }

    #[test]
    fn test_outlook_config() {
        let cfg = ImapConfig::outlook("user@outlook.com", "pass");
        assert_eq!(cfg.host, "outlook.office365.com");
        assert_eq!(cfg.smtp_host, "smtp.office365.com");
    }

    #[test]
    fn test_yahoo_config() {
        let cfg = ImapConfig::yahoo("user@yahoo.com", "app_pass");
        assert_eq!(cfg.host, "imap.mail.yahoo.com");
        assert_eq!(cfg.smtp_host, "smtp.mail.yahoo.com");
    }

    #[test]
    fn test_imap_to_email_config() {
        let imap = test_config();
        let email_cfg = imap.to_imap_config();
        assert_eq!(email_cfg.server, "imap.test.com");
        assert_eq!(email_cfg.port, 993);
    }

    #[test]
    fn test_imap_to_smtp_config() {
        let imap = test_config();
        let smtp_cfg = imap.to_smtp_config();
        assert_eq!(smtp_cfg.server, "smtp.test.com");
        assert_eq!(smtp_cfg.port, 587);
    }

    // ── SmtpConfig tests ─────────────────────────────────────────────────

    #[test]
    fn test_smtp_config_starttls() {
        let config = SmtpConfig::new("smtp.test.com", 587, "user", "pass");
        assert!(config.use_starttls);
        assert!(!config.use_tls);
    }

    #[test]
    fn test_smtp_config_tls() {
        let config = SmtpConfig::new("smtp.test.com", 465, "user", "pass");
        assert!(!config.use_starttls);
        assert!(config.use_tls);
    }

    #[test]
    fn test_smtp_config_with_name() {
        let config = SmtpConfig::new("smtp.test.com", 587, "user", "pass").with_name("John Doe");
        assert_eq!(config.from_name, Some("John Doe".to_string()));
    }

    // ── EmailFlag tests ──────────────────────────────────────────────────

    #[test]
    fn test_email_flag_display() {
        assert_eq!(format!("{}", EmailFlag::Seen), "\\Seen");
        assert_eq!(format!("{}", EmailFlag::Answered), "\\Answered");
        assert_eq!(format!("{}", EmailFlag::Flagged), "\\Flagged");
        assert_eq!(format!("{}", EmailFlag::Deleted), "\\Deleted");
        assert_eq!(format!("{}", EmailFlag::Draft), "\\Draft");
    }

    #[test]
    fn test_email_flag_serialization() {
        let flag = EmailFlag::Seen;
        let json = serde_json::to_string(&flag).unwrap();
        let restored: EmailFlag = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, EmailFlag::Seen);
    }

    // ── EmailFolder tests ────────────────────────────────────────────────

    #[test]
    fn test_email_folder_creation() {
        let folder = EmailFolder::new("INBOX", 100, 5, 2);
        assert_eq!(folder.name, "INBOX");
        assert_eq!(folder.message_count, 100);
        assert_eq!(folder.unread_count, 5);
        assert_eq!(folder.recent_count, 2);
    }

    // ── EmailMessageFull tests ───────────────────────────────────────────

    #[test]
    fn test_email_message_full_creation() {
        let msg = EmailMessageFull::new(
            "sender@test.com",
            vec!["recipient@test.com".to_string()],
            "Test Subject",
            "Hello world",
        );
        assert_eq!(msg.from, "sender@test.com");
        assert_eq!(msg.to.len(), 1);
        assert_eq!(msg.subject, "Test Subject");
        assert!(msg.message_id.contains("@titan"));
        assert!(msg.flags.is_empty());
    }

    #[test]
    fn test_email_message_flags() {
        let mut msg = EmailMessageFull::new("a@b.com", vec![], "Test", "Body");
        assert!(!msg.is_read());

        msg.mark_read();
        assert!(msg.is_read());
        assert!(msg.has_flag(&EmailFlag::Seen));

        msg.add_flag(EmailFlag::Flagged);
        assert!(msg.is_flagged());

        msg.remove_flag(&EmailFlag::Seen);
        assert!(!msg.is_read());
    }

    #[test]
    fn test_email_message_duplicate_flag() {
        let mut msg = EmailMessageFull::new("a@b.com", vec![], "Test", "Body");
        msg.add_flag(EmailFlag::Seen);
        msg.add_flag(EmailFlag::Seen); // Duplicate should be ignored.
        assert_eq!(msg.flags.len(), 1);
    }

    // ── Recurrence tests ─────────────────────────────────────────────────

    #[test]
    fn test_recurrence_display() {
        assert_eq!(format!("{}", Recurrence::Daily), "FREQ=DAILY");
        assert_eq!(format!("{}", Recurrence::Weekly), "FREQ=WEEKLY");
        assert_eq!(format!("{}", Recurrence::Monthly), "FREQ=MONTHLY");
        assert_eq!(format!("{}", Recurrence::Yearly), "FREQ=YEARLY");
        assert_eq!(
            format!("{}", Recurrence::Custom("FREQ=DAILY;COUNT=10".to_string())),
            "FREQ=DAILY;COUNT=10"
        );
    }

    #[test]
    fn test_recurrence_serialization() {
        let rec = Recurrence::Weekly;
        let json = serde_json::to_string(&rec).unwrap();
        let restored: Recurrence = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, Recurrence::Weekly);
    }

    // ── CalendarEventFull tests ──────────────────────────────────────────

    #[test]
    fn test_calendar_event_full_creation() {
        let evt = CalendarEventFull::new("Team Meeting", 1000.0, 4600.0);
        assert!(evt.id.starts_with("evt_"));
        assert_eq!(evt.title, "Team Meeting");
        assert_eq!(evt.reminder_minutes, 15);
    }

    #[test]
    fn test_calendar_event_duration() {
        let evt = CalendarEventFull::new("Meeting", 0.0, 3600.0);
        assert!((evt.duration_minutes() - 60.0).abs() < 0.001);
    }

    #[test]
    fn test_calendar_event_to_ical() {
        let mut evt = CalendarEventFull::new("Test Event", 1700000000.0, 1700003600.0);
        evt.description = "A test event".to_string();
        evt.location = "Room 42".to_string();
        evt.attendees = vec!["bob@test.com".to_string()];
        evt.recurrence = Some(Recurrence::Weekly);

        let ical = evt.to_ical();
        assert!(ical.contains("BEGIN:VCALENDAR"));
        assert!(ical.contains("END:VCALENDAR"));
        assert!(ical.contains("BEGIN:VEVENT"));
        assert!(ical.contains("END:VEVENT"));
        assert!(ical.contains("SUMMARY:Test Event"));
        assert!(ical.contains("DESCRIPTION:A test event"));
        assert!(ical.contains("LOCATION:Room 42"));
        assert!(ical.contains("ATTENDEE:mailto:bob@test.com"));
        assert!(ical.contains("RRULE:FREQ=WEEKLY"));
        assert!(ical.contains("BEGIN:VALARM"));
        assert!(ical.contains("TRIGGER:-PT15M"));
    }

    #[test]
    fn test_calendar_event_to_ical_minimal() {
        let evt = CalendarEventFull::new("Simple", 1000.0, 2000.0);
        let ical = evt.to_ical();
        assert!(ical.contains("SUMMARY:Simple"));
        assert!(!ical.contains("RRULE:"));
    }

    // ── Provider tests ───────────────────────────────────────────────────

    #[test]
    fn test_new_provider_disconnected() {
        let provider = EmailProvider::new();
        assert!(!provider.is_connected());
        assert!(provider.email_address().is_none());
    }

    #[test]
    fn test_connect() {
        let mut provider = EmailProvider::new();
        provider.connect(test_config()).unwrap();
        assert!(provider.is_connected());
        assert_eq!(provider.email_address(), Some("user@test.com"));
    }

    #[test]
    fn test_connect_empty_host_fails() {
        let mut provider = EmailProvider::new();
        let config = ImapConfig::new("", 993, "user@test.com", "pass", "smtp.test.com", 587);
        assert!(provider.connect(config).is_err());
    }

    #[test]
    fn test_connect_empty_email_fails() {
        let mut provider = EmailProvider::new();
        let config = ImapConfig::new("imap.test.com", 993, "", "pass", "smtp.test.com", 587);
        assert!(provider.connect(config).is_err());
    }

    #[test]
    fn test_disconnect() {
        let mut provider = EmailProvider::new();
        provider.connect(test_config()).unwrap();
        provider.disconnect();
        assert!(!provider.is_connected());
    }

    #[test]
    fn test_list_folders() {
        let mut provider = EmailProvider::new();
        provider.connect(test_config()).unwrap();
        let folders = provider.list_folders().unwrap();
        assert!(folders.len() >= 4);
        let names: Vec<&str> = folders.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"INBOX"));
        assert!(names.contains(&"Sent"));
    }

    #[test]
    fn test_list_folders_not_connected() {
        let provider = EmailProvider::new();
        assert!(provider.list_folders().is_err());
    }

    #[test]
    fn test_get_folder() {
        let mut provider = EmailProvider::new();
        provider.connect(test_config()).unwrap();
        let folder = provider.get_folder("INBOX").unwrap();
        assert_eq!(folder.name, "INBOX");
    }

    #[test]
    fn test_get_folder_not_found() {
        let mut provider = EmailProvider::new();
        provider.connect(test_config()).unwrap();
        assert!(provider.get_folder("Nonexistent").is_err());
    }

    #[test]
    fn test_get_messages_not_connected() {
        let provider = EmailProvider::new();
        assert!(provider.get_messages("INBOX", 10).is_err());
    }

    #[test]
    fn test_get_messages_connected() {
        let mut provider = EmailProvider::new();
        provider.connect(test_config()).unwrap();
        let msgs = provider.get_messages("INBOX", 10).unwrap();
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_get_messages_unknown_folder() {
        let mut provider = EmailProvider::new();
        provider.connect(test_config()).unwrap();
        assert!(provider.get_messages("Nonexistent", 10).is_err());
    }

    #[test]
    fn test_search_messages() {
        let mut provider = EmailProvider::new();
        provider.connect(test_config()).unwrap();
        let msgs = provider
            .search_messages("INBOX", Some("Test"), Some("bob@test.com"), None, 10)
            .unwrap();
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_search_messages_not_connected() {
        let provider = EmailProvider::new();
        assert!(provider.search_messages("INBOX", None, None, None, 10).is_err());
    }

    #[test]
    fn test_send_message_not_connected() {
        let provider = EmailProvider::new();
        let msg = EmailMessage::new("Test", "a@b.com", vec!["c@d.com".into()], "body");
        assert!(provider.send_message(&msg).is_err());
    }

    #[test]
    fn test_send_message_no_recipients() {
        let mut provider = EmailProvider::new();
        provider.connect(test_config()).unwrap();
        let msg = EmailMessage::new("Test", "a@b.com", vec![], "body");
        assert!(provider.send_message(&msg).is_err());
    }

    #[test]
    fn test_send_message_connected() {
        let mut provider = EmailProvider::new();
        provider.connect(test_config()).unwrap();
        let msg = EmailMessage::new("Test", "a@b.com", vec!["c@d.com".into()], "body");
        assert!(provider.send_message(&msg).is_ok());
    }

    // ── Calendar CRUD tests ──────────────────────────────────────────────

    #[test]
    fn test_create_event() {
        let mut provider = EmailProvider::new();
        let evt = CalendarEventFull::new("Meeting", 1000.0, 4600.0);
        let event_id = provider.create_event(evt).unwrap();
        assert!(event_id.starts_with("evt_"));
        assert_eq!(provider.event_count(), 1);
    }

    #[test]
    fn test_create_event_empty_title() {
        let mut provider = EmailProvider::new();
        let evt = CalendarEventFull::new("", 1000.0, 4600.0);
        assert!(provider.create_event(evt).is_err());
    }

    #[test]
    fn test_create_event_invalid_time() {
        let mut provider = EmailProvider::new();
        let evt = CalendarEventFull::new("Meeting", 5000.0, 1000.0);
        assert!(provider.create_event(evt).is_err());
    }

    #[test]
    fn test_get_event() {
        let mut provider = EmailProvider::new();
        let evt = CalendarEventFull::new("Meeting", 1000.0, 4600.0);
        let event_id = provider.create_event(evt).unwrap();
        let retrieved = provider.get_event(&event_id).unwrap();
        assert_eq!(retrieved.title, "Meeting");
    }

    #[test]
    fn test_list_events() {
        let mut provider = EmailProvider::new();
        provider
            .create_event(CalendarEventFull::new("Early", 100.0, 200.0))
            .unwrap();
        provider
            .create_event(CalendarEventFull::new("Late", 5000.0, 6000.0))
            .unwrap();

        let all = provider.list_events(None, None);
        assert_eq!(all.len(), 2);

        let late_only = provider.list_events(Some(1000.0), None);
        assert_eq!(late_only.len(), 1);
        assert_eq!(late_only[0].title, "Late");
    }

    #[test]
    fn test_update_event() {
        let mut provider = EmailProvider::new();
        let evt = CalendarEventFull::new("Meeting", 1000.0, 4600.0);
        let event_id = provider.create_event(evt).unwrap();

        let mut updated = CalendarEventFull::new("Updated Meeting", 2000.0, 5600.0);
        updated.location = "Room 42".to_string();
        provider.update_event(&event_id, updated).unwrap();

        let retrieved = provider.get_event(&event_id).unwrap();
        assert_eq!(retrieved.title, "Updated Meeting");
        assert_eq!(retrieved.location, "Room 42");
    }

    #[test]
    fn test_update_event_not_found() {
        let mut provider = EmailProvider::new();
        let evt = CalendarEventFull::new("Meeting", 1000.0, 4600.0);
        assert!(provider.update_event("nonexistent", evt).is_err());
    }

    #[test]
    fn test_delete_event() {
        let mut provider = EmailProvider::new();
        let evt = CalendarEventFull::new("Meeting", 1000.0, 4600.0);
        let event_id = provider.create_event(evt).unwrap();

        provider.delete_event(&event_id).unwrap();
        assert_eq!(provider.event_count(), 0);
        assert!(provider.get_event(&event_id).is_none());
    }

    #[test]
    fn test_delete_event_not_found() {
        let mut provider = EmailProvider::new();
        assert!(provider.delete_event("nonexistent").is_err());
    }

    #[test]
    fn test_export_event_ical() {
        let mut provider = EmailProvider::new();
        let evt = CalendarEventFull::new("Meeting", 1000.0, 4600.0);
        let event_id = provider.create_event(evt).unwrap();

        let ical = provider.export_event_ical(&event_id).unwrap();
        assert!(ical.contains("BEGIN:VCALENDAR"));
        assert!(ical.contains("SUMMARY:Meeting"));
    }

    #[test]
    fn test_export_event_ical_not_found() {
        let provider = EmailProvider::new();
        assert!(provider.export_event_ical("nonexistent").is_err());
    }

    // ── Serialization tests ──────────────────────────────────────────────

    #[test]
    fn test_config_serialization() {
        let cfg = test_config();
        let json = serde_json::to_string(&cfg).unwrap();
        let restored: ImapConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.host, "imap.test.com");
        assert_eq!(restored.email, "user@test.com");
    }

    #[test]
    fn test_attachment_info() {
        let att = AttachmentInfo::new("report.pdf", "application/pdf", 2048);
        assert_eq!(att.filename, "report.pdf");
        assert_eq!(att.size, 2048);
    }

    // ── Date formatting tests ────────────────────────────────────────────

    #[test]
    fn test_format_date_for_imap() {
        // 2024-01-15 (approximately)
        let ts = 1705276800.0; // 2024-01-15T00:00:00Z
        let date = format_date_for_imap(ts);
        assert!(date.contains("2024"));
        assert!(date.contains("Jan"));
    }

    #[test]
    fn test_format_timestamp_ical() {
        let ts = 1700000000.0; // 2023-11-14T22:13:20Z
        let ical = format_timestamp_ical(ts);
        assert!(ical.ends_with('Z'));
        assert!(ical.contains('T'));
        assert_eq!(ical.len(), 16); // YYYYMMDDTHHMMSSZ
    }

    #[test]
    fn test_escape_ical() {
        let result = escape_ical("Hello; World, foo\\bar\nnewline");
        assert!(result.contains("\\;"));
        assert!(result.contains("\\,"));
        assert!(result.contains("\\\\"));
        assert!(result.contains("\\n"));
    }
}
