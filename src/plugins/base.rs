//! Base plugin interface.
//!
//! Defines the `Plugin` trait that all plugins must implement.
//! Provides default no-op implementations for lifecycle hooks so that
//! simple plugins only need to supply metadata.

use super::metadata::PluginMetadata;

/// Core trait for all Sovereign Titan plugins.
///
/// Implementors must be `Send + Sync` so plugins can be held in shared
/// state across async tasks. Lifecycle methods have default no-op
/// implementations for convenience.
pub trait Plugin: Send + Sync {
    /// Return the metadata describing this plugin.
    fn metadata(&self) -> &PluginMetadata;

    /// Called when the plugin is loaded into the runtime.
    ///
    /// Perform any one-time initialization here. The default is a no-op.
    fn on_load(&self) -> Result<(), String> {
        Ok(())
    }

    /// Called when the plugin is about to be unloaded.
    ///
    /// Release resources or persist state here. The default is a no-op.
    fn on_unload(&self) -> Result<(), String> {
        Ok(())
    }

    /// Return the names of tools this plugin registers with the ToolRegistry.
    ///
    /// The default returns an empty list (no tools).
    fn registered_tools(&self) -> Vec<String> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal mock plugin for testing.
    struct MockPlugin {
        meta: PluginMetadata,
        tools: Vec<String>,
    }

    impl MockPlugin {
        fn new(name: &str) -> Self {
            Self {
                meta: PluginMetadata::new(name, "1.0.0", "test", "Mock plugin for testing"),
                tools: Vec::new(),
            }
        }

        fn with_tools(mut self, tools: Vec<&str>) -> Self {
            self.tools = tools.into_iter().map(|s| s.to_string()).collect();
            self
        }
    }

    impl Plugin for MockPlugin {
        fn metadata(&self) -> &PluginMetadata {
            &self.meta
        }

        fn registered_tools(&self) -> Vec<String> {
            self.tools.clone()
        }
    }

    /// A plugin that fails on load for testing error paths.
    struct FailPlugin {
        meta: PluginMetadata,
    }

    impl FailPlugin {
        fn new() -> Self {
            Self {
                meta: PluginMetadata::new("fail-plugin", "0.0.1", "test", "Always fails"),
            }
        }
    }

    impl Plugin for FailPlugin {
        fn metadata(&self) -> &PluginMetadata {
            &self.meta
        }

        fn on_load(&self) -> Result<(), String> {
            Err("Intentional load failure".to_string())
        }
    }

    #[test]
    fn test_mock_plugin_metadata() {
        let plugin = MockPlugin::new("greeter");
        assert_eq!(plugin.metadata().name, "greeter");
        assert_eq!(plugin.metadata().version, "1.0.0");
    }

    #[test]
    fn test_default_lifecycle_hooks() {
        let plugin = MockPlugin::new("simple");
        assert!(plugin.on_load().is_ok());
        assert!(plugin.on_unload().is_ok());
    }

    #[test]
    fn test_registered_tools() {
        let plugin = MockPlugin::new("tooled").with_tools(vec!["search", "calculate"]);
        let tools = plugin.registered_tools();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0], "search");
        assert_eq!(tools[1], "calculate");
    }

    #[test]
    fn test_default_no_tools() {
        let plugin = MockPlugin::new("empty");
        assert!(plugin.registered_tools().is_empty());
    }

    #[test]
    fn test_fail_plugin_on_load() {
        let plugin = FailPlugin::new();
        let result = plugin.on_load();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Intentional load failure");
    }

    #[test]
    fn test_plugin_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MockPlugin>();
    }
}
