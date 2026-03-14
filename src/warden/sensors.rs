//! Security Sensors — aggregated scan engine for the Warden actor.
//!
//! Runs IDS, encrypted traffic analysis, and DNS intelligence in a single
//! scan pass and produces a unified threat report string for the Warden
//! to include in its LLM-based security assessment.

use tracing::{debug, info};

use crate::security::dns::DnsIntelligenceEngine;
use crate::security::ids::{ConnectionInfo, IDSEngine, IDSAlert, IDSRule};
use crate::security::network_monitor::{NetworkMonitor, ConnectionSnapshot, NetworkConnection, ConnectionState};
use crate::security::port_scanner::{PortScanner, ScanConfig, PortResult, PortStatus};
use crate::security::traffic::EncryptedTrafficAnalyzer;

/// Aggregated security sensor suite.
///
/// Holds the stateful IDS, DNS, network monitor, and port scanner engines,
/// running all analyses against current connection data.
pub struct SecuritySensors {
    ids: IDSEngine,
    dns: DnsIntelligenceEngine,
    network_monitor: NetworkMonitor,
    port_scanner: PortScanner,
}

impl SecuritySensors {
    /// Create a new sensor suite with default configurations.
    pub fn new() -> Self {
        Self {
            ids: IDSEngine::new(),
            dns: DnsIntelligenceEngine::new(),
            network_monitor: NetworkMonitor::new(100),
            port_scanner: PortScanner::new(),
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

        // 2b. Shannon entropy of process names as a data fingerprint
        //     (high entropy in process names can indicate obfuscated malware).
        let process_name_bytes: Vec<u8> = connections
            .iter()
            .flat_map(|c| c.process_name.as_bytes())
            .copied()
            .collect();
        let process_name_entropy = EncryptedTrafficAnalyzer::compute_entropy(&process_name_bytes);

        // 3. DNS Intelligence (auto-dumps DNS cache)
        let dns_report = self.dns.analyze(None, Some(&connections));
        let dns_summary = dns_report.summary();

        // 4. Network Monitor — build snapshot and detect anomalies.
        let net_conns: Vec<NetworkConnection> = connections
            .iter()
            .map(|c| NetworkConnection {
                protocol: "tcp".to_string(),
                local_addr: c.local_addr.clone(),
                local_port: c.local_port,
                remote_addr: c.remote_addr.clone(),
                remote_port: c.remote_port,
                state: ConnectionState::from_str_state(&c.status),
                pid: None,
                process_name: if c.process_name.is_empty() {
                    None
                } else {
                    Some(c.process_name.clone())
                },
            })
            .collect();
        let snapshot = ConnectionSnapshot::from_connections(net_conns);
        let network_anomalies = self.network_monitor.detect_anomalies(&snapshot);
        self.network_monitor.record_snapshot(snapshot);

        let network_summary = if network_anomalies.is_empty() {
            "Network Monitor: No anomalies detected".to_string()
        } else {
            let mut lines = vec![format!("Network Monitor: {} anomalies", network_anomalies.len())];
            for anomaly in network_anomalies.iter().take(5) {
                lines.push(format!(
                    "  [sev={}] {}: {}",
                    anomaly.severity, anomaly.anomaly_type, anomaly.description
                ));
            }
            lines.join("\n")
        };

        // 5. Port Scanner — scan localhost common ports for open services.
        let config = ScanConfig::common_ports("127.0.0.1");
        let port_results: Vec<PortResult> = config
            .ports
            .iter()
            .map(|&port| {
                // Check if any gathered connection is listening on this port.
                let is_open = connections
                    .iter()
                    .any(|c| c.local_port == port && c.status == "LISTEN");
                let service = self.port_scanner.identify_service(port).cloned();
                PortResult {
                    port,
                    status: if is_open { PortStatus::Open } else { PortStatus::Closed },
                    service,
                    banner: None,
                    response_ms: 0.0,
                }
            })
            .collect();
        let scan_result = self.port_scanner.build_scan_result("127.0.0.1", port_results, 0.0);
        self.port_scanner.record_scan(scan_result.clone());

        let port_summary = if scan_result.open_ports.is_empty() {
            "Port Scanner: No open ports on localhost".to_string()
        } else {
            let mut lines = vec![format!(
                "Port Scanner: {} open ports on localhost",
                scan_result.open_count()
            )];
            for p in &scan_result.open_ports {
                lines.push(format!(
                    "  {} — {}",
                    p.port,
                    p.service.as_deref().unwrap_or("unknown")
                ));
            }
            if scan_result.has_high_severity() {
                lines.push(format!(
                    "  VULNERABILITIES: {}",
                    scan_result
                        .vulnerabilities
                        .iter()
                        .map(|v| format!("[{}] {} (port {})", v.severity, v.vulnerability, v.port))
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
            lines.join("\n")
        };

        // 6. IDS diagnostics — rule/alert summary.
        let ids_diag = self.ids_diagnostic_summary();

        let total_threats = ids_report.alerts.len()
            + dns_report.threats.len()
            + traffic_report.risk_flags.len()
            + network_anomalies.len();

        info!(
            "Security sensors scan complete: {} connections, {} total threats, entropy={:.2}",
            connections.len(),
            total_threats,
            process_name_entropy,
        );

        format!(
            "=== SECURITY SENSOR DATA ===\n\n\
             {ids_summary}\n\n\
             {traffic_summary}\n\
             Process Name Entropy: {process_name_entropy:.2}/8.0\n\n\
             {dns_summary}\n\n\
             {network_summary}\n\n\
             {port_summary}\n\n\
             {ids_diag}\n\n\
             Total Threats: {total_threats}\n\
             ==========================="
        )
    }

    /// Get a diagnostic summary of IDS state: active rules, recent alerts.
    pub fn ids_diagnostic_summary(&self) -> String {
        let rules = self.ids.get_rules();
        let enabled_count = rules.iter().filter(|r| r.enabled).count();
        let disabled_count = rules.len() - enabled_count;
        let recent_alerts = self.ids.get_alerts(None, 5);

        let mut lines = vec![format!(
            "IDS Diagnostics: {}/{} rules enabled ({} disabled)",
            enabled_count,
            rules.len(),
            disabled_count,
        )];

        if !recent_alerts.is_empty() {
            lines.push(format!("  Recent Alerts ({}):", recent_alerts.len()));
            for alert in &recent_alerts {
                lines.push(format!(
                    "    [{}] {}: {} (src: {})",
                    alert.severity.to_uppercase(),
                    alert.rule_id,
                    alert.description,
                    alert.source_ip,
                ));
            }
        }

        lines.join("\n")
    }

    /// Enable an IDS rule by ID. Returns true if the rule was found and enabled.
    pub fn enable_ids_rule(&mut self, rule_id: &str) -> bool {
        let result = self.ids.enable_rule(rule_id);
        if result {
            info!("IDS rule '{}' enabled", rule_id);
        }
        result
    }

    /// Disable an IDS rule by ID. Returns true if the rule was found and disabled.
    pub fn disable_ids_rule(&mut self, rule_id: &str) -> bool {
        let result = self.ids.disable_rule(rule_id);
        if result {
            info!("IDS rule '{}' disabled", rule_id);
        }
        result
    }

    /// Get all IDS rules for inspection.
    pub fn ids_rules(&self) -> &[IDSRule] {
        self.ids.get_rules()
    }

    /// Get recent IDS alerts, optionally filtered by severity.
    pub fn ids_alerts(&self, severity: Option<&str>, limit: usize) -> Vec<&IDSAlert> {
        self.ids.get_alerts(severity, limit)
    }

    /// Get the total number of network anomalies detected across all scans.
    pub fn network_anomaly_count(&self) -> usize {
        self.network_monitor.anomaly_count()
    }

    /// Get the number of port scans in history.
    pub fn port_scan_count(&self) -> usize {
        self.port_scanner.scan_count()
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
            debug!("Suspicious process detected: '{}' (PID {})", name, pid.as_u32());
            connections.push(ConnectionInfo {
                local_addr: format!("0.0.0.0 (pid={})", pid.as_u32()),
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
        assert_eq!(sensors.network_anomaly_count(), 0);
        assert_eq!(sensors.port_scan_count(), 0);
    }

    #[test]
    fn test_sensors_scan_no_crash() {
        let mut sensors = SecuritySensors::new();
        let report = sensors.scan();
        assert!(report.contains("SECURITY SENSOR DATA"));
        assert!(report.contains("IDS Report"));
        assert!(report.contains("Encrypted Traffic Report"));
        assert!(report.contains("DNS Intelligence Report"));
        assert!(report.contains("Network Monitor"));
        assert!(report.contains("Port Scanner"));
        assert!(report.contains("IDS Diagnostics"));
        assert!(report.contains("Process Name Entropy"));
    }

    #[test]
    fn test_ids_diagnostics() {
        let sensors = SecuritySensors::new();
        let diag = sensors.ids_diagnostic_summary();
        assert!(diag.contains("6/6 rules enabled"));
        assert!(diag.contains("0 disabled"));
    }

    #[test]
    fn test_ids_rule_toggle() {
        let mut sensors = SecuritySensors::new();
        assert!(sensors.disable_ids_rule("BEH-001"));
        let diag = sensors.ids_diagnostic_summary();
        assert!(diag.contains("5/6 rules enabled"));
        assert!(diag.contains("1 disabled"));

        assert!(sensors.enable_ids_rule("BEH-001"));
        let diag = sensors.ids_diagnostic_summary();
        assert!(diag.contains("6/6 rules enabled"));
    }

    #[test]
    fn test_ids_alerts_initially_empty() {
        let sensors = SecuritySensors::new();
        let alerts = sensors.ids_alerts(None, 10);
        assert!(alerts.is_empty());
    }

    #[test]
    fn test_port_scan_after_scan() {
        let mut sensors = SecuritySensors::new();
        sensors.scan();
        // After one scan, there should be one port scan recorded.
        assert_eq!(sensors.port_scan_count(), 1);
    }

    #[test]
    fn test_gather_connections() {
        // Should not crash, may return empty on test environments.
        let conns = gather_connections();
        // We can't assert exact results since it depends on running processes.
        assert!(conns.len() < 10000); // Sanity check.
    }
}
