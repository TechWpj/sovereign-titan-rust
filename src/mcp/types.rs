//! MCP data structures — JSON-RPC request/response types and tool descriptors.
//!
//! Implements the core wire types for the Model Context Protocol, which
//! uses JSON-RPC 2.0 over stdio or HTTP transports.

use serde::{Deserialize, Serialize};

/// A tool exposed by an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    /// Tool name (unique within a server).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema describing the tool's input parameters.
    pub parameters: serde_json::Value,
    /// Name of the MCP server that provides this tool.
    pub server_name: String,
}

impl McpTool {
    /// Create a new MCP tool descriptor.
    pub fn new(name: &str, description: &str, parameters: serde_json::Value, server_name: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            parameters,
            server_name: server_name.to_string(),
        }
    }

    /// Convert this tool to a JSON Schema representation suitable for
    /// inclusion in an LLM tool-calling prompt.
    pub fn to_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters,
            }
        })
    }
}

/// A JSON-RPC 2.0 request sent to an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRequest {
    /// JSON-RPC version (always "2.0").
    pub jsonrpc: String,
    /// Unique request identifier.
    pub id: u64,
    /// Method name (e.g. "tools/list", "tools/call").
    pub method: String,
    /// Method parameters.
    pub params: serde_json::Value,
}

impl McpRequest {
    /// Create a new JSON-RPC 2.0 request.
    pub fn new(id: u64, method: &str, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params,
        }
    }
}

/// A JSON-RPC 2.0 response from an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResponse {
    /// JSON-RPC version (always "2.0").
    pub jsonrpc: String,
    /// Request identifier this response corresponds to.
    pub id: u64,
    /// Successful result (mutually exclusive with `error`).
    pub result: Option<serde_json::Value>,
    /// Error result (mutually exclusive with `result`).
    pub error: Option<McpError>,
}

impl McpResponse {
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
            error: Some(McpError {
                code,
                message: message.to_string(),
            }),
        }
    }

    /// Whether this response indicates success.
    pub fn is_ok(&self) -> bool {
        self.error.is_none() && self.result.is_some()
    }
}

/// An error object within a JSON-RPC response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpError {
    /// Numeric error code (e.g. -32600 for invalid request).
    pub code: i64,
    /// Human-readable error message.
    pub message: String,
}

/// Standard JSON-RPC error codes.
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_tool_new() {
        let tool = McpTool::new(
            "read_file",
            "Read the contents of a file",
            serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            "filesystem",
        );
        assert_eq!(tool.name, "read_file");
        assert_eq!(tool.server_name, "filesystem");
    }

    #[test]
    fn test_mcp_tool_to_schema() {
        let tool = McpTool::new(
            "search",
            "Search for files",
            serde_json::json!({"type": "object", "properties": {"query": {"type": "string"}}}),
            "search-server",
        );

        let schema = tool.to_schema();
        assert_eq!(schema["type"], "function");
        assert_eq!(schema["function"]["name"], "search");
        assert_eq!(schema["function"]["description"], "Search for files");
        assert!(schema["function"]["parameters"]["properties"]["query"].is_object());
    }

    #[test]
    fn test_mcp_request_new() {
        let req = McpRequest::new(1, "tools/list", serde_json::json!({}));
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.id, 1);
        assert_eq!(req.method, "tools/list");
    }

    #[test]
    fn test_mcp_response_ok() {
        let resp = McpResponse::ok(42, serde_json::json!({"status": "done"}));
        assert!(resp.is_ok());
        assert_eq!(resp.id, 42);
        assert_eq!(resp.result.unwrap()["status"], "done");
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_mcp_response_err() {
        let resp = McpResponse::err(7, METHOD_NOT_FOUND, "Method does not exist");
        assert!(!resp.is_ok());
        assert_eq!(resp.id, 7);
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32601);
        assert_eq!(err.message, "Method does not exist");
    }

    #[test]
    fn test_request_serialization() {
        let req = McpRequest::new(99, "tools/call", serde_json::json!({"name": "read_file"}));
        let json = serde_json::to_string(&req).unwrap();
        let parsed: McpRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, 99);
        assert_eq!(parsed.method, "tools/call");
    }

    #[test]
    fn test_response_serialization_roundtrip() {
        let original = McpResponse::ok(5, serde_json::json!(["a", "b", "c"]));
        let json = serde_json::to_string(&original).unwrap();
        let restored: McpResponse = serde_json::from_str(&json).unwrap();
        assert!(restored.is_ok());
        assert_eq!(restored.result.unwrap(), serde_json::json!(["a", "b", "c"]));
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(PARSE_ERROR, -32700);
        assert_eq!(INVALID_REQUEST, -32600);
        assert_eq!(METHOD_NOT_FOUND, -32601);
        assert_eq!(INVALID_PARAMS, -32602);
        assert_eq!(INTERNAL_ERROR, -32603);
    }
}
