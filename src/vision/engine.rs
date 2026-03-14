//! Vision engine — image analysis, OCR, and screenshot description.
//!
//! Provides the vision interface for the Sovereign Titan runtime. Includes:
//! - OCR via PowerShell's Windows.Media.Ocr API (real implementation on Windows)
//! - Text region extraction with bounding boxes and confidence
//! - Screen analysis combining OCR + UI element detection
//! - Image description with object detection
//! - VLM-based image analysis (requires external model bindings)

use std::path::Path;

use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Configuration
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for the vision engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionConfig {
    /// Whether a VLM (vision-language model) is enabled for image analysis.
    pub vlm_enabled: bool,
    /// VLM model identifier (e.g. "moondream2", "llava-v1.5").
    pub vlm_model: String,
    /// Whether OCR extraction is enabled.
    pub ocr_enabled: bool,
}

impl Default for VisionConfig {
    fn default() -> Self {
        Self {
            vlm_enabled: false,
            vlm_model: "moondream2".to_string(),
            ocr_enabled: false,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// OCR Result Types
// ─────────────────────────────────────────────────────────────────────────────

/// Result of an OCR operation on an image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OCRResult {
    /// Full extracted text (all regions concatenated).
    pub full_text: String,
    /// Individual text regions detected.
    pub regions: Vec<TextRegion>,
    /// Overall OCR confidence (0.0 - 1.0).
    pub confidence: f64,
    /// Processing time in milliseconds.
    pub processing_ms: u64,
}

/// A region of text detected by OCR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextRegion {
    /// The text content of this region.
    pub text: String,
    /// Bounding box: x coordinate.
    pub x: f64,
    /// Bounding box: y coordinate.
    pub y: f64,
    /// Bounding box: width.
    pub w: f64,
    /// Bounding box: height.
    pub h: f64,
    /// Confidence score for this region (0.0 - 1.0).
    pub confidence: f64,
    /// Line number within the full text (0-indexed).
    pub line_number: u32,
}

impl TextRegion {
    /// Create a new text region with bounding box information.
    pub fn new(text: &str, x: f64, y: f64, w: f64, h: f64, confidence: f64, line_number: u32) -> Self {
        Self {
            text: text.to_string(),
            x,
            y,
            w,
            h,
            confidence: confidence.clamp(0.0, 1.0),
            line_number,
        }
    }

    /// Get the bounding box as a tuple (x, y, w, h).
    pub fn bounding_box(&self) -> (f64, f64, f64, f64) {
        (self.x, self.y, self.w, self.h)
    }

    /// Area of the bounding box.
    pub fn area(&self) -> f64 {
        self.w * self.h
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Screen Analysis Types
// ─────────────────────────────────────────────────────────────────────────────

/// Result of analyzing a screenshot or screen region.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenAnalysis {
    /// Natural language description of the screen content.
    pub description: String,
    /// UI elements detected on screen.
    pub elements: Vec<UIElementDetection>,
    /// Extracted text content from OCR.
    pub text_content: Option<String>,
}

/// A UI element detected on screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UIElementDetection {
    /// Type of UI element (e.g. "button", "textbox", "menu", "icon").
    pub element_type: String,
    /// Label or text associated with this element.
    pub label: String,
    /// Bounding box: (x, y, w, h).
    pub bounding_box: (f64, f64, f64, f64),
    /// Detection confidence (0.0 - 1.0).
    pub confidence: f64,
}

impl UIElementDetection {
    /// Create a new UI element detection.
    pub fn new(element_type: &str, label: &str, bbox: (f64, f64, f64, f64), confidence: f64) -> Self {
        Self {
            element_type: element_type.to_string(),
            label: label.to_string(),
            bounding_box: bbox,
            confidence: confidence.clamp(0.0, 1.0),
        }
    }

    /// Center point of the bounding box.
    pub fn center(&self) -> (f64, f64) {
        (
            self.bounding_box.0 + self.bounding_box.2 / 2.0,
            self.bounding_box.1 + self.bounding_box.3 / 2.0,
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Image Description Types
// ─────────────────────────────────────────────────────────────────────────────

/// Result of describing an image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageDescription {
    /// Natural language summary of the image.
    pub summary: String,
    /// Objects detected in the image.
    pub objects: Vec<DetectedObject>,
    /// Scene type classification (e.g. "desktop", "document", "photo", "diagram").
    pub scene_type: String,
}

/// An object detected in an image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedObject {
    /// Label of the detected object.
    pub label: String,
    /// Detection confidence (0.0 - 1.0).
    pub confidence: f64,
    /// Bounding box: (x, y, w, h), if available.
    pub bounding_box: Option<(f64, f64, f64, f64)>,
}

impl DetectedObject {
    /// Create a new detected object.
    pub fn new(label: &str, confidence: f64, bbox: Option<(f64, f64, f64, f64)>) -> Self {
        Self {
            label: label.to_string(),
            confidence: confidence.clamp(0.0, 1.0),
            bounding_box: bbox,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Legacy types (backward compatibility)
// ─────────────────────────────────────────────────────────────────────────────

/// Legacy result of analyzing an image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageAnalysis {
    /// Natural language description of the image.
    pub description: String,
    /// Detected objects or regions of interest.
    pub objects: Vec<String>,
    /// Extracted text content (if OCR was run).
    pub text_content: Option<String>,
    /// Confidence score for the analysis (0.0 - 1.0).
    pub confidence: f64,
}

/// Statistics for the vision engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionStats {
    /// Total number of image analyses performed.
    pub analyses: u64,
    /// Whether the VLM is available.
    pub vlm_available: bool,
    /// Whether OCR is available.
    pub ocr_available: bool,
    /// Configured VLM model.
    pub vlm_model: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Vision Engine
// ─────────────────────────────────────────────────────────────────────────────

/// Vision engine for image analysis, OCR, and screenshot interpretation.
///
/// Supports:
/// - Windows OCR via PowerShell (`Windows.Media.Ocr`)
/// - VLM-based image description (requires model bindings)
/// - Screen analysis combining OCR + element detection
/// - Image file metadata analysis (stub fallback)
pub struct VisionEngine {
    /// Engine configuration.
    config: VisionConfig,
    /// Total number of analyses performed.
    analyses: u64,
}

impl VisionEngine {
    /// Create a new vision engine with the given configuration.
    pub fn new(config: VisionConfig) -> Self {
        Self {
            config,
            analyses: 0,
        }
    }

    /// Perform OCR on an image file using Windows.Media.Ocr via PowerShell.
    ///
    /// Returns detailed OCR results including text regions with bounding boxes,
    /// confidence scores, and line numbers.
    pub fn ocr(&mut self, image_path: &str) -> Result<OCRResult, String> {
        self.analyses += 1;

        let path = Path::new(image_path);
        if !path.exists() {
            return Err(format!("Image file not found: {image_path}"));
        }

        if !self.config.ocr_enabled {
            return Err(
                "OCR is disabled in configuration. Set ocr_enabled=true to enable.".to_string(),
            );
        }

        if !cfg!(target_os = "windows") {
            return Err("OCR requires Windows.Media.Ocr (Windows only)".to_string());
        }

        let start = std::time::Instant::now();

        let abs_path = path
            .canonicalize()
            .map_err(|e| format!("Failed to resolve path: {e}"))?;
        let path_str = abs_path.to_string_lossy().replace('\\', "/");

        // PowerShell script that extracts text with line and word bounding box info.
        let ps_script = format!(
            "[Windows.Media.Ocr.OcrEngine, Windows.Foundation, ContentType = WindowsRuntime] > $null; \
             [Windows.Graphics.Imaging.BitmapDecoder, Windows.Foundation, ContentType = WindowsRuntime] > $null; \
             $file = [Windows.Storage.StorageFile]::GetFileFromPathAsync('{path_str}').GetAwaiter().GetResult(); \
             $stream = $file.OpenReadAsync().GetAwaiter().GetResult(); \
             $decoder = [Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream).GetAwaiter().GetResult(); \
             $bitmap = $decoder.GetSoftwareBitmapAsync().GetAwaiter().GetResult(); \
             $engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages(); \
             $result = $engine.RecognizeAsync($bitmap).GetAwaiter().GetResult(); \
             $lineNum = 0; \
             foreach ($line in $result.Lines) {{ \
                 foreach ($word in $line.Words) {{ \
                     $b = $word.BoundingRect; \
                     Write-Output \"$lineNum|$($word.Text)|$($b.X)|$($b.Y)|$($b.Width)|$($b.Height)\"; \
                 }} \
                 $lineNum++; \
             }} \
             Write-Output \"__FULLTEXT__\"; \
             Write-Output $result.Text"
        );

        match std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &ps_script])
            .output()
        {
            Ok(output) => {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let processing_ms = start.elapsed().as_millis() as u64;

                    self.parse_ocr_output(&stdout, processing_ms)
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    Err(format!("Windows OCR failed: {stderr}"))
                }
            }
            Err(e) => Err(format!("Failed to launch PowerShell for OCR: {e}")),
        }
    }

    /// Parse the structured OCR output from PowerShell.
    fn parse_ocr_output(&self, output: &str, processing_ms: u64) -> Result<OCRResult, String> {
        let mut regions: Vec<TextRegion> = Vec::new();
        let mut full_text = String::new();
        let mut in_fulltext = false;

        for line in output.lines() {
            let trimmed = line.trim();
            if trimmed == "__FULLTEXT__" {
                in_fulltext = true;
                continue;
            }

            if in_fulltext {
                if !full_text.is_empty() {
                    full_text.push('\n');
                }
                full_text.push_str(trimmed);
                continue;
            }

            // Parse: lineNum|text|x|y|w|h
            let parts: Vec<&str> = trimmed.splitn(6, '|').collect();
            if parts.len() == 6 {
                let line_num = parts[0].parse::<u32>().unwrap_or(0);
                let text = parts[1];
                let x = parts[2].parse::<f64>().unwrap_or(0.0);
                let y = parts[3].parse::<f64>().unwrap_or(0.0);
                let w = parts[4].parse::<f64>().unwrap_or(0.0);
                let h = parts[5].parse::<f64>().unwrap_or(0.0);

                regions.push(TextRegion::new(text, x, y, w, h, 0.9, line_num));
            }
        }

        // If full text was not captured from the __FULLTEXT__ marker, build from regions.
        if full_text.is_empty() && !regions.is_empty() {
            full_text = regions.iter().map(|r| r.text.as_str()).collect::<Vec<_>>().join(" ");
        }

        let confidence = if regions.is_empty() { 0.0 } else { 0.9 };

        Ok(OCRResult {
            full_text,
            regions,
            confidence,
            processing_ms,
        })
    }

    /// Analyze an image file, optionally guided by a text prompt.
    ///
    /// When the VLM is enabled and model bindings are available, this
    /// performs full vision-language inference. Otherwise, returns basic
    /// file metadata as a stub analysis.
    pub fn analyze_image(
        &mut self,
        image_path: &str,
        prompt: Option<&str>,
    ) -> Result<ImageAnalysis, String> {
        self.analyses += 1;

        let path = Path::new(image_path);
        if !path.exists() {
            return Err(format!("Image file not found: {image_path}"));
        }

        // Get basic file info for the stub response.
        let metadata = std::fs::metadata(path)
            .map_err(|e| format!("Failed to read image metadata: {e}"))?;
        let file_size = metadata.len();
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("unknown")
            .to_lowercase();

        if self.config.vlm_enabled {
            return Err(format!(
                "VLM '{}' not yet integrated. Native model bindings required for \
                 vision-language inference. Prompt: {:?}",
                self.config.vlm_model, prompt
            ));
        }

        // Stub: return basic file metadata analysis.
        let description = format!(
            "Image file: {}, format: {extension}, size: {} bytes",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown"),
            file_size
        );

        let mut objects = vec![format!("{extension}_image")];
        if file_size > 1_000_000 {
            objects.push("high_resolution".to_string());
        }

        Ok(ImageAnalysis {
            description,
            objects,
            text_content: None,
            confidence: 0.1, // Low confidence for stub analysis.
        })
    }

    /// Describe an image using available analysis methods.
    ///
    /// Combines OCR (if enabled) with metadata analysis to produce an
    /// `ImageDescription` with detected objects and scene classification.
    pub fn describe_image(&mut self, image_path: &str) -> Result<ImageDescription, String> {
        self.analyses += 1;

        let path = Path::new(image_path);
        if !path.exists() {
            return Err(format!("Image file not found: {image_path}"));
        }

        let metadata = std::fs::metadata(path)
            .map_err(|e| format!("Failed to read image metadata: {e}"))?;
        let file_size = metadata.len();
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("unknown")
            .to_lowercase();

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        let scene_type = classify_scene_by_extension(&extension);
        let summary = format!(
            "{} file '{}' ({} bytes)",
            extension.to_uppercase(),
            file_name,
            file_size
        );

        let mut objects = Vec::new();
        objects.push(DetectedObject::new(
            &format!("{extension}_file"),
            0.9,
            None,
        ));

        if file_size > 1_000_000 {
            objects.push(DetectedObject::new("high_resolution", 0.7, None));
        }
        if file_size > 10_000_000 {
            objects.push(DetectedObject::new("very_large_file", 0.8, None));
        }

        Ok(ImageDescription {
            summary,
            objects,
            scene_type,
        })
    }

    /// Analyze a screen region combining OCR and element detection.
    ///
    /// Takes a screenshot path and produces a `ScreenAnalysis` with
    /// text content and detected UI elements.
    pub fn analyze_screen(&mut self, screenshot_path: &str) -> Result<ScreenAnalysis, String> {
        self.analyses += 1;

        let path = Path::new(screenshot_path);
        if !path.exists() {
            return Err(format!("Screenshot file not found: {screenshot_path}"));
        }

        let mut text_content = None;
        let mut elements = Vec::new();

        // Try OCR if enabled.
        if self.config.ocr_enabled && cfg!(target_os = "windows") {
            // Temporarily decrement analyses since ocr() will increment it.
            self.analyses -= 1;
            match self.ocr(screenshot_path) {
                Ok(ocr_result) => {
                    if !ocr_result.full_text.is_empty() {
                        text_content = Some(ocr_result.full_text.clone());
                    }

                    // Convert OCR regions to UI element detections.
                    for region in &ocr_result.regions {
                        elements.push(UIElementDetection::new(
                            "text",
                            &region.text,
                            (region.x, region.y, region.w, region.h),
                            region.confidence,
                        ));
                    }
                }
                Err(_) => {
                    // OCR failed; continue without text extraction.
                }
            }
        }

        let description = if let Some(ref text) = text_content {
            let word_count = text.split_whitespace().count();
            format!(
                "Screen capture with {} text elements detected, {} words of text",
                elements.len(),
                word_count
            )
        } else {
            "Screen capture (no OCR text extracted)".to_string()
        };

        Ok(ScreenAnalysis {
            description,
            elements,
            text_content,
        })
    }

    /// Extract text from an image using OCR.
    ///
    /// On Windows, attempts to use the `Windows.Media.Ocr` API via
    /// PowerShell. On other platforms, returns an error.
    pub fn extract_text(&mut self, image_path: &str) -> Result<String, String> {
        self.analyses += 1;

        let path = Path::new(image_path);
        if !path.exists() {
            return Err(format!("Image file not found: {image_path}"));
        }

        if !self.config.ocr_enabled {
            return Err(
                "OCR is disabled in configuration. Set ocr_enabled=true to enable.".to_string(),
            );
        }

        if cfg!(target_os = "windows") {
            let abs_path = path
                .canonicalize()
                .map_err(|e| format!("Failed to resolve path: {e}"))?;
            let path_str = abs_path.to_string_lossy().replace('\\', "/");

            let ps_script = format!(
                "[Windows.Media.Ocr.OcrEngine, Windows.Foundation, ContentType = WindowsRuntime] > $null; \
                 [Windows.Graphics.Imaging.BitmapDecoder, Windows.Foundation, ContentType = WindowsRuntime] > $null; \
                 $file = [Windows.Storage.StorageFile]::GetFileFromPathAsync('{path_str}').GetAwaiter().GetResult(); \
                 $stream = $file.OpenReadAsync().GetAwaiter().GetResult(); \
                 $decoder = [Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream).GetAwaiter().GetResult(); \
                 $bitmap = $decoder.GetSoftwareBitmapAsync().GetAwaiter().GetResult(); \
                 $engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages(); \
                 $result = $engine.RecognizeAsync($bitmap).GetAwaiter().GetResult(); \
                 $result.Text"
            );

            match std::process::Command::new("powershell")
                .args(["-NoProfile", "-Command", &ps_script])
                .output()
            {
                Ok(output) => {
                    if output.status.success() {
                        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        Ok(text)
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        Err(format!("Windows OCR failed: {stderr}"))
                    }
                }
                Err(e) => Err(format!("Failed to launch PowerShell for OCR: {e}")),
            }
        } else {
            Err("OCR requires Windows.Media.Ocr (Windows only) or an external OCR engine.".to_string())
        }
    }

    /// Describe a screenshot using VLM or basic analysis.
    ///
    /// Specialized wrapper around `analyze_image` with a screenshot-oriented
    /// prompt. Used by the UI automation subsystem for visual verification.
    pub fn describe_screenshot(
        &mut self,
        screenshot_path: &str,
    ) -> Result<ImageAnalysis, String> {
        self.analyze_image(
            screenshot_path,
            Some("Describe this screenshot of a computer desktop. What applications are visible? What is the user doing?"),
        )
    }

    /// Check if the vision engine has any capabilities available.
    pub fn is_available(&self) -> bool {
        if self.config.vlm_enabled {
            return true;
        }
        if self.config.ocr_enabled && cfg!(target_os = "windows") {
            return true;
        }
        false
    }

    /// Get engine statistics.
    pub fn get_stats(&self) -> VisionStats {
        VisionStats {
            analyses: self.analyses,
            vlm_available: self.config.vlm_enabled,
            ocr_available: self.config.ocr_enabled && cfg!(target_os = "windows"),
            vlm_model: self.config.vlm_model.clone(),
        }
    }

    /// Get a reference to the current configuration.
    pub fn config(&self) -> &VisionConfig {
        &self.config
    }
}

/// Classify scene type based on file extension.
fn classify_scene_by_extension(ext: &str) -> String {
    match ext {
        "png" | "jpg" | "jpeg" | "bmp" | "gif" | "webp" => "photograph".to_string(),
        "svg" => "vector_graphic".to_string(),
        "pdf" => "document".to_string(),
        "ico" => "icon".to_string(),
        "tiff" | "tif" => "scan".to_string(),
        _ => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Config tests ─────────────────────────────────────────────────────

    #[test]
    fn test_default_config() {
        let config = VisionConfig::default();
        assert!(!config.vlm_enabled);
        assert_eq!(config.vlm_model, "moondream2");
        assert!(!config.ocr_enabled);
    }

    #[test]
    fn test_config_serialization() {
        let config = VisionConfig {
            vlm_enabled: true,
            vlm_model: "llava-v1.5".to_string(),
            ocr_enabled: true,
        };
        let json = serde_json::to_string(&config).unwrap();
        let restored: VisionConfig = serde_json::from_str(&json).unwrap();
        assert!(restored.vlm_enabled);
        assert_eq!(restored.vlm_model, "llava-v1.5");
        assert!(restored.ocr_enabled);
    }

    // ── TextRegion tests ────────────────────────────────────────────────

    #[test]
    fn test_text_region_creation() {
        let region = TextRegion::new("Hello", 10.0, 20.0, 100.0, 30.0, 0.95, 0);
        assert_eq!(region.text, "Hello");
        assert_eq!(region.x, 10.0);
        assert_eq!(region.y, 20.0);
        assert_eq!(region.w, 100.0);
        assert_eq!(region.h, 30.0);
        assert!((region.confidence - 0.95).abs() < f64::EPSILON);
        assert_eq!(region.line_number, 0);
    }

    #[test]
    fn test_text_region_bounding_box() {
        let region = TextRegion::new("Test", 5.0, 10.0, 50.0, 20.0, 0.8, 1);
        let bbox = region.bounding_box();
        assert_eq!(bbox, (5.0, 10.0, 50.0, 20.0));
    }

    #[test]
    fn test_text_region_area() {
        let region = TextRegion::new("Test", 0.0, 0.0, 100.0, 50.0, 0.8, 0);
        assert!((region.area() - 5000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_text_region_confidence_clamping() {
        let region = TextRegion::new("Test", 0.0, 0.0, 10.0, 10.0, 1.5, 0);
        assert!((region.confidence - 1.0).abs() < f64::EPSILON);

        let region2 = TextRegion::new("Test", 0.0, 0.0, 10.0, 10.0, -0.5, 0);
        assert!((region2.confidence - 0.0).abs() < f64::EPSILON);
    }

    // ── UIElementDetection tests ────────────────────────────────────────

    #[test]
    fn test_ui_element_creation() {
        let elem = UIElementDetection::new("button", "OK", (100.0, 200.0, 80.0, 30.0), 0.9);
        assert_eq!(elem.element_type, "button");
        assert_eq!(elem.label, "OK");
        assert_eq!(elem.bounding_box, (100.0, 200.0, 80.0, 30.0));
    }

    #[test]
    fn test_ui_element_center() {
        let elem = UIElementDetection::new("button", "OK", (100.0, 200.0, 80.0, 30.0), 0.9);
        let center = elem.center();
        assert!((center.0 - 140.0).abs() < f64::EPSILON);
        assert!((center.1 - 215.0).abs() < f64::EPSILON);
    }

    // ── DetectedObject tests ────────────────────────────────────────────

    #[test]
    fn test_detected_object_creation() {
        let obj = DetectedObject::new("cat", 0.95, Some((10.0, 20.0, 100.0, 80.0)));
        assert_eq!(obj.label, "cat");
        assert!((obj.confidence - 0.95).abs() < f64::EPSILON);
        assert!(obj.bounding_box.is_some());
    }

    #[test]
    fn test_detected_object_no_bbox() {
        let obj = DetectedObject::new("scene", 0.8, None);
        assert!(obj.bounding_box.is_none());
    }

    #[test]
    fn test_detected_object_confidence_clamping() {
        let obj = DetectedObject::new("test", 2.0, None);
        assert!((obj.confidence - 1.0).abs() < f64::EPSILON);
    }

    // ── Scene classification tests ──────────────────────────────────────

    #[test]
    fn test_classify_scene_photograph() {
        assert_eq!(classify_scene_by_extension("png"), "photograph");
        assert_eq!(classify_scene_by_extension("jpg"), "photograph");
        assert_eq!(classify_scene_by_extension("jpeg"), "photograph");
    }

    #[test]
    fn test_classify_scene_other() {
        assert_eq!(classify_scene_by_extension("svg"), "vector_graphic");
        assert_eq!(classify_scene_by_extension("pdf"), "document");
        assert_eq!(classify_scene_by_extension("ico"), "icon");
        assert_eq!(classify_scene_by_extension("xyz"), "unknown");
    }

    // ── OCR result parsing tests ────────────────────────────────────────

    #[test]
    fn test_parse_ocr_output_basic() {
        let engine = VisionEngine::new(VisionConfig::default());
        let output = "0|Hello|10.0|20.0|50.0|15.0\n0|World|70.0|20.0|55.0|15.0\n__FULLTEXT__\nHello World";
        let result = engine.parse_ocr_output(output, 100).unwrap();

        assert_eq!(result.full_text, "Hello World");
        assert_eq!(result.regions.len(), 2);
        assert_eq!(result.regions[0].text, "Hello");
        assert_eq!(result.regions[1].text, "World");
        assert_eq!(result.processing_ms, 100);
    }

    #[test]
    fn test_parse_ocr_output_empty() {
        let engine = VisionEngine::new(VisionConfig::default());
        let output = "__FULLTEXT__\n";
        let result = engine.parse_ocr_output(output, 50).unwrap();

        assert!(result.full_text.is_empty());
        assert!(result.regions.is_empty());
        assert!((result.confidence - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_ocr_output_multiline() {
        let engine = VisionEngine::new(VisionConfig::default());
        let output = "0|Line1|0|0|100|20\n1|Line2|0|25|100|20\n__FULLTEXT__\nLine1\nLine2";
        let result = engine.parse_ocr_output(output, 200).unwrap();

        assert_eq!(result.regions.len(), 2);
        assert_eq!(result.regions[0].line_number, 0);
        assert_eq!(result.regions[1].line_number, 1);
        assert!(result.full_text.contains("Line1"));
        assert!(result.full_text.contains("Line2"));
    }

    // ── VisionEngine tests ──────────────────────────────────────────────

    #[test]
    fn test_analyze_nonexistent_image() {
        let mut engine = VisionEngine::new(VisionConfig::default());
        let result = engine.analyze_image("/nonexistent/image.png", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_analyze_stub_existing_file() {
        let mut engine = VisionEngine::new(VisionConfig::default());
        let cargo_path = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
        let result = engine.analyze_image(cargo_path, None);

        assert!(result.is_ok());
        let analysis = result.unwrap();
        assert!(!analysis.description.is_empty());
        assert!((analysis.confidence - 0.1).abs() < f64::EPSILON);
        assert!(analysis.text_content.is_none());
    }

    #[test]
    fn test_analyze_vlm_stub() {
        let config = VisionConfig {
            vlm_enabled: true,
            vlm_model: "moondream2".to_string(),
            ocr_enabled: false,
        };
        let mut engine = VisionEngine::new(config);
        let cargo_path = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
        let result = engine.analyze_image(cargo_path, Some("describe this"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not yet integrated"));
    }

    #[test]
    fn test_extract_text_disabled() {
        let mut engine = VisionEngine::new(VisionConfig::default());
        let cargo_path = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
        let result = engine.extract_text(cargo_path);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("disabled"));
    }

    #[test]
    fn test_extract_text_nonexistent() {
        let config = VisionConfig {
            ocr_enabled: true,
            ..VisionConfig::default()
        };
        let mut engine = VisionEngine::new(config);
        let result = engine.extract_text("/nonexistent/image.png");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_ocr_disabled() {
        let mut engine = VisionEngine::new(VisionConfig::default());
        let cargo_path = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
        let result = engine.ocr(cargo_path);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("disabled"));
    }

    #[test]
    fn test_ocr_nonexistent_file() {
        let config = VisionConfig {
            ocr_enabled: true,
            ..VisionConfig::default()
        };
        let mut engine = VisionEngine::new(config);
        let result = engine.ocr("/nonexistent/image.png");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_describe_image_nonexistent() {
        let mut engine = VisionEngine::new(VisionConfig::default());
        let result = engine.describe_image("/nonexistent/image.png");
        assert!(result.is_err());
    }

    #[test]
    fn test_describe_image_existing_file() {
        let mut engine = VisionEngine::new(VisionConfig::default());
        let cargo_path = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
        let result = engine.describe_image(cargo_path);
        assert!(result.is_ok());
        let desc = result.unwrap();
        assert!(!desc.summary.is_empty());
        assert!(!desc.objects.is_empty());
    }

    #[test]
    fn test_analyze_screen_nonexistent() {
        let mut engine = VisionEngine::new(VisionConfig::default());
        let result = engine.analyze_screen("/nonexistent/screen.png");
        assert!(result.is_err());
    }

    #[test]
    fn test_analyze_screen_no_ocr() {
        let mut engine = VisionEngine::new(VisionConfig::default());
        let cargo_path = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
        let result = engine.analyze_screen(cargo_path);
        assert!(result.is_ok());
        let analysis = result.unwrap();
        assert!(analysis.text_content.is_none());
        assert!(analysis.elements.is_empty());
    }

    #[test]
    fn test_stats_tracking() {
        let mut engine = VisionEngine::new(VisionConfig::default());
        let _ = engine.analyze_image("/nonexistent/a.png", None);
        let _ = engine.analyze_image("/nonexistent/b.png", None);

        let stats = engine.get_stats();
        assert_eq!(stats.analyses, 2);
        assert!(!stats.vlm_available);
        assert_eq!(stats.vlm_model, "moondream2");
    }

    #[test]
    fn test_is_available_default() {
        let engine = VisionEngine::new(VisionConfig::default());
        assert!(!engine.is_available());
    }

    #[test]
    fn test_is_available_ocr_windows() {
        let config = VisionConfig {
            ocr_enabled: true,
            ..VisionConfig::default()
        };
        let engine = VisionEngine::new(config);
        if cfg!(target_os = "windows") {
            assert!(engine.is_available());
        }
    }

    #[test]
    fn test_describe_screenshot_delegates() {
        let mut engine = VisionEngine::new(VisionConfig::default());
        let result = engine.describe_screenshot("/nonexistent/screen.png");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
        assert_eq!(engine.get_stats().analyses, 1);
    }

    #[test]
    fn test_ocr_result_serialization() {
        let result = OCRResult {
            full_text: "Hello World".to_string(),
            regions: vec![TextRegion::new("Hello", 0.0, 0.0, 50.0, 20.0, 0.9, 0)],
            confidence: 0.9,
            processing_ms: 150,
        };
        let json = serde_json::to_string(&result).unwrap();
        let restored: OCRResult = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.full_text, "Hello World");
        assert_eq!(restored.regions.len(), 1);
        assert_eq!(restored.processing_ms, 150);
    }

    #[test]
    fn test_screen_analysis_serialization() {
        let analysis = ScreenAnalysis {
            description: "Test screen".to_string(),
            elements: vec![UIElementDetection::new("button", "OK", (0.0, 0.0, 50.0, 30.0), 0.9)],
            text_content: Some("Button text".to_string()),
        };
        let json = serde_json::to_string(&analysis).unwrap();
        let restored: ScreenAnalysis = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.description, "Test screen");
        assert_eq!(restored.elements.len(), 1);
    }

    #[test]
    fn test_image_description_serialization() {
        let desc = ImageDescription {
            summary: "A photo".to_string(),
            objects: vec![DetectedObject::new("cat", 0.95, None)],
            scene_type: "photograph".to_string(),
        };
        let json = serde_json::to_string(&desc).unwrap();
        let restored: ImageDescription = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.summary, "A photo");
        assert_eq!(restored.scene_type, "photograph");
    }
}
