//! Tone Detector — classifies user emotional tone via regex patterns.
//!
//! Ported from Python `sovereign_titan/agents/react.py` tone detection.
//! Detects emotional signals in the user's query and provides
//! tone-appropriate directives for the system prompt.

use regex::Regex;

/// Detected emotional tone of a user query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Frustrated,
    Excited,
    Confused,
    Urgent,
    Casual,
}

impl std::fmt::Display for Tone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tone::Neutral => write!(f, "neutral"),
            Tone::Frustrated => write!(f, "frustrated"),
            Tone::Excited => write!(f, "excited"),
            Tone::Confused => write!(f, "confused"),
            Tone::Urgent => write!(f, "urgent"),
            Tone::Casual => write!(f, "casual"),
        }
    }
}

/// Detects user tone from query text.
pub struct ToneDetector {
    frustrated: Regex,
    excited: Regex,
    confused: Regex,
    urgent: Regex,
    casual: Regex,
}

impl ToneDetector {
    /// Create a new tone detector with compiled regex patterns.
    pub fn new() -> Self {
        Self {
            frustrated: Regex::new(
                r"(?i)(doesn'?t work|broken|stupid|wtf|damn|ugh|hate|frustrat|annoying|useless|crap|sucks|terrible|horrible|still not|why won'?t|can'?t believe|fed up|sick of|tired of)"
            ).unwrap(),
            excited: Regex::new(
                r"(?i)(amazing|awesome|great|love|perfect|excellent|fantastic|wonderful|incredible|brilliant|omg|wow|yay|!!!|haha|lol|sweet|nice!|cool!)"
            ).unwrap(),
            confused: Regex::new(
                r"(?i)(confused|don'?t understand|what does|how does|what is|what'?s that|i'?m lost|no idea|help me understand|makes no sense|what do you mean|\?\?+|huh\??)"
            ).unwrap(),
            urgent: Regex::new(
                r"(?i)(urgent|asap|immediately|right now|hurry|critical|emergency|quick|fast|deadline|rush|time.sensitive|need.this.now)"
            ).unwrap(),
            casual: Regex::new(
                r"(?i)(hey|hi|hello|yo|sup|what'?s up|howdy|thanks|thx|ty|plz|pls|btw|lmk|nvm|idk|tbh|imo)"
            ).unwrap(),
        }
    }

    /// Detect the primary tone of a user query.
    pub fn detect(&self, query: &str) -> Tone {
        // Priority order: frustrated > urgent > confused > excited > casual > neutral
        if self.frustrated.is_match(query) {
            Tone::Frustrated
        } else if self.urgent.is_match(query) {
            Tone::Urgent
        } else if self.confused.is_match(query) {
            Tone::Confused
        } else if self.excited.is_match(query) {
            Tone::Excited
        } else if self.casual.is_match(query) {
            Tone::Casual
        } else {
            Tone::Neutral
        }
    }

    /// Get a system prompt directive appropriate for the detected tone.
    pub fn tone_directive(&self, query: &str) -> &'static str {
        match self.detect(query) {
            Tone::Neutral => "",
            Tone::Frustrated => {
                "\nTONE DIRECTIVE: The user seems frustrated. Be extra helpful, \
                 acknowledge the issue, and provide a clear solution. Avoid jargon."
            }
            Tone::Excited => {
                "\nTONE DIRECTIVE: The user is enthusiastic! Match their energy \
                 with a positive response while staying helpful and accurate."
            }
            Tone::Confused => {
                "\nTONE DIRECTIVE: The user seems confused. Break down your \
                 explanation step-by-step. Use simple language and examples."
            }
            Tone::Urgent => {
                "\nTONE DIRECTIVE: This seems urgent. Be concise and action-oriented. \
                 Skip unnecessary explanation — focus on the solution."
            }
            Tone::Casual => {
                "\nTONE DIRECTIVE: The user is being casual. You can be friendly \
                 and relaxed while still being helpful."
            }
        }
    }
}

impl Default for ToneDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detector() -> ToneDetector {
        ToneDetector::new()
    }

    #[test]
    fn test_neutral_tone() {
        assert_eq!(detector().detect("open notepad"), Tone::Neutral);
        assert_eq!(detector().detect("search for rust programming"), Tone::Neutral);
    }

    #[test]
    fn test_frustrated_tone() {
        assert_eq!(detector().detect("this doesn't work!!!"), Tone::Frustrated);
        assert_eq!(detector().detect("ugh this is so annoying"), Tone::Frustrated);
        assert_eq!(detector().detect("why won't it open"), Tone::Frustrated);
        assert_eq!(detector().detect("the app is broken"), Tone::Frustrated);
        assert_eq!(detector().detect("still not working"), Tone::Frustrated);
    }

    #[test]
    fn test_excited_tone() {
        assert_eq!(detector().detect("that's awesome!"), Tone::Excited);
        assert_eq!(detector().detect("wow this is amazing"), Tone::Excited);
        assert_eq!(detector().detect("omg it works perfectly"), Tone::Excited);
    }

    #[test]
    fn test_confused_tone() {
        assert_eq!(detector().detect("I'm confused about this"), Tone::Confused);
        assert_eq!(detector().detect("what does that mean???"), Tone::Confused);
        assert_eq!(detector().detect("I don't understand how this works"), Tone::Confused);
        assert_eq!(detector().detect("huh?"), Tone::Confused);
    }

    #[test]
    fn test_urgent_tone() {
        assert_eq!(detector().detect("I need this ASAP"), Tone::Urgent);
        assert_eq!(detector().detect("this is urgent please hurry"), Tone::Urgent);
        assert_eq!(detector().detect("do this immediately"), Tone::Urgent);
        assert_eq!(detector().detect("it's a critical issue right now"), Tone::Urgent);
    }

    #[test]
    fn test_casual_tone() {
        assert_eq!(detector().detect("hey what's up"), Tone::Casual);
        assert_eq!(detector().detect("yo can you help"), Tone::Casual);
        assert_eq!(detector().detect("thanks btw"), Tone::Casual);
        assert_eq!(detector().detect("hi there"), Tone::Casual);
    }

    #[test]
    fn test_frustrated_overrides_casual() {
        // "hey" is casual but "doesn't work" is frustrated — frustrated wins
        assert_eq!(detector().detect("hey this doesn't work"), Tone::Frustrated);
    }

    #[test]
    fn test_urgent_overrides_excited() {
        // "amazing" is excited but "immediately" is urgent — urgent wins
        assert_eq!(detector().detect("this is amazing but I need it immediately"), Tone::Urgent);
    }

    #[test]
    fn test_tone_display() {
        assert_eq!(format!("{}", Tone::Neutral), "neutral");
        assert_eq!(format!("{}", Tone::Frustrated), "frustrated");
        assert_eq!(format!("{}", Tone::Excited), "excited");
        assert_eq!(format!("{}", Tone::Confused), "confused");
        assert_eq!(format!("{}", Tone::Urgent), "urgent");
        assert_eq!(format!("{}", Tone::Casual), "casual");
    }

    #[test]
    fn test_directive_neutral_empty() {
        let d = detector();
        assert!(d.tone_directive("open notepad").is_empty());
    }

    #[test]
    fn test_directive_frustrated_not_empty() {
        let d = detector();
        let directive = d.tone_directive("this is broken and doesn't work");
        assert!(!directive.is_empty());
        assert!(directive.contains("frustrated"));
    }

    #[test]
    fn test_directive_urgent_concise() {
        let d = detector();
        let directive = d.tone_directive("urgent! do this ASAP");
        assert!(directive.contains("concise"));
    }

    #[test]
    fn test_directive_confused_step_by_step() {
        let d = detector();
        let directive = d.tone_directive("I don't understand this");
        assert!(directive.contains("step-by-step"));
    }
}
