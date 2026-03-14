//! Text-to-Speech — multi-backend synthesis engine.
//!
//! Provides the TTS interface for the voice subsystem with support for
//! multiple backends: Windows SAPI (native), Edge TTS (cloud-based),
//! and a None/disabled state. Includes SSML generation, voice listing,
//! configurable prosody (rate, pitch, volume), and file-based synthesis.

use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Backend & Configuration
// ─────────────────────────────────────────────────────────────────────────────

/// Available TTS backend engines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TTSBackend {
    /// Windows SAPI (System.Speech.Synthesis) — always available on Windows.
    WindowsSapi,
    /// Microsoft Edge TTS (cloud-based, high quality neural voices).
    EdgeTts,
    /// No backend / disabled.
    None,
}

impl std::fmt::Display for TTSBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TTSBackend::WindowsSapi => write!(f, "windows_sapi"),
            TTSBackend::EdgeTts => write!(f, "edge-tts"),
            TTSBackend::None => write!(f, "none"),
        }
    }
}

/// Configuration for the text-to-speech engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TTSConfig {
    /// Voice name / identifier (e.g. "en-US-AriaNeural", "Microsoft David Desktop").
    pub voice: String,
    /// Speech rate multiplier (0.5 = half speed, 1.0 = normal, 2.0 = double speed).
    pub rate: f64,
    /// Pitch adjustment string for SSML (e.g. "+0Hz", "+10%", "medium").
    pub pitch: String,
    /// Volume level (0 - 100).
    pub volume: u32,
}

impl Default for TTSConfig {
    fn default() -> Self {
        Self {
            voice: "en-US-AriaNeural".to_string(),
            rate: 1.0,
            pitch: "+0Hz".to_string(),
            volume: 100,
        }
    }
}

impl TTSConfig {
    /// Validate the configuration, returning an error string if invalid.
    pub fn validate(&self) -> Result<(), String> {
        if self.rate < 0.5 || self.rate > 2.0 {
            return Err(format!(
                "Rate must be between 0.5 and 2.0, got {}",
                self.rate
            ));
        }
        if self.volume > 100 {
            return Err(format!(
                "Volume must be between 0 and 100, got {}",
                self.volume
            ));
        }
        if self.voice.is_empty() {
            return Err("Voice name cannot be empty".to_string());
        }
        Ok(())
    }
}

/// Legacy configuration alias for backward compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsConfig {
    /// Backend engine to use.
    pub backend: TtsBackend,
    /// Voice name / identifier.
    pub voice: String,
    /// Speech rate multiplier (1.0 = normal).
    pub rate: f64,
}

/// Legacy backend enum for backward compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TtsBackend {
    /// Microsoft Edge TTS (cloud-based, high quality).
    EdgeTts,
    /// pyttsx3 offline synthesis.
    Pyttsx3,
    /// Google TTS (cloud-based).
    Gtts,
    /// No backend available.
    None,
}

impl std::fmt::Display for TtsBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TtsBackend::EdgeTts => write!(f, "edge-tts"),
            TtsBackend::Pyttsx3 => write!(f, "pyttsx3"),
            TtsBackend::Gtts => write!(f, "gtts"),
            TtsBackend::None => write!(f, "none"),
        }
    }
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            backend: TtsBackend::None,
            voice: "en-US-AriaNeural".to_string(),
            rate: 1.0,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Result Types
// ─────────────────────────────────────────────────────────────────────────────

/// Result of a TTS synthesis operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthesisResult {
    /// Path to the generated audio file, if saved to disk.
    pub audio_path: Option<String>,
    /// Estimated duration of the synthesized speech in milliseconds.
    pub duration_ms: u64,
    /// Which voice was used for synthesis.
    pub voice_used: String,
    /// Which backend produced the audio.
    pub backend: String,
}

/// Information about an available voice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceInfo {
    /// Voice name / identifier.
    pub name: String,
    /// Language code (e.g. "en-US").
    pub language: String,
    /// Gender of the voice.
    pub gender: VoiceGender,
}

/// Gender classification for a TTS voice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoiceGender {
    Male,
    Female,
    Neutral,
    Unknown,
}

impl std::fmt::Display for VoiceGender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VoiceGender::Male => write!(f, "male"),
            VoiceGender::Female => write!(f, "female"),
            VoiceGender::Neutral => write!(f, "neutral"),
            VoiceGender::Unknown => write!(f, "unknown"),
        }
    }
}

/// Statistics for the TTS engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsStats {
    /// Total number of synthesis attempts.
    pub syntheses: u64,
    /// Whether any TTS backend is available.
    pub available: bool,
    /// Configured backend name.
    pub backend: String,
    /// Configured voice.
    pub voice: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// SSML Builder
// ─────────────────────────────────────────────────────────────────────────────

/// Build an SSML document from text and a TTS configuration.
///
/// Wraps the text in `<speak>` and `<prosody>` tags with the configured
/// rate, pitch, and volume. Escapes XML special characters in the text.
pub fn build_ssml(text: &str, config: &TTSConfig) -> String {
    let escaped = escape_xml(text);

    // Convert rate to percentage string for SSML.
    let rate_pct = format!("{:.0}%", config.rate * 100.0);
    let volume_str = format!("{}", config.volume);

    format!(
        "<speak version=\"1.0\" xmlns=\"http://www.w3.org/2001/10/synthesis\" xml:lang=\"en-US\">\
         <voice name=\"{voice}\">\
         <prosody rate=\"{rate}\" pitch=\"{pitch}\" volume=\"{volume}\">\
         {text}\
         </prosody>\
         </voice>\
         </speak>",
        voice = escape_xml_attr(&config.voice),
        rate = rate_pct,
        pitch = escape_xml_attr(&config.pitch),
        volume = volume_str,
        text = escaped,
    )
}

/// Escape XML special characters in text content.
fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Escape XML special characters in attribute values.
fn escape_xml_attr(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Estimate speech duration in milliseconds from text.
///
/// Assumes ~150 words per minute at rate 1.0, ~5 characters per word.
fn estimate_duration_ms(text: &str, rate: f64) -> u64 {
    let word_count = text.split_whitespace().count().max(1) as f64;
    let seconds = (word_count / 150.0) * 60.0 / rate.max(0.1);
    (seconds * 1000.0) as u64
}

// ─────────────────────────────────────────────────────────────────────────────
// TextToSpeech Engine
// ─────────────────────────────────────────────────────────────────────────────

/// Text-to-speech engine with multi-backend support.
///
/// Supports Windows SAPI (via PowerShell), Edge TTS (stub for cloud API),
/// and a disabled state. Provides SSML generation, voice listing, and
/// configurable prosody.
pub struct TextToSpeech {
    /// Active backend.
    backend: TTSBackend,
    /// TTS configuration.
    config: TTSConfig,
    /// Number of synthesis attempts.
    syntheses: u64,
}

impl TextToSpeech {
    /// Create a new TTS engine with the given backend and configuration.
    pub fn new_with_backend(backend: TTSBackend, config: TTSConfig) -> Self {
        Self {
            backend,
            config,
            syntheses: 0,
        }
    }

    /// Create a new TTS engine from a legacy TtsConfig.
    pub fn new(config: TtsConfig) -> Self {
        let backend = match config.backend {
            TtsBackend::EdgeTts => TTSBackend::EdgeTts,
            TtsBackend::None | TtsBackend::Pyttsx3 | TtsBackend::Gtts => {
                if cfg!(target_os = "windows") {
                    TTSBackend::WindowsSapi
                } else {
                    TTSBackend::None
                }
            }
        };
        Self {
            backend,
            config: TTSConfig {
                voice: config.voice,
                rate: config.rate,
                pitch: "+0Hz".to_string(),
                volume: 100,
            },
            syntheses: 0,
        }
    }

    /// Synthesize text to an audio file at the specified path.
    ///
    /// Uses the configured backend to produce audio. Returns a
    /// `SynthesisResult` with the output path and estimated duration.
    pub fn synthesize(
        &mut self,
        text: &str,
        output_path: Option<&str>,
    ) -> Result<SynthesisResult, String> {
        self.syntheses += 1;

        if text.is_empty() {
            return Err("Cannot synthesize empty text".to_string());
        }

        if let Err(e) = self.config.validate() {
            return Err(format!("Invalid TTS config: {e}"));
        }

        match &self.backend {
            TTSBackend::None => {
                Err("No TTS backend configured. Set backend to WindowsSapi or EdgeTts.".to_string())
            }
            TTSBackend::WindowsSapi => self.synthesize_sapi(text, output_path),
            TTSBackend::EdgeTts => {
                Err(format!(
                    "Edge TTS backend not yet integrated. Would synthesize {} chars with voice '{}'. \
                     Output path: {:?}",
                    text.len(),
                    self.config.voice,
                    output_path
                ))
            }
        }
    }

    /// Synthesize text using Windows SAPI via PowerShell.
    fn synthesize_sapi(
        &self,
        text: &str,
        output_path: Option<&str>,
    ) -> Result<SynthesisResult, String> {
        if !cfg!(target_os = "windows") {
            return Err("Windows SAPI is only available on Windows".to_string());
        }

        let escaped = text.replace('\'', "''").replace('"', "`\"");
        let rate = ((self.config.rate - 1.0) * 5.0).round() as i32;
        let volume = self.config.volume.min(100);

        let ps_script = if let Some(path) = output_path {
            let path_escaped = path.replace('\'', "''");
            format!(
                "Add-Type -AssemblyName System.Speech; \
                 $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
                 $s.Rate = {rate}; \
                 $s.Volume = {volume}; \
                 $s.SetOutputToWaveFile('{path_escaped}'); \
                 $s.Speak('{escaped}'); \
                 $s.SetOutputToNull(); \
                 $s.Dispose()"
            )
        } else {
            format!(
                "Add-Type -AssemblyName System.Speech; \
                 $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
                 $s.Rate = {rate}; \
                 $s.Volume = {volume}; \
                 $s.Speak('{escaped}')"
            )
        };

        match std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &ps_script])
            .output()
        {
            Ok(output) => {
                if output.status.success() {
                    Ok(SynthesisResult {
                        audio_path: output_path.map(|p| p.to_string()),
                        duration_ms: estimate_duration_ms(text, self.config.rate),
                        voice_used: self.config.voice.clone(),
                        backend: "windows_sapi".to_string(),
                    })
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    Err(format!("SAPI speech failed: {stderr}"))
                }
            }
            Err(e) => Err(format!("Failed to launch PowerShell for SAPI: {e}")),
        }
    }

    /// Speak text aloud using the configured backend.
    ///
    /// Shorthand for `synthesize(text, None)` — plays through the default
    /// audio device without saving to a file.
    pub fn speak(&mut self, text: &str) -> Result<SynthesisResult, String> {
        if text.is_empty() {
            self.syntheses += 1;
            return Err("Cannot speak empty text".to_string());
        }
        self.synthesize(text, None)
    }

    /// List available system voices.
    ///
    /// On Windows, queries SAPI for installed voices. Returns a vector of
    /// `VoiceInfo` structs with name, language, and gender.
    pub fn list_voices(&self) -> Result<Vec<VoiceInfo>, String> {
        if !cfg!(target_os = "windows") {
            return Err("Voice listing only available on Windows (SAPI)".to_string());
        }

        let ps_script =
            "Add-Type -AssemblyName System.Speech; \
             $synth = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
             foreach ($v in $synth.GetInstalledVoices()) { \
                 $info = $v.VoiceInfo; \
                 Write-Output \"$($info.Name)|$($info.Culture)|$($info.Gender)\" \
             }";

        match std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", ps_script])
            .output()
        {
            Ok(output) => {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let voices: Vec<VoiceInfo> = stdout
                        .lines()
                        .filter(|line| !line.trim().is_empty())
                        .filter_map(|line| {
                            let parts: Vec<&str> = line.split('|').collect();
                            if parts.len() >= 3 {
                                let gender_val = match parts[2].trim().to_lowercase().as_str() {
                                    "male" => VoiceGender::Male,
                                    "female" => VoiceGender::Female,
                                    "neutral" | "notset" => VoiceGender::Neutral,
                                    _ => VoiceGender::Unknown,
                                };
                                Some(VoiceInfo {
                                    name: parts[0].trim().to_string(),
                                    language: parts[1].trim().to_string(),
                                    gender: gender_val,
                                })
                            } else {
                                Option::None
                            }
                        })
                        .collect();
                    Ok(voices)
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    Err(format!("Failed to list SAPI voices: {stderr}"))
                }
            }
            Err(e) => Err(format!("Failed to launch PowerShell: {e}")),
        }
    }

    /// Build SSML for the given text using the current configuration.
    pub fn build_ssml(&self, text: &str) -> String {
        build_ssml(text, &self.config)
    }

    /// Check if any TTS backend is available.
    pub fn is_available(&self) -> bool {
        match self.backend {
            TTSBackend::WindowsSapi => cfg!(target_os = "windows"),
            TTSBackend::EdgeTts => false, // Not yet integrated.
            TTSBackend::None => false,
        }
    }

    /// Get engine statistics.
    pub fn get_stats(&self) -> TtsStats {
        TtsStats {
            syntheses: self.syntheses,
            available: self.is_available(),
            backend: self.backend.to_string(),
            voice: self.config.voice.clone(),
        }
    }

    /// Get a reference to the current configuration.
    pub fn config(&self) -> &TTSConfig {
        &self.config
    }

    /// Get the active backend.
    pub fn backend(&self) -> &TTSBackend {
        &self.backend
    }

    /// Update the TTS configuration.
    pub fn set_config(&mut self, config: TTSConfig) {
        self.config = config;
    }

    /// Update the active backend.
    pub fn set_backend(&mut self, backend: TTSBackend) {
        self.backend = backend;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Backend enum tests ───────────────────────────────────────────────

    #[test]
    fn test_backend_display() {
        assert_eq!(format!("{}", TTSBackend::WindowsSapi), "windows_sapi");
        assert_eq!(format!("{}", TTSBackend::EdgeTts), "edge-tts");
        assert_eq!(format!("{}", TTSBackend::None), "none");
    }

    #[test]
    fn test_legacy_backend_display() {
        assert_eq!(format!("{}", TtsBackend::EdgeTts), "edge-tts");
        assert_eq!(format!("{}", TtsBackend::Pyttsx3), "pyttsx3");
        assert_eq!(format!("{}", TtsBackend::Gtts), "gtts");
        assert_eq!(format!("{}", TtsBackend::None), "none");
    }

    #[test]
    fn test_backend_equality() {
        assert_eq!(TTSBackend::WindowsSapi, TTSBackend::WindowsSapi);
        assert_ne!(TTSBackend::WindowsSapi, TTSBackend::EdgeTts);
        assert_ne!(TTSBackend::EdgeTts, TTSBackend::None);
    }

    // ── TTSConfig tests ─────────────────────────────────────────────────

    #[test]
    fn test_default_tts_config() {
        let config = TTSConfig::default();
        assert_eq!(config.voice, "en-US-AriaNeural");
        assert!((config.rate - 1.0).abs() < f64::EPSILON);
        assert_eq!(config.pitch, "+0Hz");
        assert_eq!(config.volume, 100);
    }

    #[test]
    fn test_config_validate_valid() {
        let config = TTSConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validate_rate_too_low() {
        let config = TTSConfig {
            rate: 0.1,
            ..TTSConfig::default()
        };
        let err = config.validate().unwrap_err();
        assert!(err.contains("Rate"));
    }

    #[test]
    fn test_config_validate_rate_too_high() {
        let config = TTSConfig {
            rate: 3.0,
            ..TTSConfig::default()
        };
        let err = config.validate().unwrap_err();
        assert!(err.contains("Rate"));
    }

    #[test]
    fn test_config_validate_volume_too_high() {
        let config = TTSConfig {
            volume: 150,
            ..TTSConfig::default()
        };
        let err = config.validate().unwrap_err();
        assert!(err.contains("Volume"));
    }

    #[test]
    fn test_config_validate_empty_voice() {
        let config = TTSConfig {
            voice: String::new(),
            ..TTSConfig::default()
        };
        let err = config.validate().unwrap_err();
        assert!(err.contains("Voice name"));
    }

    #[test]
    fn test_config_serialization() {
        let config = TTSConfig {
            voice: "en-GB-RyanNeural".to_string(),
            rate: 1.5,
            pitch: "+10%".to_string(),
            volume: 80,
        };
        let json = serde_json::to_string(&config).unwrap();
        let restored: TTSConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.voice, "en-GB-RyanNeural");
        assert!((restored.rate - 1.5).abs() < f64::EPSILON);
        assert_eq!(restored.volume, 80);
    }

    // ── Legacy TtsConfig tests ──────────────────────────────────────────

    #[test]
    fn test_default_legacy_config() {
        let config = TtsConfig::default();
        assert_eq!(config.backend, TtsBackend::None);
        assert_eq!(config.voice, "en-US-AriaNeural");
        assert!((config.rate - 1.0).abs() < f64::EPSILON);
    }

    // ── SSML builder tests ──────────────────────────────────────────────

    #[test]
    fn test_build_ssml_basic() {
        let config = TTSConfig::default();
        let ssml = build_ssml("Hello world", &config);
        assert!(ssml.contains("<speak"));
        assert!(ssml.contains("</speak>"));
        assert!(ssml.contains("Hello world"));
        assert!(ssml.contains("en-US-AriaNeural"));
        assert!(ssml.contains("100%")); // rate
    }

    #[test]
    fn test_build_ssml_escapes_xml() {
        let config = TTSConfig::default();
        let ssml = build_ssml("1 < 2 & 3 > 0", &config);
        assert!(ssml.contains("&lt;"));
        assert!(ssml.contains("&amp;"));
        assert!(ssml.contains("&gt;"));
    }

    #[test]
    fn test_build_ssml_custom_config() {
        let config = TTSConfig {
            voice: "en-GB-RyanNeural".to_string(),
            rate: 1.5,
            pitch: "+10Hz".to_string(),
            volume: 80,
        };
        let ssml = build_ssml("Test", &config);
        assert!(ssml.contains("en-GB-RyanNeural"));
        assert!(ssml.contains("150%")); // 1.5 * 100
        assert!(ssml.contains("+10Hz"));
        assert!(ssml.contains("80"));
    }

    #[test]
    fn test_escape_xml_special_chars() {
        let result = escape_xml("a&b<c>d\"e'f");
        assert_eq!(result, "a&amp;b&lt;c&gt;d&quot;e&apos;f");
    }

    // ── Duration estimation tests ────────────────────────────────────────

    #[test]
    fn test_estimate_duration_normal_rate() {
        let ms = estimate_duration_ms("one two three four five", 1.0);
        // 5 words / 150 wpm * 60s = 2.0s = 2000ms
        assert_eq!(ms, 2000);
    }

    #[test]
    fn test_estimate_duration_double_rate() {
        let ms = estimate_duration_ms("one two three four five", 2.0);
        // 5 words / 150 wpm * 60s / 2.0 = 1.0s = 1000ms
        assert_eq!(ms, 1000);
    }

    #[test]
    fn test_estimate_duration_empty_text() {
        let ms = estimate_duration_ms("", 1.0);
        // min 1 word -> 400ms
        assert_eq!(ms, 400);
    }

    // ── TextToSpeech engine tests ────────────────────────────────────────

    #[test]
    fn test_synthesize_empty_text() {
        let mut tts = TextToSpeech::new_with_backend(TTSBackend::WindowsSapi, TTSConfig::default());
        let result = tts.synthesize("", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty text"));
    }

    #[test]
    fn test_synthesize_no_backend() {
        let mut tts = TextToSpeech::new_with_backend(TTSBackend::None, TTSConfig::default());
        let result = tts.synthesize("Hello world", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No TTS backend"));
    }

    #[test]
    fn test_synthesize_edge_tts_stub() {
        let mut tts = TextToSpeech::new_with_backend(TTSBackend::EdgeTts, TTSConfig::default());
        let result = tts.synthesize("Hello world", Some("/tmp/out.wav"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not yet integrated"));
    }

    #[test]
    fn test_speak_empty_text() {
        let mut tts = TextToSpeech::new_with_backend(TTSBackend::WindowsSapi, TTSConfig::default());
        let result = tts.speak("");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty text"));
    }

    #[test]
    fn test_synthesize_invalid_config() {
        let config = TTSConfig {
            rate: 5.0, // Invalid.
            ..TTSConfig::default()
        };
        let mut tts = TextToSpeech::new_with_backend(TTSBackend::WindowsSapi, config);
        let result = tts.synthesize("test", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid TTS config"));
    }

    #[test]
    fn test_stats_tracking() {
        let mut tts = TextToSpeech::new_with_backend(TTSBackend::None, TTSConfig::default());
        let _ = tts.synthesize("test one", None);
        let _ = tts.synthesize("test two", None);

        let stats = tts.get_stats();
        assert_eq!(stats.syntheses, 2);
        assert_eq!(stats.backend, "none");
        assert_eq!(stats.voice, "en-US-AriaNeural");
    }

    #[test]
    fn test_is_available_platform() {
        let tts = TextToSpeech::new_with_backend(TTSBackend::WindowsSapi, TTSConfig::default());
        if cfg!(target_os = "windows") {
            assert!(tts.is_available());
        } else {
            assert!(!tts.is_available());
        }
    }

    #[test]
    fn test_is_available_none() {
        let tts = TextToSpeech::new_with_backend(TTSBackend::None, TTSConfig::default());
        assert!(!tts.is_available());
    }

    #[test]
    fn test_is_available_edge_tts() {
        let tts = TextToSpeech::new_with_backend(TTSBackend::EdgeTts, TTSConfig::default());
        assert!(!tts.is_available());
    }

    #[test]
    fn test_build_ssml_method() {
        let tts = TextToSpeech::new_with_backend(TTSBackend::WindowsSapi, TTSConfig::default());
        let ssml = tts.build_ssml("Hello");
        assert!(ssml.contains("<speak"));
        assert!(ssml.contains("Hello"));
    }

    #[test]
    fn test_set_config() {
        let mut tts = TextToSpeech::new_with_backend(TTSBackend::WindowsSapi, TTSConfig::default());
        let new_config = TTSConfig {
            voice: "fr-FR-DeniseNeural".to_string(),
            rate: 0.8,
            pitch: "-5Hz".to_string(),
            volume: 60,
        };
        tts.set_config(new_config);
        assert_eq!(tts.config().voice, "fr-FR-DeniseNeural");
        assert!((tts.config().rate - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn test_set_backend() {
        let mut tts = TextToSpeech::new_with_backend(TTSBackend::None, TTSConfig::default());
        tts.set_backend(TTSBackend::WindowsSapi);
        assert_eq!(*tts.backend(), TTSBackend::WindowsSapi);
    }

    #[test]
    fn test_voice_info_serialization() {
        let voice = VoiceInfo {
            name: "Microsoft David Desktop".to_string(),
            language: "en-US".to_string(),
            gender: VoiceGender::Male,
        };
        let json = serde_json::to_string(&voice).unwrap();
        let restored: VoiceInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.name, "Microsoft David Desktop");
        assert_eq!(restored.gender, VoiceGender::Male);
    }

    #[test]
    fn test_voice_gender_display() {
        assert_eq!(format!("{}", VoiceGender::Male), "male");
        assert_eq!(format!("{}", VoiceGender::Female), "female");
        assert_eq!(format!("{}", VoiceGender::Neutral), "neutral");
        assert_eq!(format!("{}", VoiceGender::Unknown), "unknown");
    }

    #[test]
    fn test_legacy_new_constructor() {
        let config = TtsConfig {
            backend: TtsBackend::None,
            voice: "test-voice".to_string(),
            rate: 1.2,
        };
        let tts = TextToSpeech::new(config);
        assert_eq!(tts.config().voice, "test-voice");
        assert!((tts.config().rate - 1.2).abs() < f64::EPSILON);
    }

    #[test]
    fn test_synthesis_result_serialization() {
        let result = SynthesisResult {
            audio_path: Some("/tmp/output.wav".to_string()),
            duration_ms: 2500,
            voice_used: "en-US-AriaNeural".to_string(),
            backend: "windows_sapi".to_string(),
        };
        let json = serde_json::to_string(&result).unwrap();
        let restored: SynthesisResult = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.duration_ms, 2500);
        assert_eq!(restored.backend, "windows_sapi");
    }
}
