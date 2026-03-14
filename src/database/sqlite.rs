//! SQLite database connector using the `rusqlite` crate.
//!
//! Provides a connection wrapper around rusqlite for executing queries,
//! retrieving schema information, and managing the connection lifecycle.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use super::base::{ColumnInfo, QueryResult, SchemaInfo, TableInfo};

/// Serializable snapshot of a SQLite connector's state (for persistence/diagnostics).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteConnectorSnapshot {
    /// Database file path.
    pub db_path: String,
    /// Whether the connection was active at snapshot time.
    pub connected: bool,
}

/// SQLite database connector.
pub struct SqliteConnector {
    /// Path to the SQLite database file.
    db_path: String,
    /// Whether the connection is currently open.
    connected: bool,
    /// The underlying rusqlite connection (present when connected).
    connection: Option<Connection>,
}

impl SqliteConnector {
    /// Create a new connector targeting the given database path.
    ///
    /// The database file is not opened until [`connect`] is called.
    pub fn new(db_path: &str) -> Self {
        Self {
            db_path: db_path.to_string(),
            connected: false,
            connection: None,
        }
    }

    /// Open the database connection. Creates the file if it does not exist.
    pub fn connect(&mut self) -> Result<(), String> {
        if self.connected {
            return Ok(());
        }

        // Validate the database path before attempting to open.
        let db_file = Path::new(&self.db_path);
        if let Some(parent) = db_file.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                return Err(format!(
                    "Parent directory does not exist: '{}'",
                    parent.display()
                ));
            }
        }

        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE;
        match Connection::open_with_flags(&self.db_path, flags) {
            Ok(conn) => {
                // Enable WAL mode for better concurrent read performance.
                let _ = conn.execute_batch("PRAGMA journal_mode=WAL;");
                info!("SqliteConnector: connected to {}", self.db_path);
                self.connection = Some(conn);
                self.connected = true;
                Ok(())
            }
            Err(e) => Err(format!("Failed to open SQLite database '{}': {e}", self.db_path)),
        }
    }

    /// Close the database connection.
    pub fn disconnect(&mut self) {
        if self.connected {
            self.connection = None;
            self.connected = false;
            debug!("SqliteConnector: disconnected from {}", self.db_path);
        }
    }

    /// Execute a SQL query and return the result.
    ///
    /// For SELECT queries, rows are returned in the `QueryResult::rows` field.
    /// For INSERT/UPDATE/DELETE, `rows_affected` is populated instead.
    pub fn execute(&self, query: &str) -> Result<QueryResult, String> {
        let conn = self
            .connection
            .as_ref()
            .ok_or_else(|| "Not connected to database".to_string())?;

        let trimmed = query.trim();
        let upper = trimmed.to_uppercase();

        if upper.starts_with("SELECT") || upper.starts_with("PRAGMA") || upper.starts_with("EXPLAIN") {
            self.execute_query(conn, trimmed)
        } else {
            self.execute_mutation(conn, trimmed)
        }
    }

    /// Execute a read query (SELECT, PRAGMA, EXPLAIN).
    fn execute_query(&self, conn: &Connection, query: &str) -> Result<QueryResult, String> {
        let mut stmt = conn.prepare(query).map_err(|e| format!("Prepare error: {e}"))?;

        let column_names: Vec<String> = stmt
            .column_names()
            .iter()
            .map(|c| c.to_string())
            .collect();

        let rows_result = stmt.query_map([], |row| {
            let mut map = HashMap::new();
            for (i, name) in column_names.iter().enumerate() {
                let value: rusqlite::Result<rusqlite::types::Value> = row.get(i);
                let json_val = match value {
                    Ok(rusqlite::types::Value::Null) => serde_json::Value::Null,
                    Ok(rusqlite::types::Value::Integer(n)) => serde_json::json!(n),
                    Ok(rusqlite::types::Value::Real(f)) => serde_json::json!(f),
                    Ok(rusqlite::types::Value::Text(ref s)) => serde_json::json!(s),
                    Ok(rusqlite::types::Value::Blob(ref b)) => {
                        serde_json::json!(base64::Engine::encode(
                            &base64::engine::general_purpose::STANDARD,
                            b
                        ))
                    }
                    Err(_) => serde_json::Value::Null,
                };
                map.insert(name.clone(), json_val);
            }
            Ok(map)
        });

        match rows_result {
            Ok(rows) => {
                let mut collected = Vec::new();
                for row in rows {
                    match row {
                        Ok(r) => collected.push(r),
                        Err(e) => {
                            warn!("Row read error: {e}");
                        }
                    }
                }
                Ok(QueryResult::ok_rows(collected))
            }
            Err(e) => Ok(QueryResult::err(format!("Query error: {e}"))),
        }
    }

    /// Execute a mutation query (INSERT, UPDATE, DELETE, CREATE, DROP).
    fn execute_mutation(&self, conn: &Connection, query: &str) -> Result<QueryResult, String> {
        match conn.execute(query, []) {
            Ok(rows_affected) => Ok(QueryResult::ok_affected(rows_affected as u64)),
            Err(e) => Ok(QueryResult::err(format!("Execution error: {e}"))),
        }
    }

    /// Retrieve the schema for all tables in the database.
    pub fn get_schema(&self) -> Result<SchemaInfo, String> {
        let conn = self
            .connection
            .as_ref()
            .ok_or_else(|| "Not connected to database".to_string())?;

        // Query sqlite_master for table names.
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
            .map_err(|e| format!("Schema query error: {e}"))?;

        let table_names: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| format!("Table list error: {e}"))?
            .filter_map(|r| r.ok())
            .collect();

        let mut tables = Vec::new();
        for table_name in &table_names {
            let pragma_query = format!("PRAGMA table_info(\"{}\")", table_name);
            let mut pragma_stmt = conn
                .prepare(&pragma_query)
                .map_err(|e| format!("PRAGMA error: {e}"))?;

            let columns: Vec<ColumnInfo> = pragma_stmt
                .query_map([], |row| {
                    let name: String = row.get(1)?;
                    let data_type: String = row.get(2)?;
                    let notnull: i32 = row.get(3)?;
                    Ok(ColumnInfo {
                        name,
                        data_type,
                        nullable: notnull == 0,
                    })
                })
                .map_err(|e| format!("Column info error: {e}"))?
                .filter_map(|r| r.ok())
                .collect();

            tables.push(TableInfo {
                name: table_name.clone(),
                columns,
            });
        }

        Ok(SchemaInfo { tables })
    }

    /// Check if the connector is currently connected.
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Get the database file path.
    pub fn db_path(&self) -> &str {
        &self.db_path
    }

    /// Create a serializable snapshot of the connector's current state.
    pub fn snapshot(&self) -> SqliteConnectorSnapshot {
        SqliteConnectorSnapshot {
            db_path: self.db_path.clone(),
            connected: self.connected,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db_path(name: &str) -> String {
        let dir = std::env::temp_dir().join("titan_sqlite_test");
        let _ = std::fs::create_dir_all(&dir);
        dir.join(format!("{name}_{}.db", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .to_string()
    }

    #[test]
    fn test_new_connector() {
        let connector = SqliteConnector::new("/tmp/test.db");
        assert!(!connector.is_connected());
        assert_eq!(connector.db_path(), "/tmp/test.db");
    }

    #[test]
    fn test_connect_creates_file() {
        let path = temp_db_path("connect");
        let mut connector = SqliteConnector::new(&path);
        connector.connect().unwrap();
        assert!(connector.is_connected());
        assert!(Path::new(&path).exists());
        connector.disconnect();
        assert!(!connector.is_connected());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_execute_create_and_insert() {
        let path = temp_db_path("exec");
        let mut connector = SqliteConnector::new(&path);
        connector.connect().unwrap();

        let create = connector
            .execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
            .unwrap();
        assert!(create.success);

        let insert = connector
            .execute("INSERT INTO users (name) VALUES ('Alice')")
            .unwrap();
        assert!(insert.success);
        assert_eq!(insert.rows_affected, 1);

        connector.disconnect();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_execute_select() {
        let path = temp_db_path("select");
        let mut connector = SqliteConnector::new(&path);
        connector.connect().unwrap();

        connector
            .execute("CREATE TABLE items (id INTEGER PRIMARY KEY, label TEXT)")
            .unwrap();
        connector
            .execute("INSERT INTO items (label) VALUES ('alpha')")
            .unwrap();
        connector
            .execute("INSERT INTO items (label) VALUES ('beta')")
            .unwrap();

        let result = connector.execute("SELECT * FROM items ORDER BY id").unwrap();
        assert!(result.success);
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0]["label"], serde_json::json!("alpha"));
        assert_eq!(result.rows[1]["label"], serde_json::json!("beta"));

        connector.disconnect();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_get_schema() {
        let path = temp_db_path("schema");
        let mut connector = SqliteConnector::new(&path);
        connector.connect().unwrap();

        connector
            .execute("CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER)")
            .unwrap();

        let schema = connector.get_schema().unwrap();
        assert_eq!(schema.tables.len(), 1);
        assert_eq!(schema.tables[0].name, "people");
        assert_eq!(schema.tables[0].columns.len(), 3);

        // id column
        assert_eq!(schema.tables[0].columns[0].name, "id");
        assert_eq!(schema.tables[0].columns[0].data_type, "INTEGER");

        // name column (NOT NULL)
        assert_eq!(schema.tables[0].columns[1].name, "name");
        assert!(!schema.tables[0].columns[1].nullable);

        // age column (nullable)
        assert_eq!(schema.tables[0].columns[2].name, "age");
        assert!(schema.tables[0].columns[2].nullable);

        connector.disconnect();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_execute_without_connection() {
        let connector = SqliteConnector::new("/tmp/nope.db");
        let result = connector.execute("SELECT 1");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Not connected"));
    }

    #[test]
    fn test_execute_invalid_sql() {
        let path = temp_db_path("invalid");
        let mut connector = SqliteConnector::new(&path);
        connector.connect().unwrap();

        let result = connector.execute("SELECTX INVALID SYNTAX");
        // rusqlite may return an error or a QueryResult with error
        // Either way, we should not panic
        match result {
            Ok(qr) => assert!(!qr.success),
            Err(e) => assert!(!e.is_empty()),
        }

        connector.disconnect();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_double_connect_is_idempotent() {
        let path = temp_db_path("double");
        let mut connector = SqliteConnector::new(&path);
        connector.connect().unwrap();
        connector.connect().unwrap(); // Should not error.
        assert!(connector.is_connected());
        connector.disconnect();
        let _ = std::fs::remove_file(&path);
    }
}
