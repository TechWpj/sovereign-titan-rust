//! Plugin metadata container.
//!
//! Stores identifying information, version, authorship, and dependency
//! details for a loaded plugin.

use serde::{Deserialize, Serialize};

/// Metadata describing a plugin: identity, version, and runtime state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    /// Unique plugin name (e.g. "weather-provider").
    pub name: String,
    /// Semantic version string (e.g. "1.0.0").
    pub version: String,
    /// Plugin author or organization.
    pub author: String,
    /// Human-readable description of the plugin.
    pub description: String,
    /// Names of plugins that must be loaded before this one.
    pub dependencies: Vec<String>,
    /// Epoch timestamp when the plugin was loaded (None if not yet loaded).
    pub loaded_at: Option<f64>,
    /// Whether the plugin is currently enabled.
    pub enabled: bool,
}

impl PluginMetadata {
    /// Create metadata for a new plugin with the given identity.
    ///
    /// Defaults to enabled with no dependencies and not yet loaded.
    pub fn new(name: &str, version: &str, author: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            author: author.to_string(),
            description: description.to_string(),
            dependencies: Vec::new(),
            loaded_at: None,
            enabled: true,
        }
    }

    /// Add a dependency on another plugin.
    pub fn with_dependency(mut self, dep: &str) -> Self {
        self.dependencies.push(dep.to_string());
        self
    }

    /// Check whether all listed dependencies are satisfied by the given set.
    pub fn dependencies_satisfied(&self, loaded: &[String]) -> bool {
        self.dependencies.iter().all(|dep| loaded.contains(dep))
    }
}

impl Default for PluginMetadata {
    fn default() -> Self {
        Self {
            name: "unnamed-plugin".to_string(),
            version: "0.1.0".to_string(),
            author: "unknown".to_string(),
            description: String::new(),
            dependencies: Vec::new(),
            loaded_at: None,
            enabled: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_metadata() {
        let meta = PluginMetadata::new("weather", "1.2.3", "Titan Team", "Weather data provider");
        assert_eq!(meta.name, "weather");
        assert_eq!(meta.version, "1.2.3");
        assert_eq!(meta.author, "Titan Team");
        assert_eq!(meta.description, "Weather data provider");
        assert!(meta.dependencies.is_empty());
        assert!(meta.loaded_at.is_none());
        assert!(meta.enabled);
    }

    #[test]
    fn test_default_metadata() {
        let meta = PluginMetadata::default();
        assert_eq!(meta.name, "unnamed-plugin");
        assert_eq!(meta.version, "0.1.0");
        assert!(!meta.enabled);
    }

    #[test]
    fn test_with_dependency() {
        let meta = PluginMetadata::new("ui-extras", "1.0.0", "dev", "UI extensions")
            .with_dependency("core-ui")
            .with_dependency("theme-engine");
        assert_eq!(meta.dependencies.len(), 2);
        assert_eq!(meta.dependencies[0], "core-ui");
        assert_eq!(meta.dependencies[1], "theme-engine");
    }

    #[test]
    fn test_dependencies_satisfied() {
        let meta = PluginMetadata::new("child", "1.0.0", "dev", "")
            .with_dependency("parent-a")
            .with_dependency("parent-b");

        let loaded = vec!["parent-a".to_string(), "parent-b".to_string(), "other".to_string()];
        assert!(meta.dependencies_satisfied(&loaded));

        let partial = vec!["parent-a".to_string()];
        assert!(!meta.dependencies_satisfied(&partial));

        let empty: Vec<String> = Vec::new();
        assert!(!meta.dependencies_satisfied(&empty));
    }

    #[test]
    fn test_serialization_roundtrip() {
        let original = PluginMetadata::new("test-plugin", "2.0.0", "tester", "A test plugin")
            .with_dependency("base");
        let json = serde_json::to_string(&original).unwrap();
        let restored: PluginMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.name, "test-plugin");
        assert_eq!(restored.version, "2.0.0");
        assert_eq!(restored.dependencies, vec!["base"]);
        assert!(restored.enabled);
    }

    #[test]
    fn test_no_dependencies_always_satisfied() {
        let meta = PluginMetadata::new("standalone", "1.0.0", "dev", "No deps");
        assert!(meta.dependencies_satisfied(&[]));
        assert!(meta.dependencies_satisfied(&["anything".to_string()]));
    }
}
