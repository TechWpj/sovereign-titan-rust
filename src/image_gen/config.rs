//! Image Generation Configuration.

use serde::{Deserialize, Serialize};

/// Configuration for image generation requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageGenerationConfig {
    pub model: String,
    pub width: u32,
    pub height: u32,
    pub num_inference_steps: u32,
    pub guidance_scale: f64,
    pub negative_prompt: String,
}

impl Default for ImageGenerationConfig {
    fn default() -> Self {
        Self {
            model: "stable-diffusion-xl".to_string(),
            width: 1024,
            height: 1024,
            num_inference_steps: 30,
            guidance_scale: 7.5,
            negative_prompt: String::new(),
        }
    }
}

impl ImageGenerationConfig {
    /// Create config with custom dimensions.
    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Set the model name.
    pub fn with_model(mut self, model: &str) -> Self {
        self.model = model.to_string();
        self
    }

    /// Set guidance scale.
    pub fn with_guidance(mut self, scale: f64) -> Self {
        self.guidance_scale = scale;
        self
    }

    /// Pixel count.
    pub fn pixel_count(&self) -> u64 {
        self.width as u64 * self.height as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default() {
        let cfg = ImageGenerationConfig::default();
        assert_eq!(cfg.width, 1024);
        assert_eq!(cfg.height, 1024);
        assert_eq!(cfg.num_inference_steps, 30);
        assert!((cfg.guidance_scale - 7.5).abs() < 0.01);
    }

    #[test]
    fn test_with_size() {
        let cfg = ImageGenerationConfig::default().with_size(512, 512);
        assert_eq!(cfg.width, 512);
        assert_eq!(cfg.height, 512);
    }

    #[test]
    fn test_with_model() {
        let cfg = ImageGenerationConfig::default().with_model("dall-e-3");
        assert_eq!(cfg.model, "dall-e-3");
    }

    #[test]
    fn test_pixel_count() {
        let cfg = ImageGenerationConfig::default();
        assert_eq!(cfg.pixel_count(), 1024 * 1024);
    }

    #[test]
    fn test_serialization() {
        let cfg = ImageGenerationConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: ImageGenerationConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.width, cfg.width);
    }
}
