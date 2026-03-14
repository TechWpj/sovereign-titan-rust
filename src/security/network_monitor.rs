//! Network Monitor — connection tracking and anomaly detection.
//!
//! Provides data structures for modelling TCP/UDP connections, snapshots
//! of the connection table, and heuristic anomaly detection (suspicious
//! ports, high external connection counts).

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// ConnectionState
// ─────────────────────────────────────────────────────────────────────────────

/// TCP connection state.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConnectionState {
    Established,
    Listen,
    TimeWait,
    CloseWait,
    SynSent,
    SynReceived,
    FinWait1,
    FinWait2,
    Closing,
    LastAck,
    Unknown,
}

impl ConnectionState {
    /// Parse a connection state from a human-readable string (case-insensitive).
    pub fn from_str_state(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "ESTABLISHED" | "ESTAB" => Self::Established,
            "LISTEN" | "LISTENING" => Self::Listen,
            "TIME_WAIT" | "TIMEWAIT" | "TIME-WAIT" => Self::TimeWait,
            "CLOSE_WAIT" | "CLOSEWAIT" | "CLOSE-WAIT" => Self::CloseWait,
            "SYN_SENT" | "SYNSENT" | "SYN-SENT" => Self::SynSent,
            "SYN_RECEIVED" | "SYNRECV" | "SYN-RECV" => Self::SynReceived,
            "FIN_WAIT_1" | "FINWAIT1" | "FIN-WAIT-1" => Self::FinWait1,
            "FIN_WAIT_2" | "FINWAIT2" | "FIN-WAIT-2" => Self::FinWait2,
            "CLOSING" => Self::Closing,
            "LAST_ACK" | "LASTACK" | "LAST-ACK" => Self::LastAck,
            _ => Self::Unknown,
        }
    }

    /// Whether this state represents an active data-carrying connection.
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Established | Self::SynSent | Self::SynReceived)
    }

    /// Whether this state is a terminal/closing state.
    pub fn is_closing(&self) -> bool {
        matches!(
            self,
            Self::TimeWait
                | Self::CloseWait
                | Self::FinWait1
                | Self::FinWait2
                | Self::Closing
                | Self::LastAck
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// NetworkConnection
// ─────────────────────────────────────────────────────────────────────────────

/// A single network connection entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConnection {
    /// Protocol (tcp, udp, tcp6, etc.).
    pub protocol: String,
    /// Local IP address.
    pub local_addr: String,
    /// Local port number.
    pub local_port: u16,
    /// Remote IP address.
    pub remote_addr: String,
    /// Remote port number.
    pub remote_port: u16,
    /// Connection state.
    pub state: ConnectionState,
    /// Process ID owning this connection.
    pub pid: Option<u32>,
    /// Process name owning this connection.
    pub process_name: Option<String>,
}

impl NetworkConnection {
    /// Whether the remote address is a loopback address.
    pub fn is_loopback(&self) -> bool {
        self.remote_addr == "127.0.0.1"
            || self.remote_addr == "::1"
            || self.remote_addr == "localhost"
    }

    /// Whether the remote address is on the public internet (not RFC-1918, not loopback).
    pub fn is_external(&self) -> bool {
        !self.is_loopback()
            && !self.remote_addr.starts_with("192.168.")
            && !self.remote_addr.starts_with("10.")
            && !self.remote_addr.starts_with("172.")
            && !self.remote_addr.is_empty()
            && self.remote_addr != "0.0.0.0"
            && self.remote_addr != "::"
            && self.remote_addr != "*"
    }

    /// Short display string for this connection.
    pub fn display_short(&self) -> String {
        format!(
            "{}:{} -> {}:{} [{}]",
            self.local_addr,
            self.local_port,
            self.remote_addr,
            self.remote_port,
            format!("{:?}", self.state)
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ConnectionSnapshot
// ─────────────────────────────────────────────────────────────────────────────

/// A point-in-time snapshot of all network connections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionSnapshot {
    /// All connections at snapshot time.
    pub connections: Vec<NetworkConnection>,
    /// Unix timestamp of the snapshot.
    pub timestamp: f64,
    /// Count of `Established` connections.
    pub total_established: usize,
    /// Count of `Listen` connections.
    pub total_listening: usize,
    /// Count of external (non-RFC-1918, non-loopback) connections.
    pub external_connections: usize,
}

impl ConnectionSnapshot {
    /// Build a snapshot from a list of connections, computing aggregates.
    pub fn from_connections(conns: Vec<NetworkConnection>) -> Self {
        let total_established = conns
            .iter()
            .filter(|c| c.state == ConnectionState::Established)
            .count();
        let total_listening = conns
            .iter()
            .filter(|c| c.state == ConnectionState::Listen)
            .count();
        let external_connections = conns.iter().filter(|c| c.is_external()).count();

        Self {
            connections: conns,
            timestamp: crate::autonomous::types::now_secs(),
            total_established,
            total_listening,
            external_connections,
        }
    }

    /// Group connections by process name.
    pub fn by_process(&self) -> HashMap<String, Vec<&NetworkConnection>> {
        let mut map: HashMap<String, Vec<&NetworkConnection>> = HashMap::new();
        for conn in &self.connections {
            let name = conn
                .process_name
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            map.entry(name).or_default().push(conn);
        }
        map
    }

    /// Get all connections involving a specific port (local or remote).
    pub fn by_port(&self, port: u16) -> Vec<&NetworkConnection> {
        self.connections
            .iter()
            .filter(|c| c.local_port == port || c.remote_port == port)
            .collect()
    }

    /// Get all connections in a specific state.
    pub fn by_state(&self, state: &ConnectionState) -> Vec<&NetworkConnection> {
        self.connections.iter().filter(|c| &c.state == state).collect()
    }

    /// Total number of connections.
    pub fn total_connections(&self) -> usize {
        self.connections.len()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// NetworkAnomaly
// ─────────────────────────────────────────────────────────────────────────────

/// A detected network anomaly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkAnomaly {
    /// Type of anomaly (e.g., "suspicious_port", "high_external_connections").
    pub anomaly_type: String,
    /// Human-readable description.
    pub description: String,
    /// Severity level (1=info, 2=warning, 3=alert, 4=critical).
    pub severity: u8,
    /// Connections associated with this anomaly.
    pub connections: Vec<NetworkConnection>,
    /// Unix timestamp when the anomaly was detected.
    pub timestamp: f64,
}

// ─────────────────────────────────────────────────────────────────────────────
// NetworkMonitor
// ─────────────────────────────────────────────────────────────────────────────

/// Monitors network connections over time and detects anomalies.
pub struct NetworkMonitor {
    /// Historical snapshots.
    snapshots: Vec<ConnectionSnapshot>,
    /// Maximum snapshots to retain.
    max_snapshots: usize,
    /// Well-known port-to-service mappings.
    known_ports: HashMap<u16, String>,
    /// Port numbers considered suspicious for outbound connections.
    suspicious_ports: Vec<u16>,
    /// All detected anomalies.
    anomalies: Vec<NetworkAnomaly>,
}

impl NetworkMonitor {
    /// Create a new monitor with a maximum snapshot history size.
    pub fn new(max_snapshots: usize) -> Self {
        Self {
            snapshots: Vec::new(),
            max_snapshots,
            known_ports: Self::default_known_ports(),
            suspicious_ports: vec![4444, 5555, 6666, 7777, 8888, 9999, 31337, 1337, 12345],
            anomalies: Vec::new(),
        }
    }

    fn default_known_ports() -> HashMap<u16, String> {
        let mut m = HashMap::new();
        for (port, name) in [
            (80, "HTTP"),
            (443, "HTTPS"),
            (22, "SSH"),
            (21, "FTP"),
            (25, "SMTP"),
            (53, "DNS"),
            (110, "POP3"),
            (143, "IMAP"),
            (3306, "MySQL"),
            (5432, "PostgreSQL"),
            (6379, "Redis"),
            (27017, "MongoDB"),
            (8080, "HTTP-Alt"),
            (8443, "HTTPS-Alt"),
            (3389, "RDP"),
            (5900, "VNC"),
            (1433, "MSSQL"),
        ] {
            m.insert(port, name.to_string());
        }
        m
    }

    /// Record a new snapshot, evicting the oldest if at capacity.
    pub fn record_snapshot(&mut self, snapshot: ConnectionSnapshot) {
        if self.snapshots.len() >= self.max_snapshots {
            self.snapshots.remove(0);
        }
        self.snapshots.push(snapshot);
    }

    /// Run anomaly detection on a snapshot, returning any anomalies found.
    pub fn detect_anomalies(&mut self, snapshot: &ConnectionSnapshot) -> Vec<NetworkAnomaly> {
        let mut anomalies = Vec::new();
        let now = crate::autonomous::types::now_secs();

        // Check for connections on suspicious ports.
        for conn in &snapshot.connections {
            if self.suspicious_ports.contains(&conn.remote_port) && conn.state.is_active() {
                anomalies.push(NetworkAnomaly {
                    anomaly_type: "suspicious_port".to_string(),
                    description: format!(
                        "Active connection to suspicious port {}",
                        conn.remote_port
                    ),
                    severity: 3,
                    connections: vec![conn.clone()],
                    timestamp: now,
                });
            }
        }

        // Check for unusually high external connection count.
        if snapshot.external_connections > 50 {
            anomalies.push(NetworkAnomaly {
                anomaly_type: "high_external_connections".to_string(),
                description: format!(
                    "{} external connections detected",
                    snapshot.external_connections
                ),
                severity: 2,
                connections: Vec::new(),
                timestamp: now,
            });
        }

        self.anomalies.extend(anomalies.clone());
        anomalies
    }

    /// Look up the service name for a well-known port.
    pub fn port_name(&self, port: u16) -> Option<&String> {
        self.known_ports.get(&port)
    }

    /// Get the most recent snapshot.
    pub fn latest_snapshot(&self) -> Option<&ConnectionSnapshot> {
        self.snapshots.last()
    }

    /// Number of recorded snapshots.
    pub fn snapshot_count(&self) -> usize {
        self.snapshots.len()
    }

    /// Number of detected anomalies.
    pub fn anomaly_count(&self) -> usize {
        self.anomalies.len()
    }

    /// Get all recorded anomalies.
    pub fn anomalies(&self) -> &[NetworkAnomaly] {
        &self.anomalies
    }

    /// Clear all anomalies.
    pub fn clear_anomalies(&mut self) {
        self.anomalies.clear();
    }

    /// Check if a port is in the suspicious list.
    pub fn is_suspicious_port(&self, port: u16) -> bool {
        self.suspicious_ports.contains(&port)
    }

    /// Add a port to the suspicious list.
    pub fn add_suspicious_port(&mut self, port: u16) {
        if !self.suspicious_ports.contains(&port) {
            self.suspicious_ports.push(port);
        }
    }
}

impl Default for NetworkMonitor {
    fn default() -> Self {
        Self::new(100)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_conn(
        remote_addr: &str,
        remote_port: u16,
        state: ConnectionState,
        process_name: Option<&str>,
    ) -> NetworkConnection {
        NetworkConnection {
            protocol: "tcp".to_string(),
            local_addr: "192.168.1.100".to_string(),
            local_port: 54321,
            remote_addr: remote_addr.to_string(),
            remote_port,
            state,
            pid: Some(1234),
            process_name: process_name.map(|s| s.to_string()),
        }
    }

    // ── ConnectionState tests ─────────────────────────────────────────────

    #[test]
    fn test_connection_state_from_str() {
        assert_eq!(
            ConnectionState::from_str_state("ESTABLISHED"),
            ConnectionState::Established
        );
        assert_eq!(
            ConnectionState::from_str_state("ESTAB"),
            ConnectionState::Established
        );
        assert_eq!(
            ConnectionState::from_str_state("LISTEN"),
            ConnectionState::Listen
        );
        assert_eq!(
            ConnectionState::from_str_state("LISTENING"),
            ConnectionState::Listen
        );
        assert_eq!(
            ConnectionState::from_str_state("TIME_WAIT"),
            ConnectionState::TimeWait
        );
        assert_eq!(
            ConnectionState::from_str_state("TIME-WAIT"),
            ConnectionState::TimeWait
        );
        assert_eq!(
            ConnectionState::from_str_state("CLOSE_WAIT"),
            ConnectionState::CloseWait
        );
        assert_eq!(
            ConnectionState::from_str_state("SYN_SENT"),
            ConnectionState::SynSent
        );
        assert_eq!(
            ConnectionState::from_str_state("SYNRECV"),
            ConnectionState::SynReceived
        );
        assert_eq!(
            ConnectionState::from_str_state("FIN_WAIT_1"),
            ConnectionState::FinWait1
        );
        assert_eq!(
            ConnectionState::from_str_state("FINWAIT2"),
            ConnectionState::FinWait2
        );
        assert_eq!(
            ConnectionState::from_str_state("CLOSING"),
            ConnectionState::Closing
        );
        assert_eq!(
            ConnectionState::from_str_state("LAST_ACK"),
            ConnectionState::LastAck
        );
        assert_eq!(
            ConnectionState::from_str_state("UNKNOWN_STATE"),
            ConnectionState::Unknown
        );
    }

    #[test]
    fn test_connection_state_case_insensitive() {
        assert_eq!(
            ConnectionState::from_str_state("established"),
            ConnectionState::Established
        );
        assert_eq!(
            ConnectionState::from_str_state("Listen"),
            ConnectionState::Listen
        );
    }

    #[test]
    fn test_connection_state_is_active() {
        assert!(ConnectionState::Established.is_active());
        assert!(ConnectionState::SynSent.is_active());
        assert!(ConnectionState::SynReceived.is_active());
        assert!(!ConnectionState::Listen.is_active());
        assert!(!ConnectionState::TimeWait.is_active());
        assert!(!ConnectionState::Unknown.is_active());
    }

    #[test]
    fn test_connection_state_is_closing() {
        assert!(ConnectionState::TimeWait.is_closing());
        assert!(ConnectionState::CloseWait.is_closing());
        assert!(ConnectionState::FinWait1.is_closing());
        assert!(ConnectionState::FinWait2.is_closing());
        assert!(ConnectionState::Closing.is_closing());
        assert!(ConnectionState::LastAck.is_closing());
        assert!(!ConnectionState::Established.is_closing());
        assert!(!ConnectionState::Listen.is_closing());
    }

    // ── NetworkConnection tests ───────────────────────────────────────────

    #[test]
    fn test_connection_is_loopback() {
        let conn = make_conn("127.0.0.1", 80, ConnectionState::Established, None);
        assert!(conn.is_loopback());

        let conn2 = make_conn("::1", 80, ConnectionState::Established, None);
        assert!(conn2.is_loopback());

        let conn3 = make_conn("localhost", 80, ConnectionState::Established, None);
        assert!(conn3.is_loopback());

        let conn4 = make_conn("8.8.8.8", 53, ConnectionState::Established, None);
        assert!(!conn4.is_loopback());
    }

    #[test]
    fn test_connection_is_external() {
        let external = make_conn("8.8.8.8", 53, ConnectionState::Established, None);
        assert!(external.is_external());

        let private_192 = make_conn("192.168.1.1", 80, ConnectionState::Established, None);
        assert!(!private_192.is_external());

        let private_10 = make_conn("10.0.0.1", 80, ConnectionState::Established, None);
        assert!(!private_10.is_external());

        let private_172 = make_conn("172.16.0.1", 80, ConnectionState::Established, None);
        assert!(!private_172.is_external());

        let loopback = make_conn("127.0.0.1", 80, ConnectionState::Established, None);
        assert!(!loopback.is_external());
    }

    #[test]
    fn test_connection_display_short() {
        let conn = make_conn("8.8.8.8", 53, ConnectionState::Established, None);
        let display = conn.display_short();
        assert!(display.contains("8.8.8.8"));
        assert!(display.contains("53"));
        assert!(display.contains("Established"));
    }

    // ── ConnectionSnapshot tests ──────────────────────────────────────────

    #[test]
    fn test_snapshot_from_connections() {
        let conns = vec![
            make_conn("8.8.8.8", 443, ConnectionState::Established, Some("chrome")),
            make_conn("0.0.0.0", 80, ConnectionState::Listen, Some("nginx")),
            make_conn("192.168.1.1", 22, ConnectionState::Established, Some("ssh")),
            make_conn("1.2.3.4", 4444, ConnectionState::Established, Some("suspicious")),
        ];
        let snapshot = ConnectionSnapshot::from_connections(conns);

        assert_eq!(snapshot.total_connections(), 4);
        assert_eq!(snapshot.total_established, 3);
        assert_eq!(snapshot.total_listening, 1);
        assert_eq!(snapshot.external_connections, 2); // 8.8.8.8 and 1.2.3.4
        assert!(snapshot.timestamp > 0.0);
    }

    #[test]
    fn test_snapshot_from_empty() {
        let snapshot = ConnectionSnapshot::from_connections(Vec::new());
        assert_eq!(snapshot.total_connections(), 0);
        assert_eq!(snapshot.total_established, 0);
        assert_eq!(snapshot.total_listening, 0);
        assert_eq!(snapshot.external_connections, 0);
    }

    #[test]
    fn test_snapshot_by_process() {
        let conns = vec![
            make_conn("8.8.8.8", 443, ConnectionState::Established, Some("chrome")),
            make_conn("1.1.1.1", 443, ConnectionState::Established, Some("chrome")),
            make_conn("0.0.0.0", 80, ConnectionState::Listen, Some("nginx")),
        ];
        let snapshot = ConnectionSnapshot::from_connections(conns);
        let by_proc = snapshot.by_process();

        assert_eq!(by_proc.get("chrome").unwrap().len(), 2);
        assert_eq!(by_proc.get("nginx").unwrap().len(), 1);
    }

    #[test]
    fn test_snapshot_by_port() {
        let conns = vec![
            make_conn("8.8.8.8", 443, ConnectionState::Established, None),
            make_conn("1.1.1.1", 80, ConnectionState::Established, None),
            make_conn("2.2.2.2", 443, ConnectionState::Established, None),
        ];
        let snapshot = ConnectionSnapshot::from_connections(conns);

        assert_eq!(snapshot.by_port(443).len(), 2);
        assert_eq!(snapshot.by_port(80).len(), 1);
        assert_eq!(snapshot.by_port(22).len(), 0);
    }

    #[test]
    fn test_snapshot_by_state() {
        let conns = vec![
            make_conn("8.8.8.8", 443, ConnectionState::Established, None),
            make_conn("0.0.0.0", 80, ConnectionState::Listen, None),
            make_conn("1.1.1.1", 443, ConnectionState::Established, None),
        ];
        let snapshot = ConnectionSnapshot::from_connections(conns);

        assert_eq!(snapshot.by_state(&ConnectionState::Established).len(), 2);
        assert_eq!(snapshot.by_state(&ConnectionState::Listen).len(), 1);
        assert_eq!(snapshot.by_state(&ConnectionState::TimeWait).len(), 0);
    }

    // ── NetworkMonitor tests ──────────────────────────────────────────────

    #[test]
    fn test_monitor_new() {
        let monitor = NetworkMonitor::new(50);
        assert_eq!(monitor.snapshot_count(), 0);
        assert_eq!(monitor.anomaly_count(), 0);
    }

    #[test]
    fn test_monitor_default() {
        let monitor = NetworkMonitor::default();
        assert_eq!(monitor.snapshot_count(), 0);
        assert_eq!(monitor.max_snapshots, 100);
    }

    #[test]
    fn test_monitor_record_snapshot() {
        let mut monitor = NetworkMonitor::new(5);
        for _ in 0..7 {
            let snapshot = ConnectionSnapshot::from_connections(Vec::new());
            monitor.record_snapshot(snapshot);
        }
        // Should be capped at 5.
        assert_eq!(monitor.snapshot_count(), 5);
    }

    #[test]
    fn test_monitor_latest_snapshot() {
        let mut monitor = NetworkMonitor::new(10);
        assert!(monitor.latest_snapshot().is_none());

        let conns = vec![make_conn(
            "8.8.8.8",
            443,
            ConnectionState::Established,
            None,
        )];
        let snapshot = ConnectionSnapshot::from_connections(conns);
        monitor.record_snapshot(snapshot);

        assert!(monitor.latest_snapshot().is_some());
        assert_eq!(monitor.latest_snapshot().unwrap().total_connections(), 1);
    }

    #[test]
    fn test_monitor_detect_suspicious_port() {
        let mut monitor = NetworkMonitor::new(10);
        let conns = vec![
            make_conn("8.8.8.8", 443, ConnectionState::Established, None),
            make_conn("1.2.3.4", 4444, ConnectionState::Established, None),
        ];
        let snapshot = ConnectionSnapshot::from_connections(conns);
        let anomalies = monitor.detect_anomalies(&snapshot);

        assert_eq!(anomalies.len(), 1);
        assert_eq!(anomalies[0].anomaly_type, "suspicious_port");
        assert_eq!(anomalies[0].severity, 3);
        assert_eq!(monitor.anomaly_count(), 1);
    }

    #[test]
    fn test_monitor_detect_high_external() {
        let mut monitor = NetworkMonitor::new(10);
        let mut conns = Vec::new();
        for i in 0..60u8 {
            conns.push(make_conn(
                &format!("{}.{}.{}.{}", i, i, i, i),
                443,
                ConnectionState::Established,
                None,
            ));
        }
        let snapshot = ConnectionSnapshot::from_connections(conns);
        let anomalies = monitor.detect_anomalies(&snapshot);

        assert!(anomalies
            .iter()
            .any(|a| a.anomaly_type == "high_external_connections"));
    }

    #[test]
    fn test_monitor_no_anomalies_normal_traffic() {
        let mut monitor = NetworkMonitor::new(10);
        let conns = vec![
            make_conn("8.8.8.8", 443, ConnectionState::Established, None),
            make_conn("1.1.1.1", 53, ConnectionState::Established, None),
        ];
        let snapshot = ConnectionSnapshot::from_connections(conns);
        let anomalies = monitor.detect_anomalies(&snapshot);

        assert!(anomalies.is_empty());
    }

    #[test]
    fn test_monitor_port_name() {
        let monitor = NetworkMonitor::default();
        assert_eq!(monitor.port_name(80), Some(&"HTTP".to_string()));
        assert_eq!(monitor.port_name(443), Some(&"HTTPS".to_string()));
        assert_eq!(monitor.port_name(22), Some(&"SSH".to_string()));
        assert_eq!(monitor.port_name(65535), None);
    }

    #[test]
    fn test_monitor_suspicious_port_check() {
        let mut monitor = NetworkMonitor::default();
        assert!(monitor.is_suspicious_port(4444));
        assert!(monitor.is_suspicious_port(31337));
        assert!(!monitor.is_suspicious_port(443));

        monitor.add_suspicious_port(9090);
        assert!(monitor.is_suspicious_port(9090));

        // Adding duplicate should not create extra entry.
        monitor.add_suspicious_port(9090);
        assert!(monitor.is_suspicious_port(9090));
    }

    #[test]
    fn test_monitor_clear_anomalies() {
        let mut monitor = NetworkMonitor::new(10);
        let conns = vec![make_conn(
            "1.2.3.4",
            4444,
            ConnectionState::Established,
            None,
        )];
        let snapshot = ConnectionSnapshot::from_connections(conns);
        monitor.detect_anomalies(&snapshot);
        assert!(monitor.anomaly_count() > 0);

        monitor.clear_anomalies();
        assert_eq!(monitor.anomaly_count(), 0);
    }

    #[test]
    fn test_connection_state_serialization() {
        let state = ConnectionState::Established;
        let json = serde_json::to_string(&state).unwrap();
        let parsed: ConnectionState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ConnectionState::Established);
    }

    #[test]
    fn test_network_connection_serialization() {
        let conn = make_conn("8.8.8.8", 443, ConnectionState::Established, Some("chrome"));
        let json = serde_json::to_string(&conn).unwrap();
        let parsed: NetworkConnection = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.remote_addr, "8.8.8.8");
        assert_eq!(parsed.remote_port, 443);
        assert_eq!(parsed.process_name, Some("chrome".to_string()));
    }
}
