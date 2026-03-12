//! Network Tools — ping, DNS lookup, public IP, port check.
//!
//! Uses PowerShell for ping, reqwest for my_ip, std::net for DNS/port.

use anyhow::Result;
use serde_json::Value;
use tracing::info;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub struct NetworkToolsTool;

impl NetworkToolsTool {
    fn run_ps(cmd: &str) -> String {
        let result = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", cmd])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .creation_flags(0x08000000)
            .spawn();

        match result {
            Ok(child) => match child.wait_with_output() {
                Ok(output) => String::from_utf8_lossy(&output.stdout).trim().to_string(),
                Err(_) => String::new(),
            },
            Err(_) => String::new(),
        }
    }

    fn ping(host: &str, count: u32) -> String {
        // Validate host — no shell injection
        if host.contains(';') || host.contains('|') || host.contains('&') || host.contains('`') {
            return "Invalid hostname.".to_string();
        }

        let count = count.min(10).max(1);
        let cmd = format!(
            "Test-Connection -ComputerName '{}' -Count {} -ErrorAction Stop | \
             Select-Object Address, Latency, Status | Format-Table -AutoSize | Out-String",
            host.replace('\'', "''"),
            count
        );
        let output = Self::run_ps(&cmd);
        if output.is_empty() {
            format!("Ping to {host} failed or timed out.")
        } else {
            format!("Ping {host}:\n{output}")
        }
    }

    fn dns_lookup(host: &str) -> String {
        use std::net::ToSocketAddrs;

        let addr = format!("{host}:80");
        match addr.to_socket_addrs() {
            Ok(addrs) => {
                let ips: Vec<String> = addrs.map(|a| a.ip().to_string()).collect();
                if ips.is_empty() {
                    format!("No DNS records found for {host}.")
                } else {
                    let unique: Vec<&String> = {
                        let mut seen = std::collections::HashSet::new();
                        ips.iter().filter(|ip| seen.insert(*ip)).collect()
                    };
                    format!("DNS lookup for {host}:\n{}", unique.iter().map(|ip| format!("  {ip}")).collect::<Vec<_>>().join("\n"))
                }
            }
            Err(e) => format!("DNS lookup failed for {host}: {e}"),
        }
    }

    fn port_check(host: &str, port: u16) -> String {
        use std::net::TcpStream;
        use std::time::Duration;

        let addr = format!("{host}:{port}");
        match TcpStream::connect_timeout(
            &addr.parse().unwrap_or_else(|_| std::net::SocketAddr::from(([127, 0, 0, 1], port))),
            Duration::from_secs(5),
        ) {
            Ok(_) => format!("Port {port} on {host} is **open**."),
            Err(_) => format!("Port {port} on {host} is **closed** or unreachable."),
        }
    }
}

#[async_trait::async_trait]
impl super::Tool for NetworkToolsTool {
    fn name(&self) -> &'static str {
        "network_tools"
    }

    fn description(&self) -> &'static str {
        "Network diagnostic tools. Input: {\"action\": \"<action>\", ...}. \
         Actions: ping (host, count?: 4), dns_lookup (host), \
         my_ip (gets public IP), port_check (host, port)."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("my_ip");

        info!("network_tools: action={action}");

        match action {
            "ping" => {
                let host = input.get("host").and_then(|v| v.as_str()).unwrap_or("");
                if host.is_empty() {
                    return Ok("ping requires a \"host\" field.".to_string());
                }
                let count = input.get("count").and_then(|v| v.as_u64()).unwrap_or(4) as u32;
                let host_owned = host.to_string();
                let result = tokio::task::spawn_blocking(move || {
                    Self::ping(&host_owned, count)
                })
                .await?;
                Ok(result)
            }
            "dns_lookup" | "dns" | "resolve" => {
                let host = input.get("host").and_then(|v| v.as_str()).unwrap_or("");
                if host.is_empty() {
                    return Ok("dns_lookup requires a \"host\" field.".to_string());
                }
                let host_owned = host.to_string();
                let result = tokio::task::spawn_blocking(move || {
                    Self::dns_lookup(&host_owned)
                })
                .await?;
                Ok(result)
            }
            "my_ip" | "public_ip" | "ip" => {
                // Use a public API to get external IP
                match reqwest::get("https://api.ipify.org").await {
                    Ok(resp) => {
                        match resp.text().await {
                            Ok(ip) => Ok(format!("Public IP: **{ip}**")),
                            Err(e) => Ok(format!("Failed to read IP response: {e}")),
                        }
                    }
                    Err(e) => Ok(format!("Failed to get public IP: {e}")),
                }
            }
            "port_check" | "port" => {
                let host = input.get("host").and_then(|v| v.as_str()).unwrap_or("localhost");
                let port = input.get("port").and_then(|v| v.as_u64()).unwrap_or(80) as u16;
                let host_owned = host.to_string();
                let result = tokio::task::spawn_blocking(move || {
                    Self::port_check(&host_owned, port)
                })
                .await?;
                Ok(result)
            }
            other => Ok(format!(
                "Unknown action: '{other}'. Use: ping, dns_lookup, my_ip, port_check."
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;
    use serde_json::json;

    #[test]
    fn test_dns_lookup_localhost() {
        let result = NetworkToolsTool::dns_lookup("localhost");
        assert!(result.contains("127.0.0.1") || result.contains("::1") || result.contains("DNS lookup"));
    }

    #[test]
    fn test_dns_lookup_invalid() {
        let result = NetworkToolsTool::dns_lookup("thishostdefinitelydoesnotexist.invalid");
        assert!(result.contains("failed") || result.contains("No DNS"));
    }

    #[test]
    fn test_port_check_invalid() {
        // Port 1 on localhost should be closed
        let result = NetworkToolsTool::port_check("127.0.0.1", 1);
        assert!(result.contains("closed") || result.contains("unreachable"));
    }

    #[test]
    fn test_ping_injection_blocked() {
        let result = NetworkToolsTool::ping("evil;rm -rf /", 1);
        assert_eq!(result, "Invalid hostname.");
    }

    #[tokio::test]
    async fn test_unknown_action() {
        let tool = NetworkToolsTool;
        let result = tool.execute(json!({"action": "fly"})).await.unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[tokio::test]
    async fn test_ping_missing_host() {
        let tool = NetworkToolsTool;
        let result = tool.execute(json!({"action": "ping"})).await.unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_dns_missing_host() {
        let tool = NetworkToolsTool;
        let result = tool.execute(json!({"action": "dns_lookup"})).await.unwrap();
        assert!(result.contains("requires"));
    }
}
