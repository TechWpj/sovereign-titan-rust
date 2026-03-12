//! DNS Intelligence & Threat Detection.
//!
//! Ported from `sovereign_titan/security/dns_intel.py`.
//! Passive DNS monitoring via Windows DNS cache, malicious domain detection,
//! DGA heuristic detection, DNS tunneling detection, and DoH bypass detection.

use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;
use tracing::{debug, warn};

use crate::security::ids::{has_known_good_parent, ConnectionInfo};

/// Known DoH provider IPs.
const DOH_PROVIDER_IPS: &[&str] = &[
    "1.1.1.1", "1.0.0.1",             // Cloudflare
    "8.8.8.8", "8.8.4.4",             // Google
    "9.9.9.9", "149.112.112.112",     // Quad9
    "208.67.222.222", "208.67.220.220", // OpenDNS
    "94.140.14.14", "94.140.15.15",   // AdGuard
];

/// Built-in threat domain patterns.
const BUILTIN_THREATS: &[&str] = &[
    "malware-traffic-analysis.net",
    "cryptomining-pool.example",
    "c2-server.example",
    "phishing-kit.example",
    "ransomware-drop.example",
];

/// Suspicious TLDs commonly used in phishing.
const SUSPICIOUS_TLDS: &[&str] = &[
    ".tk", ".ml", ".ga", ".cf", ".gq",
    ".top", ".xyz", ".buzz", ".work", ".click",
];

/// Alert cooldown in seconds.
const ALERT_COOLDOWN_SECS: f64 = 600.0;

/// A parsed DNS cache entry.
#[derive(Debug, Clone)]
pub struct DnsRecord {
    pub domain: String,
    pub record_type: String,
    pub ttl: u32,
    pub data: String,
    pub section: String,
}

/// A detected DNS threat indicator.
#[derive(Debug, Clone)]
pub struct ThreatIndicator {
    pub domain: String,
    pub threat_type: String,
    pub confidence: f32,
    pub description: String,
}

/// Full DNS intelligence report.
#[derive(Debug)]
pub struct DnsReport {
    pub timestamp: f64,
    pub total_records: usize,
    pub unique_domains: usize,
    pub threats: Vec<ThreatIndicator>,
    pub doh_detected: bool,
    pub tunneling_suspects: Vec<String>,
}

impl DnsReport {
    /// Generate a human-readable summary.
    pub fn summary(&self) -> String {
        let mut lines = vec![
            "DNS Intelligence Report:".to_string(),
            format!(
                "  Records: {}, Unique Domains: {}",
                self.total_records, self.unique_domains
            ),
        ];
        if self.threats.is_empty() {
            lines.push("  No threats detected".into());
        } else {
            lines.push(format!("  Threats Detected: {}", self.threats.len()));
            for t in self.threats.iter().take(5) {
                lines.push(format!(
                    "    - [{}] {} (confidence: {:.0}%)",
                    t.threat_type,
                    t.domain,
                    t.confidence * 100.0
                ));
            }
        }
        if self.doh_detected {
            lines.push("  WARNING: DNS-over-HTTPS bypass detected".into());
        }
        if !self.tunneling_suspects.is_empty() {
            lines.push(format!(
                "  Tunneling Suspects: {}",
                self.tunneling_suspects
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        lines.join("\n")
    }
}

/// DNS intelligence and threat detection engine.
pub struct DnsIntelligenceEngine {
    threat_domains: HashSet<String>,
    query_history: HashMap<String, Vec<f64>>,
    reported_threats: HashMap<String, f64>,
}

impl DnsIntelligenceEngine {
    /// Create a new DNS intelligence engine with built-in threat feeds.
    pub fn new() -> Self {
        let mut threat_domains = HashSet::new();
        for domain in BUILTIN_THREATS {
            threat_domains.insert(domain.to_string());
        }

        Self {
            threat_domains,
            query_history: HashMap::new(),
            reported_threats: HashMap::new(),
        }
    }

    /// Add a custom threat domain.
    pub fn add_threat_domain(&mut self, domain: &str) {
        self.threat_domains
            .insert(domain.to_lowercase().trim_end_matches('.').to_string());
    }

    /// Load threat domains from a file (one per line, # comments).
    pub fn load_threat_feed(&mut self, path: &str) {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                let mut count = 0;
                for line in content.lines() {
                    let line = line.trim();
                    if !line.is_empty() && !line.starts_with('#') {
                        self.threat_domains.insert(line.to_lowercase());
                        count += 1;
                    }
                }
                debug!("Loaded {count} threat domains from {path}");
            }
            Err(e) => {
                warn!("Failed to load threat feed from {path}: {e}");
            }
        }
    }

    /// Dump and parse the Windows DNS cache.
    pub fn dump_dns_cache(&self) -> Vec<DnsRecord> {
        let output = Command::new("ipconfig")
            .args(["/displaydns"])
            .output();

        match output {
            Ok(out) => {
                if out.status.success() {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    parse_dns_output(&stdout)
                } else {
                    warn!("ipconfig /displaydns failed");
                    Vec::new()
                }
            }
            Err(e) => {
                warn!("Failed to run ipconfig: {e}");
                Vec::new()
            }
        }
    }

    /// Run full DNS intelligence analysis.
    ///
    /// If `dns_records` is None, dumps the cache automatically.
    pub fn analyze(
        &mut self,
        dns_records: Option<&[DnsRecord]>,
        connections: Option<&[ConnectionInfo]>,
    ) -> DnsReport {
        let now = now_secs();

        let owned_records;
        let records = match dns_records {
            Some(r) => r,
            None => {
                owned_records = self.dump_dns_cache();
                &owned_records
            }
        };

        let mut threats = Vec::new();
        let mut tunneling_suspects = Vec::new();
        let mut unique_domains = HashSet::new();

        for record in records {
            let domain = record.domain.to_lowercase();
            let domain = domain.trim_end_matches('.');
            unique_domains.insert(domain.to_string());

            // Track query times for tunneling detection.
            self.query_history
                .entry(domain.to_string())
                .or_default()
                .push(now);

            // Check threat feeds.
            if let Some(threat) = self.check_threat_feed(domain) {
                if !self.is_recently_reported(domain, "feed", now) {
                    threats.push(threat);
                }
            }

            // Check suspicious TLDs.
            if let Some(threat) = check_suspicious_tld(domain) {
                if !self.is_recently_reported(domain, "tld", now) {
                    threats.push(threat);
                }
            }

            // Check for DGA-like domains.
            if let Some(threat) = check_dga(domain) {
                if !self.is_recently_reported(domain, "dga", now) {
                    threats.push(threat);
                }
            }
        }

        // Tunneling detection.
        let domains_to_check: Vec<(String, Vec<f64>)> = self
            .query_history
            .iter()
            .map(|(d, times)| {
                let recent: Vec<f64> = times.iter().filter(|&&t| now - t < 60.0).copied().collect();
                (d.clone(), recent)
            })
            .collect();

        for (domain, recent) in &domains_to_check {
            // Clean old entries.
            if let Some(times) = self.query_history.get_mut(domain.as_str()) {
                times.retain(|&t| now - t < 60.0);
            }

            if is_tunneling_suspect(domain, recent.len()) {
                if !self.is_recently_reported(domain, "tunnel", now) {
                    tunneling_suspects.push(domain.clone());
                    threats.push(ThreatIndicator {
                        domain: domain.clone(),
                        threat_type: "tunneling".to_string(),
                        confidence: 0.7,
                        description: format!(
                            "Possible DNS tunneling: {} queries in 60s with long labels",
                            recent.len()
                        ),
                    });
                }
            }
        }

        // DoH detection.
        let doh_detected = connections.is_some_and(|conns| detect_doh(conns));
        if doh_detected {
            threats.push(ThreatIndicator {
                domain: "(DoH provider)".to_string(),
                threat_type: "doh_bypass".to_string(),
                confidence: 0.6,
                description: "DNS-over-HTTPS detected — DNS queries may bypass local monitoring"
                    .to_string(),
            });
        }

        DnsReport {
            timestamp: now,
            total_records: records.len(),
            unique_domains: unique_domains.len(),
            threats,
            doh_detected,
            tunneling_suspects,
        }
    }

    /// Check a single domain against all threat indicators.
    pub fn check_domain(&self, domain: &str) -> Option<ThreatIndicator> {
        let domain = domain.to_lowercase();
        let domain = domain.trim_end_matches('.');

        if let Some(t) = self.check_threat_feed(domain) {
            return Some(t);
        }
        if let Some(t) = check_suspicious_tld(domain) {
            return Some(t);
        }
        check_dga(domain)
    }

    // ── Internal ─────────────────────────────────────────────────────────

    fn check_threat_feed(&self, domain: &str) -> Option<ThreatIndicator> {
        if self.threat_domains.contains(domain) {
            return Some(ThreatIndicator {
                domain: domain.to_string(),
                threat_type: "known_malicious".to_string(),
                confidence: 0.9,
                description: format!("Domain matches threat feed: {domain}"),
            });
        }
        // Subdomain match.
        let parts: Vec<&str> = domain.split('.').collect();
        for i in 1..parts.len() {
            let parent = parts[i..].join(".");
            if self.threat_domains.contains(&parent) {
                return Some(ThreatIndicator {
                    domain: domain.to_string(),
                    threat_type: "known_malicious".to_string(),
                    confidence: 0.85,
                    description: format!("Subdomain of known threat: {parent}"),
                });
            }
        }
        None
    }

    fn is_recently_reported(&mut self, domain: &str, threat_type: &str, now: f64) -> bool {
        let key = format!("{domain}|{threat_type}");
        if let Some(&last) = self.reported_threats.get(&key) {
            if now - last < ALERT_COOLDOWN_SECS {
                return true;
            }
        }
        self.reported_threats.insert(key, now);
        false
    }
}

impl Default for DnsIntelligenceEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ── Standalone detection functions ───────────────────────────────────────

/// Check if a domain has a suspicious TLD.
fn check_suspicious_tld(domain: &str) -> Option<ThreatIndicator> {
    for tld in SUSPICIOUS_TLDS {
        if domain.ends_with(tld) {
            return Some(ThreatIndicator {
                domain: domain.to_string(),
                threat_type: "suspicious_tld".to_string(),
                confidence: 0.3,
                description: format!("Domain uses suspicious TLD: {tld}"),
            });
        }
    }
    None
}

/// Detect Domain Generation Algorithm (DGA) patterns.
///
/// Heuristic: high consonant ratio (>75%) + long random-looking label (>15 chars).
fn check_dga(domain: &str) -> Option<ThreatIndicator> {
    if has_known_good_parent(domain) {
        return None;
    }

    let parts: Vec<&str> = domain.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let label = if parts.len() == 2 { parts[0] } else { parts[parts.len() - 2] };

    if label.len() < 10 {
        return None;
    }

    let vowels: HashSet<char> = "aeiou".chars().collect();
    let consonants: usize = label
        .chars()
        .filter(|c| c.is_alphabetic() && !vowels.contains(&c.to_lowercase().next().unwrap_or('x')))
        .count();
    let letters: usize = label.chars().filter(|c| c.is_alphabetic()).count();
    let digits: usize = label.chars().filter(|c| c.is_ascii_digit()).count();

    if letters == 0 {
        return None;
    }

    let consonant_ratio = consonants as f32 / letters as f32;
    let digit_ratio = digits as f32 / label.len() as f32;

    if label.len() > 15 && (consonant_ratio > 0.75 || digit_ratio > 0.3) {
        return Some(ThreatIndicator {
            domain: domain.to_string(),
            threat_type: "dga".to_string(),
            confidence: 0.5,
            description: format!(
                "Possible DGA domain: {label} (consonant ratio: {consonant_ratio:.2})"
            ),
        });
    }
    None
}

/// Check if a domain shows DNS tunneling patterns.
fn is_tunneling_suspect(domain: &str, recent_query_count: usize) -> bool {
    if has_known_good_parent(domain) {
        return false;
    }
    // High query rate.
    if recent_query_count > 20 {
        return true;
    }
    // Long subdomain labels.
    let parts: Vec<&str> = domain.split('.').collect();
    if parts.len() > 2 {
        for part in &parts[..parts.len() - 2] {
            if part.len() > 50 {
                return true;
            }
        }
    }
    false
}

/// Detect DNS-over-HTTPS by checking connections to known DoH providers on port 443.
fn detect_doh(connections: &[ConnectionInfo]) -> bool {
    let doh_ips: HashSet<&str> = DOH_PROVIDER_IPS.iter().copied().collect();
    connections.iter().any(|conn| {
        conn.remote_port == 443 && doh_ips.contains(conn.remote_addr.as_str())
    })
}

/// Parse `ipconfig /displaydns` output into structured records.
fn parse_dns_output(raw: &str) -> Vec<DnsRecord> {
    let mut records = Vec::new();
    let name_re = Regex::new(r"(?i)Record Name[\s.]*:\s*(.+)").unwrap();
    let type_re = Regex::new(r"(?i)Record Type[\s.]*:\s*(\d+)").unwrap();
    let ttl_re = Regex::new(r"(?i)Time To Live[\s.]*:\s*(\d+)").unwrap();
    let section_re = Regex::new(r"(?i)Section[\s.]*:\s*(\w+)").unwrap();
    let data_re =
        Regex::new(r"(?i)(?:A \(Host\)|AAAA|CNAME|PTR)\s+Record[\s.]*:\s*(.+)").unwrap();

    let mut current_domain = String::new();
    let mut current_type = String::new();
    let mut current_ttl = 0u32;
    let mut current_section = "Answer".to_string();

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('-') {
            continue;
        }

        if let Some(cap) = name_re.captures(line) {
            current_domain = cap[1].trim().to_string();
            continue;
        }
        if let Some(cap) = type_re.captures(line) {
            let type_num: u32 = cap[1].parse().unwrap_or(0);
            current_type = match type_num {
                1 => "A",
                5 => "CNAME",
                28 => "AAAA",
                12 => "PTR",
                15 => "MX",
                6 => "SOA",
                _ => "UNKNOWN",
            }
            .to_string();
            continue;
        }
        if let Some(cap) = ttl_re.captures(line) {
            current_ttl = cap[1].parse().unwrap_or(0);
            continue;
        }
        if let Some(cap) = section_re.captures(line) {
            current_section = cap[1].trim().to_string();
            continue;
        }
        if let Some(cap) = data_re.captures(line) {
            if !current_domain.is_empty() {
                records.push(DnsRecord {
                    domain: current_domain.clone(),
                    record_type: if current_type.is_empty() {
                        "A".to_string()
                    } else {
                        current_type.clone()
                    },
                    ttl: current_ttl,
                    data: cap[1].trim().to_string(),
                    section: current_section.clone(),
                });
            }
        }
    }

    records
}

/// Get current time as seconds since UNIX epoch.
fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_engine() {
        let engine = DnsIntelligenceEngine::new();
        assert_eq!(engine.threat_domains.len(), BUILTIN_THREATS.len());
    }

    #[test]
    fn test_add_threat_domain() {
        let mut engine = DnsIntelligenceEngine::new();
        engine.add_threat_domain("evil.example.com");
        assert!(engine.threat_domains.contains("evil.example.com"));
    }

    #[test]
    fn test_check_domain_known_malicious() {
        let engine = DnsIntelligenceEngine::new();
        let result = engine.check_domain("c2-server.example");
        assert!(result.is_some());
        assert_eq!(result.unwrap().threat_type, "known_malicious");
    }

    #[test]
    fn test_check_domain_subdomain() {
        let engine = DnsIntelligenceEngine::new();
        let result = engine.check_domain("sub.c2-server.example");
        assert!(result.is_some());
        assert_eq!(result.unwrap().confidence, 0.85);
    }

    #[test]
    fn test_check_domain_clean() {
        let engine = DnsIntelligenceEngine::new();
        let result = engine.check_domain("google.com");
        assert!(result.is_none());
    }

    #[test]
    fn test_suspicious_tld() {
        assert!(check_suspicious_tld("malware.tk").is_some());
        assert!(check_suspicious_tld("phishing.xyz").is_some());
        assert!(check_suspicious_tld("google.com").is_none());
    }

    #[test]
    fn test_dga_detection() {
        // Random consonant-heavy string.
        let result = check_dga("xkjhgfdsnbvcxzlqwrtyp.com");
        assert!(result.is_some());
        assert_eq!(result.unwrap().threat_type, "dga");
    }

    #[test]
    fn test_dga_clean_domain() {
        assert!(check_dga("google.com").is_none());
        assert!(check_dga("stackoverflow.com").is_none());
    }

    #[test]
    fn test_dga_skips_known_good() {
        assert!(check_dga("longsubdomainlabel12345678.microsoft.com").is_none());
    }

    #[test]
    fn test_tunneling_suspect_high_rate() {
        assert!(is_tunneling_suspect("data.evil.com", 25));
        assert!(!is_tunneling_suspect("data.evil.com", 5));
    }

    #[test]
    fn test_tunneling_suspect_long_label() {
        let long_label = "a".repeat(60);
        let domain = format!("{long_label}.evil.com");
        assert!(is_tunneling_suspect(&domain, 1));
    }

    #[test]
    fn test_tunneling_skips_known_good() {
        let domain = "longlonglonglonglonglonglonglonglonglonglonglonglonglabel.google.com";
        assert!(!is_tunneling_suspect(domain, 25));
    }

    #[test]
    fn test_doh_detection() {
        let connections = vec![ConnectionInfo {
            local_addr: "192.168.1.100".into(),
            local_port: 50000,
            remote_addr: "1.1.1.1".into(), // Cloudflare DoH
            remote_port: 443,
            process_name: "chrome".into(),
            status: "ESTABLISHED".into(),
            direction: "outbound".into(),
            is_encrypted: true,
        }];
        assert!(detect_doh(&connections));
    }

    #[test]
    fn test_doh_not_detected() {
        let connections = vec![ConnectionInfo {
            local_addr: "192.168.1.100".into(),
            local_port: 50000,
            remote_addr: "93.184.216.34".into(), // Not a DoH provider
            remote_port: 443,
            process_name: "chrome".into(),
            status: "ESTABLISHED".into(),
            direction: "outbound".into(),
            is_encrypted: true,
        }];
        assert!(!detect_doh(&connections));
    }

    #[test]
    fn test_parse_dns_output() {
        let raw = r#"
    Record Name . . . . . : www.google.com
    Record Type . . . . . : 1
    Time To Live  . . . . : 300
    Section . . . . . . . : Answer
    A (Host) Record . . . : 142.250.80.36
        "#;
        let records = parse_dns_output(raw);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].domain, "www.google.com");
        assert_eq!(records[0].record_type, "A");
        assert_eq!(records[0].data, "142.250.80.36");
        assert_eq!(records[0].ttl, 300);
    }

    #[test]
    fn test_analyze_with_threat() {
        let mut engine = DnsIntelligenceEngine::new();
        let records = vec![
            DnsRecord {
                domain: "c2-server.example".into(),
                record_type: "A".into(),
                ttl: 60,
                data: "10.0.0.1".into(),
                section: "Answer".into(),
            },
            DnsRecord {
                domain: "google.com".into(),
                record_type: "A".into(),
                ttl: 300,
                data: "142.250.80.36".into(),
                section: "Answer".into(),
            },
        ];
        let report = engine.analyze(Some(&records), None);
        assert_eq!(report.total_records, 2);
        assert_eq!(report.unique_domains, 2);
        assert!(
            report.threats.iter().any(|t| t.threat_type == "known_malicious"),
            "Should detect known malicious domain"
        );
    }

    #[test]
    fn test_dns_report_summary() {
        let report = DnsReport {
            timestamp: 0.0,
            total_records: 5,
            unique_domains: 3,
            threats: vec![ThreatIndicator {
                domain: "evil.tk".into(),
                threat_type: "suspicious_tld".into(),
                confidence: 0.3,
                description: "test".into(),
            }],
            doh_detected: true,
            tunneling_suspects: vec!["tunnel.evil.com".into()],
        };
        let summary = report.summary();
        assert!(summary.contains("Threats Detected: 1"));
        assert!(summary.contains("DNS-over-HTTPS"));
        assert!(summary.contains("Tunneling"));
    }
}
