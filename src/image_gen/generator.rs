//! Image Generator — multi-backend image generation.
//!
//! Supports OpenAI (DALL-E), Stability AI, and local diffusers backends.
//! Currently implemented as API stubs — actual generation requires HTTP calls.

use serde::{Deserialize, Serialize};

use super::config::ImageGenerationConfig;

/// Image generation backend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ImageBackend {
    OpenAI,
    StabilityAI,
    None,
}

/// Result of image generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationResult {
    pub image_path: String,
    pub width: u32,
    pub height: u32,
    pub backend: String,
    pub generation_time_ms: f64,
}

/// Generator status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratorStatus {
    pub backend: String,
    pub initialized: bool,
    pub has_api_key: bool,
    pub generations: u64,
}

/// Multi-backend image generator.
pub struct ImageGenerator {
    backend: ImageBackend,
    api_key: Option<String>,
    initialized: bool,
    generations: u64,
}

impl ImageGenerator {
    /// Create a new image generator with specified backend.
    pub fn new(backend: ImageBackend) -> Self {
        Self {
            backend,
            api_key: None,
            initialized: false,
            generations: 0,
        }
    }

    /// Auto-detect backend from environment variables.
    pub fn detect_backend() -> ImageBackend {
        if std::env::var("OPENAI_API_KEY").is_ok() {
            ImageBackend::OpenAI
        } else if std::env::var("STABILITY_API_KEY").is_ok() {
            ImageBackend::StabilityAI
        } else {
            ImageBackend::None
        }
    }

    /// Initialize the generator.
    pub fn initialize(&mut self) -> Result<(), String> {
        match self.backend {
            ImageBackend::OpenAI => {
                self.api_key = std::env::var("OPENAI_API_KEY").ok();
                if self.api_key.is_none() {
                    return Err("OPENAI_API_KEY not set".to_string());
                }
                self.initialized = true;
                Ok(())
            }
            ImageBackend::StabilityAI => {
                self.api_key = std::env::var("STABILITY_API_KEY").ok();
                if self.api_key.is_none() {
                    return Err("STABILITY_API_KEY not set".to_string());
                }
                self.initialized = true;
                Ok(())
            }
            ImageBackend::None => {
                Err("No image generation backend available".to_string())
            }
        }
    }

    /// Generate an image from a text prompt.
    pub fn generate(
        &mut self,
        prompt: &str,
        config: Option<&ImageGenerationConfig>,
        output_path: Option<&str>,
    ) -> Result<GenerationResult, String> {
        if !self.initialized {
            return Err("Generator not initialized. Call initialize() first.".to_string());
        }
        if prompt.is_empty() {
            return Err("Prompt cannot be empty.".to_string());
        }

        let cfg = config.cloned().unwrap_or_default();
        let path = output_path
            .map(String::from)
            .unwrap_or_else(|| format!("generated_{}.png", self.generations));

        // Stub: actual API calls would go here
        self.generations += 1;

        Ok(GenerationResult {
            image_path: path,
            width: cfg.width,
            height: cfg.height,
            backend: format!("{:?}", self.backend),
            generation_time_ms: 0.0,
        })
    }

    /// Enhance a simple prompt with quality descriptors.
    pub fn enhance_prompt(simple: &str) -> String {
        if simple.is_empty() {
            return String::new();
        }
        format!(
            "{simple}, highly detailed, professional quality, \
             sharp focus, beautiful lighting, 8k resolution"
        )
    }

    /// Check if the generator is available.
    pub fn is_available(&self) -> bool {
        self.initialized && self.backend != ImageBackend::None
    }

    /// Get generator status.
    pub fn get_status(&self) -> GeneratorStatus {
        GeneratorStatus {
            backend: format!("{:?}", self.backend),
            initialized: self.initialized,
            has_api_key: self.api_key.is_some(),
            generations: self.generations,
        }
    }

    /// Backend in use.
    pub fn backend(&self) -> &ImageBackend {
        &self.backend
    }

    /// Total generations performed.
    pub fn generation_count(&self) -> u64 {
        self.generations
    }
}

impl Default for ImageGenerator {
    fn default() -> Self {
        Self::new(Self::detect_backend())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_none() {
        let generator = ImageGenerator::new(ImageBackend::None);
        assert_eq!(*generator.backend(), ImageBackend::None);
        assert!(!generator.is_available());
    }

    #[test]
    fn test_initialize_none_fails() {
        let mut generator = ImageGenerator::new(ImageBackend::None);
        assert!(generator.initialize().is_err());
    }

    #[test]
    fn test_generate_not_initialized() {
        let mut generator = ImageGenerator::new(ImageBackend::None);
        let result = generator.generate("test", None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not initialized"));
    }

    #[test]
    fn test_enhance_prompt() {
        let enhanced = ImageGenerator::enhance_prompt("a cat");
        assert!(enhanced.starts_with("a cat"));
        assert!(enhanced.contains("detailed"));
    }

    #[test]
    fn test_enhance_prompt_empty() {
        let enhanced = ImageGenerator::enhance_prompt("");
        assert!(enhanced.is_empty());
    }

    #[test]
    fn test_get_status() {
        let generator = ImageGenerator::new(ImageBackend::None);
        let status = generator.get_status();
        assert!(!status.initialized);
        assert!(!status.has_api_key);
        assert_eq!(status.generations, 0);
    }

    #[test]
    fn test_generation_count() {
        let generator = ImageGenerator::new(ImageBackend::None);
        assert_eq!(generator.generation_count(), 0);
    }
}
