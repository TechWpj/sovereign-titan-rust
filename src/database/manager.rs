//! Multi-database connection manager.
//!
//! Manages named database connections with automatic type detection,
//! query caching, and schema introspection across multiple databases.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use super::base::{QueryResult, SchemaInfo};
use super::sqlite::SqliteConnector;

/// Information about an active database connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    /// Friendly name for this connection.
    pub name: String,
    /// Database type (e.g. "sqlite").
    pub db_type: String,
    /// Connection string or file path.
    pub connection_string: String,
    /// Whether the connection is currently active.
    pub connected: bool,
}

/// A named database connection wrapping a backend connector.
pub struct DatabaseConnection {
    /// Friendly name for this connection.
    pub name: String,
    /// Database type (e.g. "sqlite").
    pub db_type: String,
    /// The connection string used to open this connection.
    pub connection_string: String,
    /// The underlying SQLite connector (extensible to other backends later).
    pub connector: SqliteConnector,
}

/// Manages multiple named database connections.
///
/// Supports automatic type detection from file extensions, query caching
/// for repeated reads, and bulk operations across all connections.
pub struct DatabaseManager {
    /// Named connections keyed by their friendly name.
    connections: HashMap<String, DatabaseConnection>,
    /// Simple query result cache keyed by "connection_name:query".
    query_cache: HashMap<String, QueryResult>,
}

impl DatabaseManager {
    /// Create a new empty database manager.
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
            query_cache: HashMap::new(),
        }
    }

    /// Connect to a database under the given name.
    ///
    /// The database type is auto-detected from the connection string:
    /// - Paths ending in `.db`, `.sqlite`, or `.sqlite3` use SQLite.
    ///
    /// Returns an error if the type cannot be detected or connection fails.
    pub fn connect(&mut self, name: &str, connection_string: &str) -> Result<(), String> {
        if self.connections.contains_key(name) {
            warn!("DatabaseManager: duplicate connection name '{name}' rejected");
            return Err(format!("Connection '{name}' already exists"));
        }

        let db_type = Self::detect_type(connection_string)?;
        info!(
            "DatabaseManager: connecting '{name}' ({db_type}) -> {connection_string}"
        );

        match db_type.as_str() {
            "sqlite" => {
                let mut connector = SqliteConnector::new(connection_string);
                connector.connect()?;
                self.connections.insert(
                    name.to_string(),
                    DatabaseConnection {
                        name: name.to_string(),
                        db_type,
                        connection_string: connection_string.to_string(),
                        connector,
                    },
                );
                Ok(())
            }
            other => Err(format!("Unsupported database type: {other}")),
        }
    }

    /// Disconnect and remove a named connection. Returns true if it existed.
    pub fn disconnect(&mut self, name: &str) -> bool {
        if let Some(mut conn) = self.connections.remove(name) {
            conn.connector.disconnect();
            // Invalidate cached queries for this connection.
            self.query_cache
                .retain(|key, _| !key.starts_with(&format!("{name}:")));
            debug!("DatabaseManager: disconnected '{name}'");
            true
        } else {
            false
        }
    }

    /// Execute a query on a named connection.
    pub fn query(&mut self, name: &str, query: &str) -> Result<QueryResult, String> {
        let conn = self
            .connections
            .get(name)
            .ok_or_else(|| format!("No connection named '{name}'"))?;

        let result = conn.connector.execute(query)?;

        // Cache successful SELECT results.
        if result.success && query.trim().to_uppercase().starts_with("SELECT") {
            let cache_key = format!("{name}:{query}");
            self.query_cache.insert(cache_key, result.clone());
        }

        Ok(result)
    }

    /// Get a cached query result, if available.
    pub fn get_cached(&self, name: &str, query: &str) -> Option<&QueryResult> {
        let cache_key = format!("{name}:{query}");
        self.query_cache.get(&cache_key)
    }

    /// Clear the query cache (all connections or a specific one).
    pub fn clear_cache(&mut self, name: Option<&str>) {
        match name {
            Some(n) => self.query_cache.retain(|k, _| !k.starts_with(&format!("{n}:"))),
            None => self.query_cache.clear(),
        }
    }

    /// List all active connections.
    pub fn list_connections(&self) -> Vec<ConnectionInfo> {
        self.connections
            .values()
            .map(|conn| ConnectionInfo {
                name: conn.name.clone(),
                db_type: conn.db_type.clone(),
                connection_string: conn.connection_string.clone(),
                connected: conn.connector.is_connected(),
            })
            .collect()
    }

    /// Get the schema for a named connection.
    pub fn get_schema(&self, name: &str) -> Result<SchemaInfo, String> {
        let conn = self
            .connections
            .get(name)
            .ok_or_else(|| format!("No connection named '{name}'"))?;
        conn.connector.get_schema()
    }

    /// Number of active connections.
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Auto-detect database type from the connection string.
    fn detect_type(connection_string: &str) -> Result<String, String> {
        let lower = connection_string.to_lowercase();
        if lower.ends_with(".db")
            || lower.ends_with(".sqlite")
            || lower.ends_with(".sqlite3")
            || lower.starts_with("sqlite:")
        {
            Ok("sqlite".to_string())
        } else {
            Err(format!(
                "Cannot detect database type from connection string: '{connection_string}'. \
                 Supported extensions: .db, .sqlite, .sqlite3"
            ))
        }
    }
}

impl Default for DatabaseManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db_path(name: &str) -> String {
        let dir = std::env::temp_dir().join("titan_dbmgr_test");
        let _ = std::fs::create_dir_all(&dir);
        dir.join(format!("{name}_{}.db", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .to_string()
    }

    #[test]
    fn test_new_manager() {
        let mgr = DatabaseManager::new();
        assert_eq!(mgr.connection_count(), 0);
        assert!(mgr.list_connections().is_empty());
    }

    #[test]
    fn test_connect_sqlite() {
        let path = temp_db_path("mgr_connect");
        let mut mgr = DatabaseManager::new();
        mgr.connect("test", &path).unwrap();
        assert_eq!(mgr.connection_count(), 1);
        let conns = mgr.list_connections();
        assert_eq!(conns[0].name, "test");
        assert_eq!(conns[0].db_type, "sqlite");
        assert!(conns[0].connected);
        mgr.disconnect("test");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_connect_duplicate_name() {
        let path = temp_db_path("mgr_dup");
        let mut mgr = DatabaseManager::new();
        mgr.connect("mydb", &path).unwrap();
        let result = mgr.connect("mydb", &path);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already exists"));
        mgr.disconnect("mydb");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_disconnect_nonexistent() {
        let mut mgr = DatabaseManager::new();
        assert!(!mgr.disconnect("nope"));
    }

    #[test]
    fn test_query_through_manager() {
        let path = temp_db_path("mgr_query");
        let mut mgr = DatabaseManager::new();
        mgr.connect("qdb", &path).unwrap();

        mgr.query("qdb", "CREATE TABLE t (id INTEGER PRIMARY KEY, val TEXT)")
            .unwrap();
        mgr.query("qdb", "INSERT INTO t (val) VALUES ('hello')")
            .unwrap();

        let result = mgr.query("qdb", "SELECT * FROM t").unwrap();
        assert!(result.success);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0]["val"], serde_json::json!("hello"));

        mgr.disconnect("qdb");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_query_caching() {
        let path = temp_db_path("mgr_cache");
        let mut mgr = DatabaseManager::new();
        mgr.connect("cdb", &path).unwrap();

        mgr.query("cdb", "CREATE TABLE t (id INTEGER PRIMARY KEY)")
            .unwrap();

        // This SELECT should be cached.
        mgr.query("cdb", "SELECT * FROM t").unwrap();
        let cached = mgr.get_cached("cdb", "SELECT * FROM t");
        assert!(cached.is_some());
        assert!(cached.unwrap().success);

        // Clear cache.
        mgr.clear_cache(Some("cdb"));
        assert!(mgr.get_cached("cdb", "SELECT * FROM t").is_none());

        mgr.disconnect("cdb");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_get_schema_through_manager() {
        let path = temp_db_path("mgr_schema");
        let mut mgr = DatabaseManager::new();
        mgr.connect("sdb", &path).unwrap();

        mgr.query("sdb", "CREATE TABLE widgets (id INTEGER PRIMARY KEY, name TEXT)")
            .unwrap();

        let schema = mgr.get_schema("sdb").unwrap();
        assert_eq!(schema.tables.len(), 1);
        assert_eq!(schema.tables[0].name, "widgets");

        mgr.disconnect("sdb");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_detect_type_unknown() {
        let result = DatabaseManager::connect(&mut DatabaseManager::new(), "test", "/tmp/data.csv");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Cannot detect"));
    }

    #[test]
    fn test_query_nonexistent_connection() {
        let mut mgr = DatabaseManager::new();
        let result = mgr.query("ghost", "SELECT 1");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No connection named"));
    }
}
