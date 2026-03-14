//! Multi-MCP-server connection manager.
//!
//! Manages connections to multiple MCP servers, provides a registry of
//! well-known servers, and routes tool calls to the correct server.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use super::client::{McpClient, McpTransport};
use super::types::McpTool;

/// Descriptor for a well-known MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownServer {
    /// Command to spawn the server (stdio transport).
    pub command: Vec<String>,
    /// Environment variables required by the server.
    pub env_required: Vec<String>,
    /// Human-readable description.
    pub description: String,
}

/// Manages multiple MCP server connections.
///
/// Maintains a set of connected clients, a registry of known servers,
/// and provides cross-server tool discovery and invocation.
pub struct McpManager {
    /// Active MCP clients keyed by server name.
    clients: HashMap<String, McpClient>,
    /// Well-known server configurations.
    known_servers: HashMap<String, KnownServer>,
}

impl McpManager {
    /// Create a new manager pre-populated with well-known server definitions.
    pub fn new() -> Self {
        let mut known_servers = HashMap::new();

        known_servers.insert(
            "filesystem".to_string(),
            KnownServer {
                command: vec![
                    "npx".to_string(),
                    "-y".to_string(),
                    "@modelcontextprotocol/server-filesystem".to_string(),
                ],
                env_required: Vec::new(),
                description: "File system operations (read, write, search, list)".to_string(),
            },
        );

        known_servers.insert(
            "github".to_string(),
            KnownServer {
                command: vec![
                    "npx".to_string(),
                    "-y".to_string(),
                    "@modelcontextprotocol/server-github".to_string(),
                ],
                env_required: vec!["GITHUB_PERSONAL_ACCESS_TOKEN".to_string()],
                description: "GitHub operations (repos, issues, PRs, code search)".to_string(),
            },
        );

        known_servers.insert(
            "slack".to_string(),
            KnownServer {
                command: vec![
                    "npx".to_string(),
                    "-y".to_string(),
                    "@modelcontextprotocol/server-slack".to_string(),
                ],
                env_required: vec!["SLACK_BOT_TOKEN".to_string()],
                description: "Slack operations (messages, channels, users)".to_string(),
            },
        );

        known_servers.insert(
            "postgres".to_string(),
            KnownServer {
                command: vec![
                    "npx".to_string(),
                    "-y".to_string(),
                    "@modelcontextprotocol/server-postgres".to_string(),
                ],
                env_required: vec!["POSTGRES_CONNECTION_STRING".to_string()],
                description: "PostgreSQL database queries and schema inspection".to_string(),
            },
        );

        known_servers.insert(
            "sqlite".to_string(),
            KnownServer {
                command: vec![
                    "npx".to_string(),
                    "-y".to_string(),
                    "@modelcontextprotocol/server-sqlite".to_string(),
                ],
                env_required: Vec::new(),
                description: "SQLite database operations via MCP".to_string(),
            },
        );

        info!(
            "McpManager: initialized with {} known servers",
            known_servers.len()
        );

        Self {
            clients: HashMap::new(),
            known_servers,
        }
    }

    /// Connect to an MCP server.
    ///
    /// If `command` is provided, creates a stdio client. If `url` is provided,
    /// creates an HTTP client. If neither is provided but the name matches a
    /// known server, uses the known server's command.
    pub fn connect(
        &mut self,
        name: &str,
        command: Option<Vec<String>>,
        url: Option<&str>,
    ) -> Result<(), String> {
        if self.clients.contains_key(name) {
            return Err(format!("Server '{name}' is already connected"));
        }

        let mut client = if let Some(url) = url {
            McpClient::new_http(name, url)
        } else if let Some(cmd) = command {
            McpClient::new_stdio(name, cmd)
        } else if let Some(known) = self.known_servers.get(name) {
            // Check required env vars.
            for var in &known.env_required {
                if std::env::var(var).is_err() {
                    warn!(
                        "McpManager: known server '{name}' requires env var '{var}' which is not set"
                    );
                }
            }
            McpClient::new_stdio(name, known.command.clone())
        } else {
            return Err(format!(
                "No command, URL, or known server configuration for '{name}'"
            ));
        };

        client.connect()?;
        info!("McpManager: connected to server '{name}'");
        self.clients.insert(name.to_string(), client);
        Ok(())
    }

    /// Disconnect from a specific server. Returns `true` if it was connected.
    pub fn disconnect(&mut self, name: &str) -> bool {
        if let Some(mut client) = self.clients.remove(name) {
            client.disconnect();
            debug!("McpManager: disconnected from '{name}'");
            true
        } else {
            false
        }
    }

    /// Disconnect from all servers.
    pub fn disconnect_all(&mut self) {
        let names: Vec<String> = self.clients.keys().cloned().collect();
        for name in &names {
            if let Some(mut client) = self.clients.remove(name) {
                client.disconnect();
            }
        }
        info!("McpManager: disconnected all ({} servers)", names.len());
    }

    /// List names of all connected servers.
    pub fn list_servers(&self) -> Vec<&str> {
        self.clients.keys().map(|k| k.as_str()).collect()
    }

    /// List all tools across all connected servers.
    ///
    /// Returns a map from server name to a list of tool names.
    pub fn list_all_tools(&self) -> HashMap<String, Vec<String>> {
        self.clients
            .iter()
            .map(|(name, client)| {
                let tools = client.list_tools().iter().map(|t| t.to_string()).collect();
                (name.clone(), tools)
            })
            .collect()
    }

    /// Call a tool on a specific server.
    pub fn call_tool(
        &mut self,
        server: &str,
        tool: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let client = self
            .clients
            .get_mut(server)
            .ok_or_else(|| format!("Server '{server}' is not connected"))?;
        client.call_tool(tool, args)
    }

    /// Get descriptions of all well-known servers.
    ///
    /// Returns pairs of (name, description).
    pub fn get_known_servers(&self) -> Vec<(&str, &str)> {
        self.known_servers
            .iter()
            .map(|(name, server)| (name.as_str(), server.description.as_str()))
            .collect()
    }

    /// Number of currently connected servers.
    pub fn connection_count(&self) -> usize {
        self.clients.len()
    }

    /// Check whether a specific server is connected.
    pub fn is_connected(&self, name: &str) -> bool {
        self.clients
            .get(name)
            .map(|c| c.is_connected())
            .unwrap_or(false)
    }

    /// Get a reference to a connected client.
    pub fn get_client(&self, name: &str) -> Option<&McpClient> {
        self.clients.get(name)
    }

    /// Get a mutable reference to a connected client.
    pub fn get_client_mut(&mut self, name: &str) -> Option<&mut McpClient> {
        self.clients.get_mut(name)
    }

    /// Get the transport type for a connected server.
    pub fn get_transport(&self, name: &str) -> Option<McpTransport> {
        self.clients.get(name).map(|c| c.transport().clone())
    }

    /// Discover tools from a specific connected server.
    ///
    /// Returns the list of [`McpTool`] descriptors discovered from the server.
    pub fn discover_tools(&mut self, name: &str) -> Result<Vec<McpTool>, String> {
        let client = self
            .clients
            .get_mut(name)
            .ok_or_else(|| format!("Server '{name}' is not connected"))?;
        client.discover_tools()
    }
}

impl Default for McpManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_manager_has_known_servers() {
        let mgr = McpManager::new();
        let known = mgr.get_known_servers();
        assert!(known.len() >= 5);

        let names: Vec<&str> = known.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"filesystem"));
        assert!(names.contains(&"github"));
        assert!(names.contains(&"slack"));
        assert!(names.contains(&"postgres"));
        assert!(names.contains(&"sqlite"));
    }

    #[test]
    fn test_connect_with_command() {
        let mut mgr = McpManager::new();
        mgr.connect("custom", Some(vec!["echo".to_string()]), None)
            .unwrap();
        assert!(mgr.is_connected("custom"));
        assert_eq!(mgr.connection_count(), 1);
    }

    #[test]
    fn test_connect_with_url() {
        let mut mgr = McpManager::new();
        mgr.connect("remote", None, Some("https://mcp.example.com"))
            .unwrap();
        assert!(mgr.is_connected("remote"));
    }

    #[test]
    fn test_connect_known_server() {
        let mut mgr = McpManager::new();
        // "filesystem" is a known server with no required env vars.
        mgr.connect("filesystem", None, None).unwrap();
        assert!(mgr.is_connected("filesystem"));
    }

    #[test]
    fn test_connect_duplicate() {
        let mut mgr = McpManager::new();
        mgr.connect("srv", Some(vec!["echo".to_string()]), None)
            .unwrap();
        let result = mgr.connect("srv", Some(vec!["echo".to_string()]), None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already connected"));
    }

    #[test]
    fn test_connect_unknown_no_args() {
        let mut mgr = McpManager::new();
        let result = mgr.connect("mystery", None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No command"));
    }

    #[test]
    fn test_disconnect() {
        let mut mgr = McpManager::new();
        mgr.connect("srv", Some(vec!["echo".to_string()]), None)
            .unwrap();
        assert!(mgr.disconnect("srv"));
        assert!(!mgr.is_connected("srv"));
        assert_eq!(mgr.connection_count(), 0);
    }

    #[test]
    fn test_disconnect_nonexistent() {
        let mut mgr = McpManager::new();
        assert!(!mgr.disconnect("ghost"));
    }

    #[test]
    fn test_disconnect_all() {
        let mut mgr = McpManager::new();
        mgr.connect("a", Some(vec!["echo".to_string()]), None)
            .unwrap();
        mgr.connect("b", Some(vec!["echo".to_string()]), None)
            .unwrap();
        assert_eq!(mgr.connection_count(), 2);

        mgr.disconnect_all();
        assert_eq!(mgr.connection_count(), 0);
    }

    #[test]
    fn test_list_servers() {
        let mut mgr = McpManager::new();
        mgr.connect("alpha", Some(vec!["echo".to_string()]), None)
            .unwrap();
        mgr.connect("beta", Some(vec!["echo".to_string()]), None)
            .unwrap();

        let servers = mgr.list_servers();
        assert_eq!(servers.len(), 2);
        assert!(servers.contains(&"alpha"));
        assert!(servers.contains(&"beta"));
    }

    #[test]
    fn test_call_tool_through_manager() {
        let mut mgr = McpManager::new();
        mgr.connect("srv", Some(vec!["echo".to_string()]), None)
            .unwrap();

        let result = mgr
            .call_tool("srv", "read_file", serde_json::json!({"path": "/tmp/x"}))
            .unwrap();
        assert_eq!(result["isError"], false);
    }

    #[test]
    fn test_call_tool_unknown_server() {
        let mut mgr = McpManager::new();
        let result = mgr.call_tool("nope", "tool", serde_json::json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not connected"));
    }

    #[test]
    fn test_list_all_tools_empty() {
        let mgr = McpManager::new();
        let tools = mgr.list_all_tools();
        assert!(tools.is_empty());
    }
}
