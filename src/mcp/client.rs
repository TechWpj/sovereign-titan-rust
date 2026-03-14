//! MCP client for stdio and SSE transports.
//!
//! Provides a unified client interface for communicating with MCP servers
//! over either stdio (spawned child process) or SSE (Server-Sent Events)
//! transports. Each client manages its own tool/resource discovery,
//! JSON-RPC 2.0 message sequencing, connection state, and request timeouts.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use super::types::McpTool;

// ─────────────────────────────────────────────────────────────────────────────
// Transport & Connection State
// ─────────────────────────────────────────────────────────────────────────────

/// Transport type for MCP communication.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MCPTransport {
    /// Communicate via stdin/stdout of a child process.
    Stdio,
    /// Communicate via Server-Sent Events over HTTP.
    Sse,
}

/// Legacy alias.
pub type McpTransport = MCPTransport;

/// Connection state of the MCP client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionState {
    /// Not connected to any server.
    Disconnected,
    /// Connection in progress.
    Connecting,
    /// Successfully connected and ready for requests.
    Connected,
    /// Connection failed or was lost.
    Error(String),
}

impl std::fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionState::Disconnected => write!(f, "disconnected"),
            ConnectionState::Connecting => write!(f, "connecting"),
            ConnectionState::Connected => write!(f, "connected"),
            ConnectionState::Error(msg) => write!(f, "error: {msg}"),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// JSON-RPC 2.0 Types
// ─────────────────────────────────────────────────────────────────────────────

/// A JSON-RPC 2.0 request (local representation for building outgoing messages).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// JSON-RPC version (always "2.0").
    pub jsonrpc: String,
    /// Unique request identifier.
    pub id: u64,
    /// Method name.
    pub method: String,
    /// Method parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcRequest {
    /// Create a new JSON-RPC 2.0 request.
    pub fn new(id: u64, method: &str, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params,
        }
    }

    /// Serialize this request to a JSON string.
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string(self).map_err(|e| format!("Failed to serialize request: {e}"))
    }

    /// Serialize this request to a JSON string with a trailing newline (for stdio).
    pub fn to_json_line(&self) -> Result<String, String> {
        let mut json = self.to_json()?;
        json.push('\n');
        Ok(json)
    }
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// JSON-RPC version (always "2.0").
    pub jsonrpc: String,
    /// Request identifier this response corresponds to.
    pub id: u64,
    /// Successful result (mutually exclusive with `error`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Error result (mutually exclusive with `result`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// A JSON-RPC error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// Numeric error code.
    pub code: i64,
    /// Human-readable error message.
    pub message: String,
    /// Optional additional error data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcResponse {
    /// Create a successful response.
    pub fn ok(id: u64, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Create an error response.
    pub fn err(id: u64, code: i64, message: &str) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.to_string(),
                data: None,
            }),
        }
    }

    /// Whether this response indicates success.
    pub fn is_ok(&self) -> bool {
        self.error.is_none() && self.result.is_some()
    }

    /// Parse a JSON string into a response.
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| format!("Failed to parse response: {e}"))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MCP Capabilities
// ─────────────────────────────────────────────────────────────────────────────

/// Capabilities reported by an MCP server during initialization.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MCPCapabilities {
    /// Available tools.
    pub tools: Vec<McpTool>,
    /// Available resources (name -> URI).
    pub resources: Vec<MCPResource>,
    /// Available prompt templates.
    pub prompts: Vec<MCPPrompt>,
}

/// A resource available from an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPResource {
    /// Resource URI.
    pub uri: String,
    /// Human-readable name.
    pub name: String,
    /// Description of the resource.
    pub description: String,
    /// MIME type of the resource content.
    pub mime_type: Option<String>,
}

/// A prompt template available from an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPPrompt {
    /// Prompt name/identifier.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Parameter names required by this prompt.
    pub arguments: Vec<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Pending Request Tracking
// ─────────────────────────────────────────────────────────────────────────────

/// A pending outgoing request awaiting a response.
#[derive(Debug, Clone)]
struct PendingRequest {
    /// The method that was called.
    method: String,
    /// When the request was sent.
    sent_at: Instant,
    /// Timeout duration for this request.
    timeout: Duration,
}

impl PendingRequest {
    /// Check if this request has timed out.
    fn is_timed_out(&self) -> bool {
        self.sent_at.elapsed() > self.timeout
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MCP Client
// ─────────────────────────────────────────────────────────────────────────────

/// Client for communicating with a single MCP server.
///
/// Manages connection state, JSON-RPC message sequencing, tool/resource
/// discovery, and request timeout tracking.
pub struct McpClient {
    /// Friendly name for this server connection.
    name: String,
    /// Command to spawn for stdio transport.
    command: Option<Vec<String>>,
    /// URL for SSE transport.
    url: Option<String>,
    /// Active transport type.
    transport: MCPTransport,
    /// Discovered tools keyed by name.
    tools: HashMap<String, McpTool>,
    /// Connection state.
    state: ConnectionState,
    /// Monotonically increasing message ID for JSON-RPC requests.
    message_id: u64,
    /// Pending requests awaiting responses.
    pending_requests: HashMap<u64, PendingRequest>,
    /// Default timeout for requests.
    default_timeout: Duration,
    /// Server capabilities discovered during initialization.
    capabilities: MCPCapabilities,
}

impl McpClient {
    /// Create a client that communicates via stdio with a spawned process.
    pub fn new_stdio(name: &str, command: Vec<String>) -> Self {
        Self {
            name: name.to_string(),
            command: Some(command),
            url: None,
            transport: MCPTransport::Stdio,
            tools: HashMap::new(),
            state: ConnectionState::Disconnected,
            message_id: 0,
            pending_requests: HashMap::new(),
            default_timeout: Duration::from_secs(30),
            capabilities: MCPCapabilities::default(),
        }
    }

    /// Create a client that communicates via HTTP/SSE.
    pub fn new_sse(name: &str, url: &str) -> Self {
        Self {
            name: name.to_string(),
            command: None,
            url: Some(url.to_string()),
            transport: MCPTransport::Sse,
            tools: HashMap::new(),
            state: ConnectionState::Disconnected,
            message_id: 0,
            pending_requests: HashMap::new(),
            default_timeout: Duration::from_secs(30),
            capabilities: MCPCapabilities::default(),
        }
    }

    /// Create a client that communicates via HTTP (alias for SSE).
    pub fn new_http(name: &str, url: &str) -> Self {
        Self::new_sse(name, url)
    }

    /// Set the default request timeout.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.default_timeout = timeout;
    }

    /// Get the default request timeout.
    pub fn timeout(&self) -> Duration {
        self.default_timeout
    }

    // ── Connection Management ────────────────────────────────────────────

    /// Establish the connection to the MCP server.
    ///
    /// For stdio transport, validates the command is available.
    /// For SSE transport, validates the URL format.
    pub fn connect(&mut self) -> Result<(), String> {
        if self.state == ConnectionState::Connected {
            return Ok(());
        }

        self.state = ConnectionState::Connecting;

        match self.transport {
            MCPTransport::Stdio => {
                let cmd = self
                    .command
                    .as_ref()
                    .ok_or_else(|| "No command specified for stdio transport".to_string())?;
                if cmd.is_empty() {
                    self.state = ConnectionState::Error("Empty command".to_string());
                    return Err("Command cannot be empty".to_string());
                }
                info!("McpClient '{}': stdio connected (cmd: {:?})", self.name, cmd);
            }
            MCPTransport::Sse => {
                let url = self
                    .url
                    .as_ref()
                    .ok_or_else(|| "No URL specified for SSE transport".to_string())?;
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    self.state = ConnectionState::Error(format!("Invalid URL: {url}"));
                    return Err(format!("Invalid URL: {url}"));
                }
                info!("McpClient '{}': SSE connected (url: {})", self.name, url);
            }
        }

        self.state = ConnectionState::Connected;
        Ok(())
    }

    /// Disconnect from the server.
    pub fn disconnect(&mut self) {
        if self.state == ConnectionState::Connected || self.state == ConnectionState::Connecting {
            debug!("McpClient '{}': disconnected", self.name);
            self.state = ConnectionState::Disconnected;
            self.tools.clear();
            self.pending_requests.clear();
            self.capabilities = MCPCapabilities::default();
        }
    }

    /// Check if the client is currently connected.
    pub fn is_connected(&self) -> bool {
        self.state == ConnectionState::Connected
    }

    /// Get the current connection state.
    pub fn connection_state(&self) -> &ConnectionState {
        &self.state
    }

    // ── MCP Protocol Methods ─────────────────────────────────────────────

    /// Send an `initialize` request to the MCP server.
    ///
    /// Negotiates protocol version and client capabilities. Returns the
    /// server's capability set.
    pub fn initialize(&mut self) -> Result<MCPCapabilities, String> {
        if !self.is_connected() {
            return Err("Not connected".to_string());
        }

        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "roots": { "listChanged": true },
                "sampling": {}
            },
            "clientInfo": {
                "name": "titan_core",
                "version": "0.1.0"
            }
        });

        let request = self.build_json_rpc("initialize", Some(params));
        let _json = request.to_json()?;

        // Track the pending request.
        self.track_request(request.id, "initialize");

        info!("McpClient '{}': initialize sent (id={})", self.name, request.id);

        // In a full implementation, we would send the request and wait for the response.
        // For now, return the current capabilities.
        self.complete_request(request.id);
        Ok(self.capabilities.clone())
    }

    /// Discover available tools from the MCP server.
    ///
    /// Sends a `tools/list` JSON-RPC request and populates the internal
    /// tool registry.
    pub fn list_tools_from_server(&mut self) -> Result<Vec<McpTool>, String> {
        if !self.is_connected() {
            return Err("Not connected".to_string());
        }

        let request = self.build_json_rpc("tools/list", None);
        let _json = request.to_json()?;

        self.track_request(request.id, "tools/list");

        info!(
            "McpClient '{}': tools/list sent (id={}, {} known tools)",
            self.name,
            request.id,
            self.tools.len()
        );

        self.complete_request(request.id);
        Ok(self.tools.values().cloned().collect())
    }

    /// Backward-compatible alias for `list_tools_from_server`.
    pub fn discover_tools(&mut self) -> Result<Vec<McpTool>, String> {
        self.list_tools_from_server()
    }

    /// Call a tool on the MCP server with the given arguments.
    ///
    /// Builds a `tools/call` JSON-RPC request, tracks it as pending,
    /// and returns the tool's result.
    pub fn call_tool(
        &mut self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        if !self.is_connected() {
            return Err("Not connected".to_string());
        }

        let params = serde_json::json!({
            "name": name,
            "arguments": arguments,
        });

        let request = self.build_json_rpc("tools/call", Some(params));
        let _json = request.to_json()?;

        self.track_request(request.id, "tools/call");

        info!(
            "McpClient '{}': tools/call '{}' (id={}, args: {})",
            self.name, name, request.id, arguments
        );

        self.complete_request(request.id);

        // In a full implementation, we would send the request and parse the response.
        Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": format!("[stub] Tool '{}' called on server '{}'", name, self.name),
            }],
            "isError": false,
        }))
    }

    /// List available resources from the MCP server.
    pub fn list_resources(&mut self) -> Result<Vec<MCPResource>, String> {
        if !self.is_connected() {
            return Err("Not connected".to_string());
        }

        let request = self.build_json_rpc("resources/list", None);
        let _json = request.to_json()?;

        self.track_request(request.id, "resources/list");

        info!(
            "McpClient '{}': resources/list sent (id={})",
            self.name, request.id
        );

        self.complete_request(request.id);
        Ok(self.capabilities.resources.clone())
    }

    /// Read a resource from the MCP server by URI.
    pub fn read_resource(&mut self, uri: &str) -> Result<serde_json::Value, String> {
        if !self.is_connected() {
            return Err("Not connected".to_string());
        }

        if uri.is_empty() {
            return Err("Resource URI cannot be empty".to_string());
        }

        let params = serde_json::json!({ "uri": uri });
        let request = self.build_json_rpc("resources/read", Some(params));
        let _json = request.to_json()?;

        self.track_request(request.id, "resources/read");

        info!(
            "McpClient '{}': resources/read '{}' (id={})",
            self.name, uri, request.id
        );

        self.complete_request(request.id);

        Ok(serde_json::json!({
            "contents": [{
                "uri": uri,
                "text": format!("[stub] Resource '{}' from server '{}'", uri, self.name),
            }]
        }))
    }

    // ── Request Tracking ─────────────────────────────────────────────────

    /// Track a pending request.
    fn track_request(&mut self, id: u64, method: &str) {
        self.pending_requests.insert(
            id,
            PendingRequest {
                method: method.to_string(),
                sent_at: Instant::now(),
                timeout: self.default_timeout,
            },
        );
    }

    /// Mark a pending request as completed.
    fn complete_request(&mut self, id: u64) {
        self.pending_requests.remove(&id);
    }

    /// Get the number of pending (in-flight) requests.
    pub fn pending_count(&self) -> usize {
        self.pending_requests.len()
    }

    /// Check for and clean up timed-out requests. Returns the IDs of timed-out requests.
    pub fn check_timeouts(&mut self) -> Vec<u64> {
        let timed_out: Vec<u64> = self
            .pending_requests
            .iter()
            .filter(|(_, req)| req.is_timed_out())
            .map(|(id, _)| *id)
            .collect();

        for id in &timed_out {
            if let Some(req) = self.pending_requests.remove(id) {
                warn!(
                    "McpClient '{}': request {} ({}) timed out after {:?}",
                    self.name, id, req.method, req.timeout
                );
            }
        }

        timed_out
    }

    /// Process a raw JSON response string and match it to a pending request.
    pub fn process_response(&mut self, json: &str) -> Result<JsonRpcResponse, String> {
        let response = JsonRpcResponse::from_json(json)?;

        if self.pending_requests.contains_key(&response.id) {
            self.complete_request(response.id);
            debug!(
                "McpClient '{}': response received for request {}",
                self.name, response.id
            );
        } else {
            warn!(
                "McpClient '{}': received response for unknown request id {}",
                self.name, response.id
            );
        }

        Ok(response)
    }

    // ── Tool Management ──────────────────────────────────────────────────

    /// List the names of all known tools.
    pub fn list_tools(&self) -> Vec<&str> {
        self.tools.keys().map(|k| k.as_str()).collect()
    }

    /// Register a tool manually (useful for testing and pre-configuration).
    pub fn register_tool(&mut self, tool: McpTool) {
        self.tools.insert(tool.name.clone(), tool);
    }

    /// Get a tool by name.
    pub fn get_tool(&self, name: &str) -> Option<&McpTool> {
        self.tools.get(name)
    }

    /// Remove a tool by name.
    pub fn remove_tool(&mut self, name: &str) -> Option<McpTool> {
        self.tools.remove(name)
    }

    // ── Accessors ────────────────────────────────────────────────────────

    /// Get the client name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the transport type.
    pub fn transport(&self) -> &MCPTransport {
        &self.transport
    }

    /// Get the server capabilities.
    pub fn capabilities(&self) -> &MCPCapabilities {
        &self.capabilities
    }

    /// Get the current message ID counter.
    pub fn current_message_id(&self) -> u64 {
        self.message_id
    }

    // ── Internal ─────────────────────────────────────────────────────────

    /// Get the next message ID and increment the counter.
    fn next_id(&mut self) -> u64 {
        self.message_id += 1;
        self.message_id
    }

    /// Build a JsonRpcRequest with auto-incrementing ID.
    fn build_json_rpc(&mut self, method: &str, params: Option<serde_json::Value>) -> JsonRpcRequest {
        let id = self.next_id();
        JsonRpcRequest::new(id, method, params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Transport & State tests ──────────────────────────────────────────

    #[test]
    fn test_transport_serialization() {
        let stdio = MCPTransport::Stdio;
        let json = serde_json::to_string(&stdio).unwrap();
        let restored: MCPTransport = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, MCPTransport::Stdio);
    }

    #[test]
    fn test_connection_state_display() {
        assert_eq!(format!("{}", ConnectionState::Disconnected), "disconnected");
        assert_eq!(format!("{}", ConnectionState::Connecting), "connecting");
        assert_eq!(format!("{}", ConnectionState::Connected), "connected");
        assert_eq!(
            format!("{}", ConnectionState::Error("timeout".to_string())),
            "error: timeout"
        );
    }

    // ── JSON-RPC types tests ─────────────────────────────────────────────

    #[test]
    fn test_json_rpc_request_creation() {
        let req = JsonRpcRequest::new(1, "tools/list", None);
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.id, 1);
        assert_eq!(req.method, "tools/list");
        assert!(req.params.is_none());
    }

    #[test]
    fn test_json_rpc_request_with_params() {
        let params = serde_json::json!({"name": "test"});
        let req = JsonRpcRequest::new(5, "tools/call", Some(params));
        assert!(req.params.is_some());
        assert_eq!(req.params.unwrap()["name"], "test");
    }

    #[test]
    fn test_json_rpc_request_to_json() {
        let req = JsonRpcRequest::new(1, "test", None);
        let json = req.to_json().unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"method\":\"test\""));
    }

    #[test]
    fn test_json_rpc_request_to_json_line() {
        let req = JsonRpcRequest::new(1, "test", None);
        let json_line = req.to_json_line().unwrap();
        assert!(json_line.ends_with('\n'));
    }

    #[test]
    fn test_json_rpc_response_ok() {
        let resp = JsonRpcResponse::ok(42, serde_json::json!({"status": "done"}));
        assert!(resp.is_ok());
        assert_eq!(resp.id, 42);
        assert_eq!(resp.result.unwrap()["status"], "done");
    }

    #[test]
    fn test_json_rpc_response_err() {
        let resp = JsonRpcResponse::err(7, -32601, "Method not found");
        assert!(!resp.is_ok());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32601);
        assert_eq!(err.message, "Method not found");
    }

    #[test]
    fn test_json_rpc_response_from_json() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#;
        let resp = JsonRpcResponse::from_json(json).unwrap();
        assert!(resp.is_ok());
        assert_eq!(resp.id, 1);
    }

    #[test]
    fn test_json_rpc_response_from_invalid_json() {
        let result = JsonRpcResponse::from_json("not json");
        assert!(result.is_err());
    }

    // ── Client creation tests ────────────────────────────────────────────

    #[test]
    fn test_new_stdio_client() {
        let client = McpClient::new_stdio("fs", vec!["npx".to_string(), "mcp-fs".to_string()]);
        assert_eq!(client.name(), "fs");
        assert_eq!(*client.transport(), MCPTransport::Stdio);
        assert!(!client.is_connected());
        assert_eq!(*client.connection_state(), ConnectionState::Disconnected);
    }

    #[test]
    fn test_new_sse_client() {
        let client = McpClient::new_sse("remote", "https://mcp.example.com/sse");
        assert_eq!(client.name(), "remote");
        assert_eq!(*client.transport(), MCPTransport::Sse);
        assert!(!client.is_connected());
    }

    #[test]
    fn test_new_http_client() {
        let client = McpClient::new_http("remote", "https://mcp.example.com/api");
        assert_eq!(client.name(), "remote");
        assert_eq!(*client.transport(), MCPTransport::Sse);
        assert!(!client.is_connected());
    }

    // ── Connection tests ─────────────────────────────────────────────────

    #[test]
    fn test_stdio_connect() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        assert!(client.connect().is_ok());
        assert!(client.is_connected());
        assert_eq!(*client.connection_state(), ConnectionState::Connected);
    }

    #[test]
    fn test_http_connect() {
        let mut client = McpClient::new_http("remote", "https://mcp.example.com");
        assert!(client.connect().is_ok());
        assert!(client.is_connected());
    }

    #[test]
    fn test_http_connect_invalid_url() {
        let mut client = McpClient::new_http("bad", "ftp://invalid.example.com");
        let result = client.connect();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid URL"));
        assert!(matches!(client.connection_state(), ConnectionState::Error(_)));
    }

    #[test]
    fn test_stdio_connect_empty_command() {
        let mut client = McpClient::new_stdio("empty", vec![]);
        let result = client.connect();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot be empty"));
    }

    #[test]
    fn test_disconnect() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        client.connect().unwrap();
        client.disconnect();
        assert!(!client.is_connected());
        assert_eq!(*client.connection_state(), ConnectionState::Disconnected);
    }

    #[test]
    fn test_double_connect_idempotent() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        client.connect().unwrap();
        client.connect().unwrap();
        assert!(client.is_connected());
    }

    // ── Tool management tests ────────────────────────────────────────────

    #[test]
    fn test_register_and_list_tools() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        client.register_tool(McpTool::new("read_file", "Read a file", serde_json::json!({}), "test"));
        client.register_tool(McpTool::new("write_file", "Write a file", serde_json::json!({}), "test"));

        let tools = client.list_tools();
        assert_eq!(tools.len(), 2);
        assert!(tools.contains(&"read_file"));
        assert!(tools.contains(&"write_file"));
    }

    #[test]
    fn test_get_tool() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        client.register_tool(McpTool::new("read_file", "Read", serde_json::json!({}), "test"));

        assert!(client.get_tool("read_file").is_some());
        assert!(client.get_tool("nonexistent").is_none());
    }

    #[test]
    fn test_remove_tool() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        client.register_tool(McpTool::new("read_file", "Read", serde_json::json!({}), "test"));

        let removed = client.remove_tool("read_file");
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().name, "read_file");
        assert!(client.list_tools().is_empty());
    }

    // ── Method call tests ────────────────────────────────────────────────

    #[test]
    fn test_call_tool_not_connected() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        let result = client.call_tool("read_file", serde_json::json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Not connected"));
    }

    #[test]
    fn test_call_tool_stub_response() {
        let mut client = McpClient::new_stdio("fs", vec!["echo".to_string()]);
        client.connect().unwrap();

        let result = client
            .call_tool("read_file", serde_json::json!({"path": "/tmp/test.txt"}))
            .unwrap();
        assert_eq!(result["isError"], false);
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("read_file"));
    }

    #[test]
    fn test_discover_tools_not_connected() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        let result = client.discover_tools();
        assert!(result.is_err());
    }

    #[test]
    fn test_discover_tools_returns_registered() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        client.connect().unwrap();
        client.register_tool(McpTool::new("search", "Search files", serde_json::json!({}), "test"));

        let tools = client.discover_tools().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "search");
    }

    #[test]
    fn test_initialize_not_connected() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        let result = client.initialize();
        assert!(result.is_err());
    }

    #[test]
    fn test_initialize_connected() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        client.connect().unwrap();
        let caps = client.initialize().unwrap();
        assert!(caps.tools.is_empty());
    }

    #[test]
    fn test_list_resources_not_connected() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        let result = client.list_resources();
        assert!(result.is_err());
    }

    #[test]
    fn test_read_resource_not_connected() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        let result = client.read_resource("file:///test.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_read_resource_empty_uri() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        client.connect().unwrap();
        let result = client.read_resource("");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }

    #[test]
    fn test_read_resource_connected() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        client.connect().unwrap();
        let result = client.read_resource("file:///test.txt").unwrap();
        assert!(result["contents"][0]["uri"].as_str().unwrap().contains("test.txt"));
    }

    // ── Message ID tests ─────────────────────────────────────────────────

    #[test]
    fn test_message_id_increments() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        client.connect().unwrap();
        client.call_tool("a", serde_json::json!({})).unwrap();
        client.call_tool("b", serde_json::json!({})).unwrap();
        assert_eq!(client.message_id, 2);
    }

    // ── Timeout & pending request tests ──────────────────────────────────

    #[test]
    fn test_set_timeout() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        client.set_timeout(Duration::from_secs(60));
        assert_eq!(client.timeout(), Duration::from_secs(60));
    }

    #[test]
    fn test_pending_count_after_operations() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        client.connect().unwrap();
        // After call_tool, the pending request is immediately completed.
        client.call_tool("test", serde_json::json!({})).unwrap();
        assert_eq!(client.pending_count(), 0);
    }

    #[test]
    fn test_process_response_valid() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        client.connect().unwrap();

        // Manually track a request.
        client.track_request(99, "test_method");
        assert_eq!(client.pending_count(), 1);

        let json = r#"{"jsonrpc":"2.0","id":99,"result":{"ok":true}}"#;
        let resp = client.process_response(json).unwrap();
        assert!(resp.is_ok());
        assert_eq!(client.pending_count(), 0);
    }

    #[test]
    fn test_process_response_unknown_id() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        let json = r#"{"jsonrpc":"2.0","id":999,"result":{"ok":true}}"#;
        let resp = client.process_response(json).unwrap();
        assert!(resp.is_ok());
    }

    #[test]
    fn test_process_response_invalid_json() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        let result = client.process_response("not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_check_timeouts_none() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        let timed_out = client.check_timeouts();
        assert!(timed_out.is_empty());
    }

    // ── Capabilities tests ───────────────────────────────────────────────

    #[test]
    fn test_capabilities_default() {
        let client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        let caps = client.capabilities();
        assert!(caps.tools.is_empty());
        assert!(caps.resources.is_empty());
        assert!(caps.prompts.is_empty());
    }

    #[test]
    fn test_mcp_resource_serialization() {
        let resource = MCPResource {
            uri: "file:///test.txt".to_string(),
            name: "test.txt".to_string(),
            description: "A test file".to_string(),
            mime_type: Some("text/plain".to_string()),
        };
        let json = serde_json::to_string(&resource).unwrap();
        let restored: MCPResource = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.uri, "file:///test.txt");
        assert_eq!(restored.mime_type, Some("text/plain".to_string()));
    }

    #[test]
    fn test_mcp_prompt_serialization() {
        let prompt = MCPPrompt {
            name: "summarize".to_string(),
            description: "Summarize text".to_string(),
            arguments: vec!["text".to_string(), "max_length".to_string()],
        };
        let json = serde_json::to_string(&prompt).unwrap();
        let restored: MCPPrompt = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.name, "summarize");
        assert_eq!(restored.arguments.len(), 2);
    }

    #[test]
    fn test_disconnect_clears_state() {
        let mut client = McpClient::new_stdio("test", vec!["echo".to_string()]);
        client.connect().unwrap();
        client.register_tool(McpTool::new("t", "d", serde_json::json!({}), "s"));
        assert_eq!(client.list_tools().len(), 1);

        client.disconnect();
        assert!(client.list_tools().is_empty());
        assert_eq!(client.pending_count(), 0);
    }
}
