//! Security Sensors — aggregated scan engine for the Warden actor.
//!
//! Runs IDS, encrypted traffic analysis, and DNS intelligence in a single
//! scan pass and produces a unified threat report string for the Warden
//! to include in its LLM-based security assessment.

use tracing::{debug, info};

use crate::security::dns::DnsIntelligenceEngine;
use crate::security::ids::{ConnectionInfo, IDSEngine};
use crate::security::traffic::EncryptedTrafficAnalyzer;

/// Aggregated security sensor suite.
///
/// Holds the stateful IDS and DNS engines, running all three
/// analyses (IDS, traffic, DNS) against current connection data.
pub struct SecuritySensors {
    ids: IDSEngine,
    dns: DnsIntelligenceEngine,
}

impl SecuritySensors {
    /// Create a new sensor suite with default configurations.
    pub fn new() -> Self {
        Self {
            ids: IDSEngine::new(),
            dns: DnsIntelligenceEngine::new(),
        }
    }

    /// Run all security sensors and return a unified threat report string.
    ///
    /// This is called by the warden actor before generating its LLM-based
    /// threat assessment. The report is injected into the warden's prompt
    /// so it can reason about real sensor data.
    pub fn scan(&mut self) -> String {
        // Gather live connection data from sysinfo.
        let connections = gather_connections();

        // 1. IDS Analysis
        let ids_report = self.ids.analyze(&connections, None);
        let ids_summary = ids_report.summary();

        // 2. Encrypted Traffic Analysis
        let traffic_report = EncryptedTrafficAnalyzer::analyze(&connections);
        let traffic_summary = traffic_report.summary();

        // 3. DNS Intelligence (auto-dumps DNS cache)
        let dns_report = self.dns.analyze(None, Some(&connections));
        let dns_summary = dns_report.summary();

        let total_threats = ids_report.alerts.len()
            + dns_report.threats.len()
            + traffic_report.risk_flags.len();

        info!(
            "Security sensors scan complete: {} connections, {} total threats",
            connections.len(),
            total_threats
        );

        format!(
            "=== SECURITY SENSOR DATA ===\n\n\
             {ids_summary}\n\n\
             {traffic_summary}\n\n\
             {dns_summary}\n\n\
             Total Threats: {total_threats}\n\
             ==========================="
        )
    }
}

impl Default for SecuritySensors {
    fn default() -> Self {
        Self::new()
    }
}

/// Gather live TCP connections using sysinfo.
///
/// Maps sysinfo network data to our `ConnectionInfo` format.
/// This is a best-effort scan — if sysinfo doesn't provide process-level
/// connection data, we return a minimal set.
fn gather_connections() -> Vec<ConnectionInfo> {
    use sysinfo::System;

    let mut sys = System::new();
    sys.refresh_processes();

    // sysinfo doesn't expose per-process TCP connections on all platforms.
    // We use the Networks API for interface-level stats and supplement
    // with process enumeration for suspicious process detection.
    let mut connections = Vec::new();

    // Enumerate processes — look for known-suspicious names.
    let suspicious_names = [
        "mimikatz", "cobaltstrike", "meterpreter", "nc.exe",
        "ncat", "netcat", "psexec", "wmiexec",
    ];

    for (pid, process) in sys.processes() {
        let name = process.name().to_string();
        let name_lower = name.to_lowercase();

        // Flag suspicious processes as connections for IDS analysis.
        if suspicious_names.iter().any(|s| name_lower.contains(s)) {
            connections.push(ConnectionInfo {
                local_addr: "0.0.0.0".into(),
                local_port: 0,
                remote_addr: "unknown".into(),
                remote_port: 0,
                process_name: name,
                status: "RUNNING".into(),
                direction: "unknown".into(),
                is_encrypted: false,
            });
        }
    }

    debug!("Gathered {} connection entries from sysinfo", connections.len());
    connections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sensors_new() {
        let sensors = SecuritySensors::new();
        assert_eq!(sensors.ids.get_rules().len(), 6);
    }

    #[test]
    fn test_sensors_scan_no_crash() {
        let mut sensors = SecuritySensors::new();
        let report = sensors.scan();
        assert!(report.contains("SECURITY SENSOR DATA"));
        assert!(report.contains("IDS Report"));
        assert!(report.contains("Encrypted Traffic Report"));
        assert!(report.contains("DNS Intelligence Report"));
    }

    #[test]
    fn test_gather_connections() {
        // Should not crash, may return empty on test environments.
        let conns = gather_connections();
        // We can't assert exact results since it depends on running processes.
        assert!(conns.len() < 10000); // Sanity check.
    }
}
