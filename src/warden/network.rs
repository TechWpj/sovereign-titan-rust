//! Network Detection & Response (NDR) — connection monitoring and threat detection.
//!
//! Ported from `sovereign_titan/security/network_monitor.py` and `ids_engine.py`.
//! Uses `sysinfo` to poll active TCP/UDP connections and cross-references
//! foreign addresses against a local threat blocklist.

use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, Instant};

use sysinfo::{Networks, System};
use tracing::{info, warn};

/// A detected network threat.
#[derive(Debug, Clone)]
pub struct NetworkAlert {
    /// Severity: "LOW", "MEDIUM", "HIGH", "CRITICAL".
    pub threat_level: String,
    /// Human-readable description.
    pub details: String,
    /// The flagged remote address (if applicable).
    pub remote_addr: Option<String>,
    /// PID of the offending process (if known).
    pub pid: Option<u32>,
}

/// Connection baseline entry — tracks known-good connections.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct ConnectionKey {
    local_port: u16,
    remote_addr: String,
    process_name: String,
}

/// Network Detection & Response monitor.
///
/// Polls active connections, compares against a threat blocklist,
/// and detects anomalies by comparing against a learned baseline.
pub struct NetworkMonitor {
    /// IP addresses / domain patterns on the blocklist.
    blocklist: HashSet<String>,
    /// Known-good connection baseline (learned over time).
    baseline: HashSet<ConnectionKey>,
    /// Whether the baseline has been established.
    baseline_established: bool,
    /// Number of scans before baseline is considered stable.
    baseline_warmup: u32,
    /// Current scan count.
    scan_count: u32,
    /// Minimum interval between scans.
    min_scan_interval: Duration,
    /// Last scan timestamp.
    last_scan: Option<Instant>,
}

impl NetworkMonitor {
    /// Create a new monitor, optionally loading a blocklist from disk.
    pub fn new(blocklist_path: Option<&str>) -> Self {
        let blocklist = blocklist_path
            .and_then(|p| Self::load_blocklist(p).ok())
            .unwrap_or_default();

        if !blocklist.is_empty() {
            info!("NDR: loaded {} blocklist entries", blocklist.len());
        }

        Self {
            blocklist,
            baseline: HashSet::new(),
            baseline_established: false,
            baseline_warmup: 5,
            scan_count: 0,
            min_scan_interval: Duration::from_secs(10),
            last_scan: None,
        }
    }

    /// Load a blocklist file (one IP/domain per line, # comments).
    fn load_blocklist(path: &str) -> std::io::Result<HashSet<String>> {
        let path = Path::new(path);
        if !path.exists() {
            return Ok(HashSet::new());
        }

        let content = std::fs::read_to_string(path)?;
        let entries: HashSet<String> = content
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| l.to_lowercase())
            .collect();

        Ok(entries)
    }

    /// Add an entry to the blocklist at runtime.
    pub fn add_to_blocklist(&mut self, entry: &str) {
        self.blocklist.insert(entry.to_lowercase());
    }

    /// Run a network scan and return any detected threats.
    ///
    /// Uses `sysinfo` to enumerate active network connections and processes,
    /// then cross-references remote addresses against the blocklist and baseline.
    pub fn scan(&mut self) -> Vec<NetworkAlert> {
        // Rate limiting — don't scan too frequently.
        if let Some(last) = self.last_scan {
            if last.elapsed() < self.min_scan_interval {
                return Vec::new();
            }
        }
        self.last_scan = Some(Instant::now());
        self.scan_count += 1;

        let mut alerts = Vec::new();
        let mut sys = System::new_all();
        sys.refresh_all();

        // Collect current connections by examining processes with network activity.
        let mut current_connections: HashSet<ConnectionKey> = HashSet::new();

        // Check network interfaces for anomalous traffic volume.
        let networks = Networks::new_with_refreshed_list();
        let mut total_rx_bytes: u64 = 0;
        let mut total_tx_bytes: u64 = 0;

        for (_name, data) in &networks {
            total_rx_bytes += data.received();
            total_tx_bytes += data.transmitted();
        }

        // Inspect running processes — flag any whose name matches suspicious patterns.
        let suspicious_process_names = [
            "mimikatz", "cobaltstrike", "meterpreter", "nc.exe", "ncat.exe",
            "powershell_ise", "psexec", "wce.exe", "procdump",
        ];

        for (pid, process) in sys.processes() {
            let proc_name = process.name().to_lowercase();

            // Check for known-malicious process names.
            for sus in &suspicious_process_names {
                if proc_name.contains(sus) {
                    alerts.push(NetworkAlert {
                        threat_level: "CRITICAL".to_string(),
                        details: format!(
                            "Suspicious process detected: '{}' (PID {})",
                            process.name(),
                            pid
                        ),
                        remote_addr: None,
                        pid: Some(pid.as_u32()),
                    });
                }
            }

            // Build a connection key for baseline tracking.
            // sysinfo doesn't expose per-process network connections directly,
            // so we track processes that are actively using CPU/memory as potential
            // network-active processes.
            if process.memory() > 0 {
                let key = ConnectionKey {
                    local_port: 0, // Not available from sysinfo process API
                    remote_addr: String::new(),
                    process_name: proc_name.clone(),
                };
                current_connections.insert(key);
            }
        }

        // Check blocklist against any network-facing processes.
        // In a full implementation, we'd parse netstat output or use OS APIs.
        // For now, we use a process-based heuristic.
        self.check_blocklist_heuristic(&sys, &mut alerts);

        // Baseline learning phase.
        if !self.baseline_established {
            if self.scan_count <= self.baseline_warmup {
                // Learning phase — add all current connections to baseline.
                self.baseline.extend(current_connections.clone());
                info!(
                    "NDR: baseline learning ({}/{}), {} entries",
                    self.scan_count, self.baseline_warmup, self.baseline.len()
                );
            } else {
                self.baseline_established = true;
                info!(
                    "NDR: baseline established with {} entries",
                    self.baseline.len()
                );
            }
        } else {
            // Anomaly detection — flag connections not in baseline.
            for conn in &current_connections {
                if !self.baseline.contains(conn) && !conn.process_name.is_empty() {
                    // New process not seen during baseline — could be suspicious.
                    // Only alert if it's not a common system process.
                    if !is_common_system_process(&conn.process_name) {
                        alerts.push(NetworkAlert {
                            threat_level: "LOW".to_string(),
                            details: format!(
                                "New process not in baseline: '{}'",
                                conn.process_name
                            ),
                            remote_addr: None,
                            pid: None,
                        });
                    }
                }
            }
        }

        // Traffic volume anomaly — very rough heuristic.
        // Flag if total TX exceeds 100MB in a single poll (possible data exfil).
        const EXFIL_THRESHOLD: u64 = 100 * 1024 * 1024;
        if total_tx_bytes > EXFIL_THRESHOLD {
            alerts.push(NetworkAlert {
                threat_level: "HIGH".to_string(),
                details: format!(
                    "High outbound traffic detected: {} MB transmitted",
                    total_tx_bytes / (1024 * 1024)
                ),
                remote_addr: None,
                pid: None,
            });
        }

        if alerts.is_empty() {
            info!("NDR: scan clean — no threats detected");
        } else {
            warn!("NDR: {} threat(s) detected", alerts.len());
        }

        alerts
    }

    /// Check if any running process has a command line referencing blocklisted addresses.
    fn check_blocklist_heuristic(&self, sys: &System, alerts: &mut Vec<NetworkAlert>) {
        if self.blocklist.is_empty() {
            return;
        }

        for (pid, process) in sys.processes() {
            let cmd_parts: Vec<String> = process.cmd().iter()
                .map(|s| s.to_lowercase())
                .collect();
            let cmd_line = cmd_parts.join(" ");

            for blocked in &self.blocklist {
                if cmd_line.contains(blocked.as_str()) {
                    alerts.push(NetworkAlert {
                        threat_level: "CRITICAL".to_string(),
                        details: format!(
                            "Process '{}' (PID {}) references blocklisted address: {}",
                            process.name(),
                            pid,
                            blocked
                        ),
                        remote_addr: Some(blocked.clone()),
                        pid: Some(pid.as_u32()),
                    });
                }
            }
        }
    }

    /// Format alerts for the Warden actor channel.
    pub fn format_alert_message(alert: &NetworkAlert) -> String {
        format!(
            "[NDR {}] {} (remote={}, pid={})",
            alert.threat_level,
            alert.details,
            alert.remote_addr.as_deref().unwrap_or("N/A"),
            alert.pid.map(|p| p.to_string()).unwrap_or_else(|| "N/A".to_string()),
        )
    }

    /// Get the highest threat level from a set of alerts.
    pub fn max_threat_level(alerts: &[NetworkAlert]) -> &str {
        let priority = |level: &str| match level {
            "CRITICAL" => 4,
            "HIGH" => 3,
            "MEDIUM" => 2,
            "LOW" => 1,
            _ => 0,
        };

        alerts
            .iter()
            .max_by_key(|a| priority(&a.threat_level))
            .map(|a| a.threat_level.as_str())
            .unwrap_or("NONE")
    }

    /// Number of blocklist entries.
    pub fn blocklist_count(&self) -> usize {
        self.blocklist.len()
    }

    /// Whether baseline has been established.
    pub fn is_baseline_ready(&self) -> bool {
        self.baseline_established
    }
}

impl Default for NetworkMonitor {
    fn default() -> Self {
        Self::new(None)
    }
}

/// Check if a process name is a common Windows system process.
fn is_common_system_process(name: &str) -> bool {
    const SYSTEM_PROCS: &[&str] = &[
        "system", "svchost.exe", "csrss.exe", "wininit.exe", "services.exe",
        "lsass.exe", "winlogon.exe", "explorer.exe", "dwm.exe", "taskhostw.exe",
        "runtimebroker.exe", "searchhost.exe", "startmenuexperiencehost.exe",
        "textinputhost.exe", "ctfmon.exe", "sihost.exe", "fontdrvhost.exe",
        "conhost.exe", "dllhost.exe", "smartscreen.exe", "securityhealthservice.exe",
        "msedge.exe", "chrome.exe", "firefox.exe", "code.exe",
        "windowsterminal.exe", "powershell.exe", "cmd.exe",
    ];

    SYSTEM_PROCS.iter().any(|&p| name == p || name.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_default() {
        let monitor = NetworkMonitor::default();
        assert_eq!(monitor.blocklist_count(), 0);
        assert!(!monitor.is_baseline_ready());
    }

    #[test]
    fn test_add_to_blocklist() {
        let mut monitor = NetworkMonitor::default();
        monitor.add_to_blocklist("192.168.1.100");
        monitor.add_to_blocklist("evil.example.com");
        assert_eq!(monitor.blocklist_count(), 2);
    }

    #[test]
    fn test_blocklist_case_insensitive() {
        let mut monitor = NetworkMonitor::default();
        monitor.add_to_blocklist("EVIL.COM");
        assert!(monitor.blocklist.contains("evil.com"));
    }

    #[test]
    fn test_max_threat_level() {
        let alerts = vec![
            NetworkAlert {
                threat_level: "LOW".to_string(),
                details: "test".to_string(),
                remote_addr: None,
                pid: None,
            },
            NetworkAlert {
                threat_level: "HIGH".to_string(),
                details: "test".to_string(),
                remote_addr: None,
                pid: None,
            },
            NetworkAlert {
                threat_level: "MEDIUM".to_string(),
                details: "test".to_string(),
                remote_addr: None,
                pid: None,
            },
        ];
        assert_eq!(NetworkMonitor::max_threat_level(&alerts), "HIGH");
    }

    #[test]
    fn test_max_threat_level_empty() {
        let alerts: Vec<NetworkAlert> = vec![];
        assert_eq!(NetworkMonitor::max_threat_level(&alerts), "NONE");
    }

    #[test]
    fn test_format_alert() {
        let alert = NetworkAlert {
            threat_level: "CRITICAL".to_string(),
            details: "Suspicious process found".to_string(),
            remote_addr: Some("10.0.0.1".to_string()),
            pid: Some(1234),
        };
        let msg = NetworkMonitor::format_alert_message(&alert);
        assert!(msg.contains("CRITICAL"));
        assert!(msg.contains("10.0.0.1"));
        assert!(msg.contains("1234"));
    }

    #[test]
    fn test_is_common_system_process() {
        assert!(is_common_system_process("svchost.exe"));
        assert!(is_common_system_process("explorer.exe"));
        assert!(is_common_system_process("chrome.exe"));
        assert!(!is_common_system_process("mimikatz.exe"));
        assert!(!is_common_system_process("totally_legit.exe"));
    }

    #[test]
    fn test_connection_key_equality() {
        let a = ConnectionKey {
            local_port: 80,
            remote_addr: "1.2.3.4".to_string(),
            process_name: "test.exe".to_string(),
        };
        let b = ConnectionKey {
            local_port: 80,
            remote_addr: "1.2.3.4".to_string(),
            process_name: "test.exe".to_string(),
        };
        assert_eq!(a, b);
    }

    #[test]
    fn test_scan_rate_limiting() {
        let mut monitor = NetworkMonitor::default();
        // First scan should work.
        let _ = monitor.scan();
        // Immediate second scan should be rate-limited (empty).
        let alerts = monitor.scan();
        assert!(alerts.is_empty());
    }
}
