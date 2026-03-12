//! Encrypted Traffic Analyzer.
//!
//! Ported from `sovereign_titan/security/encrypted_traffic.py`.
//! Analyzes TLS/SSL traffic patterns WITHOUT decryption — port-based
//! classification, per-process encryption ratios, cleartext-on-sensitive-port
//! flagging, and Shannon entropy computation.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::debug;

use crate::security::ids::ConnectionInfo;

/// Ports that typically carry TLS/SSL traffic.
const TLS_PORTS: &[u16] = &[443, 465, 587, 636, 853, 993, 995, 8443, 9443];

/// Sensitive ports where cleartext is a risk flag.
const SENSITIVE_CLEARTEXT_PORTS: &[u16] = &[21, 23, 25, 110, 143];

/// Encryption profile for a single process.
#[derive(Debug, Clone)]
pub struct EncryptionProfile {
    pub process_name: String,
    pub total_connections: usize,
    pub encrypted_connections: usize,
    pub cleartext_connections: usize,
    pub encryption_ratio: f32,
    pub cleartext_on_sensitive: Vec<(String, u16)>,
}

/// A risk flag from traffic analysis.
#[derive(Debug, Clone)]
pub struct RiskFlag {
    pub severity: String,
    pub description: String,
}

/// Full encrypted traffic analysis report.
#[derive(Debug)]
pub struct TrafficReport {
    pub timestamp: f64,
    pub total_connections: usize,
    pub encrypted_count: usize,
    pub cleartext_count: usize,
    pub encryption_ratio: f32,
    pub process_profiles: Vec<EncryptionProfile>,
    pub risk_flags: Vec<RiskFlag>,
}

impl TrafficReport {
    /// Generate a human-readable summary.
    pub fn summary(&self) -> String {
        let mut lines = vec![
            "Encrypted Traffic Report:".to_string(),
            format!("  Total: {} connections", self.total_connections),
            format!(
                "  Encrypted: {} ({:.0}%)",
                self.encrypted_count,
                self.encryption_ratio * 100.0
            ),
            format!("  Cleartext: {}", self.cleartext_count),
        ];
        if !self.risk_flags.is_empty() {
            lines.push(format!("  Risk Flags: {}", self.risk_flags.len()));
            for rf in self.risk_flags.iter().take(5) {
                lines.push(format!("    - [{}] {}", rf.severity, rf.description));
            }
        }
        lines.join("\n")
    }
}

/// Encrypted traffic analyzer.
///
/// Classifies connections as encrypted or cleartext based on port,
/// builds per-process encryption profiles, and flags cleartext usage
/// on sensitive ports.
pub struct EncryptedTrafficAnalyzer;

impl EncryptedTrafficAnalyzer {
    /// Analyze connections for encryption patterns.
    pub fn analyze(connections: &[ConnectionInfo]) -> TrafficReport {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        let tls_ports: std::collections::HashSet<u16> = TLS_PORTS.iter().copied().collect();
        let sensitive_ports: std::collections::HashSet<u16> =
            SENSITIVE_CLEARTEXT_PORTS.iter().copied().collect();

        let mut process_map: HashMap<String, EncryptionProfile> = HashMap::new();
        let mut risk_flags = Vec::new();
        let mut encrypted_count = 0usize;
        let mut cleartext_count = 0usize;

        for conn in connections {
            if conn.status == "LISTEN" {
                continue;
            }

            let proc_name = if conn.process_name.is_empty() {
                "unknown"
            } else {
                &conn.process_name
            };

            let profile = process_map
                .entry(proc_name.to_string())
                .or_insert_with(|| EncryptionProfile {
                    process_name: proc_name.to_string(),
                    total_connections: 0,
                    encrypted_connections: 0,
                    cleartext_connections: 0,
                    encryption_ratio: 0.0,
                    cleartext_on_sensitive: Vec::new(),
                });

            profile.total_connections += 1;

            if conn.is_encrypted || tls_ports.contains(&conn.remote_port) {
                encrypted_count += 1;
                profile.encrypted_connections += 1;
            } else {
                cleartext_count += 1;
                profile.cleartext_connections += 1;

                // Flag cleartext on sensitive ports.
                if sensitive_ports.contains(&conn.remote_port) {
                    profile
                        .cleartext_on_sensitive
                        .push((conn.remote_addr.clone(), conn.remote_port));
                    risk_flags.push(RiskFlag {
                        severity: "high".to_string(),
                        description: format!(
                            "{proc_name} using cleartext on port {} to {}",
                            conn.remote_port, conn.remote_addr
                        ),
                    });
                }
            }

            // Update ratio.
            if profile.total_connections > 0 {
                profile.encryption_ratio =
                    profile.encrypted_connections as f32 / profile.total_connections as f32;
            }
        }

        let total = encrypted_count + cleartext_count;
        let ratio = if total > 0 {
            encrypted_count as f32 / total as f32
        } else {
            0.0
        };

        debug!(
            "Traffic analysis: {encrypted_count} encrypted, {cleartext_count} cleartext, {:.0}% ratio",
            ratio * 100.0
        );

        TrafficReport {
            timestamp: now,
            total_connections: total,
            encrypted_count,
            cleartext_count,
            encryption_ratio: ratio,
            process_profiles: process_map.into_values().collect(),
            risk_flags,
        }
    }

    /// Compute Shannon entropy of a byte sequence.
    ///
    /// Returns a value between 0.0 (no randomness) and 8.0 (max entropy).
    /// Encrypted data typically has entropy > 7.5.
    pub fn compute_entropy(data: &[u8]) -> f64 {
        if data.is_empty() {
            return 0.0;
        }

        let mut freq = [0u32; 256];
        for &byte in data {
            freq[byte as usize] += 1;
        }

        let len = data.len() as f64;
        let mut entropy = 0.0;
        for &count in &freq {
            if count > 0 {
                let p = count as f64 / len;
                entropy -= p * p.log2();
            }
        }
        entropy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_conn(
        remote_port: u16,
        process: &str,
        is_encrypted: bool,
    ) -> ConnectionInfo {
        ConnectionInfo {
            local_addr: "192.168.1.100".into(),
            local_port: 50000,
            remote_addr: "10.0.0.1".into(),
            remote_port,
            process_name: process.into(),
            status: "ESTABLISHED".into(),
            direction: "outbound".into(),
            is_encrypted,
        }
    }

    #[test]
    fn test_all_encrypted() {
        let connections = vec![
            make_conn(443, "chrome", false), // TLS port → encrypted
            make_conn(993, "outlook", false), // TLS port → encrypted
        ];
        let report = EncryptedTrafficAnalyzer::analyze(&connections);
        assert_eq!(report.encrypted_count, 2);
        assert_eq!(report.cleartext_count, 0);
        assert!((report.encryption_ratio - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_cleartext_on_sensitive_port() {
        let connections = vec![
            make_conn(23, "telnet_client", false), // Telnet = cleartext + sensitive
        ];
        let report = EncryptedTrafficAnalyzer::analyze(&connections);
        assert_eq!(report.cleartext_count, 1);
        assert!(!report.risk_flags.is_empty());
        assert_eq!(report.risk_flags[0].severity, "high");
        assert!(report.risk_flags[0].description.contains("telnet_client"));
    }

    #[test]
    fn test_mixed_traffic() {
        let connections = vec![
            make_conn(443, "chrome", false),
            make_conn(80, "curl", false),
            make_conn(8080, "app", true), // Explicitly encrypted
        ];
        let report = EncryptedTrafficAnalyzer::analyze(&connections);
        assert_eq!(report.encrypted_count, 2); // 443 + explicit
        assert_eq!(report.cleartext_count, 1); // 80
    }

    #[test]
    fn test_skip_listen() {
        let connections = vec![ConnectionInfo {
            local_addr: "0.0.0.0".into(),
            local_port: 80,
            remote_addr: "".into(),
            remote_port: 0,
            process_name: "httpd".into(),
            status: "LISTEN".into(),
            direction: "".into(),
            is_encrypted: false,
        }];
        let report = EncryptedTrafficAnalyzer::analyze(&connections);
        assert_eq!(report.total_connections, 0);
    }

    #[test]
    fn test_process_profiles() {
        let connections = vec![
            make_conn(443, "chrome", false),
            make_conn(443, "chrome", false),
            make_conn(80, "chrome", false),
            make_conn(443, "firefox", false),
        ];
        let report = EncryptedTrafficAnalyzer::analyze(&connections);
        let chrome = report
            .process_profiles
            .iter()
            .find(|p| p.process_name == "chrome")
            .unwrap();
        assert_eq!(chrome.total_connections, 3);
        assert_eq!(chrome.encrypted_connections, 2);
        assert_eq!(chrome.cleartext_connections, 1);
    }

    #[test]
    fn test_entropy_encrypted_data() {
        // Random-looking data (high entropy).
        let data: Vec<u8> = (0..=255).collect();
        let entropy = EncryptedTrafficAnalyzer::compute_entropy(&data);
        assert!(entropy > 7.9, "uniform data should have ~8.0 entropy: {entropy}");
    }

    #[test]
    fn test_entropy_cleartext() {
        let data = b"Hello world. This is plaintext. It has low entropy.";
        let entropy = EncryptedTrafficAnalyzer::compute_entropy(data);
        assert!(
            entropy < 5.0,
            "plaintext should have lower entropy: {entropy}"
        );
    }

    #[test]
    fn test_entropy_empty() {
        assert_eq!(EncryptedTrafficAnalyzer::compute_entropy(&[]), 0.0);
    }

    #[test]
    fn test_summary() {
        let report = TrafficReport {
            timestamp: 0.0,
            total_connections: 10,
            encrypted_count: 8,
            cleartext_count: 2,
            encryption_ratio: 0.8,
            process_profiles: Vec::new(),
            risk_flags: vec![RiskFlag {
                severity: "high".into(),
                description: "test flag".into(),
            }],
        };
        let summary = report.summary();
        assert!(summary.contains("80%"));
        assert!(summary.contains("test flag"));
    }
}
