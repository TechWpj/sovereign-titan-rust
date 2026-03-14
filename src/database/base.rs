//! Abstract database connector interface and common data types.
//!
//! Defines the core result types used by all database backends.
//! Each backend (SQLite, etc.) returns data through these common types
//! to ensure a uniform API regardless of the underlying engine.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Result of a database query execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    /// Whether the query executed successfully.
    pub success: bool,
    /// Rows returned by a SELECT query. Each row is a column-name to value map.
    pub rows: Vec<HashMap<String, serde_json::Value>>,
    /// Number of rows affected by INSERT/UPDATE/DELETE.
    pub rows_affected: u64,
    /// Error message if the query failed.
    pub error: Option<String>,
}

impl QueryResult {
    /// Create a successful query result with rows.
    pub fn ok_rows(rows: Vec<HashMap<String, serde_json::Value>>) -> Self {
        Self {
            success: true,
            rows,
            rows_affected: 0,
            error: None,
        }
    }

    /// Create a successful mutation result (INSERT/UPDATE/DELETE).
    pub fn ok_affected(rows_affected: u64) -> Self {
        Self {
            success: true,
            rows: Vec::new(),
            rows_affected,
            error: None,
        }
    }

    /// Create an error result.
    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            rows: Vec::new(),
            rows_affected: 0,
            error: Some(msg.into()),
        }
    }
}

/// Schema information for an entire database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaInfo {
    /// All tables in the database.
    pub tables: Vec<TableInfo>,
}

/// Schema information for a single table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableInfo {
    /// Table name.
    pub name: String,
    /// Columns in the table.
    pub columns: Vec<ColumnInfo>,
}

/// Schema information for a single column.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnInfo {
    /// Column name.
    pub name: String,
    /// Data type (e.g. "TEXT", "INTEGER", "REAL").
    pub data_type: String,
    /// Whether the column allows NULL values.
    pub nullable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_result_ok_rows() {
        let mut row = HashMap::new();
        row.insert("id".to_string(), serde_json::json!(1));
        row.insert("name".to_string(), serde_json::json!("Alice"));

        let result = QueryResult::ok_rows(vec![row]);
        assert!(result.success);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows_affected, 0);
        assert!(result.error.is_none());
        assert_eq!(result.rows[0]["name"], serde_json::json!("Alice"));
    }

    #[test]
    fn test_query_result_ok_affected() {
        let result = QueryResult::ok_affected(5);
        assert!(result.success);
        assert!(result.rows.is_empty());
        assert_eq!(result.rows_affected, 5);
        assert!(result.error.is_none());
    }

    #[test]
    fn test_query_result_err() {
        let result = QueryResult::err("syntax error near WHERE");
        assert!(!result.success);
        assert!(result.rows.is_empty());
        assert_eq!(result.rows_affected, 0);
        assert_eq!(result.error.as_deref(), Some("syntax error near WHERE"));
    }

    #[test]
    fn test_schema_info_serialization() {
        let schema = SchemaInfo {
            tables: vec![TableInfo {
                name: "users".to_string(),
                columns: vec![
                    ColumnInfo {
                        name: "id".to_string(),
                        data_type: "INTEGER".to_string(),
                        nullable: false,
                    },
                    ColumnInfo {
                        name: "email".to_string(),
                        data_type: "TEXT".to_string(),
                        nullable: true,
                    },
                ],
            }],
        };

        let json = serde_json::to_string(&schema).unwrap();
        let deserialized: SchemaInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.tables.len(), 1);
        assert_eq!(deserialized.tables[0].name, "users");
        assert_eq!(deserialized.tables[0].columns.len(), 2);
        assert!(!deserialized.tables[0].columns[0].nullable);
        assert!(deserialized.tables[0].columns[1].nullable);
    }

    #[test]
    fn test_query_result_serialization_roundtrip() {
        let mut row = HashMap::new();
        row.insert("count".to_string(), serde_json::json!(42));
        let original = QueryResult::ok_rows(vec![row]);

        let json = serde_json::to_string(&original).unwrap();
        let restored: QueryResult = serde_json::from_str(&json).unwrap();
        assert!(restored.success);
        assert_eq!(restored.rows[0]["count"], serde_json::json!(42));
    }
}
