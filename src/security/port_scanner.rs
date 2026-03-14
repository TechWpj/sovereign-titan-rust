//! Port Scanner — TCP connect scanning with service detection.
//!
//! Provides port result structures, scan configuration, vulnerability
//! checking against known dangerous services, and scan history.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// PortStatus
// ─────────────────────────────────────────────────────────────────────────────

/// Status of a scanned port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortStatus {
    Open,
    Closed,
    Filtered,
    Unknown,
}

// ─────────────────────────────────────────────────────────────────────────────
// PortResult
// ─────────────────────────────────────────────────────────────────────────────

/// Result of scanning a single port.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortResult {
    /// The port number.
    pub port: u16,
    /// Whether the port is open, closed, or filtered.
    pub status: PortStatus,
    /// Identified service name (from the service database).
    pub service: Option<String>,
    /// Banner grabbed from the service, if any.
    pub banner: Option<String>,
    /// Time to get a response, in milliseconds.
    pub response_ms: f64,
}

// ─────────────────────────────────────────────────────────────────────────────
// ScanConfig
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for a port scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanConfig {
    /// Target host or IP address.
    pub target: String,
    /// Ports to scan.
    pub ports: Vec<u16>,
    /// Timeout per port in milliseconds.
    pub timeout_ms: u64,
    /// Maximum concurrent connections.
    pub max_concurrent: usize,
    /// Whether to attempt banner grabbing on open ports.
    pub grab_banners: bool,
}

impl ScanConfig {
    /// Preset: scan the most common service ports.
    pub fn common_ports(target: &str) -> Self {
        Self {
            target: target.to_string(),
            ports: vec![
                21, 22, 23, 25, 53, 80, 110, 135, 139, 143, 443, 445, 993, 995, 1433, 1521,
                3306, 3389, 5432, 5900, 6379, 8080, 8443, 27017,
            ],
            timeout_ms: 2000,
            max_concurrent: 10,
            grab_banners: false,
        }
    }

    /// Preset: scan a contiguous range of ports.
    pub fn port_range(target: &str, start: u16, end: u16) -> Self {
        Self {
            target: target.to_string(),
            ports: (start..=end).collect(),
            timeout_ms: 1000,
            max_concurrent: 20,
            grab_banners: false,
        }
    }

    /// Number of ports to be scanned.
    pub fn port_count(&self) -> usize {
        self.ports.len()
    }

    /// Set banner grabbing on or off.
    pub fn with_banners(mut self, grab: bool) -> Self {
        self.grab_banners = grab;
        self
    }

    /// Set timeout per port.
    pub fn with_timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }

    /// Set max concurrency.
    pub fn with_concurrency(mut self, max: usize) -> Self {
        self.max_concurrent = max;
        self
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ScanResult
// ─────────────────────────────────────────────────────────────────────────────

/// Aggregate result of a port scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    /// Target that was scanned.
    pub target: String,
    /// Ports that were found open.
    pub open_ports: Vec<PortResult>,
    /// Number of closed ports.
    pub closed_ports: usize,
    /// Number of filtered ports.
    pub filtered_ports: usize,
    /// Total ports scanned.
    pub total_scanned: usize,
    /// Total scan duration in milliseconds.
    pub duration_ms: f64,
    /// Detected vulnerabilities based on open ports.
    pub vulnerabilities: Vec<PortVulnerability>,
}

impl ScanResult {
    /// Number of open ports.
    pub fn open_count(&self) -> usize {
        self.open_ports.len()
    }

    /// Whether a specific port was found open.
    pub fn has_port(&self, port: u16) -> bool {
        self.open_ports.iter().any(|p| p.port == port)
    }

    /// Get all open ports running a particular service.
    pub fn service_ports(&self, service: &str) -> Vec<&PortResult> {
        self.open_ports
            .iter()
            .filter(|p| p.service.as_deref() == Some(service))
            .collect()
    }

    /// Whether any high-severity vulnerabilities were detected.
    pub fn has_high_severity(&self) -> bool {
        self.vulnerabilities
            .iter()
            .any(|v| v.severity == "High" || v.severity == "Critical")
    }

    /// Get vulnerabilities filtered by severity level.
    pub fn vulnerabilities_by_severity(&self, severity: &str) -> Vec<&PortVulnerability> {
        self.vulnerabilities
            .iter()
            .filter(|v| v.severity == severity)
            .collect()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PortVulnerability
// ─────────────────────────────────────────────────────────────────────────────

/// A vulnerability associated with an open port.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortVulnerability {
    /// The port number.
    pub port: u16,
    /// The service running on the port.
    pub service: String,
    /// Description of the vulnerability.
    pub vulnerability: String,
    /// Severity level: Low, Medium, High, Critical.
    pub severity: String,
    /// Recommended mitigation.
    pub recommendation: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// VulnRule (internal)
// ─────────────────────────────────────────────────────────────────────────────

struct VulnRule {
    port: u16,
    service: String,
    vuln: String,
    severity: String,
    recommendation: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// PortScanner
// ─────────────────────────────────────────────────────────────────────────────

/// Port scanner with service identification and vulnerability detection.
pub struct PortScanner {
    service_db: HashMap<u16, String>,
    vuln_rules: Vec<VulnRule>,
    scan_history: Vec<ScanResult>,
}

impl PortScanner {
    /// Create a new scanner with default service/vuln databases.
    pub fn new() -> Self {
        Self {
            service_db: Self::default_services(),
            vuln_rules: Self::default_vuln_rules(),
            scan_history: Vec::new(),
        }
    }

    fn default_services() -> HashMap<u16, String> {
        let mut m = HashMap::new();
        for (port, svc) in [
            (21, "FTP"),
            (22, "SSH"),
            (23, "Telnet"),
            (25, "SMTP"),
            (53, "DNS"),
            (80, "HTTP"),
            (110, "POP3"),
            (135, "MSRPC"),
            (139, "NetBIOS"),
            (143, "IMAP"),
            (443, "HTTPS"),
            (445, "SMB"),
            (993, "IMAPS"),
            (995, "POP3S"),
            (1433, "MSSQL"),
            (1521, "Oracle"),
            (3306, "MySQL"),
            (3389, "RDP"),
            (5432, "PostgreSQL"),
            (5900, "VNC"),
            (6379, "Redis"),
            (8080, "HTTP-Proxy"),
            (8443, "HTTPS-Alt"),
            (27017, "MongoDB"),
        ] {
            m.insert(port, svc.to_string());
        }
        m
    }

    fn default_vuln_rules() -> Vec<VulnRule> {
        vec![
            VulnRule {
                port: 21,
                service: "FTP".into(),
                vuln: "FTP allows unencrypted file transfer".into(),
                severity: "Medium".into(),
                recommendation: "Use SFTP instead".into(),
            },
            VulnRule {
                port: 23,
                service: "Telnet".into(),
                vuln: "Telnet transmits credentials in plaintext".into(),
                severity: "High".into(),
                recommendation: "Use SSH instead".into(),
            },
            VulnRule {
                port: 445,
                service: "SMB".into(),
                vuln: "SMB may be vulnerable to EternalBlue variants".into(),
                severity: "High".into(),
                recommendation: "Ensure latest patches applied".into(),
            },
            VulnRule {
                port: 3389,
                service: "RDP".into(),
                vuln: "RDP exposed to network".into(),
                severity: "Medium".into(),
                recommendation: "Use VPN or restrict access".into(),
            },
            VulnRule {
                port: 6379,
                service: "Redis".into(),
                vuln: "Redis may have no authentication".into(),
                severity: "High".into(),
                recommendation: "Enable AUTH and bind to localhost".into(),
            },
            VulnRule {
                port: 27017,
                service: "MongoDB".into(),
                vuln: "MongoDB may be unprotected".into(),
                severity: "High".into(),
                recommendation: "Enable authentication".into(),
            },
        ]
    }

    /// Look up the service name for a port.
    pub fn identify_service(&self, port: u16) -> Option<&String> {
        self.service_db.get(&port)
    }

    /// Check open ports against the vulnerability rule database.
    pub fn check_vulnerabilities(&self, open_ports: &[PortResult]) -> Vec<PortVulnerability> {
        let mut vulns = Vec::new();
        for result in open_ports {
            for rule in &self.vuln_rules {
                if result.port == rule.port {
                    vulns.push(PortVulnerability {
                        port: rule.port,
                        service: rule.service.clone(),
                        vulnerability: rule.vuln.clone(),
                        severity: rule.severity.clone(),
                        recommendation: rule.recommendation.clone(),
                    });
                }
            }
        }
        vulns
    }

    /// Build a `ScanResult` from individual port results.
    pub fn build_scan_result(
        &self,
        target: &str,
        port_results: Vec<PortResult>,
        duration_ms: f64,
    ) -> ScanResult {
        let total = port_results.len();
        let open: Vec<PortResult> = port_results
            .iter()
            .filter(|p| p.status == PortStatus::Open)
            .cloned()
            .collect();
        let closed = port_results
            .iter()
            .filter(|p| p.status == PortStatus::Closed)
            .count();
        let filtered = port_results
            .iter()
            .filter(|p| p.status == PortStatus::Filtered)
            .count();
        let vulns = self.check_vulnerabilities(&open);

        ScanResult {
            target: target.to_string(),
            open_ports: open,
            closed_ports: closed,
            filtered_ports: filtered,
            total_scanned: total,
            duration_ms,
            vulnerabilities: vulns,
        }
    }

    /// Record a completed scan in the history.
    pub fn record_scan(&mut self, result: ScanResult) {
        self.scan_history.push(result);
    }

    /// Number of scans in history.
    pub fn scan_count(&self) -> usize {
        self.scan_history.len()
    }

    /// Get the most recent scan result.
    pub fn last_scan(&self) -> Option<&ScanResult> {
        self.scan_history.last()
    }

    /// Get all scan history.
    pub fn history(&self) -> &[ScanResult] {
        &self.scan_history
    }

    /// Clear scan history.
    pub fn clear_history(&mut self) {
        self.scan_history.clear();
    }
}

impl Default for PortScanner {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_port_result(port: u16, status: PortStatus, service: Option<&str>) -> PortResult {
        PortResult {
            port,
            status,
            service: service.map(|s| s.to_string()),
            banner: None,
            response_ms: 10.0,
        }
    }

    // ── ScanConfig tests ──────────────────────────────────────────────────

    #[test]
    fn test_common_ports_config() {
        let config = ScanConfig::common_ports("192.168.1.1");
        assert_eq!(config.target, "192.168.1.1");
        assert!(config.ports.contains(&80));
        assert!(config.ports.contains(&443));
        assert!(config.ports.contains(&22));
        assert_eq!(config.timeout_ms, 2000);
        assert_eq!(config.max_concurrent, 10);
        assert!(!config.grab_banners);
    }

    #[test]
    fn test_port_range_config() {
        let config = ScanConfig::port_range("10.0.0.1", 1, 100);
        assert_eq!(config.target, "10.0.0.1");
        assert_eq!(config.ports.len(), 100);
        assert_eq!(config.ports[0], 1);
        assert_eq!(config.ports[99], 100);
    }

    #[test]
    fn test_config_port_count() {
        let config = ScanConfig::common_ports("host");
        assert_eq!(config.port_count(), config.ports.len());
    }

    #[test]
    fn test_config_builder_methods() {
        let config = ScanConfig::common_ports("host")
            .with_banners(true)
            .with_timeout(500)
            .with_concurrency(50);
        assert!(config.grab_banners);
        assert_eq!(config.timeout_ms, 500);
        assert_eq!(config.max_concurrent, 50);
    }

    #[test]
    fn test_config_serialization() {
        let config = ScanConfig::common_ports("target");
        let json = serde_json::to_string(&config).unwrap();
        let parsed: ScanConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.target, "target");
        assert_eq!(parsed.ports.len(), config.ports.len());
    }

    // ── PortScanner tests ─────────────────────────────────────────────────

    #[test]
    fn test_scanner_new() {
        let scanner = PortScanner::new();
        assert_eq!(scanner.scan_count(), 0);
        assert!(scanner.last_scan().is_none());
    }

    #[test]
    fn test_scanner_default() {
        let scanner = PortScanner::default();
        assert_eq!(scanner.scan_count(), 0);
    }

    #[test]
    fn test_identify_service() {
        let scanner = PortScanner::new();
        assert_eq!(scanner.identify_service(80), Some(&"HTTP".to_string()));
        assert_eq!(scanner.identify_service(443), Some(&"HTTPS".to_string()));
        assert_eq!(scanner.identify_service(22), Some(&"SSH".to_string()));
        assert_eq!(scanner.identify_service(3306), Some(&"MySQL".to_string()));
        assert_eq!(scanner.identify_service(12345), None);
    }

    #[test]
    fn test_check_vulnerabilities_ftp() {
        let scanner = PortScanner::new();
        let open = vec![make_port_result(21, PortStatus::Open, Some("FTP"))];
        let vulns = scanner.check_vulnerabilities(&open);
        assert_eq!(vulns.len(), 1);
        assert_eq!(vulns[0].port, 21);
        assert_eq!(vulns[0].severity, "Medium");
        assert!(vulns[0].vulnerability.contains("unencrypted"));
    }

    #[test]
    fn test_check_vulnerabilities_telnet() {
        let scanner = PortScanner::new();
        let open = vec![make_port_result(23, PortStatus::Open, Some("Telnet"))];
        let vulns = scanner.check_vulnerabilities(&open);
        assert_eq!(vulns.len(), 1);
        assert_eq!(vulns[0].severity, "High");
        assert!(vulns[0].vulnerability.contains("plaintext"));
    }

    #[test]
    fn test_check_vulnerabilities_multiple() {
        let scanner = PortScanner::new();
        let open = vec![
            make_port_result(23, PortStatus::Open, Some("Telnet")),
            make_port_result(6379, PortStatus::Open, Some("Redis")),
            make_port_result(80, PortStatus::Open, Some("HTTP")),
        ];
        let vulns = scanner.check_vulnerabilities(&open);
        // Telnet (port 23) + Redis (port 6379), HTTP (80) has no vuln rule.
        assert_eq!(vulns.len(), 2);
    }

    #[test]
    fn test_check_vulnerabilities_none() {
        let scanner = PortScanner::new();
        let open = vec![make_port_result(443, PortStatus::Open, Some("HTTPS"))];
        let vulns = scanner.check_vulnerabilities(&open);
        assert!(vulns.is_empty());
    }

    #[test]
    fn test_build_scan_result() {
        let scanner = PortScanner::new();
        let port_results = vec![
            make_port_result(22, PortStatus::Open, Some("SSH")),
            make_port_result(80, PortStatus::Open, Some("HTTP")),
            make_port_result(81, PortStatus::Closed, None),
            make_port_result(82, PortStatus::Filtered, None),
            make_port_result(23, PortStatus::Open, Some("Telnet")),
        ];

        let result = scanner.build_scan_result("192.168.1.1", port_results, 500.0);
        assert_eq!(result.target, "192.168.1.1");
        assert_eq!(result.open_count(), 3);
        assert_eq!(result.closed_ports, 1);
        assert_eq!(result.filtered_ports, 1);
        assert_eq!(result.total_scanned, 5);
        assert!((result.duration_ms - 500.0).abs() < f64::EPSILON);
        // Telnet should trigger a vulnerability.
        assert!(!result.vulnerabilities.is_empty());
    }

    #[test]
    fn test_scan_result_has_port() {
        let scanner = PortScanner::new();
        let port_results = vec![
            make_port_result(22, PortStatus::Open, Some("SSH")),
            make_port_result(80, PortStatus::Open, Some("HTTP")),
        ];
        let result = scanner.build_scan_result("host", port_results, 100.0);
        assert!(result.has_port(22));
        assert!(result.has_port(80));
        assert!(!result.has_port(443));
    }

    #[test]
    fn test_scan_result_service_ports() {
        let scanner = PortScanner::new();
        let port_results = vec![
            make_port_result(80, PortStatus::Open, Some("HTTP")),
            make_port_result(8080, PortStatus::Open, Some("HTTP")),
            make_port_result(443, PortStatus::Open, Some("HTTPS")),
        ];
        let result = scanner.build_scan_result("host", port_results, 100.0);
        assert_eq!(result.service_ports("HTTP").len(), 2);
        assert_eq!(result.service_ports("HTTPS").len(), 1);
        assert_eq!(result.service_ports("FTP").len(), 0);
    }

    #[test]
    fn test_scan_result_has_high_severity() {
        let scanner = PortScanner::new();

        // Telnet (port 23) triggers a High severity vuln.
        let with_vuln = vec![make_port_result(23, PortStatus::Open, Some("Telnet"))];
        let result = scanner.build_scan_result("host", with_vuln, 100.0);
        assert!(result.has_high_severity());

        // Only HTTPS, no vuln rules match.
        let no_vuln = vec![make_port_result(443, PortStatus::Open, Some("HTTPS"))];
        let result2 = scanner.build_scan_result("host", no_vuln, 100.0);
        assert!(!result2.has_high_severity());
    }

    #[test]
    fn test_record_and_retrieve_scan() {
        let mut scanner = PortScanner::new();
        let port_results = vec![make_port_result(80, PortStatus::Open, Some("HTTP"))];
        let result = scanner.build_scan_result("target1", port_results, 200.0);
        scanner.record_scan(result);

        assert_eq!(scanner.scan_count(), 1);
        assert!(scanner.last_scan().is_some());
        assert_eq!(scanner.last_scan().unwrap().target, "target1");
    }

    #[test]
    fn test_clear_history() {
        let mut scanner = PortScanner::new();
        let result = scanner.build_scan_result("host", Vec::new(), 0.0);
        scanner.record_scan(result);
        assert_eq!(scanner.scan_count(), 1);

        scanner.clear_history();
        assert_eq!(scanner.scan_count(), 0);
        assert!(scanner.last_scan().is_none());
    }

    #[test]
    fn test_scan_result_serialization() {
        let scanner = PortScanner::new();
        let port_results = vec![
            make_port_result(22, PortStatus::Open, Some("SSH")),
            make_port_result(80, PortStatus::Open, Some("HTTP")),
        ];
        let result = scanner.build_scan_result("host", port_results, 150.0);
        let json = serde_json::to_string(&result).unwrap();
        let parsed: ScanResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.target, "host");
        assert_eq!(parsed.open_count(), 2);
    }

    #[test]
    fn test_port_status_eq() {
        assert_eq!(PortStatus::Open, PortStatus::Open);
        assert_ne!(PortStatus::Open, PortStatus::Closed);
        assert_ne!(PortStatus::Filtered, PortStatus::Unknown);
    }

    #[test]
    fn test_vulnerabilities_by_severity() {
        let scanner = PortScanner::new();
        let port_results = vec![
            make_port_result(21, PortStatus::Open, Some("FTP")),     // Medium
            make_port_result(23, PortStatus::Open, Some("Telnet")),  // High
            make_port_result(6379, PortStatus::Open, Some("Redis")), // High
        ];
        let result = scanner.build_scan_result("host", port_results, 100.0);
        assert_eq!(result.vulnerabilities_by_severity("High").len(), 2);
        assert_eq!(result.vulnerabilities_by_severity("Medium").len(), 1);
        assert_eq!(result.vulnerabilities_by_severity("Low").len(), 0);
    }
}
