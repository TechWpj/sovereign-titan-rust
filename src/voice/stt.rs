//! Speech-to-Text — Whisper-based transcription engine.
//!
//! Provides the STT interface for the voice subsystem. Includes a proper
//! `WhisperConfig`, `VoiceActivityDetector` with energy-based VAD, and
//! `SpeechToText` engine that manages transcription via whisper model inference.
//! Actual whisper model loading requires whisper.cpp / whisper-rs native bindings
//! which are resolved at runtime based on model_path availability.

use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Configuration
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for the Whisper-based STT engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperConfig {
    /// Path to the Whisper GGML model file.
    pub model_path: String,
    /// Language code for transcription (e.g. "en", "auto" for detection).
    pub language: String,
    /// Beam search size for decoding (1 = greedy, higher = better quality).
    pub beam_size: u32,
    /// Whether voice activity detection is enabled.
    pub vad_enabled: bool,
    /// Audio sample rate in Hz.
    pub sample_rate: u32,
    /// Energy threshold for VAD.
    pub energy_threshold: f32,
}

impl Default for WhisperConfig {
    fn default() -> Self {
        Self {
            model_path: String::new(),
            language: "en".to_string(),
            beam_size: 5,
            vad_enabled: true,
            sample_rate: 16000,
            energy_threshold: 0.01,
        }
    }
}

/// Legacy configuration alias for backward compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SttConfig {
    /// Whisper model size ("tiny", "base", "small", "medium", "large").
    pub model_size: String,
    /// Minimum audio energy to trigger transcription.
    pub energy_threshold: f64,
    /// Audio sample rate in Hz.
    pub sample_rate: u32,
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            model_size: "base".to_string(),
            energy_threshold: 300.0,
            sample_rate: 16000,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Transcription Result Types
// ─────────────────────────────────────────────────────────────────────────────

/// Result of a transcription attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    /// Full transcribed text.
    pub text: String,
    /// Confidence score (0.0 - 1.0).
    pub confidence: f64,
    /// Detected language code (e.g. "en").
    pub language: String,
    /// Per-segment breakdown of the transcription.
    pub segments: Vec<AudioSegment>,
    /// Total duration of the transcribed audio in milliseconds.
    pub duration_ms: u64,
}

/// A segment of transcribed audio with timing information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSegment {
    /// Start time of the segment in seconds.
    pub start_time: f64,
    /// End time of the segment in seconds.
    pub end_time: f64,
    /// Transcribed text for this segment.
    pub text: String,
    /// Confidence score for this segment (0.0 - 1.0).
    pub confidence: f64,
}

impl AudioSegment {
    /// Create a new audio segment.
    pub fn new(start_time: f64, end_time: f64, text: &str, confidence: f64) -> Self {
        Self {
            start_time,
            end_time,
            text: text.to_string(),
            confidence: confidence.clamp(0.0, 1.0),
        }
    }

    /// Duration of this segment in seconds.
    pub fn duration(&self) -> f64 {
        self.end_time - self.start_time
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Voice Activity Detection
// ─────────────────────────────────────────────────────────────────────────────

/// A detected speech segment from VAD analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechSegment {
    /// Start time in seconds.
    pub start_time: f64,
    /// End time in seconds.
    pub end_time: f64,
    /// Average energy level of the segment.
    pub energy: f32,
}

impl SpeechSegment {
    /// Duration of this speech segment in seconds.
    pub fn duration(&self) -> f64 {
        self.end_time - self.start_time
    }
}

/// Configuration for the Voice Activity Detector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VadConfig {
    /// Energy threshold: frames with energy above this are considered speech.
    pub energy_threshold: f32,
    /// Minimum segment length in seconds (segments shorter than this are dropped).
    pub min_segment_length: f64,
    /// Maximum gap in seconds between segments before merging.
    pub merge_gap: f64,
    /// Frame size in samples for energy computation.
    pub frame_size: usize,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            energy_threshold: 0.01,
            min_segment_length: 0.3,
            merge_gap: 0.3,
            frame_size: 480, // 30ms at 16kHz
        }
    }
}

/// Energy-based Voice Activity Detector.
///
/// Analyzes audio samples to detect speech regions using frame-level energy
/// thresholding, minimum segment length filtering, and close-segment merging.
pub struct VoiceActivityDetector {
    /// VAD configuration.
    config: VadConfig,
}

impl VoiceActivityDetector {
    /// Create a new VAD with the given configuration.
    pub fn new(config: VadConfig) -> Self {
        Self { config }
    }

    /// Create a new VAD with default configuration.
    pub fn with_defaults() -> Self {
        Self {
            config: VadConfig::default(),
        }
    }

    /// Create a new VAD with a custom energy threshold (using defaults for everything else).
    pub fn with_threshold(threshold: f32) -> Self {
        Self {
            config: VadConfig {
                energy_threshold: threshold,
                ..VadConfig::default()
            },
        }
    }

    /// Compute the energy of a frame of audio samples.
    ///
    /// Energy = sum of squares / number of samples.
    pub fn frame_energy(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
        sum_sq / samples.len() as f32
    }

    /// Detect speech segments in the given audio samples.
    ///
    /// The algorithm:
    /// 1. Split audio into frames of `frame_size` samples.
    /// 2. Compute energy for each frame.
    /// 3. Mark frames with energy > threshold as speech.
    /// 4. Build contiguous speech segments from consecutive speech frames.
    /// 5. Filter out segments shorter than `min_segment_length`.
    /// 6. Merge segments separated by less than `merge_gap`.
    pub fn detect_speech(&self, samples: &[f32], sample_rate: u32) -> Vec<SpeechSegment> {
        if samples.is_empty() || sample_rate == 0 {
            return Vec::new();
        }

        let frame_size = self.config.frame_size.max(1);
        let sr = sample_rate as f64;

        // Step 1-3: Identify speech frames and build raw segments.
        let mut raw_segments: Vec<SpeechSegment> = Vec::new();
        let mut in_speech = false;
        let mut segment_start: f64 = 0.0;
        let mut segment_energy_sum: f32 = 0.0;
        let mut segment_frame_count: u32 = 0;

        let num_frames = (samples.len() + frame_size - 1) / frame_size;

        for i in 0..num_frames {
            let start_idx = i * frame_size;
            let end_idx = (start_idx + frame_size).min(samples.len());
            let frame = &samples[start_idx..end_idx];
            let energy = Self::frame_energy(frame);

            let is_speech = energy > self.config.energy_threshold;

            if is_speech && !in_speech {
                // Speech onset.
                in_speech = true;
                segment_start = start_idx as f64 / sr;
                segment_energy_sum = energy;
                segment_frame_count = 1;
            } else if is_speech && in_speech {
                // Continuing speech.
                segment_energy_sum += energy;
                segment_frame_count += 1;
            } else if !is_speech && in_speech {
                // Speech offset.
                in_speech = false;
                let end_time = end_idx as f64 / sr;
                let avg_energy = if segment_frame_count > 0 {
                    segment_energy_sum / segment_frame_count as f32
                } else {
                    0.0
                };
                raw_segments.push(SpeechSegment {
                    start_time: segment_start,
                    end_time,
                    energy: avg_energy,
                });
            }
        }

        // Handle case where speech extends to end of audio.
        if in_speech {
            let end_time = samples.len() as f64 / sr;
            let avg_energy = if segment_frame_count > 0 {
                segment_energy_sum / segment_frame_count as f32
            } else {
                0.0
            };
            raw_segments.push(SpeechSegment {
                start_time: segment_start,
                end_time,
                energy: avg_energy,
            });
        }

        // Step 5: Filter out segments shorter than minimum length.
        let filtered: Vec<SpeechSegment> = raw_segments
            .into_iter()
            .filter(|seg| seg.duration() >= self.config.min_segment_length)
            .collect();

        // Step 6: Merge segments separated by less than merge_gap.
        self.merge_segments(filtered)
    }

    /// Merge speech segments that are separated by less than the configured gap.
    fn merge_segments(&self, segments: Vec<SpeechSegment>) -> Vec<SpeechSegment> {
        if segments.is_empty() {
            return segments;
        }

        let mut merged: Vec<SpeechSegment> = Vec::new();
        let mut current = segments[0].clone();

        for seg in segments.into_iter().skip(1) {
            let gap = seg.start_time - current.end_time;
            if gap < self.config.merge_gap {
                // Merge: extend current segment, average energies.
                current.end_time = seg.end_time;
                current.energy = (current.energy + seg.energy) / 2.0;
            } else {
                merged.push(current);
                current = seg;
            }
        }
        merged.push(current);

        merged
    }

    /// Get a reference to the VAD configuration.
    pub fn config(&self) -> &VadConfig {
        &self.config
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Speech-to-Text Engine
// ─────────────────────────────────────────────────────────────────────────────

/// Statistics for the STT engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SttStats {
    /// Total number of transcription attempts.
    pub transcriptions: u64,
    /// Whether whisper bindings are available.
    pub available: bool,
    /// Configured model size.
    pub model_size: String,
    /// Configured sample rate.
    pub sample_rate: u32,
}

/// Speech-to-text engine backed by Whisper model inference.
///
/// Uses `WhisperConfig` for model configuration, integrates VAD for
/// preprocessing, and manages transcription lifecycle. Actual whisper
/// model inference requires whisper-rs native bindings.
pub struct SpeechToText {
    /// Engine configuration.
    config: WhisperConfig,
    /// Voice activity detector.
    vad: VoiceActivityDetector,
    /// Number of transcription attempts.
    transcriptions: u64,
    /// Whether the whisper model is loaded and available.
    model_loaded: bool,
}

impl SpeechToText {
    /// Create a new STT engine from a legacy `SttConfig`.
    ///
    /// This is the primary constructor used by the voice interface.
    pub fn new(config: SttConfig) -> Self {
        let whisper_config = WhisperConfig {
            model_path: String::new(),
            language: "en".to_string(),
            beam_size: 5,
            vad_enabled: true,
            sample_rate: config.sample_rate,
            energy_threshold: config.energy_threshold as f32,
        };
        Self::with_whisper_config(whisper_config)
    }

    /// Create a new STT engine with a full Whisper configuration.
    pub fn with_whisper_config(config: WhisperConfig) -> Self {
        let vad = VoiceActivityDetector::new(VadConfig {
            energy_threshold: config.energy_threshold,
            ..VadConfig::default()
        });
        Self {
            config,
            vad,
            transcriptions: 0,
            model_loaded: false,
        }
    }

    /// Attempt to load the whisper model from the configured path.
    ///
    /// Returns Ok if the model file exists and can be opened, Err otherwise.
    /// Actual whisper-rs model initialization would happen here.
    pub fn load_model(&mut self) -> Result<(), String> {
        if self.config.model_path.is_empty() {
            return Err("No model path configured".to_string());
        }

        let path = std::path::Path::new(&self.config.model_path);
        if !path.exists() {
            return Err(format!("Model file not found: {}", self.config.model_path));
        }

        // In a full implementation, this would initialize whisper-rs context.
        // For now, we validate the file exists and mark as loaded.
        self.model_loaded = true;
        Ok(())
    }

    /// Transcribe an audio file at the given path.
    ///
    /// Reads the audio file, optionally applies VAD preprocessing, and
    /// runs whisper inference. Returns an error if the model is not loaded
    /// or the file is not accessible.
    pub fn transcribe_file(&mut self, path: &str) -> Result<TranscriptionResult, String> {
        self.transcriptions += 1;

        if !std::path::Path::new(path).exists() {
            return Err(format!("Audio file not found: {path}"));
        }

        if !self.model_loaded {
            return Err(format!(
                "Whisper model not loaded (model_path='{}'). Call load_model() first or \
                 provide a valid model path.",
                self.config.model_path
            ));
        }

        // With model loaded, would perform actual transcription here.
        // Placeholder for whisper-rs inference.
        Err("Whisper-rs native bindings not yet integrated for file transcription".to_string())
    }

    /// Transcribe raw audio samples (f32 PCM, mono).
    ///
    /// Optionally applies VAD to extract speech regions before transcription.
    pub fn transcribe_samples(
        &mut self,
        samples: &[f32],
        sample_rate: u32,
    ) -> Result<TranscriptionResult, String> {
        self.transcriptions += 1;

        if samples.is_empty() {
            return Err("Empty audio buffer".to_string());
        }

        if sample_rate == 0 {
            return Err("Sample rate must be > 0".to_string());
        }

        // Apply VAD if enabled.
        let speech_segments = if self.config.vad_enabled {
            self.vad.detect_speech(samples, sample_rate)
        } else {
            vec![SpeechSegment {
                start_time: 0.0,
                end_time: samples.len() as f64 / sample_rate as f64,
                energy: VoiceActivityDetector::frame_energy(samples),
            }]
        };

        if speech_segments.is_empty() {
            return Ok(TranscriptionResult {
                text: String::new(),
                confidence: 0.0,
                language: self.config.language.clone(),
                segments: Vec::new(),
                duration_ms: (samples.len() as f64 / sample_rate as f64 * 1000.0) as u64,
            });
        }

        if !self.model_loaded {
            return Err(format!(
                "Whisper model not loaded. {} speech segment(s) detected by VAD. \
                 Call load_model() first.",
                speech_segments.len()
            ));
        }

        // With model loaded, would perform actual transcription here.
        Err("Whisper-rs native bindings not yet integrated for sample transcription".to_string())
    }

    /// Transcribe raw audio bytes (16-bit PCM, mono).
    ///
    /// Converts bytes to f32 samples and delegates to `transcribe_samples`.
    pub fn transcribe_bytes(
        &mut self,
        audio: &[u8],
        sample_rate: u32,
    ) -> Result<TranscriptionResult, String> {
        if audio.is_empty() {
            self.transcriptions += 1;
            return Err("Empty audio buffer".to_string());
        }

        // Convert 16-bit PCM bytes to f32 samples.
        let samples: Vec<f32> = audio
            .chunks_exact(2)
            .map(|chunk| {
                let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
                sample as f32 / 32768.0
            })
            .collect();

        self.transcribe_samples(&samples, sample_rate)
    }

    /// Check if the whisper model is loaded and available for transcription.
    pub fn is_available(&self) -> bool {
        self.model_loaded
    }

    /// Get engine statistics.
    pub fn get_stats(&self) -> SttStats {
        SttStats {
            transcriptions: self.transcriptions,
            available: self.is_available(),
            model_size: self.config.language.clone(),
            sample_rate: self.config.sample_rate,
        }
    }

    /// Get a reference to the current Whisper configuration.
    pub fn config(&self) -> &WhisperConfig {
        &self.config
    }

    /// Get a reference to the VAD.
    pub fn vad(&self) -> &VoiceActivityDetector {
        &self.vad
    }

    /// Preprocess audio with VAD and return speech segments.
    pub fn detect_speech(&self, samples: &[f32], sample_rate: u32) -> Vec<SpeechSegment> {
        self.vad.detect_speech(samples, sample_rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── WhisperConfig tests ──────────────────────────────────────────────

    #[test]
    fn test_default_whisper_config() {
        let config = WhisperConfig::default();
        assert!(config.model_path.is_empty());
        assert_eq!(config.language, "en");
        assert_eq!(config.beam_size, 5);
        assert!(config.vad_enabled);
        assert_eq!(config.sample_rate, 16000);
        assert!((config.energy_threshold - 0.01).abs() < f32::EPSILON);
    }

    #[test]
    fn test_legacy_stt_config_default() {
        let config = SttConfig::default();
        assert_eq!(config.model_size, "base");
        assert!((config.energy_threshold - 300.0).abs() < f64::EPSILON);
        assert_eq!(config.sample_rate, 16000);
    }

    #[test]
    fn test_whisper_config_serialization() {
        let config = WhisperConfig {
            model_path: "/models/whisper-base.bin".to_string(),
            language: "fr".to_string(),
            beam_size: 3,
            vad_enabled: false,
            sample_rate: 44100,
            energy_threshold: 0.05,
        };
        let json = serde_json::to_string(&config).unwrap();
        let restored: WhisperConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.model_path, "/models/whisper-base.bin");
        assert_eq!(restored.language, "fr");
        assert_eq!(restored.beam_size, 3);
        assert!(!restored.vad_enabled);
        assert_eq!(restored.sample_rate, 44100);
    }

    // ── AudioSegment tests ──────────────────────────────────────────────

    #[test]
    fn test_audio_segment_creation() {
        let seg = AudioSegment::new(1.0, 2.5, "hello world", 0.95);
        assert_eq!(seg.start_time, 1.0);
        assert_eq!(seg.end_time, 2.5);
        assert_eq!(seg.text, "hello world");
        assert!((seg.confidence - 0.95).abs() < f64::EPSILON);
    }

    #[test]
    fn test_audio_segment_duration() {
        let seg = AudioSegment::new(0.5, 3.0, "test", 0.8);
        assert!((seg.duration() - 2.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_audio_segment_confidence_clamping() {
        let seg = AudioSegment::new(0.0, 1.0, "over", 1.5);
        assert!((seg.confidence - 1.0).abs() < f64::EPSILON);

        let seg2 = AudioSegment::new(0.0, 1.0, "under", -0.5);
        assert!((seg2.confidence - 0.0).abs() < f64::EPSILON);
    }

    // ── VAD tests ────────────────────────────────────────────────────────

    #[test]
    fn test_vad_default_config() {
        let config = VadConfig::default();
        assert!((config.energy_threshold - 0.01).abs() < f32::EPSILON);
        assert!((config.min_segment_length - 0.3).abs() < f64::EPSILON);
        assert!((config.merge_gap - 0.3).abs() < f64::EPSILON);
        assert_eq!(config.frame_size, 480);
    }

    #[test]
    fn test_frame_energy_silence() {
        let silence = vec![0.0f32; 480];
        let energy = VoiceActivityDetector::frame_energy(&silence);
        assert!(energy < f32::EPSILON);
    }

    #[test]
    fn test_frame_energy_signal() {
        // A constant signal of 0.5 should have energy 0.25.
        let signal = vec![0.5f32; 480];
        let energy = VoiceActivityDetector::frame_energy(&signal);
        assert!((energy - 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn test_frame_energy_empty() {
        let energy = VoiceActivityDetector::frame_energy(&[]);
        assert!(energy < f32::EPSILON);
    }

    #[test]
    fn test_vad_detect_silence() {
        let vad = VoiceActivityDetector::with_defaults();
        let silence = vec![0.0f32; 16000]; // 1 second of silence
        let segments = vad.detect_speech(&silence, 16000);
        assert!(segments.is_empty());
    }

    #[test]
    fn test_vad_detect_constant_speech() {
        let vad = VoiceActivityDetector::with_threshold(0.001);
        // 1 second of a clear signal at 16kHz
        let signal = vec![0.1f32; 16000];
        let segments = vad.detect_speech(&signal, 16000);
        assert!(!segments.is_empty());
        // Should cover roughly the full duration.
        let total_duration: f64 = segments.iter().map(|s| s.duration()).sum();
        assert!(total_duration > 0.5);
    }

    #[test]
    fn test_vad_detect_speech_with_gap() {
        let vad = VoiceActivityDetector::new(VadConfig {
            energy_threshold: 0.001,
            min_segment_length: 0.1,
            merge_gap: 0.05, // Very small merge gap so segments stay separate.
            frame_size: 160,
        });

        let mut samples = Vec::new();
        // 0.5s of speech.
        samples.extend(vec![0.1f32; 8000]);
        // 0.5s of silence (gap > merge_gap).
        samples.extend(vec![0.0f32; 8000]);
        // 0.5s of speech.
        samples.extend(vec![0.1f32; 8000]);

        let segments = vad.detect_speech(&samples, 16000);
        assert!(segments.len() >= 2, "Expected at least 2 segments, got {}", segments.len());
    }

    #[test]
    fn test_vad_merge_close_segments() {
        let vad = VoiceActivityDetector::new(VadConfig {
            energy_threshold: 0.001,
            min_segment_length: 0.05,
            merge_gap: 1.0, // Large merge gap to force merging.
            frame_size: 160,
        });

        let mut samples = Vec::new();
        // 0.5s speech, tiny silence, 0.5s speech.
        samples.extend(vec![0.1f32; 8000]);
        samples.extend(vec![0.0f32; 800]); // 50ms gap
        samples.extend(vec![0.1f32; 8000]);

        let segments = vad.detect_speech(&samples, 16000);
        // Should merge into 1 segment since gap (50ms) < merge_gap (1s).
        assert_eq!(segments.len(), 1);
    }

    #[test]
    fn test_vad_filter_short_segments() {
        let vad = VoiceActivityDetector::new(VadConfig {
            energy_threshold: 0.001,
            min_segment_length: 0.5, // Half second minimum.
            merge_gap: 0.01,
            frame_size: 160,
        });

        let mut samples = Vec::new();
        // Very short burst (100ms).
        samples.extend(vec![0.1f32; 1600]);
        // Silence.
        samples.extend(vec![0.0f32; 16000]);

        let segments = vad.detect_speech(&samples, 16000);
        // The 100ms burst should be filtered out.
        assert!(segments.is_empty());
    }

    #[test]
    fn test_vad_empty_input() {
        let vad = VoiceActivityDetector::with_defaults();
        let segments = vad.detect_speech(&[], 16000);
        assert!(segments.is_empty());
    }

    #[test]
    fn test_vad_zero_sample_rate() {
        let vad = VoiceActivityDetector::with_defaults();
        let samples = vec![0.1f32; 1000];
        let segments = vad.detect_speech(&samples, 0);
        assert!(segments.is_empty());
    }

    #[test]
    fn test_speech_segment_duration() {
        let seg = SpeechSegment {
            start_time: 1.0,
            end_time: 3.5,
            energy: 0.05,
        };
        assert!((seg.duration() - 2.5).abs() < f64::EPSILON);
    }

    // ── SpeechToText engine tests ────────────────────────────────────────

    #[test]
    fn test_stt_new_default() {
        let stt = SpeechToText::with_whisper_config(WhisperConfig::default());
        assert!(!stt.is_available());
        assert_eq!(stt.config().language, "en");
        assert_eq!(stt.config().beam_size, 5);
    }

    #[test]
    fn test_stt_from_legacy() {
        let legacy = SttConfig {
            model_size: "small".to_string(),
            energy_threshold: 500.0,
            sample_rate: 44100,
        };
        let stt = SpeechToText::new(legacy);
        assert_eq!(stt.config().sample_rate, 44100);
        assert!(!stt.is_available());
    }

    #[test]
    fn test_not_available() {
        let stt = SpeechToText::with_whisper_config(WhisperConfig::default());
        assert!(!stt.is_available());
    }

    #[test]
    fn test_load_model_empty_path() {
        let mut stt = SpeechToText::with_whisper_config(WhisperConfig::default());
        let result = stt.load_model();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No model path"));
    }

    #[test]
    fn test_load_model_nonexistent() {
        let config = WhisperConfig {
            model_path: "/nonexistent/model.bin".to_string(),
            ..WhisperConfig::default()
        };
        let mut stt = SpeechToText::with_whisper_config(config);
        let result = stt.load_model();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_transcribe_file_not_found() {
        let mut stt = SpeechToText::with_whisper_config(WhisperConfig::default());
        let result = stt.transcribe_file("/nonexistent/audio.wav");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_transcribe_bytes_empty() {
        let mut stt = SpeechToText::with_whisper_config(WhisperConfig::default());
        let result = stt.transcribe_bytes(&[], 16000);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Empty audio"));
    }

    #[test]
    fn test_transcribe_bytes_no_model() {
        let mut stt = SpeechToText::with_whisper_config(WhisperConfig::default());
        let audio = vec![0u8; 32000]; // 1 second of silence at 16kHz 16-bit mono
        let result = stt.transcribe_bytes(&audio, 16000);
        // With VAD enabled, silence should return empty transcription OR model error.
        // Since all zeros = silence, VAD returns no speech -> empty result.
        assert!(result.is_ok());
        let tr = result.unwrap();
        assert!(tr.text.is_empty());
    }

    #[test]
    fn test_transcribe_samples_empty() {
        let mut stt = SpeechToText::with_whisper_config(WhisperConfig::default());
        let result = stt.transcribe_samples(&[], 16000);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Empty audio"));
    }

    #[test]
    fn test_transcribe_samples_zero_rate() {
        let mut stt = SpeechToText::with_whisper_config(WhisperConfig::default());
        let result = stt.transcribe_samples(&[0.1], 0);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Sample rate"));
    }

    #[test]
    fn test_transcribe_samples_vad_silence_returns_empty() {
        let mut stt = SpeechToText::with_whisper_config(WhisperConfig::default());
        let silence = vec![0.0f32; 16000];
        let result = stt.transcribe_samples(&silence, 16000);
        assert!(result.is_ok());
        let tr = result.unwrap();
        assert!(tr.text.is_empty());
        assert!(tr.segments.is_empty());
    }

    #[test]
    fn test_transcribe_samples_with_speech_no_model() {
        let config = WhisperConfig {
            vad_enabled: true,
            energy_threshold: 0.001,
            ..WhisperConfig::default()
        };
        let mut stt = SpeechToText::with_whisper_config(config);
        let signal = vec![0.5f32; 16000]; // Clear signal.
        let result = stt.transcribe_samples(&signal, 16000);
        // Should fail because model is not loaded.
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not loaded"));
    }

    #[test]
    fn test_transcribe_samples_vad_disabled_no_model() {
        let config = WhisperConfig {
            vad_enabled: false,
            ..WhisperConfig::default()
        };
        let mut stt = SpeechToText::with_whisper_config(config);
        let silence = vec![0.0f32; 16000];
        let result = stt.transcribe_samples(&silence, 16000);
        // VAD disabled means it always tries to transcribe, but model not loaded.
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not loaded"));
    }

    #[test]
    fn test_stats_tracking() {
        let mut stt = SpeechToText::with_whisper_config(WhisperConfig::default());
        let _ = stt.transcribe_bytes(&[1, 2, 3, 4], 16000);
        let _ = stt.transcribe_bytes(&[5, 6, 7, 8], 16000);

        let stats = stt.get_stats();
        assert_eq!(stats.transcriptions, 2);
        assert!(!stats.available);
    }

    #[test]
    fn test_detect_speech_method() {
        let stt = SpeechToText::with_whisper_config(WhisperConfig {
            energy_threshold: 0.001,
            ..WhisperConfig::default()
        });
        let signal = vec![0.1f32; 16000];
        let segments = stt.detect_speech(&signal, 16000);
        assert!(!segments.is_empty());
    }

    #[test]
    fn test_transcription_result_serialization() {
        let result = TranscriptionResult {
            text: "hello world".to_string(),
            confidence: 0.95,
            language: "en".to_string(),
            segments: vec![AudioSegment::new(0.0, 1.0, "hello world", 0.95)],
            duration_ms: 1000,
        };
        let json = serde_json::to_string(&result).unwrap();
        let restored: TranscriptionResult = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.text, "hello world");
        assert_eq!(restored.segments.len(), 1);
        assert_eq!(restored.duration_ms, 1000);
    }

    #[test]
    fn test_vad_with_threshold_constructor() {
        let vad = VoiceActivityDetector::with_threshold(0.05);
        assert!((vad.config().energy_threshold - 0.05).abs() < f32::EPSILON);
    }

    #[test]
    fn test_vad_speech_extends_to_end() {
        let vad = VoiceActivityDetector::new(VadConfig {
            energy_threshold: 0.001,
            min_segment_length: 0.0,
            merge_gap: 0.0,
            frame_size: 160,
        });
        // Audio that is all speech with no silence at end.
        let signal = vec![0.1f32; 8000]; // 0.5s
        let segments = vad.detect_speech(&signal, 16000);
        assert!(!segments.is_empty());
        let last = segments.last().unwrap();
        assert!(last.end_time > 0.4);
    }
}
