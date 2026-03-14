//! Tool Parameter Schemas — schema definitions, validation, and defaults.
//!
//! Provides a `ToolSchema` struct that describes the expected parameters for
//! a tool, including types, required fields, and defaults. The `validate_input`
//! function checks a `serde_json::Value` against a schema and returns detailed
//! errors on mismatch.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ─────────────────────────────────────────────────────────────────────────────
// Schema types
// ─────────────────────────────────────────────────────────────────────────────

/// The type of a parameter in a tool schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamType {
    String,
    Number,
    Boolean,
    Array,
    Object,
}

impl std::fmt::Display for ParamType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParamType::String => write!(f, "string"),
            ParamType::Number => write!(f, "number"),
            ParamType::Boolean => write!(f, "boolean"),
            ParamType::Array => write!(f, "array"),
            ParamType::Object => write!(f, "object"),
        }
    }
}

/// Schema for a single parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamSchema {
    /// Parameter name (matches JSON key).
    pub name: String,
    /// Expected type.
    pub param_type: ParamType,
    /// Human-readable description.
    pub description: String,
    /// Default value (used when parameter is absent and not required).
    pub default: Option<Value>,
}

/// Schema for an entire tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    /// Tool name (must match `Tool::name()`).
    pub name: String,
    /// Human-readable description of the tool.
    pub description: String,
    /// All parameters the tool accepts.
    pub parameters: Vec<ParamSchema>,
    /// Names of parameters that are required.
    pub required: Vec<String>,
}

impl ToolSchema {
    /// Get a parameter schema by name.
    pub fn param(&self, name: &str) -> Option<&ParamSchema> {
        self.parameters.iter().find(|p| p.name == name)
    }

    /// Check whether a parameter is required.
    pub fn is_required(&self, name: &str) -> bool {
        self.required.iter().any(|r| r == name)
    }

    /// Return the list of parameter names.
    pub fn param_names(&self) -> Vec<&str> {
        self.parameters.iter().map(|p| p.name.as_str()).collect()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Validation
// ─────────────────────────────────────────────────────────────────────────────

/// Check whether a JSON value matches the expected `ParamType`.
fn value_matches_type(value: &Value, expected: &ParamType) -> bool {
    match expected {
        ParamType::String => value.is_string(),
        ParamType::Number => value.is_number(),
        ParamType::Boolean => value.is_boolean(),
        ParamType::Array => value.is_array(),
        ParamType::Object => value.is_object(),
    }
}

/// Validate an input `Value` against a `ToolSchema`.
///
/// Checks:
/// 1. Input must be a JSON object.
/// 2. All required fields must be present.
/// 3. Every present field that has a schema entry must match the expected type.
///
/// Returns `Ok(())` on success, or an `Err` with a descriptive message listing
/// all validation errors.
pub fn validate_input(schema: &ToolSchema, input: &Value) -> Result<()> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => bail!(
            "Tool '{}' expects a JSON object, got {}",
            schema.name,
            value_type_name(input)
        ),
    };

    let mut errors: Vec<String> = Vec::new();

    // Check required fields
    for req in &schema.required {
        if !obj.contains_key(req) {
            errors.push(format!("Missing required parameter: '{req}'"));
        }
    }

    // Type-check present fields
    for (key, value) in obj {
        if let Some(param) = schema.param(key) {
            if !value_matches_type(value, &param.param_type) {
                errors.push(format!(
                    "Parameter '{}' expected type {}, got {}",
                    key,
                    param.param_type,
                    value_type_name(value)
                ));
            }
        }
        // Unknown fields are allowed (ignored) — tools can handle them freely.
    }

    if errors.is_empty() {
        Ok(())
    } else {
        bail!(
            "Validation failed for tool '{}':\n  - {}",
            schema.name,
            errors.join("\n  - ")
        );
    }
}

/// Produce a human-readable type name for a JSON value.
fn value_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Default schemas for key tools
// ─────────────────────────────────────────────────────────────────────────────

/// Return a set of schemas for the most commonly used tools.
pub fn default_schemas() -> Vec<ToolSchema> {
    vec![
        ToolSchema {
            name: "file_search".to_string(),
            description: "Search for files by name, extension, or content.".to_string(),
            parameters: vec![
                ParamSchema {
                    name: "action".to_string(),
                    param_type: ParamType::String,
                    description: "Search action: find, grep, read, list.".to_string(),
                    default: Some(Value::String("find".to_string())),
                },
                ParamSchema {
                    name: "query".to_string(),
                    param_type: ParamType::String,
                    description: "Search query or filename pattern.".to_string(),
                    default: None,
                },
                ParamSchema {
                    name: "path".to_string(),
                    param_type: ParamType::String,
                    description: "Directory to search in.".to_string(),
                    default: Some(Value::String(".".to_string())),
                },
            ],
            required: vec!["query".to_string()],
        },
        ToolSchema {
            name: "shell".to_string(),
            description: "Execute shell commands.".to_string(),
            parameters: vec![
                ParamSchema {
                    name: "command".to_string(),
                    param_type: ParamType::String,
                    description: "Shell command to execute.".to_string(),
                    default: None,
                },
                ParamSchema {
                    name: "timeout".to_string(),
                    param_type: ParamType::Number,
                    description: "Timeout in seconds.".to_string(),
                    default: Some(Value::Number(serde_json::Number::from(30))),
                },
            ],
            required: vec!["command".to_string()],
        },
        ToolSchema {
            name: "calculator".to_string(),
            description: "Evaluate mathematical expressions.".to_string(),
            parameters: vec![ParamSchema {
                name: "expression".to_string(),
                param_type: ParamType::String,
                description: "Mathematical expression to evaluate.".to_string(),
                default: None,
            }],
            required: vec!["expression".to_string()],
        },
        ToolSchema {
            name: "web".to_string(),
            description: "Web search and page retrieval.".to_string(),
            parameters: vec![
                ParamSchema {
                    name: "action".to_string(),
                    param_type: ParamType::String,
                    description: "Action: search, fetch, screenshot.".to_string(),
                    default: Some(Value::String("search".to_string())),
                },
                ParamSchema {
                    name: "query".to_string(),
                    param_type: ParamType::String,
                    description: "Search query or URL.".to_string(),
                    default: None,
                },
            ],
            required: vec!["query".to_string()],
        },
        ToolSchema {
            name: "system_control".to_string(),
            description: "Control system operations (launch apps, adjust settings).".to_string(),
            parameters: vec![
                ParamSchema {
                    name: "action".to_string(),
                    param_type: ParamType::String,
                    description: "Action to perform.".to_string(),
                    default: None,
                },
                ParamSchema {
                    name: "target".to_string(),
                    param_type: ParamType::String,
                    description: "Target application or system resource.".to_string(),
                    default: None,
                },
            ],
            required: vec!["action".to_string()],
        },
        ToolSchema {
            name: "file_ops".to_string(),
            description: "File operations (copy, move, delete, rename, etc.).".to_string(),
            parameters: vec![
                ParamSchema {
                    name: "action".to_string(),
                    param_type: ParamType::String,
                    description: "Operation: copy, move_file, delete, rename, create_dir, exists, size.".to_string(),
                    default: None,
                },
                ParamSchema {
                    name: "path".to_string(),
                    param_type: ParamType::String,
                    description: "File or directory path.".to_string(),
                    default: None,
                },
                ParamSchema {
                    name: "source".to_string(),
                    param_type: ParamType::String,
                    description: "Source path (for copy/move).".to_string(),
                    default: None,
                },
                ParamSchema {
                    name: "destination".to_string(),
                    param_type: ParamType::String,
                    description: "Destination path (for copy/move).".to_string(),
                    default: None,
                },
            ],
            required: vec!["action".to_string()],
        },
        ToolSchema {
            name: "container_tools".to_string(),
            description: "Docker/container management.".to_string(),
            parameters: vec![
                ParamSchema {
                    name: "action".to_string(),
                    param_type: ParamType::String,
                    description: "Action: list_containers, start, stop, logs, images, pull, run.".to_string(),
                    default: Some(Value::String("list_containers".to_string())),
                },
                ParamSchema {
                    name: "name".to_string(),
                    param_type: ParamType::String,
                    description: "Container name (for start/stop/logs).".to_string(),
                    default: None,
                },
                ParamSchema {
                    name: "image".to_string(),
                    param_type: ParamType::String,
                    description: "Image name (for pull/run).".to_string(),
                    default: None,
                },
            ],
            required: vec!["action".to_string()],
        },
        ToolSchema {
            name: "external_ai".to_string(),
            description: "Query external AI APIs.".to_string(),
            parameters: vec![
                ParamSchema {
                    name: "action".to_string(),
                    param_type: ParamType::String,
                    description: "Action: query, list_providers.".to_string(),
                    default: Some(Value::String("list_providers".to_string())),
                },
                ParamSchema {
                    name: "provider".to_string(),
                    param_type: ParamType::String,
                    description: "AI provider: openai, anthropic, gemini.".to_string(),
                    default: None,
                },
                ParamSchema {
                    name: "prompt".to_string(),
                    param_type: ParamType::String,
                    description: "Prompt to send to the AI.".to_string(),
                    default: None,
                },
                ParamSchema {
                    name: "api_key".to_string(),
                    param_type: ParamType::String,
                    description: "API key for the provider.".to_string(),
                    default: None,
                },
            ],
            required: vec!["action".to_string()],
        },
        ToolSchema {
            name: "preflight".to_string(),
            description: "Pre-flight system health checks.".to_string(),
            parameters: vec![ParamSchema {
                name: "action".to_string(),
                param_type: ParamType::String,
                description: "Check: check_all, check_network, check_disk, check_memory, check_gpu.".to_string(),
                default: Some(Value::String("check_all".to_string())),
            }],
            required: vec![],
        },
    ]
}

/// Look up a schema by tool name from the default set.
pub fn schema_for(tool_name: &str) -> Option<ToolSchema> {
    default_schemas().into_iter().find(|s| s.name == tool_name)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_test_schema() -> ToolSchema {
        ToolSchema {
            name: "test_tool".to_string(),
            description: "A test tool.".to_string(),
            parameters: vec![
                ParamSchema {
                    name: "action".to_string(),
                    param_type: ParamType::String,
                    description: "The action.".to_string(),
                    default: None,
                },
                ParamSchema {
                    name: "count".to_string(),
                    param_type: ParamType::Number,
                    description: "A number.".to_string(),
                    default: Some(Value::Number(serde_json::Number::from(10))),
                },
                ParamSchema {
                    name: "verbose".to_string(),
                    param_type: ParamType::Boolean,
                    description: "Verbose flag.".to_string(),
                    default: Some(Value::Bool(false)),
                },
                ParamSchema {
                    name: "tags".to_string(),
                    param_type: ParamType::Array,
                    description: "Tags list.".to_string(),
                    default: None,
                },
                ParamSchema {
                    name: "config".to_string(),
                    param_type: ParamType::Object,
                    description: "Config object.".to_string(),
                    default: None,
                },
            ],
            required: vec!["action".to_string()],
        }
    }

    #[test]
    fn test_validate_required() {
        let schema = make_test_schema();
        let input = json!({"action": "run", "count": 5});
        assert!(validate_input(&schema, &input).is_ok());
    }

    #[test]
    fn test_validate_missing_required() {
        let schema = make_test_schema();
        let input = json!({"count": 5});
        let err = validate_input(&schema, &input);
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("Missing required parameter"));
        assert!(msg.contains("action"));
    }

    #[test]
    fn test_validate_types() {
        let schema = make_test_schema();

        // All correct types
        let good = json!({
            "action": "test",
            "count": 42,
            "verbose": true,
            "tags": ["a", "b"],
            "config": {"key": "value"}
        });
        assert!(validate_input(&schema, &good).is_ok());
    }

    #[test]
    fn test_validate_wrong_type() {
        let schema = make_test_schema();

        // count should be a number, not a string
        let bad = json!({"action": "test", "count": "not_a_number"});
        let err = validate_input(&schema, &bad);
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("count"));
        assert!(msg.contains("number"));
    }

    #[test]
    fn test_validate_wrong_boolean_type() {
        let schema = make_test_schema();
        let bad = json!({"action": "test", "verbose": "yes"});
        let err = validate_input(&schema, &bad);
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("verbose"));
        assert!(msg.contains("boolean"));
    }

    #[test]
    fn test_validate_wrong_array_type() {
        let schema = make_test_schema();
        let bad = json!({"action": "test", "tags": "not_an_array"});
        let err = validate_input(&schema, &bad);
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("tags"));
        assert!(msg.contains("array"));
    }

    #[test]
    fn test_validate_wrong_object_type() {
        let schema = make_test_schema();
        let bad = json!({"action": "test", "config": [1, 2, 3]});
        let err = validate_input(&schema, &bad);
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("config"));
        assert!(msg.contains("object"));
    }

    #[test]
    fn test_validate_non_object_input() {
        let schema = make_test_schema();
        let bad = json!("just a string");
        let err = validate_input(&schema, &bad);
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("expects a JSON object"));
    }

    #[test]
    fn test_validate_unknown_fields_allowed() {
        let schema = make_test_schema();
        // Extra field "unknown" not in schema — should be fine
        let input = json!({"action": "test", "unknown": "extra"});
        assert!(validate_input(&schema, &input).is_ok());
    }

    #[test]
    fn test_default_schemas_not_empty() {
        let schemas = default_schemas();
        assert!(!schemas.is_empty());
        // Should have at least file_search, shell, calculator
        let names: Vec<&str> = schemas.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"file_search"));
        assert!(names.contains(&"shell"));
        assert!(names.contains(&"calculator"));
    }

    #[test]
    fn test_schema_for_lookup() {
        let schema = schema_for("calculator");
        assert!(schema.is_some());
        let schema = schema.unwrap();
        assert_eq!(schema.name, "calculator");
        assert!(schema.is_required("expression"));
    }

    #[test]
    fn test_schema_for_missing() {
        let schema = schema_for("nonexistent_tool");
        assert!(schema.is_none());
    }

    #[test]
    fn test_param_names() {
        let schema = make_test_schema();
        let names = schema.param_names();
        assert_eq!(names.len(), 5);
        assert!(names.contains(&"action"));
        assert!(names.contains(&"count"));
    }

    #[test]
    fn test_param_type_display() {
        assert_eq!(format!("{}", ParamType::String), "string");
        assert_eq!(format!("{}", ParamType::Number), "number");
        assert_eq!(format!("{}", ParamType::Boolean), "boolean");
        assert_eq!(format!("{}", ParamType::Array), "array");
        assert_eq!(format!("{}", ParamType::Object), "object");
    }

    #[test]
    fn test_multiple_errors_reported() {
        let schema = ToolSchema {
            name: "multi".to_string(),
            description: "Test".to_string(),
            parameters: vec![
                ParamSchema {
                    name: "a".to_string(),
                    param_type: ParamType::String,
                    description: "A".to_string(),
                    default: None,
                },
                ParamSchema {
                    name: "b".to_string(),
                    param_type: ParamType::Number,
                    description: "B".to_string(),
                    default: None,
                },
            ],
            required: vec!["a".to_string(), "b".to_string()],
        };

        // Missing both required fields
        let input = json!({});
        let err = validate_input(&schema, &input);
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("'a'"));
        assert!(msg.contains("'b'"));
    }
}
