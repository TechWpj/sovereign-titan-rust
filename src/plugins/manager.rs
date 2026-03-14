//! Plugin lifecycle management.
//!
//! Discovers, loads, enables/disables, and unloads plugins. Manages the
//! full collection of active plugins and their metadata.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use super::base::Plugin;
use super::metadata::PluginMetadata;

/// Summary information about a loaded plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    /// Plugin name.
    pub name: String,
    /// Plugin version.
    pub version: String,
    /// Plugin author.
    pub author: String,
    /// Plugin description.
    pub description: String,
    /// Whether the plugin is enabled.
    pub enabled: bool,
    /// Number of tools registered by this plugin.
    pub tool_count: usize,
}

/// Manages the lifecycle of all loaded plugins.
///
/// Plugins are registered by name and can be individually enabled, disabled,
/// or unloaded. The manager also discovers plugin files from configured
/// directories.
pub struct PluginManager {
    /// Loaded plugins keyed by name.
    plugins: HashMap<String, Box<dyn Plugin>>,
    /// Directories to search for plugin files.
    plugin_dirs: Vec<String>,
}

impl PluginManager {
    /// Create a new plugin manager with optional search directories.
    pub fn new(plugin_dirs: Option<Vec<String>>) -> Self {
        let dirs = plugin_dirs.unwrap_or_else(|| vec!["plugins".to_string()]);
        info!("PluginManager: initialized with dirs {:?}", dirs);
        Self {
            plugins: HashMap::new(),
            plugin_dirs: dirs,
        }
    }

    /// Discover plugin files in the configured directories.
    ///
    /// Returns a list of discovered file paths. On Windows this looks for
    /// `.dll` files; on Linux `.so` files; on macOS `.dylib` files.
    pub fn discover(&self) -> Vec<String> {
        let extension = if cfg!(target_os = "windows") {
            "dll"
        } else if cfg!(target_os = "macos") {
            "dylib"
        } else {
            "so"
        };

        let mut found = Vec::new();
        for dir in &self.plugin_dirs {
            let path = std::path::Path::new(dir);
            if !path.is_dir() {
                debug!("Plugin directory does not exist: {dir}");
                continue;
            }

            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    let entry_path = entry.path();
                    if let Some(ext) = entry_path.extension() {
                        if ext == extension {
                            if let Some(s) = entry_path.to_str() {
                                found.push(s.to_string());
                            }
                        }
                    }
                }
            }
        }

        info!("PluginManager: discovered {} plugin files", found.len());
        found
    }

    /// Register a plugin instance. Calls `on_load()` and stores it.
    ///
    /// Returns `true` if registration succeeded, `false` if a plugin with
    /// the same name already exists or `on_load()` failed.
    pub fn register_plugin(&mut self, name: &str, plugin: Box<dyn Plugin>) -> bool {
        if self.plugins.contains_key(name) {
            warn!("PluginManager: plugin '{name}' already registered");
            return false;
        }

        if let Err(e) = plugin.on_load() {
            warn!("PluginManager: plugin '{name}' failed on_load: {e}");
            return false;
        }

        info!(
            "PluginManager: registered '{}' v{}",
            plugin.metadata().name,
            plugin.metadata().version
        );
        self.plugins.insert(name.to_string(), plugin);
        true
    }

    /// Unload and remove a plugin. Calls `on_unload()` before removal.
    ///
    /// Returns `true` if the plugin existed and was removed.
    pub fn unload_plugin(&mut self, name: &str) -> bool {
        if let Some(plugin) = self.plugins.remove(name) {
            if let Err(e) = plugin.on_unload() {
                warn!("PluginManager: plugin '{name}' on_unload error: {e}");
            }
            info!("PluginManager: unloaded '{name}'");
            true
        } else {
            false
        }
    }

    /// Get a reference to a loaded plugin by name.
    pub fn get_plugin(&self, name: &str) -> Option<&dyn Plugin> {
        self.plugins.get(name).map(|p| p.as_ref())
    }

    /// List summary information for all loaded plugins.
    pub fn list_plugins(&self) -> Vec<PluginInfo> {
        self.plugins
            .values()
            .map(|p| {
                let meta = p.metadata();
                PluginInfo {
                    name: meta.name.clone(),
                    version: meta.version.clone(),
                    author: meta.author.clone(),
                    description: meta.description.clone(),
                    enabled: meta.enabled,
                    tool_count: p.registered_tools().len(),
                }
            })
            .collect()
    }

    /// Enable a plugin by name. Returns `true` if the plugin was found.
    ///
    /// Note: Since metadata is owned by the plugin, this requests the plugin
    /// to report itself as enabled. The actual enabled state is managed via
    /// the metadata inside the plugin. This method currently returns whether
    /// the plugin exists; for full mutability the Plugin trait would need
    /// an `enable()` method.
    pub fn enable_plugin(&self, name: &str) -> bool {
        self.plugins.contains_key(name)
    }

    /// Disable a plugin by name. Returns `true` if the plugin was found.
    pub fn disable_plugin(&self, name: &str) -> bool {
        self.plugins.contains_key(name)
    }

    /// Number of currently loaded plugins.
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    /// Get the configured plugin directories.
    pub fn plugin_dirs(&self) -> &[String] {
        &self.plugin_dirs
    }

    /// Validate plugin metadata before loading.
    ///
    /// Checks that name, version, and author are non-empty, and that
    /// all declared dependencies are already loaded.
    pub fn validate_metadata(&self, metadata: &PluginMetadata) -> Result<(), String> {
        if metadata.name.is_empty() {
            return Err("Plugin metadata: name cannot be empty".to_string());
        }
        if metadata.version.is_empty() {
            return Err("Plugin metadata: version cannot be empty".to_string());
        }
        if metadata.author.is_empty() {
            return Err("Plugin metadata: author cannot be empty".to_string());
        }
        let loaded_names: Vec<String> = self.plugins.keys().cloned().collect();
        if !metadata.dependencies_satisfied(&loaded_names) {
            return Err(format!(
                "Plugin '{}' has unsatisfied dependencies: {:?}",
                metadata.name, metadata.dependencies
            ));
        }
        Ok(())
    }

    /// Get all tool names registered across all plugins.
    pub fn all_tools(&self) -> Vec<String> {
        self.plugins
            .values()
            .flat_map(|p| p.registered_tools())
            .collect()
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test plugin implementation.
    struct TestPlugin {
        meta: PluginMetadata,
        tools: Vec<String>,
    }

    impl TestPlugin {
        fn new(name: &str) -> Self {
            Self {
                meta: PluginMetadata::new(name, "1.0.0", "test", &format!("{name} plugin")),
                tools: Vec::new(),
            }
        }

        fn with_tools(mut self, tools: Vec<&str>) -> Self {
            self.tools = tools.into_iter().map(|s| s.to_string()).collect();
            self
        }
    }

    impl Plugin for TestPlugin {
        fn metadata(&self) -> &PluginMetadata {
            &self.meta
        }

        fn registered_tools(&self) -> Vec<String> {
            self.tools.clone()
        }
    }

    /// Plugin that fails on load.
    struct FailLoadPlugin {
        meta: PluginMetadata,
    }

    impl Plugin for FailLoadPlugin {
        fn metadata(&self) -> &PluginMetadata {
            &self.meta
        }
        fn on_load(&self) -> Result<(), String> {
            Err("load failed".to_string())
        }
    }

    #[test]
    fn test_new_manager() {
        let mgr = PluginManager::new(None);
        assert_eq!(mgr.plugin_count(), 0);
        assert_eq!(mgr.plugin_dirs(), &["plugins"]);
    }

    #[test]
    fn test_new_manager_custom_dirs() {
        let mgr = PluginManager::new(Some(vec!["/opt/plugins".to_string(), "/home/plugins".to_string()]));
        assert_eq!(mgr.plugin_dirs().len(), 2);
    }

    #[test]
    fn test_register_and_get_plugin() {
        let mut mgr = PluginManager::new(None);
        let plugin = TestPlugin::new("greeter");
        assert!(mgr.register_plugin("greeter", Box::new(plugin)));
        assert_eq!(mgr.plugin_count(), 1);

        let p = mgr.get_plugin("greeter").unwrap();
        assert_eq!(p.metadata().name, "greeter");
    }

    #[test]
    fn test_register_duplicate() {
        let mut mgr = PluginManager::new(None);
        mgr.register_plugin("dup", Box::new(TestPlugin::new("dup")));
        assert!(!mgr.register_plugin("dup", Box::new(TestPlugin::new("dup"))));
        assert_eq!(mgr.plugin_count(), 1);
    }

    #[test]
    fn test_register_fail_on_load() {
        let mut mgr = PluginManager::new(None);
        let plugin = FailLoadPlugin {
            meta: PluginMetadata::new("bad", "0.1.0", "test", "fails"),
        };
        assert!(!mgr.register_plugin("bad", Box::new(plugin)));
        assert_eq!(mgr.plugin_count(), 0);
    }

    #[test]
    fn test_unload_plugin() {
        let mut mgr = PluginManager::new(None);
        mgr.register_plugin("temp", Box::new(TestPlugin::new("temp")));
        assert!(mgr.unload_plugin("temp"));
        assert_eq!(mgr.plugin_count(), 0);
        assert!(mgr.get_plugin("temp").is_none());
    }

    #[test]
    fn test_unload_nonexistent() {
        let mut mgr = PluginManager::new(None);
        assert!(!mgr.unload_plugin("ghost"));
    }

    #[test]
    fn test_list_plugins() {
        let mut mgr = PluginManager::new(None);
        mgr.register_plugin(
            "alpha",
            Box::new(TestPlugin::new("alpha").with_tools(vec!["tool_a"])),
        );
        mgr.register_plugin("beta", Box::new(TestPlugin::new("beta")));

        let list = mgr.list_plugins();
        assert_eq!(list.len(), 2);

        let alpha = list.iter().find(|p| p.name == "alpha").unwrap();
        assert_eq!(alpha.tool_count, 1);

        let beta = list.iter().find(|p| p.name == "beta").unwrap();
        assert_eq!(beta.tool_count, 0);
    }

    #[test]
    fn test_all_tools() {
        let mut mgr = PluginManager::new(None);
        mgr.register_plugin(
            "a",
            Box::new(TestPlugin::new("a").with_tools(vec!["search", "fetch"])),
        );
        mgr.register_plugin(
            "b",
            Box::new(TestPlugin::new("b").with_tools(vec!["calculate"])),
        );

        let tools = mgr.all_tools();
        assert_eq!(tools.len(), 3);
        assert!(tools.contains(&"search".to_string()));
        assert!(tools.contains(&"fetch".to_string()));
        assert!(tools.contains(&"calculate".to_string()));
    }

    #[test]
    fn test_enable_disable_plugin() {
        let mut mgr = PluginManager::new(None);
        mgr.register_plugin("toggle", Box::new(TestPlugin::new("toggle")));
        assert!(mgr.enable_plugin("toggle"));
        assert!(mgr.disable_plugin("toggle"));
        assert!(!mgr.enable_plugin("nonexistent"));
    }

    #[test]
    fn test_discover_empty_dirs() {
        let mgr = PluginManager::new(Some(vec!["/nonexistent/path/abc123".to_string()]));
        let discovered = mgr.discover();
        assert!(discovered.is_empty());
    }
}
