//! Email and Calendar Assistant — scheduling, free-time detection, and draft
//! composition.
//!
//! Ported from `sovereign_titan/email_calendar/assistant.py`. Manages an
//! in-memory calendar and email drafts, with optional IMAP provider integration.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::providers::{ImapConfig, ImapProvider};
use super::types::{CalendarEvent, EmailMessage};

/// A block of free time between scheduled events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreeSlot {
    /// Start time as Unix timestamp.
    pub start: f64,
    /// End time as Unix timestamp.
    pub end: f64,
    /// Duration in minutes.
    pub duration_minutes: u32,
}

/// Summary statistics for the assistant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantStats {
    /// Total number of calendar events.
    pub total_events: usize,
    /// Total number of email drafts.
    pub total_drafts: usize,
    /// Whether an email provider is connected.
    pub email_connected: bool,
}

/// Email and calendar assistant combining scheduling with email management.
pub struct EmailCalendarAssistant {
    /// Calendar events keyed by event ID.
    events: HashMap<String, CalendarEvent>,
    /// Email drafts keyed by message ID.
    drafts: HashMap<String, EmailMessage>,
    /// Optional IMAP email provider.
    provider: Option<ImapProvider>,
}

impl EmailCalendarAssistant {
    /// Create a new assistant with no events, drafts, or provider.
    pub fn new() -> Self {
        Self {
            events: HashMap::new(),
            drafts: HashMap::new(),
            provider: None,
        }
    }

    /// Connect an IMAP email provider.
    pub fn connect_email(&mut self, config: ImapConfig) -> Result<(), String> {
        let mut provider = ImapProvider::new();
        provider.connect(config)?;
        self.provider = Some(provider);
        Ok(())
    }

    /// Check whether an email provider is connected.
    pub fn is_email_connected(&self) -> bool {
        self.provider
            .as_ref()
            .map_or(false, |p| p.is_connected())
    }

    /// Schedule a new calendar event and return it.
    ///
    /// `start_time` is a Unix timestamp. `duration_minutes` determines the
    /// end time.
    pub fn schedule_event(
        &mut self,
        title: &str,
        start_time: f64,
        duration_minutes: u32,
        description: &str,
        attendees: Vec<String>,
    ) -> CalendarEvent {
        let end_time = start_time + (duration_minutes as f64 * 60.0);
        let mut event = CalendarEvent::new(title, start_time, end_time, description);
        event.attendees = attendees;

        self.events.insert(event.id.clone(), event.clone());
        event
    }

    /// Get a reference to a calendar event by ID.
    pub fn get_event(&self, id: &str) -> Option<&CalendarEvent> {
        self.events.get(id)
    }

    /// Remove a calendar event by ID. Returns `true` if removed.
    pub fn cancel_event(&mut self, id: &str) -> bool {
        self.events.remove(id).is_some()
    }

    /// Get all events in a time range, sorted by start time.
    ///
    /// Returns events where the event's time range overlaps with the query
    /// range `[date_start, date_end]`.
    pub fn get_schedule(&self, date_start: f64, date_end: f64) -> Vec<&CalendarEvent> {
        let mut events: Vec<&CalendarEvent> = self
            .events
            .values()
            .filter(|e| e.start_time < date_end && e.end_time > date_start)
            .collect();
        events.sort_by(|a, b| a.start_time.partial_cmp(&b.start_time).unwrap());
        events
    }

    /// Find free time slots within a range that are at least `duration_minutes` long.
    ///
    /// Examines gaps between sorted events in the range and returns available
    /// slots.
    pub fn find_free_time(
        &self,
        date_start: f64,
        date_end: f64,
        duration_minutes: u32,
    ) -> Vec<FreeSlot> {
        let min_duration_secs = duration_minutes as f64 * 60.0;

        // Collect and sort events that overlap or are within the range.
        let mut events: Vec<&CalendarEvent> = self
            .events
            .values()
            .filter(|e| e.start_time < date_end && e.end_time > date_start)
            .collect();
        events.sort_by(|a, b| a.start_time.partial_cmp(&b.start_time).unwrap());

        let mut free_slots = Vec::new();
        let mut cursor = date_start;

        for event in &events {
            let gap_start = cursor;
            let gap_end = event.start_time.min(date_end);

            if gap_end > gap_start {
                let gap_secs = gap_end - gap_start;
                if gap_secs >= min_duration_secs {
                    free_slots.push(FreeSlot {
                        start: gap_start,
                        end: gap_end,
                        duration_minutes: (gap_secs / 60.0) as u32,
                    });
                }
            }

            // Advance cursor past this event.
            if event.end_time > cursor {
                cursor = event.end_time;
            }
        }

        // Check gap after the last event.
        if cursor < date_end {
            let gap_secs = date_end - cursor;
            if gap_secs >= min_duration_secs {
                free_slots.push(FreeSlot {
                    start: cursor,
                    end: date_end,
                    duration_minutes: (gap_secs / 60.0) as u32,
                });
            }
        }

        free_slots
    }

    /// Compose an email draft and store it. Returns the draft message.
    pub fn compose_draft(
        &mut self,
        to: Vec<String>,
        subject: &str,
        body: &str,
    ) -> EmailMessage {
        let sender = self
            .provider
            .as_ref()
            .and_then(|p| p.email_address())
            .unwrap_or("user@localhost")
            .to_string();

        let msg = EmailMessage::new(subject, &sender, to, body);
        self.drafts.insert(msg.id.clone(), msg.clone());
        msg
    }

    /// Get a reference to a stored draft by ID.
    pub fn get_draft(&self, id: &str) -> Option<&EmailMessage> {
        self.drafts.get(id)
    }

    /// Delete a draft by ID. Returns `true` if removed.
    pub fn discard_draft(&mut self, id: &str) -> bool {
        self.drafts.remove(id).is_some()
    }

    /// List all current draft messages.
    pub fn list_drafts(&self) -> Vec<&EmailMessage> {
        self.drafts.values().collect()
    }

    /// Send a draft (removes it from drafts and forwards to the provider).
    pub fn send_draft(&mut self, id: &str) -> Result<(), String> {
        let draft = self
            .drafts
            .remove(id)
            .ok_or_else(|| format!("Draft '{}' not found", id))?;

        if let Some(ref provider) = self.provider {
            provider.send_message(&draft)?;
        } else {
            return Err("No email provider connected".to_string());
        }

        Ok(())
    }

    /// Get summary statistics.
    pub fn get_stats(&self) -> AssistantStats {
        AssistantStats {
            total_events: self.events.len(),
            total_drafts: self.drafts.len(),
            email_connected: self.is_email_connected(),
        }
    }

    /// Total number of scheduled events.
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Total number of stored drafts.
    pub fn draft_count(&self) -> usize {
        self.drafts.len()
    }
}

impl Default for EmailCalendarAssistant {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_assistant() {
        let asst = EmailCalendarAssistant::new();
        assert_eq!(asst.event_count(), 0);
        assert_eq!(asst.draft_count(), 0);
        assert!(!asst.is_email_connected());
    }

    #[test]
    fn test_schedule_event() {
        let mut asst = EmailCalendarAssistant::new();
        let event = asst.schedule_event("Standup", 1000.0, 30, "Daily standup", vec![]);
        assert_eq!(event.title, "Standup");
        assert_eq!(event.end_time, 1000.0 + 30.0 * 60.0);
        assert_eq!(asst.event_count(), 1);
    }

    #[test]
    fn test_schedule_event_with_attendees() {
        let mut asst = EmailCalendarAssistant::new();
        let event = asst.schedule_event(
            "Review",
            2000.0,
            60,
            "Code review",
            vec!["alice@test.com".into(), "bob@test.com".into()],
        );
        assert_eq!(event.attendees.len(), 2);
    }

    #[test]
    fn test_get_schedule() {
        let mut asst = EmailCalendarAssistant::new();
        asst.schedule_event("A", 1000.0, 30, "", vec![]);
        asst.schedule_event("B", 5000.0, 30, "", vec![]);
        asst.schedule_event("C", 10000.0, 30, "", vec![]);

        let schedule = asst.get_schedule(0.0, 6000.0);
        assert_eq!(schedule.len(), 2);
        assert_eq!(schedule[0].title, "A");
        assert_eq!(schedule[1].title, "B");
    }

    #[test]
    fn test_get_schedule_sorted() {
        let mut asst = EmailCalendarAssistant::new();
        asst.schedule_event("Late", 5000.0, 30, "", vec![]);
        asst.schedule_event("Early", 1000.0, 30, "", vec![]);

        let schedule = asst.get_schedule(0.0, 10000.0);
        assert_eq!(schedule[0].title, "Early");
        assert_eq!(schedule[1].title, "Late");
    }

    #[test]
    fn test_cancel_event() {
        let mut asst = EmailCalendarAssistant::new();
        let event = asst.schedule_event("Cancel me", 1000.0, 30, "", vec![]);
        assert!(asst.cancel_event(&event.id));
        assert_eq!(asst.event_count(), 0);
        assert!(!asst.cancel_event("nonexistent_id"));
    }

    #[test]
    fn test_find_free_time_empty_calendar() {
        let asst = EmailCalendarAssistant::new();
        let slots = asst.find_free_time(0.0, 7200.0, 30);
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].duration_minutes, 120);
    }

    #[test]
    fn test_find_free_time_between_events() {
        let mut asst = EmailCalendarAssistant::new();
        // Event from 1000 to 2800 (30 min)
        asst.schedule_event("A", 1000.0, 30, "", vec![]);
        // Event from 5000 to 6800 (30 min)
        asst.schedule_event("B", 5000.0, 30, "", vec![]);

        let slots = asst.find_free_time(0.0, 10000.0, 15);
        // Should find: [0, 1000], [2800, 5000], [6800, 10000]
        assert_eq!(slots.len(), 3);
        assert!((slots[0].start - 0.0).abs() < 0.001);
        assert!((slots[0].end - 1000.0).abs() < 0.001);
    }

    #[test]
    fn test_find_free_time_respects_min_duration() {
        let mut asst = EmailCalendarAssistant::new();
        // Two events with only a 5-minute gap between them.
        asst.schedule_event("A", 0.0, 30, "", vec![]);
        asst.schedule_event("B", 2100.0, 30, "", vec![]); // starts 5 min after A ends

        let slots = asst.find_free_time(0.0, 5000.0, 10);
        // The 5-minute gap (1800..2100) is too short for a 10-minute slot.
        // Only the gap after B qualifies.
        for slot in &slots {
            assert!(slot.duration_minutes >= 10);
        }
    }

    #[test]
    fn test_compose_draft() {
        let mut asst = EmailCalendarAssistant::new();
        let draft = asst.compose_draft(
            vec!["bob@example.com".into()],
            "Hello",
            "Hi Bob!",
        );
        assert_eq!(draft.subject, "Hello");
        assert_eq!(draft.sender, "user@localhost"); // No provider connected.
        assert_eq!(asst.draft_count(), 1);
    }

    #[test]
    fn test_get_and_discard_draft() {
        let mut asst = EmailCalendarAssistant::new();
        let draft = asst.compose_draft(vec![], "Test", "Body");
        assert!(asst.get_draft(&draft.id).is_some());
        assert!(asst.discard_draft(&draft.id));
        assert!(asst.get_draft(&draft.id).is_none());
        assert_eq!(asst.draft_count(), 0);
    }

    #[test]
    fn test_list_drafts() {
        let mut asst = EmailCalendarAssistant::new();
        asst.compose_draft(vec![], "Draft 1", "");
        asst.compose_draft(vec![], "Draft 2", "");
        assert_eq!(asst.list_drafts().len(), 2);
    }

    #[test]
    fn test_send_draft_no_provider() {
        let mut asst = EmailCalendarAssistant::new();
        let draft = asst.compose_draft(vec!["a@b.com".into()], "Test", "Body");
        let result = asst.send_draft(&draft.id);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No email provider"));
    }

    #[test]
    fn test_send_draft_not_found() {
        let mut asst = EmailCalendarAssistant::new();
        let result = asst.send_draft("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_get_stats() {
        let mut asst = EmailCalendarAssistant::new();
        asst.schedule_event("E", 0.0, 30, "", vec![]);
        asst.compose_draft(vec![], "D", "");
        let stats = asst.get_stats();
        assert_eq!(stats.total_events, 1);
        assert_eq!(stats.total_drafts, 1);
        assert!(!stats.email_connected);
    }

    #[test]
    fn test_connect_email() {
        let mut asst = EmailCalendarAssistant::new();
        let config = ImapConfig::gmail("user@gmail.com", "app_pass");
        asst.connect_email(config).unwrap();
        assert!(asst.is_email_connected());
    }

    #[test]
    fn test_free_slot_serialization() {
        let slot = FreeSlot {
            start: 100.0,
            end: 3700.0,
            duration_minutes: 60,
        };
        let json = serde_json::to_string(&slot).unwrap();
        let restored: FreeSlot = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.duration_minutes, 60);
        assert!((restored.start - 100.0).abs() < 0.001);
    }
}
